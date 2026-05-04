use crate::core::audio::AudioCapturer;
use crate::core::stt::BasicTranscriber;
use crate::core::injection::TextInjector;

use global_hotkey::GlobalHotKeyEvent;
use serde::{Deserialize, Serialize};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    stt: Arc<BasicTranscriber>,
    app_handle: AppHandle,
    hotkey_id: u32,
}

impl CoreEngine {
    pub fn new(app_handle: AppHandle, hotkey_id: u32) -> Result<Self, String> {
        Ok(Self {
            audio: Arc::new(Mutex::new(AudioCapturer::new())),
            stt: Arc::new(BasicTranscriber::new()?),
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
        match panic::catch_unwind(AssertUnwindSafe(|| self.stt.transcribe(audio_data))) {
            Ok(result) => result,
            Err(_) => Err("STT engine panicked during transcription".to_string()),
        }
    }

    pub fn run(&self) {
        let global_hotkey_channel = GlobalHotKeyEvent::receiver();

        let mut is_recording = false;

        // Emit initial idle state
        self.emit_state(EngineState::Idle);

        loop {
            if let Ok(event) = global_hotkey_channel.try_recv() {
                if event.id == self.hotkey_id {
                    match event.state {
                        global_hotkey::HotKeyState::Pressed => {
                            if !is_recording {
                                let Ok(mut audio) = self.audio.lock() else {
                                    self.emit_state(EngineState::Error(
                                        "Audio recorder lock is unavailable".to_string(),
                                    ));
                                    continue;
                                };
                                match audio.start() {
                                    Ok(_) => {
                                        is_recording = true;
                                        self.emit_state(EngineState::Recording);
                                    }
                                    Err(e) => {
                                        self.emit_state(EngineState::Error(
                                            format!("Microphone access failed: {}", e),
                                        ));
                                        // Return to idle after error
                                        thread::sleep(Duration::from_secs(2));
                                        self.emit_state(EngineState::Idle);
                                    }
                                }
                            }
                        }
                        global_hotkey::HotKeyState::Released => {
                            if is_recording {
                                let Ok(mut audio) = self.audio.lock() else {
                                    is_recording = false;
                                    self.emit_state(EngineState::Error(
                                        "Audio recorder lock is unavailable".to_string(),
                                    ));
                                    continue;
                                };
                                audio.stop();
                                is_recording = false;

                                let audio_data = audio.get_resampled_audio();
                                if !audio_data.is_empty() {
                                    self.emit_state(EngineState::Transcribing);

                                    match self.transcribe_safely(&audio_data) {
                                        Ok(text) => {
                                            self.emit_state(EngineState::Inserting);
                                            if let Err(e) = TextInjector::inject(text) {
                                                self.emit_state(EngineState::Error(
                                                    format!("Text injection failed: {}", e),
                                                ));
                                                thread::sleep(Duration::from_secs(2));
                                            }
                                        }
                                        Err(e) => {
                                            self.emit_state(EngineState::Error(
                                                format!("Transcription failed: {}", e),
                                            ));
                                            thread::sleep(Duration::from_secs(2));
                                        }
                                    }
                                }
                                audio.clear();
                                self.emit_state(EngineState::Idle);
                            }
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
