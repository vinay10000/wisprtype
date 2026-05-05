use crate::core::model_paths::{ensure_model_root_dir, map_fs_error};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::fs;
use std::io::Write;
use std::num::NonZeroU32;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;

const MODEL_NAME: &str = "qwen2.5-0.5b-instruct-q4_k_m.gguf";
const MODEL_URL: &str =
    "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf";
const CONTEXT_TOKENS: u32 = 2048;
const MIN_GENERATION_TOKENS: usize = 32;
const MAX_GENERATION_TOKENS: usize = 192;

pub struct RefinementEngine {
    backend: LlamaBackend,
    model: LlamaModel,
}

impl RefinementEngine {
    pub fn new() -> Result<Self, String> {
        let model_path = Self::ensure_model_downloaded()?;
        let mut backend = LlamaBackend::init()
            .map_err(|e| format!("Failed to initialize refinement backend: {}", e))?;
        backend.void_logs();

        let model = LlamaModel::load_from_file(&backend, model_path, &LlamaModelParams::default())
            .map_err(|e| format!("Failed to load refinement model: {}", e))?;

        Ok(Self { backend, model })
    }

    pub fn clean(&self, raw_text: String) -> String {
        if Self::should_skip(&raw_text) {
            return raw_text;
        }

        let fallback = raw_text.clone();
        match panic::catch_unwind(AssertUnwindSafe(|| self.clean_inner(&raw_text))) {
            Ok(Ok(cleaned)) => Self::finalize_output(&fallback, &cleaned).unwrap_or(fallback),
            Ok(Err(e)) => {
                eprintln!("Transcript refinement failed: {}", e);
                fallback
            }
            Err(_) => {
                eprintln!("Transcript refinement panicked; using raw transcript.");
                fallback
            }
        }
    }

    fn ensure_model_downloaded() -> Result<PathBuf, String> {
        let model_dir = ensure_model_root_dir()?;

        let model_path = model_dir.join(MODEL_NAME);
        if model_path.exists() {
            return Ok(model_path);
        }

        println!(
            "Downloading refinement model ({})... this may take a while.",
            MODEL_NAME
        );

        let tmp_path = model_path.with_extension("gguf.download");
        let download_result = (|| -> Result<(), String> {
            let response = reqwest::blocking::get(MODEL_URL)
                .and_then(|response| response.error_for_status())
                .map_err(|e| format!("Failed to download refinement model: {}", e))?;
            let bytes = response
                .bytes()
                .map_err(|e| format!("Failed to read refinement model download: {}", e))?;

            let mut file = fs::File::create(&tmp_path)
                .map_err(|e| map_fs_error("create refinement model file", &tmp_path, &e))?;
            file.write_all(&bytes)
                .map_err(|e| map_fs_error("write refinement model file", &tmp_path, &e))?;
            fs::rename(&tmp_path, &model_path)
                .map_err(|e| map_fs_error("finalize refinement model file", &model_path, &e))?;
            Ok(())
        })();

        if download_result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        download_result?;

        println!("Refinement model download complete.");
        Ok(model_path)
    }

    fn clean_inner(&self, raw_text: &str) -> Result<String, String> {
        let prompt = Self::build_prompt(raw_text);
        let prompt_tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| format!("Failed to tokenize refinement prompt: {}", e))?;
        let max_new_tokens = Self::max_new_tokens(raw_text);

        if prompt_tokens.len() + max_new_tokens + 1 >= CONTEXT_TOKENS as usize {
            return Err("Refinement prompt is too long for the local model context".to_string());
        }

        let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(CONTEXT_TOKENS));
        let mut context = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| format!("Failed to create refinement context: {}", e))?;

        let mut batch = LlamaBatch::new(prompt_tokens.len().max(1), 1);
        batch
            .add_sequence(&prompt_tokens, 0, false)
            .map_err(|e| format!("Failed to prepare refinement prompt batch: {}", e))?;
        context
            .decode(&mut batch)
            .map_err(|e| format!("Failed to decode refinement prompt: {}", e))?;

        let mut sampler = LlamaSampler::greedy();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut generated = String::new();
        let mut position = prompt_tokens.len() as i32;

        for _ in 0..max_new_tokens {
            let token = sampler.sample(&context, -1);
            if self.model.is_eog_token(token) {
                break;
            }

            sampler.accept(token);
            let piece = self
                .model
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(|e| format!("Failed to decode refinement token: {}", e))?;
            generated.push_str(&piece);

            batch.clear();
            batch
                .add(token, position, &[0], true)
                .map_err(|e| format!("Failed to prepare refinement token batch: {}", e))?;
            context
                .decode(&mut batch)
                .map_err(|e| format!("Failed to decode refinement token: {}", e))?;
            position += 1;
        }

        Ok(generated)
    }

    fn build_prompt(raw_text: &str) -> String {
        format!(
            "<|im_start|>system\nYou rewrite speech transcripts. Remove filler words like \"um\", \"uh\", and \"like\" only when used as filler. Add punctuation and capitalization. Preserve wording and meaning. Output only the cleaned transcript. If unsure, return the original text unchanged.\n<|im_end|>\n<|im_start|>user\nClean this transcript:\n{}\n<|im_end|>\n<|im_start|>assistant\n",
            raw_text.trim()
        )
    }

    fn should_skip(raw_text: &str) -> bool {
        raw_text.trim().is_empty()
    }

    fn max_new_tokens(raw_text: &str) -> usize {
        let estimated_input_tokens = raw_text.chars().count().div_ceil(4);
        (estimated_input_tokens + 24).clamp(MIN_GENERATION_TOKENS, MAX_GENERATION_TOKENS)
    }

    fn finalize_output(raw_text: &str, generated: &str) -> Option<String> {
        let cleaned = generated
            .split("<|im_end|>")
            .next()
            .unwrap_or(generated)
            .split("<|endoftext|>")
            .next()
            .unwrap_or(generated)
            .trim()
            .trim_matches('"')
            .trim()
            .to_string();

        if cleaned.is_empty() {
            return None;
        }

        let max_reasonable_len = raw_text.len().saturating_mul(3).saturating_add(120);
        if cleaned.len() > max_reasonable_len {
            return None;
        }

        Some(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::{RefinementEngine, MODEL_NAME};

    #[test]
    fn qwen_refinement_model_is_the_configured_default() {
        assert!(MODEL_NAME.contains("qwen2.5"));
    }

    #[test]
    fn prompt_locks_cleanup_to_transcript_refinement() {
        let prompt = RefinementEngine::build_prompt("um this is a test");
        assert!(prompt.contains("Remove filler words"));
        assert!(prompt.contains("Add punctuation and capitalization"));
        assert!(prompt.contains("Preserve wording and meaning"));
        assert!(prompt.contains("Output only the cleaned transcript"));
    }

    #[test]
    fn empty_transcript_is_skipped_before_inference() {
        assert!(RefinementEngine::should_skip(""));
        assert!(RefinementEngine::should_skip("   \n\t  "));
        assert!(!RefinementEngine::should_skip("um this is a test"));
    }

    #[test]
    fn failed_or_empty_generation_falls_back_to_raw() {
        assert_eq!(RefinementEngine::finalize_output("raw text", ""), None);
        assert_eq!(RefinementEngine::finalize_output("raw text", "   "), None);
    }

    #[test]
    fn generated_special_tokens_are_trimmed() {
        assert_eq!(
            RefinementEngine::finalize_output("um hi", "Hi. <|im_end|>"),
            Some("Hi.".to_string())
        );
    }

    #[test]
    fn oversized_generation_is_rejected() {
        let generated = "x".repeat(1000);
        assert_eq!(RefinementEngine::finalize_output("short", &generated), None);
    }
}
