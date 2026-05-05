use crate::core::cloud::{CloudCredentials, CloudProviderKind};
use crate::core::dictionary::DictionaryTerm;
use crate::core::settings::AppSettings;
use crate::settings::{apply_pill_settings, AppRuntime};
use global_hotkey::Error as HotkeyError;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};

const RUN_KEY_PATH: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE_NAME: &str = "wisprflow";
const TRAY_ID: &str = "wisprflow-tray";
const MENU_OPEN_SETTINGS: &str = "open-settings";
const MENU_TOGGLE_PILL: &str = "toggle-pill";
const MENU_QUIT: &str = "quit";
const AUDIO_ARTIFACT_EXTENSIONS: &[&str] = &["wav", "mp3", "m4a", "flac", "ogg", "pcm", "raw"];

#[derive(Debug, Clone, Serialize)]
pub struct ValidationCheck {
    pub id: String,
    pub label: String,
    pub status: ValidationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub checks: Vec<ValidationCheck>,
}

pub fn install_tray(app: &AppHandle) -> Result<(), String> {
    let tray_menu = MenuBuilder::new(app)
        .text(MENU_OPEN_SETTINGS, "Open Settings")
        .text(MENU_TOGGLE_PILL, "Toggle Aurora Pill")
        .separator()
        .text(MENU_QUIT, "Quit wisprflow")
        .build()
        .map_err(|e| format!("Failed to build tray menu: {}", e))?;

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id() == MENU_OPEN_SETTINGS {
                let _ = show_main_window(app);
            } else if event.id() == MENU_TOGGLE_PILL {
                let _ = toggle_pill_visibility(app);
            } else if event.id() == MENU_QUIT {
                if let Some(runtime) = app.try_state::<AppRuntime>() {
                    runtime.request_exit();
                }
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    tray_builder
        .build(app)
        .map(|_| ())
        .map_err(|e| format!("Failed to create tray icon: {}", e))
}

pub fn handle_window_event(window_label: &str, event: &WindowEvent, app: &AppHandle) {
    if window_label != "main" {
        return;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        let allow_close = app
            .try_state::<AppRuntime>()
            .map(|runtime| runtime.allow_window_close())
            .unwrap_or(false);

        if !allow_close {
            api.prevent_close();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
    }
}

pub fn sync_launch_at_login_setting(settings: &AppSettings) -> Result<(), String> {
    let current_exe = env::current_exe().map_err(|e| {
        format!(
            "Failed to resolve current executable for launch-at-login: {}",
            e
        )
    })?;
    let command = startup_command_for(&current_exe);

    if settings.launch_at_login {
        set_run_value(RUN_VALUE_NAME, &command)
    } else {
        delete_run_value(RUN_VALUE_NAME)
    }
}

pub fn format_hotkey_registration_error(hotkey_label: &str, error: HotkeyError) -> String {
    match error {
        HotkeyError::AlreadyRegistered(_) => format!(
            "Hotkey `{}` is already reserved by another application. Choose a different binding.",
            hotkey_label
        ),
        other => format!("Failed to register hotkey `{}`: {}", hotkey_label, other),
    }
}

#[tauri::command]
pub fn run_validation_checks(
    app: AppHandle,
    state: State<'_, AppRuntime>,
) -> Result<ValidationReport, String> {
    let settings = state
        .settings_store
        .lock()
        .map_err(|_| "Settings store lock is unavailable".to_string())?
        .load()?;
    let dictionary_terms = state
        .dictionary_store
        .lock()
        .map_err(|_| "Dictionary store lock is unavailable".to_string())?
        .list_terms()?;

    Ok(ValidationReport {
        checks: vec![
            check_launch_at_login(&settings),
            check_tray_ready(&app),
            check_pill_window(&app),
            check_worker_binaries(),
            check_dictionary_seed(&dictionary_terms),
            check_cloud_provider_status(&settings),
            check_audio_disk_hygiene(),
        ],
    })
}

fn toggle_pill_visibility(app: &AppHandle) -> Result<(), String> {
    let runtime = app
        .try_state::<AppRuntime>()
        .ok_or_else(|| "Application runtime is unavailable".to_string())?;

    let settings_store = runtime
        .settings_store
        .lock()
        .map_err(|_| "Settings store lock is unavailable".to_string())?;
    let mut settings = settings_store.load()?;
    settings.pill_visible = !settings.pill_visible;
    settings_store.persist(&settings)?;
    drop(settings_store);

    apply_pill_settings(app, &settings);
    Ok(())
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

fn check_launch_at_login(settings: &AppSettings) -> ValidationCheck {
    match query_run_value(RUN_VALUE_NAME) {
        Ok(Some(value)) if settings.launch_at_login => {
            let expected = env::current_exe()
                .ok()
                .map(|exe| startup_command_for(&exe))
                .unwrap_or_default();
            let matches = expected.is_empty() || value.contains(expected.trim_matches('"'));
            ValidationCheck {
                id: "launch-at-login".to_string(),
                label: "Launch at login".to_string(),
                status: if matches {
                    ValidationStatus::Pass
                } else {
                    ValidationStatus::Warn
                },
                detail: if matches {
                    "Registry entry is present for the current app binary.".to_string()
                } else {
                    "Registry entry exists, but it does not match the current executable path."
                        .to_string()
                },
            }
        }
        Ok(None) if !settings.launch_at_login => ValidationCheck {
            id: "launch-at-login".to_string(),
            label: "Launch at login".to_string(),
            status: ValidationStatus::Pass,
            detail: "Launch-at-login is disabled and no startup entry is registered.".to_string(),
        },
        Ok(Some(_)) => ValidationCheck {
            id: "launch-at-login".to_string(),
            label: "Launch at login".to_string(),
            status: ValidationStatus::Fail,
            detail: "Settings disable launch-at-login, but a startup registry entry still exists."
                .to_string(),
        },
        Ok(None) => ValidationCheck {
            id: "launch-at-login".to_string(),
            label: "Launch at login".to_string(),
            status: ValidationStatus::Fail,
            detail: "Launch-at-login is enabled, but the startup registry entry is missing."
                .to_string(),
        },
        Err(e) => ValidationCheck {
            id: "launch-at-login".to_string(),
            label: "Launch at login".to_string(),
            status: ValidationStatus::Warn,
            detail: format!("Startup registry could not be queried: {}", e),
        },
    }
}

fn check_tray_ready(app: &AppHandle) -> ValidationCheck {
    let tray_ready = app.tray_by_id(TRAY_ID).is_some();
    ValidationCheck {
        id: "tray".to_string(),
        label: "System tray".to_string(),
        status: if tray_ready {
            ValidationStatus::Pass
        } else {
            ValidationStatus::Fail
        },
        detail: if tray_ready {
            "Tray icon is registered and available for background control.".to_string()
        } else {
            "Tray icon is missing, so background controls are not available.".to_string()
        },
    }
}

fn check_pill_window(app: &AppHandle) -> ValidationCheck {
    let pill_ready = app.get_webview_window("pill").is_some();
    ValidationCheck {
        id: "pill-window".to_string(),
        label: "Aurora pill window".to_string(),
        status: if pill_ready {
            ValidationStatus::Pass
        } else {
            ValidationStatus::Fail
        },
        detail: if pill_ready {
            "The pill overlay window is available for state-driven UI feedback.".to_string()
        } else {
            "The pill overlay window could not be found.".to_string()
        },
    }
}

fn check_worker_binaries() -> ValidationCheck {
    match worker_binary_paths() {
        Ok(paths) => {
            let missing = paths
                .into_iter()
                .filter(|path| !path.exists())
                .collect::<Vec<_>>();
            if missing.is_empty() {
                ValidationCheck {
                    id: "workers".to_string(),
                    label: "Native workers".to_string(),
                    status: ValidationStatus::Pass,
                    detail: "Speech and refinement worker executables are present next to the app."
                        .to_string(),
                }
            } else {
                ValidationCheck {
                    id: "workers".to_string(),
                    label: "Native workers".to_string(),
                    status: ValidationStatus::Warn,
                    detail: format!(
                        "Missing worker binaries: {}",
                        missing
                            .into_iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }
            }
        }
        Err(e) => ValidationCheck {
            id: "workers".to_string(),
            label: "Native workers".to_string(),
            status: ValidationStatus::Warn,
            detail: e,
        },
    }
}

fn check_dictionary_seed(dictionary_terms: &[DictionaryTerm]) -> ValidationCheck {
    ValidationCheck {
        id: "dictionary".to_string(),
        label: "Dictionary biasing".to_string(),
        status: if dictionary_terms.is_empty() {
            ValidationStatus::Warn
        } else {
            ValidationStatus::Pass
        },
        detail: if dictionary_terms.is_empty() {
            "No dictionary terms exist yet; add at least one domain term before manual QA."
                .to_string()
        } else {
            format!(
                "{} dictionary term(s) are available for manual bias-validation.",
                dictionary_terms.len()
            )
        },
    }
}

fn check_cloud_provider_status(settings: &AppSettings) -> ValidationCheck {
    let provider = match settings.cloud_provider {
        crate::core::settings::CloudProvider::Gladia => "gladia",
        crate::core::settings::CloudProvider::OpenAi => "openai",
        crate::core::settings::CloudProvider::Groq => "groq",
        crate::core::settings::CloudProvider::Deepgram => "deepgram",
    };
    match CloudCredentials::read_api_key(CloudProviderKind::parse(&provider)) {
        Ok(Some(_)) => ValidationCheck {
            id: "cloud-key".to_string(),
            label: "Cloud provider key".to_string(),
            status: ValidationStatus::Pass,
            detail: format!(
                "A {} API key is available in Windows Credential Manager.",
                provider
            ),
        },
        Ok(None) => ValidationCheck {
            id: "cloud-key".to_string(),
            label: "Cloud provider key".to_string(),
            status: ValidationStatus::Warn,
            detail: format!(
                "No {} API key is configured yet; cloud-engine QA will need one.",
                provider
            ),
        },
        Err(e) => ValidationCheck {
            id: "cloud-key".to_string(),
            label: "Cloud provider key".to_string(),
            status: ValidationStatus::Warn,
            detail: format!("Credential lookup failed: {}", e),
        },
    }
}

fn check_audio_disk_hygiene() -> ValidationCheck {
    match crate::core::settings::app_data_dir() {
        Ok(app_dir) => match collect_audio_artifacts(&app_dir) {
            Ok(paths) if paths.is_empty() => ValidationCheck {
                id: "audio-disk".to_string(),
                label: "Audio disk hygiene".to_string(),
                status: ValidationStatus::Pass,
                detail: "No audio artifacts were found in the wisprflow app data directory."
                    .to_string(),
            },
            Ok(paths) => ValidationCheck {
                id: "audio-disk".to_string(),
                label: "Audio disk hygiene".to_string(),
                status: ValidationStatus::Fail,
                detail: format!(
                    "Potential audio artifacts detected: {}",
                    paths
                        .into_iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Err(e) => ValidationCheck {
                id: "audio-disk".to_string(),
                label: "Audio disk hygiene".to_string(),
                status: ValidationStatus::Warn,
                detail: format!("Failed to scan app data directory: {}", e),
            },
        },
        Err(e) => ValidationCheck {
            id: "audio-disk".to_string(),
            label: "Audio disk hygiene".to_string(),
            status: ValidationStatus::Warn,
            detail: format!("App data directory is unavailable: {}", e),
        },
    }
}

fn collect_audio_artifacts(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut stack = vec![root.to_path_buf()];
    let mut matches = Vec::new();

    while let Some(path) = stack.pop() {
        let entries = fs::read_dir(&path).map_err(|e| e.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase());
            if extension
                .as_deref()
                .map(|extension| AUDIO_ARTIFACT_EXTENSIONS.contains(&extension))
                .unwrap_or(false)
            {
                matches.push(path);
            }
        }
    }

    Ok(matches)
}

fn worker_binary_paths() -> Result<[PathBuf; 2], String> {
    let current_exe =
        env::current_exe().map_err(|e| format!("Failed to locate current executable: {}", e))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| "Current executable has no parent directory".to_string())?;
    Ok([
        parent.join(worker_binary_name("wisprtype-stt-worker")),
        parent.join(worker_binary_name("wisprtype-refinement-worker")),
    ])
}

fn worker_binary_name(base: &str) -> String {
    #[cfg(windows)]
    {
        format!("{base}.exe")
    }

    #[cfg(not(windows))]
    {
        base.to_string()
    }
}

fn set_run_value(name: &str, value: &str) -> Result<(), String> {
    run_reg_command(&[
        "add",
        RUN_KEY_PATH,
        "/v",
        name,
        "/t",
        "REG_SZ",
        "/d",
        value,
        "/f",
    ])
    .map(|_| ())
}

fn delete_run_value(name: &str) -> Result<(), String> {
    match run_reg_command(&["delete", RUN_KEY_PATH, "/v", name, "/f"]) {
        Ok(_) => Ok(()),
        Err(e)
            if e.to_ascii_lowercase().contains("unable to find")
                || e.to_ascii_lowercase().contains("was unable to find") =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn query_run_value(name: &str) -> Result<Option<String>, String> {
    let output = Command::new("reg")
        .args(["query", RUN_KEY_PATH, "/v", name])
        .output()
        .map_err(|e| format!("Failed to query startup registry: {}", e))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains(name) {
            let value = line
                .split_once("REG_SZ")
                .map(|(_, value)| value.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            if value.is_some() {
                return Ok(value);
            }
        }
    }

    Ok(None)
}

fn run_reg_command(args: &[&str]) -> Result<String, String> {
    let output = Command::new("reg")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run `reg {}`: {}", args.join(" "), e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

fn startup_command_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(test)]
mod tests {
    use super::{format_hotkey_registration_error, startup_command_for};
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::Error as HotkeyError;
    use std::path::Path;

    #[test]
    fn launch_command_quotes_paths_with_spaces() {
        let command = startup_command_for(Path::new(r"C:\Program Files\wisprflow\app.exe"));
        assert_eq!(command, r#""C:\Program Files\wisprflow\app.exe""#);
    }

    #[test]
    fn hotkey_conflicts_are_reported_clearly() {
        let hotkey = HotKey::new(Some(Modifiers::SUPER), Code::Space);
        let message =
            format_hotkey_registration_error("Super+Space", HotkeyError::AlreadyRegistered(hotkey));
        assert!(message.contains("already reserved"));
        assert!(message.contains("Super+Space"));
    }
}
