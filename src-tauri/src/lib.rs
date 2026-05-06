pub mod core;
mod settings;
mod system;

use core::dictionary::DictionaryStore;
use core::history::TranscriptionStore;
use core::settings::{app_data_dir, SettingsStore};
use global_hotkey::GlobalHotKeyManager;
use settings::AppRuntime;
use std::thread;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::save_settings,
            settings::list_dictionary_terms,
            settings::add_dictionary_term,
            settings::remove_dictionary_term,
            settings::list_transcriptions,
            settings::get_transcription,
            settings::delete_transcription,
            settings::reinsert_transcription,
            settings::cloud_api_key_status,
            settings::store_cloud_api_key,
            settings::delete_cloud_api_key,
            system::run_validation_checks
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let app_dir = match app_data_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    let _ = app.handle().emit(
                        "engine-state",
                        core::engine::EngineState::Error(format!(
                            "Failed to initialize app data directory: {}",
                            e
                        )),
                    );
                    return Ok(());
                }
            };
            let settings_store = SettingsStore::new(&app_dir);
            let settings = settings_store.load().unwrap_or_else(|e| {
                eprintln!("Failed to load settings; using defaults: {}", e);
                core::settings::AppSettings::default()
            });
            if let Err(e) = system::sync_launch_at_login_setting(&settings) {
                eprintln!("Failed to synchronize launch-at-login: {}", e);
                let _ = app.handle().emit(
                    "engine-state",
                    core::engine::EngineState::Error(format!(
                        "Failed to configure launch-at-login: {}",
                        e
                    )),
                );
            }
            let dictionary_store = match DictionaryStore::new(&app_dir) {
                Ok(store) => store,
                Err(e) => {
                    let _ = app.handle().emit(
                        "engine-state",
                        core::engine::EngineState::Error(format!(
                            "Failed to initialize dictionary: {}",
                            e
                        )),
                    );
                    return Ok(());
                }
            };
            let transcription_store = match TranscriptionStore::new(&app_dir) {
                Ok(store) => store,
                Err(e) => {
                    let _ = app.handle().emit(
                        "engine-state",
                        core::engine::EngineState::Error(format!(
                            "Failed to initialize transcription history: {}",
                            e
                        )),
                    );
                    return Ok(());
                }
            };

            let (hotkey, hotkey_source) = match settings::parse_hotkey(&settings.hotkey) {
                Ok(hotkey) => (hotkey, settings.hotkey.as_str()),
                Err(e) => {
                    eprintln!("Invalid saved hotkey; falling back to Super+Space: {}", e);
                    (
                        settings::parse_hotkey("Super+Space").expect("default hotkey parses"),
                        "Super+Space",
                    )
                }
            };
            let active_binding =
                settings::parse_hotkey_binding(hotkey_source, hotkey.id()).unwrap_or_default();
            let hotkey_manager = match GlobalHotKeyManager::new() {
                Ok(manager) => manager,
                Err(e) => {
                    let _ = app.handle().emit(
                        "engine-state",
                        core::engine::EngineState::Error(format!(
                            "Failed to initialize global hotkey manager: {}",
                            e
                        )),
                    );
                    return Ok(());
                }
            };

            if let Err(e) = hotkey_manager.register(hotkey) {
                let _ = app.handle().emit(
                    "engine-state",
                    core::engine::EngineState::Error(format!(
                        "{}",
                        system::format_hotkey_registration_error(hotkey_source, e)
                    )),
                );
                return Ok(());
            }

            let mut runtime = AppRuntime::new(
                settings_store,
                dictionary_store,
                transcription_store,
                hotkey_manager,
                hotkey,
                active_binding,
            );
            let active_hotkey = runtime.active_binding.clone();
            let shutdown_requested = runtime.shutdown_requested.clone();

            if let Some(pill) = app.get_webview_window("pill") {
                if let Err(e) = pill.set_ignore_cursor_events(true) {
                    eprintln!("Failed to configure pill click-through: {}", e);
                }
            }
            settings::apply_pill_settings(app.handle(), &settings);
            if let Err(e) = system::install_tray(app.handle()) {
                let _ = app.handle().emit(
                    "engine-state",
                    core::engine::EngineState::Error(format!(
                        "Failed to initialize system tray: {}",
                        e
                    )),
                );
                return Ok(());
            }

            // Spawn CoreEngine thread with app handle for event emission
            let handle = app.handle().clone();
            let event_handle = handle.clone();
            let engine_thread = thread::spawn(move || {
                match core::engine::CoreEngine::new(handle, active_hotkey, shutdown_requested) {
                    Ok(engine) => engine.run(),
                    Err(e) => {
                        eprintln!("Failed to initialize CoreEngine: {}", e);
                        let _ = event_handle.emit(
                            "engine-state",
                            core::engine::EngineState::Error(format!(
                                "Engine initialization failed: {}",
                                e
                            )),
                        );
                    }
                }
            });
            runtime.track_engine_thread(engine_thread);
            app.manage(runtime);

            Ok(())
        })
        .on_window_event(|window, event| {
            system::handle_window_event(window.label(), event, &window.app_handle());
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                if let Some(runtime) = app.try_state::<AppRuntime>() {
                    runtime.cleanup();
                }
            }
        });
}
