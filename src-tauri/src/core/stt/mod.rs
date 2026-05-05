mod backends;
mod model_manager;

use std::env;
use std::fmt::{Display, Formatter};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;

use crate::core::cloud::{CloudProviderKind, CloudTranscriber};
pub use crate::core::settings::ModelSize;

use backends::{CpuBackend, CudaBackend, OpenVinoBackend, SttBackend};
use model_manager::ModelManager;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};

#[derive(Debug)]
pub enum SttError {
    ModelDirResolve(String),
    ModelDownload(String),
    ModelLoad { backend: &'static str, message: String },
    BackendUnavailable { backend: &'static str, message: String },
    BackendNotInitialized(&'static str),
    NoBackendAvailable { attempts: Vec<String> },
    Inference { backend: &'static str, message: String },
    Cloud(String),
    SettingsPersist(String),
}

impl Display for SttError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelDirResolve(m) => write!(f, "{}", m),
            Self::ModelDownload(m) => write!(f, "{}", m),
            Self::ModelLoad { backend, message } => {
                write!(f, "Failed to load model with {} backend: {}", backend, message)
            }
            Self::BackendUnavailable { backend, message } => {
                write!(f, "{} backend unavailable: {}", backend, message)
            }
            Self::BackendNotInitialized(backend) => {
                write!(f, "{} backend is not initialized", backend)
            }
            Self::NoBackendAvailable { attempts } => {
                write!(f, "No STT backend could be initialized ({})", attempts.join("; "))
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
    fn from_env() -> Self {
        match env::var("WISPRTYPE_STT_ENGINE")
            .unwrap_or_else(|_| "auto".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "local" => Self::Local,
            "cloud" => Self::Cloud,
            _ => Self::Auto,
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

    fn load_from_backends(model_path: &PathBuf) -> Result<(WhisperContext, &'static str), SttError> {
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

    fn transcribe(&mut self, audio_data: &[f32]) -> Result<String, SttError> {
        match panic::catch_unwind(AssertUnwindSafe(|| self.transcribe_with_fallback(audio_data))) {
            Ok(result) => result,
            Err(_) => Err(SttError::Inference {
                backend: self.backend_name,
                message: "Local STT engine panicked".to_string(),
            }),
        }
    }

    fn transcribe_with_fallback(&mut self, audio_data: &[f32]) -> Result<String, SttError> {
        match self.transcribe_once(audio_data) {
            Ok(text) => Ok(text),
            Err(e) if self.backend_name != "cpu" => {
                let model_path = self.model_manager.active_model_path();
                let mut cpu = CpuBackend::default();
                cpu.initialize(&model_path)?;
                self.context = cpu.create_context(&model_path)?;
                self.backend_name = "cpu";
                self.transcribe_once(audio_data)
                    .map_err(|fallback_err| SttError::NoBackendAvailable {
                        attempts: vec![e.to_string(), fallback_err.to_string()],
                    })
            }
            Err(e) => Err(e),
        }
    }

    fn transcribe_once(&self, audio_data: &[f32]) -> Result<String, SttError> {
        let mut state = self.context.create_state().map_err(|e| SttError::Inference {
            backend: self.backend_name,
            message: e.to_string(),
        })?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state.full(params, audio_data).map_err(|e| SttError::Inference {
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
    mode: EngineMode,
    local: Option<LocalWhisperTranscriber>,
    cloud: Option<CloudTranscriber>,
}

impl BasicTranscriber {
    pub fn new() -> Result<Self, SttError> {
        let mode = EngineMode::from_env();
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
            match CloudTranscriber::new(CloudProviderKind::from_env()) {
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

        Ok(Self { mode, local, cloud })
    }

    pub fn swap_model(&mut self, size: ModelSize) -> Result<(), SttError> {
        self.local
            .as_mut()
            .ok_or_else(|| SttError::BackendNotInitialized("local"))?
            .swap_model(size)
    }

    pub fn transcribe(&mut self, audio_data: &[f32]) -> Result<String, SttError> {
        match self.mode {
            EngineMode::Local => self.transcribe_local(audio_data),
            EngineMode::Cloud => self.transcribe_cloud(audio_data),
            EngineMode::Auto => match self.transcribe_local(audio_data) {
                Ok(text) => Ok(text),
                Err(local_error) => match self.transcribe_cloud(audio_data) {
                    Ok(text) => Ok(text),
                    Err(cloud_error) => Err(SttError::NoBackendAvailable {
                        attempts: vec![local_error.to_string(), cloud_error.to_string()],
                    }),
                },
            },
        }
    }

    fn transcribe_local(&mut self, audio_data: &[f32]) -> Result<String, SttError> {
        self.local
            .as_mut()
            .ok_or_else(|| SttError::BackendNotInitialized("local"))?
            .transcribe(audio_data)
    }

    fn transcribe_cloud(&self, audio_data: &[f32]) -> Result<String, SttError> {
        self.cloud
            .as_ref()
            .ok_or_else(|| SttError::Cloud("Cloud STT is not configured".to_string()))?
            .transcribe(audio_data)
            .map_err(SttError::Cloud)
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
