mod backends;
mod model_manager;
mod prompting;

use std::fmt::{Display, Formatter};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;

use crate::core::cloud::{CloudProviderKind, CloudTranscriber};
use crate::core::dictionary::DictionaryStore;
pub use crate::core::settings::ModelSize;
use crate::core::settings::{app_data_dir, AppSettings, SettingsStore, SttEngineMode};

use backends::{CpuBackend, CudaBackend, OpenVinoBackend, SttBackend};
use model_manager::ModelManager;
use prompting::build_initial_prompt;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};

#[derive(Debug)]
pub enum SttError {
    ModelDirResolve(String),
    ModelDownload(String),
    ModelLoad {
        backend: &'static str,
        message: String,
    },
    BackendUnavailable {
        backend: &'static str,
        message: String,
    },
    BackendNotInitialized(&'static str),
    NoBackendAvailable {
        attempts: Vec<String>,
    },
    Inference {
        backend: &'static str,
        message: String,
    },
    Cloud(String),
    SettingsPersist(String),
}

impl Display for SttError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelDirResolve(m) => write!(f, "{}", m),
            Self::ModelDownload(m) => write!(f, "{}", m),
            Self::ModelLoad { backend, message } => {
                write!(
                    f,
                    "Failed to load model with {} backend: {}",
                    backend, message
                )
            }
            Self::BackendUnavailable { backend, message } => {
                write!(f, "{} backend unavailable: {}", backend, message)
            }
            Self::BackendNotInitialized(backend) => {
                write!(f, "{} backend is not initialized", backend)
            }
            Self::NoBackendAvailable { attempts } => {
                write!(
                    f,
                    "No STT backend could be initialized ({})",
                    attempts.join("; ")
                )
            }
            Self::Inference { backend, message } => {
                write!(f, "{} backend inference failed: {}", backend, message)
            }
            Self::Cloud(m) => write!(f, "{}", m),
            Self::SettingsPersist(m) => write!(f, "Failed to persist settings: {}", m),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineMode {
    Local,
    Cloud,
    Auto,
}

impl EngineMode {
    fn from_settings(settings: &AppSettings) -> Self {
        match settings.stt_engine {
            SttEngineMode::Local => Self::Local,
            SttEngineMode::Cloud => Self::Cloud,
            SttEngineMode::Auto => Self::Auto,
        }
    }
}

struct LocalWhisperTranscriber {
    context: WhisperContext,
    backend_name: &'static str,
    model_manager: ModelManager,
}

impl LocalWhisperTranscriber {
    fn new() -> Result<Self, SttError> {
        let model_manager = ModelManager::new().map_err(SttError::ModelDirResolve)?;
        let model_path = model_manager.ensure_model_downloaded()?;
        let (context, backend_name) = Self::load_from_backends(&model_path)?;

        Ok(Self {
            context,
            backend_name,
            model_manager,
        })
    }

    fn load_from_backends(
        model_path: &PathBuf,
    ) -> Result<(WhisperContext, &'static str), SttError> {
        let mut attempts = Vec::new();
        let mut candidates: Vec<Box<dyn SttBackend>> = vec![
            Box::new(CudaBackend::default()),
            Box::new(OpenVinoBackend::default()),
            Box::new(CpuBackend::default()),
        ];

        for mut backend in candidates.drain(..) {
            let capabilities = backend.capabilities();
            let name = capabilities.name;
            if capabilities.accelerated {
                eprintln!("Attempting accelerated STT backend: {}", name);
            }

            match backend
                .initialize(model_path)
                .and_then(|_| backend.create_context(model_path))
            {
                Ok(context) => return Ok((context, name)),
                Err(e) => attempts.push(e.to_string()),
            }
        }

        Err(SttError::NoBackendAvailable { attempts })
    }

    fn swap_model(&mut self, size: ModelSize) -> Result<(), SttError> {
        let model_path = self.model_manager.swap_model(size)?;
        let (new_context, new_backend) = Self::load_from_backends(&model_path)?;
        self.context = new_context;
        self.backend_name = new_backend;
        Ok(())
    }

    fn transcribe(
        &mut self,
        audio_data: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String, SttError> {
        match panic::catch_unwind(AssertUnwindSafe(|| {
            self.transcribe_with_fallback(audio_data, initial_prompt)
        })) {
            Ok(result) => result,
            Err(_) => Err(SttError::Inference {
                backend: self.backend_name,
                message: "Local STT engine panicked".to_string(),
            }),
        }
    }

    fn transcribe_with_fallback(
        &mut self,
        audio_data: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String, SttError> {
        match self.transcribe_once(audio_data, initial_prompt) {
            Ok(text) => Ok(text),
            Err(e) if self.backend_name != "cpu" => {
                let model_path = self.model_manager.active_model_path();
                let mut cpu = CpuBackend::default();
                cpu.initialize(&model_path)?;
                self.context = cpu.create_context(&model_path)?;
                self.backend_name = "cpu";
                self.transcribe_once(audio_data, initial_prompt)
                    .map_err(|fallback_err| SttError::NoBackendAvailable {
                        attempts: vec![e.to_string(), fallback_err.to_string()],
                    })
            }
            Err(e) => Err(e),
        }
    }

    fn transcribe_once(
        &self,
        audio_data: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String, SttError> {
        let mut state = self
            .context
            .create_state()
            .map_err(|e| SttError::Inference {
                backend: self.backend_name,
                message: e.to_string(),
            })?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(prompt) = initial_prompt {
            params.set_initial_prompt(prompt);
        }

        state
            .full(params, audio_data)
            .map_err(|e| SttError::Inference {
                backend: self.backend_name,
                message: e.to_string(),
            })?;

        let mut result = String::new();
        for segment in state.as_iter() {
            result.push_str(
                segment
                    .to_str_lossy()
                    .map_err(|e| SttError::Inference {
                        backend: self.backend_name,
                        message: e.to_string(),
                    })?
                    .as_ref(),
            );
        }

        Ok(result.trim().to_string())
    }
}

pub struct BasicTranscriber {
    settings: AppSettings,
    mode: EngineMode,
    local: Option<LocalWhisperTranscriber>,
    cloud: Option<CloudTranscriber>,
    dictionary_store: Option<DictionaryStore>,
}

impl BasicTranscriber {
    pub fn new() -> Result<Self, SttError> {
        let settings_store = SettingsStore::new_default().map_err(SttError::ModelDirResolve)?;
        let settings = settings_store.load().map_err(SttError::SettingsPersist)?;
        let mode = EngineMode::from_settings(&settings);
        let local = if mode != EngineMode::Cloud {
            match LocalWhisperTranscriber::new() {
                Ok(local) => Some(local),
                Err(e) if mode == EngineMode::Auto => {
                    eprintln!(
                        "Local STT initialization failed; cloud fallback may be used: {}",
                        e
                    );
                    None
                }
                Err(e) => return Err(e),
            }
        } else {
            None
        };

        let cloud = if mode != EngineMode::Local {
            match CloudTranscriber::new(CloudProviderKind::from_settings(settings.cloud_provider)) {
                Ok(cloud) => Some(cloud),
                Err(e) if mode == EngineMode::Auto => {
                    eprintln!("Cloud STT initialization skipped: {}", e);
                    None
                }
                Err(e) => return Err(SttError::Cloud(e)),
            }
        } else {
            None
        };

        if local.is_none() && cloud.is_none() {
            return Err(SttError::NoBackendAvailable {
                attempts: vec!["No local or cloud STT engine is available".to_string()],
            });
        }

        let dictionary_store = app_data_dir()
            .ok()
            .and_then(|dir| DictionaryStore::new(&dir).ok());

        Ok(Self {
            settings,
            mode,
            local,
            cloud,
            dictionary_store,
        })
    }

    fn refresh_settings(&mut self) -> Result<(), SttError> {
        let settings_store = SettingsStore::new_default().map_err(SttError::ModelDirResolve)?;
        let settings = settings_store.load().map_err(SttError::SettingsPersist)?;
        if settings == self.settings {
            return Ok(());
        }

        let mode_changed = EngineMode::from_settings(&settings) != self.mode
            || settings.cloud_provider != self.settings.cloud_provider;

        if mode_changed {
            *self = Self::new()?;
            return Ok(());
        }

        if settings.stt_model_size != self.settings.stt_model_size {
            if let Some(local) = self.local.as_mut() {
                local.swap_model(settings.stt_model_size)?;
            }
        }

        self.settings = settings;
        Ok(())
    }

    pub fn swap_model(&mut self, size: ModelSize) -> Result<(), SttError> {
        self.local
            .as_mut()
            .ok_or_else(|| SttError::BackendNotInitialized("local"))?
            .swap_model(size)
    }

    pub fn transcribe(&mut self, audio_data: &[f32]) -> Result<String, SttError> {
        self.refresh_settings()?;
        let prompt = self.initial_prompt();
        match self.mode {
            EngineMode::Local => self.transcribe_local(audio_data, prompt.as_deref()),
            EngineMode::Cloud => self.transcribe_cloud(audio_data, prompt.as_deref()),
            EngineMode::Auto => match self.transcribe_local(audio_data, prompt.as_deref()) {
                Ok(text) => Ok(text),
                Err(local_error) => match self.transcribe_cloud(audio_data, prompt.as_deref()) {
                    Ok(text) => Ok(text),
                    Err(cloud_error) => Err(SttError::NoBackendAvailable {
                        attempts: vec![local_error.to_string(), cloud_error.to_string()],
                    }),
                },
            },
        }
    }

    fn transcribe_local(
        &mut self,
        audio_data: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String, SttError> {
        self.local
            .as_mut()
            .ok_or_else(|| SttError::BackendNotInitialized("local"))?
            .transcribe(audio_data, initial_prompt)
    }

    fn transcribe_cloud(
        &self,
        audio_data: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String, SttError> {
        self.cloud
            .as_ref()
            .ok_or_else(|| SttError::Cloud("Cloud STT is not configured".to_string()))?
            .transcribe(audio_data, initial_prompt)
            .map_err(SttError::Cloud)
    }

    fn initial_prompt(&self) -> Option<String> {
        self.dictionary_store
            .as_ref()
            .and_then(|store| store.prompt_terms().ok())
            .and_then(|terms| build_initial_prompt(&terms))
    }
}

#[cfg(test)]
mod tests {
    use super::EngineMode;

    #[test]
    fn unknown_engine_mode_defaults_to_auto() {
        assert_eq!(EngineMode::Auto, {
            match "surprise" {
                "local" => EngineMode::Local,
                "cloud" => EngineMode::Cloud,
                _ => EngineMode::Auto,
            }
        });
    }
}
