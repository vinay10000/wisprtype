use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
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

    pub fn download_url(self) -> String {
        format!("{}/{}", MODEL_BASE_URL, self.filename())
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AppSettings {
    #[serde(default)]
    stt_model_size: ModelSize,
}

pub struct ModelManager {
    model_dir: PathBuf,
    selected_size: ModelSize,
}

impl ModelManager {
    pub fn new() -> Result<Self, String> {
        let cwd = std::env::current_dir()
            .map_err(|e| format!("Failed to resolve app directory: {}", e))?;
        let settings = Self::load_settings(&cwd)?;
        let model_dir = cwd.join("models");
        fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

        Ok(Self {
            model_dir,
            selected_size: settings.stt_model_size,
        })
    }

    pub fn selected_size(&self) -> ModelSize {
        self.selected_size
    }

    pub fn active_model_path(&self) -> PathBuf {
        self.model_dir.join(self.selected_size.filename())
    }

    pub fn ensure_model_downloaded(&self) -> Result<PathBuf, String> {
        let model_path = self.active_model_path();
        if model_path.exists() {
            return Ok(model_path);
        }

        let model_name = self.selected_size.filename();
        println!("Downloading Whisper model ({})... this may take a minute.", model_name);

        let response = reqwest::blocking::get(self.selected_size.download_url())
            .and_then(|response| response.error_for_status())
            .map_err(|e| e.to_string())?;

        let mut file = fs::File::create(&model_path).map_err(|e| e.to_string())?;
        let bytes = response.bytes().map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;

        println!("Download complete.");
        Ok(model_path)
    }

    pub fn swap_model(&mut self, size: ModelSize) -> Result<PathBuf, String> {
        if self.selected_size == size {
            return self.ensure_model_downloaded();
        }

        self.selected_size = size;
        self.persist_settings()?;
        self.ensure_model_downloaded()
    }

    fn settings_path(cwd: &std::path::Path) -> PathBuf {
        cwd.join(SETTINGS_FILE)
    }

    fn load_settings(cwd: &std::path::Path) -> Result<AppSettings, String> {
        let settings_path = Self::settings_path(cwd);
        if !settings_path.exists() {
            return Ok(AppSettings::default());
        }

        let settings_raw = fs::read_to_string(settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&settings_raw).map_err(|e| e.to_string())
    }

    fn persist_settings(&self) -> Result<(), String> {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let settings_path = Self::settings_path(&cwd);
        let settings = AppSettings {
            stt_model_size: self.selected_size,
        };

        let body = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        fs::write(settings_path, body).map_err(|e| e.to_string())
    }
}
