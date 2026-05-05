use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::core::settings::ModelSize;
use crate::core::stt::SttError;

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const DOWNLOAD_BUFFER_SIZE: usize = 64 * 1024;

impl ModelSize {
    pub fn download_url(self) -> String {
        format!("{}/{}", MODEL_BASE_URL, self.filename())
    }
}

use crate::core::settings::{app_data_dir, SettingsStore};

pub struct ModelManager {
    model_dir: PathBuf,
    settings_store: SettingsStore,
    selected_size: ModelSize,
}

fn resolve_app_data_dir() -> Result<PathBuf, String> {
    app_data_dir()
}

impl ModelManager {
    pub fn new() -> Result<Self, String> {
        let app_dir = resolve_app_data_dir()?;
        let settings_store = SettingsStore::new(&app_dir);
        let settings = settings_store.load()?;
        let model_dir = app_dir.join("models");
        fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

        Ok(Self {
            model_dir,
            settings_store,
            selected_size: settings.stt_model_size,
        })
    }

    pub fn active_model_path(&self) -> PathBuf {
        self.model_dir.join(self.selected_size.filename())
    }

    pub fn ensure_model_downloaded(&self) -> Result<PathBuf, SttError> {
        let model_path = self.active_model_path();
        if model_path.exists() {
            return Ok(model_path);
        }

        let model_name = self.selected_size.filename();
        eprintln!(
            "Downloading Whisper model ({})... this may take a minute.",
            model_name
        );

        let tmp_path = model_path.with_extension("tmp");
        let download_result = self.stream_download_model(&tmp_path);

        if let Err(e) = download_result {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }

        fs::rename(&tmp_path, &model_path).map_err(|e| SttError::ModelDownload(e.to_string()))?;

        eprintln!("Download complete.");
        Ok(model_path)
    }

    fn stream_download_model(&self, dest: &PathBuf) -> Result<(), SttError> {
        let mut response = reqwest::blocking::get(self.selected_size.download_url())
            .and_then(|r| r.error_for_status())
            .map_err(|e| SttError::ModelDownload(e.to_string()))?;

        let mut file =
            fs::File::create(dest).map_err(|e| SttError::ModelDownload(e.to_string()))?;

        let mut buf = [0u8; DOWNLOAD_BUFFER_SIZE];
        loop {
            let n = response
                .read(&mut buf)
                .map_err(|e| SttError::ModelDownload(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| SttError::ModelDownload(e.to_string()))?;
        }

        file.flush()
            .map_err(|e| SttError::ModelDownload(e.to_string()))
    }

    pub fn swap_model(&mut self, size: ModelSize) -> Result<PathBuf, SttError> {
        if self.selected_size == size {
            return self.ensure_model_downloaded();
        }

        let previous_size = self.selected_size;
        self.selected_size = size;

        match self.ensure_model_downloaded() {
            Ok(path) => {
                self.persist_settings()?;
                Ok(path)
            }
            Err(e) => {
                self.selected_size = previous_size;
                Err(e)
            }
        }
    }

    fn persist_settings(&self) -> Result<(), SttError> {
        let mut settings = self
            .settings_store
            .load()
            .map_err(SttError::SettingsPersist)?;
        settings.stt_model_size = self.selected_size;
        self.settings_store
            .persist(&settings)
            .map_err(|e| SttError::SettingsPersist(e))
    }
}
