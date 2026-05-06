use crate::core::cloud::{CloudCredentials, CloudProviderKind};
use crate::core::dictionary::{DictionaryStore, DictionaryTerm};
use crate::core::history::{TranscriptionEntry, TranscriptionStore};
use crate::core::injection::TextInjector;
use crate::core::settings::{AppSettings, SettingsStore};
use crate::system;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyManager};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_SPACE};

const PILL_BOTTOM_MARGIN: i32 = 10;
const FALLBACK_PILL_WIDTH: i32 = 168;
const FALLBACK_PILL_HEIGHT: i32 = 44;

#[derive(Debug, Clone, Copy)]
pub struct ActiveHotkey {
    pub id: u32,
    pub key: VIRTUAL_KEY,
    pub modifiers: HotkeyModifiers,
}

impl Default for ActiveHotkey {
    fn default() -> Self {
        Self {
            id: 0,
            key: VK_SPACE,
            modifiers: HotkeyModifiers {
                super_key: true,
                ..HotkeyModifiers::default()
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HotkeyModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

pub struct AppRuntime {
    pub settings_store: Mutex<SettingsStore>,
    pub dictionary_store: Mutex<DictionaryStore>,
    pub transcription_store: Mutex<TranscriptionStore>,
    pub hotkey_manager: Mutex<GlobalHotKeyManager>,
    pub registered_hotkey: Mutex<HotKey>,
    pub active_binding: Arc<Mutex<ActiveHotkey>>,
    pub shutdown_requested: Arc<AtomicBool>,
    allow_window_close: Arc<AtomicBool>,
    cleanup_completed: AtomicBool,
    engine_thread: Mutex<Option<JoinHandle<()>>>,
}

impl AppRuntime {
    pub fn new(
        settings_store: SettingsStore,
        dictionary_store: DictionaryStore,
        transcription_store: TranscriptionStore,
        hotkey_manager: GlobalHotKeyManager,
        active_hotkey: HotKey,
        active_binding: ActiveHotkey,
    ) -> Self {
        Self {
            settings_store: Mutex::new(settings_store),
            dictionary_store: Mutex::new(dictionary_store),
            transcription_store: Mutex::new(transcription_store),
            hotkey_manager: Mutex::new(hotkey_manager),
            registered_hotkey: Mutex::new(active_hotkey),
            active_binding: Arc::new(Mutex::new(active_binding)),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            allow_window_close: Arc::new(AtomicBool::new(false)),
            cleanup_completed: AtomicBool::new(false),
            engine_thread: Mutex::new(None),
        }
    }

    pub fn track_engine_thread(&mut self, handle: JoinHandle<()>) {
        if let Ok(mut engine_thread) = self.engine_thread.lock() {
            *engine_thread = Some(handle);
        }
    }

    pub fn request_exit(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        self.allow_window_close.store(true, Ordering::SeqCst);
    }

    pub fn allow_window_close(&self) -> bool {
        self.allow_window_close.load(Ordering::SeqCst)
    }

    pub fn cleanup(&self) {
        self.request_exit();
        if self.cleanup_completed.swap(true, Ordering::SeqCst) {
            return;
        }

        if let (Ok(hotkey_manager), Ok(registered_hotkey)) =
            (self.hotkey_manager.lock(), self.registered_hotkey.lock())
        {
            let _ = hotkey_manager.unregister(*registered_hotkey);
        }

        if let Ok(mut engine_thread) = self.engine_thread.lock() {
            if let Some(handle) = engine_thread.take() {
                let _ = handle.join();
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudApiKeyStatus {
    pub provider: String,
    pub configured: bool,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppRuntime>) -> Result<AppSettings, String> {
    state
        .settings_store
        .lock()
        .map_err(|_| "Settings store lock is unavailable".to_string())?
        .load()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppRuntime>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let parsed_hotkey = parse_hotkey(&settings.hotkey)?;
    let parsed_binding = parse_hotkey_binding(&settings.hotkey, parsed_hotkey.id())?;
    apply_hotkey_change(&state, parsed_hotkey, parsed_binding, &settings.hotkey)?;
    system::sync_launch_at_login_setting(&settings)?;

    state
        .settings_store
        .lock()
        .map_err(|_| "Settings store lock is unavailable".to_string())?
        .persist(&settings)?;

    apply_pill_settings(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub fn list_dictionary_terms(state: State<'_, AppRuntime>) -> Result<Vec<DictionaryTerm>, String> {
    state
        .dictionary_store
        .lock()
        .map_err(|_| "Dictionary store lock is unavailable".to_string())?
        .list_terms()
}

#[tauri::command]
pub fn add_dictionary_term(
    state: State<'_, AppRuntime>,
    term: String,
) -> Result<DictionaryTerm, String> {
    state
        .dictionary_store
        .lock()
        .map_err(|_| "Dictionary store lock is unavailable".to_string())?
        .add_term(&term)
}

#[tauri::command]
pub fn remove_dictionary_term(state: State<'_, AppRuntime>, id: i64) -> Result<(), String> {
    state
        .dictionary_store
        .lock()
        .map_err(|_| "Dictionary store lock is unavailable".to_string())?
        .remove_term(id)
}

#[tauri::command]
pub fn list_transcriptions(
    state: State<'_, AppRuntime>,
    limit: Option<i64>,
    query: Option<String>,
) -> Result<Vec<TranscriptionEntry>, String> {
    state
        .transcription_store
        .lock()
        .map_err(|_| "Transcription history store lock is unavailable".to_string())?
        .search(query.as_deref(), limit.unwrap_or(100))
}

#[tauri::command]
pub fn get_transcription(
    state: State<'_, AppRuntime>,
    id: i64,
) -> Result<Option<TranscriptionEntry>, String> {
    state
        .transcription_store
        .lock()
        .map_err(|_| "Transcription history store lock is unavailable".to_string())?
        .get(id)
}

#[tauri::command]
pub fn delete_transcription(state: State<'_, AppRuntime>, id: i64) -> Result<(), String> {
    state
        .transcription_store
        .lock()
        .map_err(|_| "Transcription history store lock is unavailable".to_string())?
        .delete(id)
}

#[tauri::command]
pub fn reinsert_transcription(state: State<'_, AppRuntime>, id: i64) -> Result<TranscriptionEntry, String> {
    let entry = state
        .transcription_store
        .lock()
        .map_err(|_| "Transcription history store lock is unavailable".to_string())?
        .get(id)?
        .ok_or_else(|| format!("Transcription {} could not be found", id))?;

    TextInjector::inject(entry.text.clone())?;
    Ok(entry)
}

#[tauri::command]
pub fn cloud_api_key_status(provider: String) -> Result<CloudApiKeyStatus, String> {
    let provider_kind = CloudProviderKind::parse(&provider);
    let configured = CloudCredentials::read_api_key(provider_kind)?.is_some();
    Ok(CloudApiKeyStatus {
        provider,
        configured,
    })
}

#[tauri::command]
pub fn store_cloud_api_key(provider: String, api_key: String) -> Result<(), String> {
    CloudCredentials::write_api_key(CloudProviderKind::parse(&provider), &api_key)
}

#[tauri::command]
pub fn delete_cloud_api_key(provider: String) -> Result<(), String> {
    CloudCredentials::delete_api_key(CloudProviderKind::parse(&provider))
}

pub fn parse_hotkey(value: &str) -> Result<HotKey, String> {
    let normalized = normalize_hotkey(value);
    if normalized.is_empty() {
        return Err("Hotkey cannot be empty".to_string());
    }

    HotKey::try_from(normalized.as_str()).map_err(|e| format!("Invalid hotkey `{}`: {}", value, e))
}

pub fn parse_hotkey_binding(value: &str, id: u32) -> Result<ActiveHotkey, String> {
    let normalized = normalize_hotkey(value);
    let mut tokens = normalized
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    let key_token = tokens
        .pop()
        .ok_or_else(|| "Hotkey must include a key".to_string())?;
    let mut modifiers = HotkeyModifiers::default();

    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.control = true,
            "alt" | "option" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            "super" | "win" | "windows" | "cmd" | "command" | "meta" => modifiers.super_key = true,
            _ => return Err(format!("Unsupported hotkey modifier `{}`", token)),
        }
    }

    Ok(ActiveHotkey {
        id,
        key: parse_virtual_key(key_token)?,
        modifiers,
    })
}

pub fn apply_pill_settings(app: &AppHandle, settings: &AppSettings) {
    if let Some(pill) = app.get_webview_window("pill") {
        let _ = if settings.pill_visible {
            pill.show()
        } else {
            pill.hide()
        };
        position_pill_window(&pill);
        let _ = pill.emit("pill-settings", settings);
    }
}

pub fn position_pill_window(pill: &WebviewWindow) {
    let monitor = pill
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| pill.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        return;
    };

    let work_area = monitor.work_area();
    let window_size = pill.outer_size().ok();
    let width = window_size
        .map(|size| size.width as i32)
        .unwrap_or(FALLBACK_PILL_WIDTH);
    let height = window_size
        .map(|size| size.height as i32)
        .unwrap_or(FALLBACK_PILL_HEIGHT);

    let x = work_area.position.x + ((work_area.size.width as i32 - width) / 2).max(0);
    let y =
        work_area.position.y + (work_area.size.height as i32 - height - PILL_BOTTOM_MARGIN).max(0);

    let _ = pill.set_position(PhysicalPosition::new(x, y));
}

fn parse_virtual_key(token: &str) -> Result<VIRTUAL_KEY, String> {
    match token.to_ascii_lowercase().as_str() {
        "space" => Ok(VK_SPACE),
        "a" | "keya" => Ok(VIRTUAL_KEY(0x41)),
        "b" | "keyb" => Ok(VIRTUAL_KEY(0x42)),
        "c" | "keyc" => Ok(VIRTUAL_KEY(0x43)),
        "d" | "keyd" => Ok(VIRTUAL_KEY(0x44)),
        "e" | "keye" => Ok(VIRTUAL_KEY(0x45)),
        "f" | "keyf" => Ok(VIRTUAL_KEY(0x46)),
        "g" | "keyg" => Ok(VIRTUAL_KEY(0x47)),
        "h" | "keyh" => Ok(VIRTUAL_KEY(0x48)),
        "i" | "keyi" => Ok(VIRTUAL_KEY(0x49)),
        "j" | "keyj" => Ok(VIRTUAL_KEY(0x4A)),
        "k" | "keyk" => Ok(VIRTUAL_KEY(0x4B)),
        "l" | "keyl" => Ok(VIRTUAL_KEY(0x4C)),
        "m" | "keym" => Ok(VIRTUAL_KEY(0x4D)),
        "n" | "keyn" => Ok(VIRTUAL_KEY(0x4E)),
        "o" | "keyo" => Ok(VIRTUAL_KEY(0x4F)),
        "p" | "keyp" => Ok(VIRTUAL_KEY(0x50)),
        "q" | "keyq" => Ok(VIRTUAL_KEY(0x51)),
        "r" | "keyr" => Ok(VIRTUAL_KEY(0x52)),
        "s" | "keys" => Ok(VIRTUAL_KEY(0x53)),
        "t" | "keyt" => Ok(VIRTUAL_KEY(0x54)),
        "u" | "keyu" => Ok(VIRTUAL_KEY(0x55)),
        "v" | "keyv" => Ok(VIRTUAL_KEY(0x56)),
        "w" | "keyw" => Ok(VIRTUAL_KEY(0x57)),
        "x" | "keyx" => Ok(VIRTUAL_KEY(0x58)),
        "y" | "keyy" => Ok(VIRTUAL_KEY(0x59)),
        "z" | "keyz" => Ok(VIRTUAL_KEY(0x5A)),
        "0" | "digit0" => Ok(VIRTUAL_KEY(0x30)),
        "1" | "digit1" => Ok(VIRTUAL_KEY(0x31)),
        "2" | "digit2" => Ok(VIRTUAL_KEY(0x32)),
        "3" | "digit3" => Ok(VIRTUAL_KEY(0x33)),
        "4" | "digit4" => Ok(VIRTUAL_KEY(0x34)),
        "5" | "digit5" => Ok(VIRTUAL_KEY(0x35)),
        "6" | "digit6" => Ok(VIRTUAL_KEY(0x36)),
        "7" | "digit7" => Ok(VIRTUAL_KEY(0x37)),
        "8" | "digit8" => Ok(VIRTUAL_KEY(0x38)),
        "9" | "digit9" => Ok(VIRTUAL_KEY(0x39)),
        "f1" => Ok(VIRTUAL_KEY(0x70)),
        "f2" => Ok(VIRTUAL_KEY(0x71)),
        "f3" => Ok(VIRTUAL_KEY(0x72)),
        "f4" => Ok(VIRTUAL_KEY(0x73)),
        "f5" => Ok(VIRTUAL_KEY(0x74)),
        "f6" => Ok(VIRTUAL_KEY(0x75)),
        "f7" => Ok(VIRTUAL_KEY(0x76)),
        "f8" => Ok(VIRTUAL_KEY(0x77)),
        "f9" => Ok(VIRTUAL_KEY(0x78)),
        "f10" => Ok(VIRTUAL_KEY(0x79)),
        "f11" => Ok(VIRTUAL_KEY(0x7A)),
        "f12" => Ok(VIRTUAL_KEY(0x7B)),
        _ => Err(format!("Unsupported hotkey key `{}`", token)),
    }
}

fn apply_hotkey_change(
    state: &State<'_, AppRuntime>,
    hotkey: HotKey,
    binding: ActiveHotkey,
    hotkey_label: &str,
) -> Result<(), String> {
    let mut registered_hotkey = state
        .registered_hotkey
        .lock()
        .map_err(|_| "Active hotkey lock is unavailable".to_string())?;

    if registered_hotkey.id() == hotkey.id() {
        return Ok(());
    }

    let hotkey_manager = state
        .hotkey_manager
        .lock()
        .map_err(|_| "Hotkey manager lock is unavailable".to_string())?;
    hotkey_manager
        .register(hotkey)
        .map_err(|e| system::format_hotkey_registration_error(hotkey_label, e))?;

    if let Err(e) = hotkey_manager.unregister(*registered_hotkey) {
        let _ = hotkey_manager.unregister(hotkey);
        return Err(format!("Failed to unregister previous hotkey: {}", e));
    }

    *registered_hotkey = hotkey;
    *state
        .active_binding
        .lock()
        .map_err(|_| "Active hotkey binding lock is unavailable".to_string())? = binding;
    Ok(())
}

fn normalize_hotkey(value: &str) -> String {
    value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "win" | "windows" | "cmd" | "command" | "meta" => "Super".to_string(),
            "control" => "Ctrl".to_string(),
            "option" => "Alt".to_string(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}
