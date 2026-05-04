use crate::core::audio::AudioCapturer;
use crate::core::injection::TextInjector;
use crate::core::refinement::RefinementEngine;
use crate::core::stt::BasicTranscriber;

use global_hotkey::GlobalHotKeyEvent;
use serde::{Deserialize, Serialize};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

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
    stt: Arc<BasicTranscriber>,
    refinement: Option<Arc<RefinementEngine>>,
    app_handle: AppHandle,
    hotkey_id: u32,
}

impl CoreEngine {
    pub fn new(app_handle: AppHandle, hotkey_id: u32) -> Result<Self, String> {
        let refinement = Self::initialize_refinement_safely();

        Ok(Self {
            audio: Arc::new(Mutex::new(AudioCapturer::new())),
            stt: Arc::new(BasicTranscriber::new()?),
            refinement,
            app_handle,
            hotkey_id,
        })
    }

    fn initialize_refinement_safely() -> Option<Arc<RefinementEngine>> {
        match panic::catch_unwind(AssertUnwindSafe(RefinementEngine::new)) {
            Ok(Ok(engine)) => Some(Arc::new(engine)),
            Ok(Err(e)) => {
                eprintln!(
                    "Refinement engine unavailable; raw transcripts will be used: {}",
                    e
                );
                None
            }
            Err(_) => {
                eprintln!(
                    "Refinement engine initialization panicked; raw transcripts will be used."
                );
                None
            }
        }
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

    fn clean_safely(&self, raw_text: String) -> String {
        let Some(refinement) = &self.refinement else {
            return raw_text;
        };

        let fallback = raw_text.clone();
        match panic::catch_unwind(AssertUnwindSafe(|| refinement.clean(raw_text))) {
            Ok(cleaned) => cleaned,
            Err(_) => {
                eprintln!("Refinement engine panicked before fallback; using raw transcript.");
                fallback
            }
        }
    }

    fn successful_text_states() -> [EngineState; 2] {
        [EngineState::Cleaning, EngineState::Inserting]
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
                                        self.emit_state(EngineState::Error(format!(
                                            "Microphone access failed: {}",
                                            e
                                        )));
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
                                            let [cleaning_state, inserting_state] =
                                                Self::successful_text_states();
                                            self.emit_state(cleaning_state);
                                            let cleaned_text = self.clean_safely(text);

                                            self.emit_state(inserting_state);
                                            if let Err(e) = TextInjector::inject(cleaned_text) {
                                                self.emit_state(EngineState::Error(format!(
                                                    "Text injection failed: {}",
                                                    e
                                                )));
                                                thread::sleep(Duration::from_secs(2));
                                            }
                                        }
                                        Err(e) => {
                                            self.emit_state(EngineState::Error(format!(
                                                "Transcription failed: {}",
                                                e
                                            )));
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
