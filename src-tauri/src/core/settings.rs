use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSize {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
}

impl Default for ModelSize {
    fn default() -> Self {
        Self::Base
    }
}

impl ModelSize {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Tiny => "ggml-tiny.en.bin",
            Self::Base => "ggml-base.en.bin",
            Self::Small => "ggml-small.en.bin",
            Self::Medium => "ggml-medium.en.bin",
            Self::Large => "ggml-large-v3.bin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub stt_model_size: ModelSize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            stt_model_size: ModelSize::default(),
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            path: base_dir.join(SETTINGS_FILE),
        }
    }

    pub fn load(&self) -> Result<AppSettings, String> {
        if !self.path.exists() {
            return Ok(AppSettings::default());
        }

        let settings_raw = fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
        serde_json::from_str(&settings_raw).map_err(|e| e.to_string())
    }

    pub fn persist(&self, settings: &AppSettings) -> Result<(), String> {
        let body = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        fs::write(&self.path, body).map_err(|e| e.to_string())
    }
}
