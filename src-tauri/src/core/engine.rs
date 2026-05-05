use crate::core::audio::AudioCapturer;
use crate::core::injection::TextInjector;
use crate::core::stt::ModelSize;
use crate::core::worker::{NativeWorker, WorkerKind};

use global_hotkey::GlobalHotKeyEvent;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_LWIN, VK_RWIN, VK_SPACE,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "message")]
pub enum EngineState {
    Idle,
    Recording,
    Transcribing,
    Cleaning,
    Inserting,
    Error(String),
}

pub struct CoreEngine {
    audio: Arc<Mutex<AudioCapturer>>,
    stt_worker: Arc<Mutex<NativeWorker>>,
    refinement_worker: Arc<Mutex<NativeWorker>>,
    app_handle: AppHandle,
    hotkey_id: u32,
}

impl CoreEngine {
    pub fn new(app_handle: AppHandle, hotkey_id: u32) -> Result<Self, String> {
        Ok(Self {
            audio: Arc::new(Mutex::new(AudioCapturer::new())),
            stt_worker: Arc::new(Mutex::new(NativeWorker::new(WorkerKind::Stt))),
            refinement_worker: Arc::new(Mutex::new(NativeWorker::new(WorkerKind::Refinement))),
            app_handle,
            hotkey_id,
        })
    }

    fn emit_state(&self, state: EngineState) {
        if let Err(e) = self.app_handle.emit("engine-state", &state) {
            eprintln!("Failed to emit engine state: {}", e);
        }
    }

    fn transcribe_safely(&self, audio_data: &[f32]) -> Result<String, String> {
        let mut worker = self
            .stt_worker
            .lock()
            .map_err(|_| "STT worker lock is unavailable".to_string())?;
        worker.transcribe(audio_data)
    }

    fn clean_safely(&self, raw_text: String) -> String {
        let fallback = raw_text.clone();
        let Ok(mut worker) = self.refinement_worker.lock() else {
            eprintln!("Refinement worker lock is unavailable; using raw transcript.");
            return fallback;
        };

        match worker.refine(raw_text) {
            Ok(cleaned) => cleaned,
            Err(e) => {
                eprintln!("Transcript refinement failed; using raw transcript: {}", e);
                fallback
            }
        }
    }

    fn successful_text_states() -> [EngineState; 2] {
        [EngineState::Cleaning, EngineState::Inserting]
    }

    fn key_is_down(key: VIRTUAL_KEY) -> bool {
        unsafe { (GetAsyncKeyState(key.0 as i32) as u16 & 0x8000) != 0 }
    }

    fn hotkey_is_down() -> bool {
        Self::key_is_down(VK_SPACE) && (Self::key_is_down(VK_LWIN) || Self::key_is_down(VK_RWIN))
    }

    fn wait_for_hotkey_release() {
        thread::sleep(Duration::from_millis(25));
        while Self::hotkey_is_down() {
            thread::sleep(Duration::from_millis(10));
        }
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
                self.emit_state(EngineState::Recording);
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

    fn finish_recording(&self) {
        let Ok(mut audio) = self.audio.lock() else {
            self.emit_state(EngineState::Error(
                "Audio recorder lock is unavailable".to_string(),
            ));
            return;
        };

        audio.stop();
        let audio_data = audio.get_resampled_audio();
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
                if let Err(e) = TextInjector::inject(cleaned_text) {
                    self.emit_state(EngineState::Error(format!("Text injection failed: {}", e)));
                    thread::sleep(Duration::from_secs(2));
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

        let mut is_recording = false;

        // Emit initial idle state
        self.emit_state(EngineState::Idle);

        loop {
            if let Ok(event) = global_hotkey_channel.try_recv() {
                if event.id == self.hotkey_id && !is_recording {
                    if self.start_recording() {
                        is_recording = true;
                        Self::wait_for_hotkey_release();
                        self.finish_recording();
                        is_recording = false;
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
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
            [EngineState::Cleaning, EngineState::Inserting]
        );
    }
}
