use crate::core::audio::AudioCapturer;
use crate::core::history::{TranscriptionEntry, TranscriptionStore};
use crate::core::injection::TextInjector;
use crate::core::settings::{app_data_dir, ModelSize};
use crate::core::worker::{NativeWorker, WorkerKind};
use crate::settings::ActiveHotkey;

use global_hotkey::GlobalHotKeyEvent;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

const MIN_REFINEMENT_WORDS: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "message")]
pub enum EngineState {
    Idle,
    Listening,
    Transcribing,
    Refining,
    Inserting,
    Error(String),
}

pub struct CoreEngine {
    audio: Arc<Mutex<AudioCapturer>>,
    stt_worker: Arc<Mutex<NativeWorker>>,
    refinement_worker: Arc<Mutex<NativeWorker>>,
    app_handle: AppHandle,
    active_hotkey: Arc<Mutex<ActiveHotkey>>,
    shutdown_requested: Arc<AtomicBool>,
}

impl CoreEngine {
    pub fn new(
        app_handle: AppHandle,
        active_hotkey: Arc<Mutex<ActiveHotkey>>,
        shutdown_requested: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        Ok(Self {
            audio: Arc::new(Mutex::new(AudioCapturer::new())),
            stt_worker: Arc::new(Mutex::new(NativeWorker::new(WorkerKind::Stt))),
            refinement_worker: Arc::new(Mutex::new(NativeWorker::new(WorkerKind::Refinement))),
            app_handle,
            active_hotkey,
            shutdown_requested,
        })
    }

    fn emit_state(&self, state: EngineState) {
        if let Err(e) = self.app_handle.emit("engine-state", &state) {
            eprintln!("Failed to emit engine state: {}", e);
        }
    }

    fn save_transcription_history(
        &self,
        text: &str,
        duration_secs: i64,
    ) -> Result<TranscriptionEntry, String> {
        let app_dir = app_data_dir()?;
        let store = TranscriptionStore::new(&app_dir)?;
        let entry = store.add(text, duration_secs)?;
        let _ = self.app_handle.emit("transcription-created", &entry);
        Ok(entry)
    }

    fn transcribe_safely(&self, audio_data: &[f32]) -> Result<String, String> {
        let mut worker = self
            .stt_worker
            .lock()
            .map_err(|_| "STT worker lock is unavailable".to_string())?;
        worker.transcribe(audio_data)
    }

    fn clean_safely(&self, raw_text: String) -> String {
        if !Self::should_refine(&raw_text) {
            return Self::light_cleanup(raw_text);
        }

        let fallback = raw_text.clone();
        let Ok(mut worker) = self.refinement_worker.lock() else {
            eprintln!("Refinement worker lock is unavailable; using raw transcript.");
            return Self::light_cleanup(fallback);
        };

        match worker.refine(raw_text) {
            Ok(cleaned) => cleaned,
            Err(e) => {
                eprintln!("Transcript refinement failed; using raw transcript: {}", e);
                Self::light_cleanup(fallback)
            }
        }
    }

    fn should_refine(raw_text: &str) -> bool {
        raw_text.split_whitespace().count() >= MIN_REFINEMENT_WORDS
    }

    fn light_cleanup(raw_text: String) -> String {
        raw_text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn successful_text_states() -> [EngineState; 2] {
        [EngineState::Refining, EngineState::Inserting]
    }

    fn key_is_down(key: VIRTUAL_KEY) -> bool {
        unsafe { (GetAsyncKeyState(key.0 as i32) as u16 & 0x8000) != 0 }
    }

    fn hotkey_is_down(hotkey: ActiveHotkey) -> bool {
        let modifiers = hotkey.modifiers;
        Self::key_is_down(hotkey.key)
            && (!modifiers.control || Self::key_is_down(VK_CONTROL))
            && (!modifiers.alt || Self::key_is_down(VK_MENU))
            && (!modifiers.shift || Self::key_is_down(VK_SHIFT))
            && (!modifiers.super_key || Self::key_is_down(VK_LWIN) || Self::key_is_down(VK_RWIN))
    }

    fn foreground_window_id() -> isize {
        unsafe { GetForegroundWindow().0 }
    }

    fn foreground_window_matches(captured_window: isize) -> bool {
        captured_window == 0 || Self::foreground_window_id() == captured_window
    }

    fn wait_for_hotkey_release(&self, hotkey: ActiveHotkey) -> bool {
        thread::sleep(Duration::from_millis(25));
        while Self::hotkey_is_down(hotkey) {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        true
    }

    fn cancel_recording(&self) {
        if let Ok(mut audio) = self.audio.lock() {
            audio.stop();
            audio.clear();
        }
        self.emit_state(EngineState::Idle);
    }

    fn start_recording(&self) -> bool {
        let Ok(mut audio) = self.audio.lock() else {
            self.emit_state(EngineState::Error(
                "Audio recorder lock is unavailable".to_string(),
            ));
            return false;
        };

        match audio.start() {
            Ok(_) => {
                self.emit_state(EngineState::Listening);
                true
            }
            Err(e) => {
                self.emit_state(EngineState::Error(format!(
                    "Microphone access failed: {}",
                    e
                )));
                thread::sleep(Duration::from_secs(2));
                self.emit_state(EngineState::Idle);
                false
            }
        }
    }

    fn finish_recording(&self, target_window: isize) {
        let Ok(mut audio) = self.audio.lock() else {
            self.emit_state(EngineState::Error(
                "Audio recorder lock is unavailable".to_string(),
            ));
            return;
        };

        audio.stop();
        let audio_data = audio.get_resampled_audio();
        let duration_secs = (audio_data.len() as f64 / 16_000.0).round() as i64;
        audio.clear();
        drop(audio);

        if audio_data.is_empty() {
            self.emit_state(EngineState::Idle);
            return;
        }

        self.emit_state(EngineState::Transcribing);
        match self.transcribe_safely(&audio_data) {
            Ok(text) => {
                let [cleaning_state, inserting_state] = Self::successful_text_states();
                self.emit_state(cleaning_state);
                let cleaned_text = self.clean_safely(text);

                self.emit_state(inserting_state);
                if !Self::foreground_window_matches(target_window) {
                    self.emit_state(EngineState::Error(
                        "Active window changed before insertion; dictation was cancelled."
                            .to_string(),
                    ));
                    thread::sleep(Duration::from_secs(2));
                    return;
                }

                if let Err(e) = TextInjector::inject(cleaned_text.clone()) {
                    self.emit_state(EngineState::Error(format!("Text injection failed: {}", e)));
                    thread::sleep(Duration::from_secs(2));
                } else if let Err(e) =
                    self.save_transcription_history(&cleaned_text, duration_secs)
                {
                    eprintln!("Failed to save transcription history: {}", e);
                }
            }
            Err(e) => {
                self.emit_state(EngineState::Error(format!("Transcription failed: {}", e)));
                thread::sleep(Duration::from_secs(2));
            }
        }

        self.emit_state(EngineState::Idle);
    }

    pub fn swap_model(&self, size: ModelSize) -> Result<(), String> {
        let mut worker = self
            .stt_worker
            .lock()
            .map_err(|_| "STT worker lock is unavailable".to_string())?;
        worker.swap_model(size).map(|_| ())
    }
    pub fn run(&self) {
        let global_hotkey_channel = GlobalHotKeyEvent::receiver();

        // Emit initial idle state
        self.emit_state(EngineState::Idle);

        while !self.shutdown_requested.load(Ordering::SeqCst) {
            if let Ok(event) = global_hotkey_channel.recv_timeout(Duration::from_millis(100)) {
                let active_hotkey = self
                    .active_hotkey
                    .lock()
                    .map(|hotkey| *hotkey)
                    .unwrap_or(ActiveHotkey::default());
                let target_window = Self::foreground_window_id();
                if event.id == active_hotkey.id && self.start_recording() {
                    if self.wait_for_hotkey_release(active_hotkey) {
                        self.finish_recording(target_window);
                    } else {
                        self.cancel_recording();
                    }
                }
            }
        }
    }
}

impl Drop for CoreEngine {
    fn drop(&mut self) {
        if let Ok(mut audio) = self.audio.lock() {
            audio.stop();
            audio.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreEngine, EngineState};

    #[test]
    fn successful_text_flow_emits_cleaning_before_inserting() {
        assert_eq!(
            CoreEngine::successful_text_states(),
            [EngineState::Refining, EngineState::Inserting]
        );
    }

    #[test]
    fn short_transcripts_skip_refinement_worker() {
        assert!(!CoreEngine::should_refine("turn this into a quick note"));
        assert!(CoreEngine::should_refine(
            "this transcript has enough spoken words to justify running the local refinement model before insertion now"
        ));
    }

    #[test]
    fn light_cleanup_collapses_spacing_without_rewriting_words() {
        assert_eq!(
            CoreEngine::light_cleanup("  keep   the same words \n please ".to_string()),
            "keep the same words please"
        );
    }
}
