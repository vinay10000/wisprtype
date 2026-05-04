pub mod core;

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};
use std::thread;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if core::worker::run_if_requested() {
        return;
    }

    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let hotkey = HotKey::new(Some(Modifiers::SUPER), Code::Space);
            let hotkey_id = hotkey.id();
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
                        "Failed to register Win+Space hotkey: {}",
                        e
                    )),
                );
                return Ok(());
            }

            app.manage(hotkey_manager);

            if let Some(pill) = app.get_webview_window("pill") {
                if let Err(e) = pill.set_ignore_cursor_events(true) {
                    eprintln!("Failed to configure pill click-through: {}", e);
                }
            }

            // Spawn CoreEngine thread with app handle for event emission
            let handle = app.handle().clone();
            let event_handle = handle.clone();
            thread::spawn(
                move || match core::engine::CoreEngine::new(handle, hotkey_id) {
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
                },
            );

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
