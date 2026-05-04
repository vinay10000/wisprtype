use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
use std::path::PathBuf;
use std::fs;
use std::io::Write;

pub struct BasicTranscriber {
    context: WhisperContext,
}

impl BasicTranscriber {
    pub fn new() -> Result<Self, String> {
        let model_path = Self::ensure_model_downloaded()?;
        
        let ctx = WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        ).map_err(|e| format!("Failed to load model: {}", e))?;

        Ok(Self { context: ctx })
    }

    fn ensure_model_downloaded() -> Result<PathBuf, String> {
        // We'll use ggml-base.en.bin for the tracer bullet
        let model_name = "ggml-base.en.bin";
        let model_dir = std::env::current_dir()
            .map_err(|e| format!("Failed to resolve model directory: {}", e))?
            .join("models");
        
        if !model_dir.exists() {
            fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;
        }

        let model_path = model_dir.join(model_name);

        if !model_path.exists() {
            println!("Downloading Whisper model ({})... this may take a minute.", model_name);
            let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", model_name);
            let response = reqwest::blocking::get(&url)
                .and_then(|response| response.error_for_status())
                .map_err(|e| e.to_string())?;
            
            let mut file = fs::File::create(&model_path).map_err(|e| e.to_string())?;
            let bytes = response.bytes().map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            println!("Download complete.");
        }

        Ok(model_path)
    }

    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        let mut state = self.context.create_state().map_err(|e| e.to_string())?;
        
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state.full(params, audio_data).map_err(|e| e.to_string())?;

        let num_segments = state.full_n_segments().map_err(|e| e.to_string())?;
        let mut result = String::new();
        
        for i in 0..num_segments {
            let segment = state.full_get_segment_text(i).map_err(|e| e.to_string())?;
            result.push_str(&segment);
        }

        Ok(result.trim().to_string())
    }
}
