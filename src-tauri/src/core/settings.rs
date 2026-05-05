use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "settings.json";
const APP_DATA_DIR: &str = "WisprFlow";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SttEngineMode {
    Local,
    Cloud,
    Auto,
}

impl Default for SttEngineMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudProvider {
    Gladia,
    OpenAi,
    Groq,
    Deepgram,
}

impl Default for CloudProvider {
    fn default() -> Self {
        Self::Gladia
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PillStyle {
    Aurora,
    Minimal,
}

impl Default for PillStyle {
    fn default() -> Self {
        Self::Aurora
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub stt_model_size: ModelSize,
    #[serde(default)]
    pub stt_engine: SttEngineMode,
    #[serde(default)]
    pub cloud_provider: CloudProvider,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_pill_visible")]
    pub pill_visible: bool,
    #[serde(default)]
    pub pill_style: PillStyle,
    #[serde(default = "default_launch_at_login")]
    pub launch_at_login: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            stt_model_size: ModelSize::default(),
            stt_engine: SttEngineMode::default(),
            cloud_provider: CloudProvider::default(),
            hotkey: default_hotkey(),
            pill_visible: default_pill_visible(),
            pill_style: PillStyle::default(),
            launch_at_login: default_launch_at_login(),
        }
    }
}

pub fn app_data_dir() -> Result<PathBuf, String> {
    let app_data = std::env::var("APPDATA")
        .map_err(|e| format!("Failed to resolve APPDATA directory: {}", e))?;
    Ok(PathBuf::from(app_data).join(APP_DATA_DIR))
}

fn default_hotkey() -> String {
    "Super+Space".to_string()
}

fn default_pill_visible() -> bool {
    true
}

fn default_launch_at_login() -> bool {
    true
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new_default() -> Result<Self, String> {
        Ok(Self::new(&app_data_dir()?))
    }

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
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        fs::write(&self.path, body).map_err(|e| e.to_string())
    }
}
