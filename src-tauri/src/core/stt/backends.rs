use std::path::Path;

use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::core::stt::SttError;

#[derive(Debug, Clone)]
pub struct BackendCapabilities {
    pub name: &'static str,
    pub accelerated: bool,
}

pub trait SttBackend: Send {
    fn initialize(&mut self, model_path: &Path) -> Result<(), SttError>;
    fn capabilities(&self) -> BackendCapabilities;
    fn create_context(&self, model_path: &Path) -> Result<WhisperContext, SttError>;
}

#[derive(Default)]
pub struct CpuBackend {
    initialized: bool,
}

impl SttBackend for CpuBackend {
    fn initialize(&mut self, _model_path: &Path) -> Result<(), SttError> {
        self.initialized = true;
        Ok(())
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "cpu",
            accelerated: false,
        }
    }

    fn create_context(&self, model_path: &Path) -> Result<WhisperContext, SttError> {
        if !self.initialized {
            return Err(SttError::BackendNotInitialized("cpu"));
        }

        WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| SttError::ModelLoad {
            backend: "cpu",
            message: e.to_string(),
        })
    }
}

#[derive(Default)]
pub struct CudaBackend;

impl SttBackend for CudaBackend {
    fn initialize(&mut self, _model_path: &Path) -> Result<(), SttError> {
        Err(SttError::BackendUnavailable {
            backend: "cuda",
            message: "CUDA backend is not available in this build".to_string(),
        })
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "cuda",
            accelerated: true,
        }
    }

    fn create_context(&self, _model_path: &Path) -> Result<WhisperContext, SttError> {
        Err(SttError::BackendUnavailable {
            backend: "cuda",
            message: "CUDA backend is not available in this build".to_string(),
        })
    }
}

#[derive(Default)]
pub struct OpenVinoBackend;

impl SttBackend for OpenVinoBackend {
    fn initialize(&mut self, _model_path: &Path) -> Result<(), SttError> {
        Err(SttError::BackendUnavailable {
            backend: "openvino",
            message: "OpenVINO backend is not available in this build".to_string(),
        })
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "openvino",
            accelerated: true,
        }
    }

    fn create_context(&self, _model_path: &Path) -> Result<WhisperContext, SttError> {
        Err(SttError::BackendUnavailable {
            backend: "openvino",
            message: "OpenVINO backend is not available in this build".to_string(),
        })
    }
}
