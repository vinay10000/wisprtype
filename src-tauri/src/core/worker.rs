use crate::core::refinement::RefinementEngine;
use crate::core::stt::BasicTranscriber;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::process::{self, Child, ChildStdin, Command, Stdio};

const WORKER_ARG: &str = "--wisprtype-worker";
const STT_WORKER: &str = "stt";
const REFINEMENT_WORKER: &str = "refine";

#[derive(Debug, Clone, Copy)]
pub enum WorkerKind {
    Stt,
    Refinement,
}

impl WorkerKind {
    fn as_arg(self) -> &'static str {
        match self {
            Self::Stt => STT_WORKER,
            Self::Refinement => REFINEMENT_WORKER,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Stt => "STT worker",
            Self::Refinement => "refinement worker",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
enum WorkerRequest {
    Transcribe(Vec<f32>),
    Refine(String),
}

#[derive(Serialize, Deserialize)]
struct WorkerEnvelope {
    id: u64,
    request: WorkerRequest,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", content = "payload")]
enum WorkerResponse {
    Ok(String),
    Err(String),
}

#[derive(Serialize, Deserialize)]
struct WorkerResponseEnvelope {
    id: u64,
    response: WorkerResponse,
}

struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

pub struct NativeWorker {
    kind: WorkerKind,
    process: Option<WorkerProcess>,
    next_id: u64,
}

enum RequestError {
    Worker(String),
    Transport(String),
}

impl NativeWorker {
    pub fn new(kind: WorkerKind) -> Self {
        Self {
            kind,
            process: None,
            next_id: 1,
        }
    }

    pub fn transcribe(&mut self, audio_data: &[f32]) -> Result<String, String> {
        self.request(WorkerRequest::Transcribe(audio_data.to_vec()))
    }

    pub fn refine(&mut self, raw_text: String) -> Result<String, String> {
        self.request(WorkerRequest::Refine(raw_text))
    }

    fn request(&mut self, request: WorkerRequest) -> Result<String, String> {
        for attempt in 0..2 {
            if self.process.is_none() {
                self.process = Some(Self::spawn_process(self.kind)?);
            }

            match self.try_request(request.clone()) {
                Ok(text) => return Ok(text),
                Err(RequestError::Worker(e)) => return Err(e),
                Err(RequestError::Transport(e)) => {
                    self.stop_process();
                    if attempt == 1 {
                        return Err(e);
                    }
                }
            }
        }

        Err(format!("{} did not return a response", self.kind.label()))
    }

    fn try_request(&mut self, request: WorkerRequest) -> Result<String, RequestError> {
        let label = self.kind.label();
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let envelope = WorkerEnvelope { id, request };
        let payload = serde_json::to_string(&envelope)
            .map_err(|e| RequestError::Worker(format!("Failed to encode worker request: {}", e)))?;

        let process = self
            .process
            .as_mut()
            .ok_or_else(|| RequestError::Transport("Worker process is not running".to_string()))?;

        writeln!(process.stdin, "{}", payload)
            .and_then(|_| process.stdin.flush())
            .map_err(|e| {
                RequestError::Transport(format!(
                    "Failed to send request to {}: {}",
                    label, e
                ))
            })?;

        let mut line = String::new();
        let bytes = process.stdout.read_line(&mut line).map_err(|e| {
            RequestError::Transport(format!(
                "Failed to read response from {}: {}",
                label, e
            ))
        })?;

        if bytes == 0 {
            return Err(RequestError::Transport(format!(
                "{} exited before responding",
                label
            )));
        }

        let response: WorkerResponseEnvelope = serde_json::from_str(&line).map_err(|e| {
            RequestError::Transport(format!(
                "{} returned invalid response: {}",
                label, e
            ))
        })?;

        if response.id != id {
            return Err(RequestError::Transport(format!(
                "{} response id mismatch: expected {}, got {}",
                label, id, response.id
            )));
        }

        match response.response {
            WorkerResponse::Ok(text) => Ok(text),
            WorkerResponse::Err(e) => Err(RequestError::Worker(e)),
        }
    }

    fn spawn_process(kind: WorkerKind) -> Result<WorkerProcess, String> {
        let exe = env::current_exe().map_err(|e| {
            format!(
                "Failed to locate current executable for {}: {}",
                kind.label(),
                e
            )
        })?;

        let mut child = Command::new(exe)
            .arg(WORKER_ARG)
            .arg(kind.as_arg())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start {}: {}", kind.label(), e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("Failed to open stdin for {}", kind.label()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("Failed to open stdout for {}", kind.label()))?;

        Ok(WorkerProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn stop_process(&mut self) {
        if let Some(mut process) = self.process.take() {
            if matches!(process.child.try_wait(), Ok(None)) {
                let _ = process.child.kill();
            }
            let _ = process.child.wait();
        }
    }
}

impl Drop for NativeWorker {
    fn drop(&mut self) {
        self.stop_process();
    }
}

pub fn run_if_requested() -> bool {
    let mut args = env::args();
    let _exe = args.next();
    if args.next().as_deref() != Some(WORKER_ARG) {
        return false;
    }

    let exit_code = match args.next().as_deref() {
        Some(STT_WORKER) => run_stt_worker(),
        Some(REFINEMENT_WORKER) => run_refinement_worker(),
        Some(other) => {
            eprintln!("Unknown WisprType worker kind: {}", other);
            2
        }
        None => {
            eprintln!("Missing WisprType worker kind");
            2
        }
    };

    process::exit(exit_code);
}

fn run_stt_worker() -> i32 {
    let mut transcriber = match BasicTranscriber::new() {
        Ok(transcriber) => transcriber,
        Err(e) => {
            eprintln!("Failed to initialize STT worker: {}", e);
            return 1;
        }
    };

    serve_worker(|request| match request {
        WorkerRequest::Transcribe(audio) => transcriber.transcribe(&audio).map_err(|e| e.to_string()),
        WorkerRequest::Refine(_) => Err("STT worker received a refinement request".to_string()),
    })
}

fn run_refinement_worker() -> i32 {
    let refinement = match RefinementEngine::new() {
        Ok(refinement) => refinement,
        Err(e) => {
            eprintln!("Failed to initialize refinement worker: {}", e);
            return 1;
        }
    };

    serve_worker(|request| match request {
        WorkerRequest::Refine(text) => Ok(refinement.clean(text)),
        WorkerRequest::Transcribe(_) => {
            Err("Refinement worker received a transcription request".to_string())
        }
    })
}

fn serve_worker<F>(mut handle_request: F) -> i32
where
    F: FnMut(WorkerRequest) -> Result<String, String>,
{
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("Failed to read worker request: {}", e);
                return 1;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let envelope: WorkerEnvelope = match serde_json::from_str(&line) {
            Ok(envelope) => envelope,
            Err(e) => {
                eprintln!("Failed to parse worker request: {}", e);
                continue;
            }
        };

        let id = envelope.id;
        let response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_request(envelope.request)
        })) {
            Ok(Ok(text)) => WorkerResponse::Ok(text),
            Ok(Err(e)) => WorkerResponse::Err(e),
            Err(_) => WorkerResponse::Err("Native worker request panicked".to_string()),
        };

        let response = WorkerResponseEnvelope { id, response };
        let encoded = match serde_json::to_string(&response) {
            Ok(encoded) => encoded,
            Err(e) => {
                eprintln!("Failed to encode worker response: {}", e);
                return 1;
            }
        };

        if writeln!(stdout, "{}", encoded)
            .and_then(|_| stdout.flush())
            .is_err()
        {
            return 1;
        }
    }

    0
}
