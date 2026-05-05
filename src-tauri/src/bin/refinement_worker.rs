use app_lib::core::refinement::RefinementEngine;
use app_lib::core::worker::{serve_worker, WorkerRequest};

fn main() {
    let refinement = match RefinementEngine::new() {
        Ok(refinement) => refinement,
        Err(e) => {
            eprintln!("Failed to initialize refinement worker: {}", e);
            std::process::exit(1);
        }
    };

    let exit_code = serve_worker(|request| match request {
        WorkerRequest::Refine(text) => Ok(refinement.clean(text)),
        WorkerRequest::SwapModel(_) => {
            Err("Refinement worker received a model swap request".to_string())
        }
        WorkerRequest::Transcribe(_) => {
            Err("Refinement worker received a transcription request".to_string())
        }
    });

    std::process::exit(exit_code);
}
