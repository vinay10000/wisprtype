mod model_manager;

use model_manager::ModelManager;
pub use model_manager::ModelSize;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct BasicTranscriber {
    context: WhisperContext,
    model_manager: ModelManager,
}

impl BasicTranscriber {
    pub fn new() -> Result<Self, String> {
        let model_manager = ModelManager::new()?;
        let model_path = model_manager.ensure_model_downloaded()?;

        let ctx = WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| format!("Failed to load model: {}", e))?;

        Ok(Self {
            context: ctx,
            model_manager,
        })
    }

    pub fn swap_model(&mut self, size: ModelSize) -> Result<(), String> {
        let model_path = self.model_manager.swap_model(size)?;
        let new_context = WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| format!("Failed to load model: {}", e))?;

        self.context = new_context;
        Ok(())
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
