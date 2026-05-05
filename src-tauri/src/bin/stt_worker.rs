use app_lib::core::stt::BasicTranscriber;
use app_lib::core::worker::{serve_worker, WorkerRequest};

fn main() {
    let mut transcriber = match BasicTranscriber::new() {
        Ok(transcriber) => transcriber,
        Err(e) => {
            eprintln!("Failed to initialize STT worker: {}", e);
            std::process::exit(1);
        }
    };

    let exit_code = serve_worker(|request| match request {
        WorkerRequest::Transcribe(audio) => {
            transcriber.transcribe(&audio).map_err(|e| e.to_string())
        }
        WorkerRequest::SwapModel(size) => transcriber
            .swap_model(size)
            .map(|_| "ok".to_string())
            .map_err(|e| e.to_string()),
        WorkerRequest::Refine(_) => Err("STT worker received a refinement request".to_string()),
    });

    std::process::exit(exit_code);
}
