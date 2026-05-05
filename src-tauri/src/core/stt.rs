mod backends;

use std::fmt::{Display, Formatter};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use backends::{CpuBackend, CudaBackend, OpenVinoBackend, SttBackend};
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
        }
    }
}

pub struct BasicTranscriber {
    context: WhisperContext,
    backend_name: &'static str,
    model_path: PathBuf,
}

impl BasicTranscriber {
    pub fn new() -> Result<Self, SttError> {
        let model_path = Self::ensure_model_downloaded()?;
        Self::from_backends(&model_path)
    }

    fn from_backends(model_path: &PathBuf) -> Result<Self, SttError> {
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

            match backend.initialize(model_path).and_then(|_| backend.create_context(model_path)) {
                Ok(context) => {
                    return Ok(Self { context, backend_name: name, model_path: model_path.clone() });
                }
                Err(e) => attempts.push(e.to_string()),
            }
        }

        Err(SttError::NoBackendAvailable { attempts })
    }

    fn ensure_model_downloaded() -> Result<PathBuf, SttError> {
        let model_name = "ggml-base.en.bin";
        let model_dir = std::env::current_dir()
            .map_err(|e| SttError::ModelDirResolve(format!("Failed to resolve model directory: {}", e)))?
            .join("models");

        if !model_dir.exists() {
            fs::create_dir_all(&model_dir)
                .map_err(|e| SttError::ModelDownload(format!("Failed to create model directory: {}", e)))?;
        }

        let model_path = model_dir.join(model_name);

        if !model_path.exists() {
            println!("Downloading Whisper model ({})... this may take a minute.", model_name);
            let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", model_name);
            let response = reqwest::blocking::get(&url)
                .and_then(|response| response.error_for_status())
                .map_err(|e| SttError::ModelDownload(e.to_string()))?;

            let mut file = fs::File::create(&model_path)
                .map_err(|e| SttError::ModelDownload(e.to_string()))?;
            let bytes = response
                .bytes()
                .map_err(|e| SttError::ModelDownload(e.to_string()))?;
            file.write_all(&bytes)
                .map_err(|e| SttError::ModelDownload(e.to_string()))?;
            println!("Download complete.");
        }

        Ok(model_path)
    }

    pub fn transcribe(&mut self, audio_data: &[f32]) -> Result<String, SttError> {
        match self.transcribe_once(audio_data) {
            Ok(text) => Ok(text),
            Err(e) if self.backend_name != "cpu" => {
                let mut cpu = CpuBackend::default();
                cpu.initialize(&self.model_path)?;
                self.context = cpu.create_context(&self.model_path)?;
                self.backend_name = "cpu";
                self.transcribe_once(audio_data).map_err(|fallback_err| SttError::NoBackendAvailable {
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

        let num_segments = state.full_n_segments().map_err(|e| SttError::Inference {
            backend: self.backend_name,
            message: e.to_string(),
        })?;
        let mut result = String::new();

        for i in 0..num_segments {
            let segment = state
                .full_get_segment_text(i)
                .map_err(|e| SttError::Inference {
                    backend: self.backend_name,
                    message: e.to_string(),
                })?;
            result.push_str(&segment);
        }

        Ok(result.trim().to_string())
    }
}
