use crate::core::settings::ModelSize;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

const STT_WORKER_EXE: &str = "wisprtype-stt-worker";
const REFINEMENT_WORKER_EXE: &str = "wisprtype-refinement-worker";
const TARGET_TRIPLE: &str = match option_env!("TAURI_ENV_TARGET_TRIPLE") {
    Some(value) => value,
    None => "x86_64-pc-windows-msvc",
};

#[derive(Debug, Clone, Copy)]
pub enum WorkerKind {
    Stt,
    Refinement,
}

impl WorkerKind {
    fn label(self) -> &'static str {
        match self {
            Self::Stt => "STT worker",
            Self::Refinement => "refinement worker",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum WorkerRequest {
    Transcribe(Vec<f32>),
    Refine(String),
    SwapModel(ModelSize),
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

    pub fn swap_model(&mut self, size: ModelSize) -> Result<String, String> {
        self.request(WorkerRequest::SwapModel(size))
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
                RequestError::Transport(format!("Failed to send request to {}: {}", label, e))
            })?;

        let mut line = String::new();
        let bytes = process.stdout.read_line(&mut line).map_err(|e| {
            RequestError::Transport(format!("Failed to read response from {}: {}", label, e))
        })?;

        if bytes == 0 {
            return Err(RequestError::Transport(format!(
                "{} exited before responding",
                label
            )));
        }

        let response: WorkerResponseEnvelope = serde_json::from_str(&line).map_err(|e| {
            RequestError::Transport(format!("{} returned invalid response: {}", label, e))
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
        let exe = worker_executable(kind)?;

        let mut child = Command::new(exe)
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

fn worker_executable(kind: WorkerKind) -> Result<PathBuf, String> {
    let base_name = worker_base_name(kind);
    let current_exe = env::current_exe().map_err(|e| {
        format!(
            "Failed to locate current executable for {}: {}",
            kind.label(),
            e
        )
    })?;

    if let Some(dir) = current_exe.parent() {
        for candidate in worker_candidates(dir, base_name) {
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    let cwd =
        env::current_dir().map_err(|e| format!("Failed to resolve current directory: {}", e))?;
    for candidate in worker_candidates(&cwd, base_name) {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "{} executable `{}` was not found in the app directory, bundled sidecar directory, or current working directory",
        kind.label(),
        worker_exe_name(base_name)
    ))
}

fn worker_base_name(kind: WorkerKind) -> &'static str {
    match kind {
        WorkerKind::Stt => STT_WORKER_EXE,
        WorkerKind::Refinement => REFINEMENT_WORKER_EXE,
    }
}

fn worker_exe_name(base: &str) -> String {
    #[cfg(windows)]
    {
        format!("{base}.exe")
    }

    #[cfg(not(windows))]
    {
        base.to_string()
    }
}

fn worker_packaged_name(base: &str) -> String {
    #[cfg(windows)]
    {
        format!("{base}-{TARGET_TRIPLE}.exe")
    }

    #[cfg(not(windows))]
    {
        format!("{base}-{TARGET_TRIPLE}")
    }
}

fn worker_candidates(root: &Path, base: &str) -> Vec<PathBuf> {
    let exe_name = worker_exe_name(base);
    let packaged_name = worker_packaged_name(base);
    vec![
        root.join(&exe_name),
        root.join(&packaged_name),
        root.join("binaries").join(&exe_name),
        root.join("binaries").join(&packaged_name),
    ]
}

#[cfg(test)]
mod path_tests {
    use super::{worker_candidates, worker_exe_name, worker_packaged_name};
    use std::path::Path;

    #[test]
    fn bundled_sidecar_candidates_include_binaries_directory() {
        let root = Path::new(r"C:\Apps\wisprflow");
        let candidates = worker_candidates(root, "wisprtype-stt-worker");
        assert!(candidates.iter().any(|path| {
            path.ends_with(Path::new("binaries").join(worker_packaged_name("wisprtype-stt-worker")))
        }));
        assert!(candidates
            .iter()
            .any(|path| path.ends_with(worker_exe_name("wisprtype-stt-worker"))));
    }
}

impl Drop for NativeWorker {
    fn drop(&mut self) {
        self.stop_process();
    }
}

pub fn serve_worker<F>(mut handle_request: F) -> i32
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
