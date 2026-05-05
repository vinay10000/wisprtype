use reqwest::blocking::{multipart, Client, Response};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::env;
use std::thread;
use std::time::{Duration, Instant};

use crate::core::settings::CloudProvider as SettingsCloudProvider;

const SAMPLE_RATE: u32 = 16_000;
const REQUEST_TIMEOUT_SECS: u64 = 25;
const GLADIA_POLL_INTERVAL_MS: u64 = 750;
const GLADIA_MAX_WAIT_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProviderKind {
    Gladia,
    OpenAi,
    Groq,
    Deepgram,
}

impl CloudProviderKind {
    pub fn from_env() -> Self {
        Self::parse(&env::var("WISPRFLOW_CLOUD_PROVIDER").unwrap_or_else(|_| "gladia".to_string()))
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "gladia" => Self::Gladia,
            "groq" => Self::Groq,
            "deepgram" => Self::Deepgram,
            _ => Self::OpenAi,
        }
    }

    pub fn from_settings(provider: SettingsCloudProvider) -> Self {
        match provider {
            SettingsCloudProvider::Gladia => Self::Gladia,
            SettingsCloudProvider::OpenAi => Self::OpenAi,
            SettingsCloudProvider::Groq => Self::Groq,
            SettingsCloudProvider::Deepgram => Self::Deepgram,
        }
    }

    fn credential_name(self) -> &'static str {
        match self {
            Self::Gladia => "gladia",
            Self::OpenAi => "openai",
            Self::Groq => "groq",
            Self::Deepgram => "deepgram",
        }
    }

    fn env_key(self) -> &'static str {
        match self {
            Self::Gladia => "GLADIA_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Groq => "GROQ_API_KEY",
            Self::Deepgram => "DEEPGRAM_API_KEY",
        }
    }
}

pub trait CloudProvider {
    fn transcribe(&self, wav: Vec<u8>, initial_prompt: Option<&str>) -> Result<String, String>;
}

pub struct CloudTranscriber {
    provider: Box<dyn CloudProvider + Send + Sync>,
}

impl CloudTranscriber {
    pub fn new(kind: CloudProviderKind) -> Result<Self, String> {
        let api_key = CloudCredentials::read_api_key(kind)?
            .or_else(|| env::var(kind.env_key()).ok())
            .ok_or_else(|| format!("Missing API key for {:?}", kind))?;
        let client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| format!("Failed to create cloud STT client: {}", e))?;

        let provider: Box<dyn CloudProvider + Send + Sync> = match kind {
            CloudProviderKind::Gladia => Box::new(GladiaProvider { client, api_key }),
            CloudProviderKind::OpenAi => Box::new(OpenAiProvider { client, api_key }),
            CloudProviderKind::Groq => Box::new(GroqProvider { client, api_key }),
            CloudProviderKind::Deepgram => Box::new(DeepgramProvider { client, api_key }),
        };

        Ok(Self { provider })
    }

    pub fn transcribe(
        &self,
        audio_data: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String, String> {
        self.provider
            .transcribe(encode_wav(audio_data, SAMPLE_RATE), initial_prompt)
    }
}

pub struct CloudCredentials;

impl CloudCredentials {
    pub fn read_api_key(kind: CloudProviderKind) -> Result<Option<String>, String> {
        credential_manager::read_secret(&credential_target(kind))
    }

    pub fn write_api_key(kind: CloudProviderKind, api_key: &str) -> Result<(), String> {
        if api_key.trim().is_empty() {
            return Err("API key cannot be empty".to_string());
        }
        credential_manager::write_secret(&credential_target(kind), api_key)
    }

    pub fn delete_api_key(kind: CloudProviderKind) -> Result<(), String> {
        credential_manager::delete_secret(&credential_target(kind))
    }
}

fn credential_target(kind: CloudProviderKind) -> String {
    format!("wisprflow/cloud/{}", kind.credential_name())
}

struct GladiaProvider {
    client: Client,
    api_key: String,
}

impl CloudProvider for GladiaProvider {
    fn transcribe(&self, wav: Vec<u8>, initial_prompt: Option<&str>) -> Result<String, String> {
        let audio_url = self.upload_audio(wav)?;
        let job = self.start_transcription(audio_url, initial_prompt)?;
        self.poll_transcription(&job.id)
    }
}

impl GladiaProvider {
    fn upload_audio(&self, wav: Vec<u8>) -> Result<String, String> {
        let form = multipart::Form::new().part("audio", wav_part(wav)?);
        let response = self
            .client
            .post("https://api.gladia.io/v2/upload")
            .header("x-gladia-key", &self.api_key)
            .multipart(form)
            .send()
            .map_err(|e| classify_transport_error("Gladia", e))?;

        let status = response.status();
        let payload: GladiaUploadResponse = response
            .json()
            .map_err(|e| format!("Gladia returned an unreadable upload response: {}", e))?;

        if !status.is_success() {
            return Err(classify_status("Gladia", status, payload.error_message()));
        }

        payload
            .audio_url
            .filter(|url| !url.trim().is_empty())
            .ok_or_else(|| "Gladia upload returned no audio URL".to_string())
    }

    fn start_transcription(
        &self,
        audio_url: String,
        initial_prompt: Option<&str>,
    ) -> Result<GladiaJobResponse, String> {
        let custom_vocabulary = initial_prompt
            .map(extract_prompt_terms)
            .filter(|terms| !terms.is_empty());
        let body = GladiaTranscriptionRequest {
            audio_url,
            custom_vocabulary,
            punctuation_enhanced: true,
            language_config: GladiaLanguageConfig {
                languages: vec!["en".to_string()],
                code_switching: false,
            },
        };

        let response = self
            .client
            .post("https://api.gladia.io/v2/pre-recorded")
            .header("x-gladia-key", &self.api_key)
            .json(&body)
            .send()
            .map_err(|e| classify_transport_error("Gladia", e))?;

        let status = response.status();
        let payload: GladiaJobResponse = response
            .json()
            .map_err(|e| format!("Gladia returned an unreadable job response: {}", e))?;

        if !status.is_success() {
            return Err(classify_status("Gladia", status, payload.error_message()));
        }

        Ok(payload)
    }

    fn poll_transcription(&self, id: &str) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(GLADIA_MAX_WAIT_SECS);

        loop {
            let response = self
                .client
                .get(format!("https://api.gladia.io/v2/pre-recorded/{id}"))
                .header("x-gladia-key", &self.api_key)
                .send()
                .map_err(|e| classify_transport_error("Gladia", e))?;

            let status = response.status();
            let payload: GladiaResultResponse = response
                .json()
                .map_err(|e| format!("Gladia returned an unreadable result response: {}", e))?;

            if !status.is_success() {
                return Err(classify_status("Gladia", status, payload.error_message()));
            }

            match payload.status.as_deref() {
                Some("done") => {
                    return payload
                        .result
                        .and_then(|result| result.transcription)
                        .and_then(|transcription| transcription.full_transcript)
                        .filter(|text| !text.trim().is_empty())
                        .ok_or_else(|| "Gladia returned an empty transcript".to_string())
                }
                Some("error") => {
                    return Err(format!(
                        "Gladia transcription failed: {}",
                        payload.error_message()
                    ))
                }
                _ if Instant::now() >= deadline => {
                    return Err("Gladia transcription timed out".to_string())
                }
                _ => thread::sleep(Duration::from_millis(GLADIA_POLL_INTERVAL_MS)),
            }
        }
    }
}

#[derive(Deserialize)]
struct GladiaUploadResponse {
    audio_url: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl GladiaUploadResponse {
    fn error_message(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.error.clone())
            .unwrap_or_else(|| "HTTP error".to_string())
    }
}

#[derive(Serialize)]
struct GladiaTranscriptionRequest {
    audio_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_vocabulary: Option<Vec<String>>,
    punctuation_enhanced: bool,
    language_config: GladiaLanguageConfig,
}

#[derive(Serialize)]
struct GladiaLanguageConfig {
    languages: Vec<String>,
    code_switching: bool,
}

#[derive(Deserialize)]
struct GladiaJobResponse {
    id: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl GladiaJobResponse {
    fn error_message(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.error.clone())
            .unwrap_or_else(|| "HTTP error".to_string())
    }
}

#[derive(Deserialize)]
struct GladiaResultResponse {
    status: Option<String>,
    result: Option<GladiaResult>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl GladiaResultResponse {
    fn error_message(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.error.clone())
            .unwrap_or_else(|| "HTTP error".to_string())
    }
}

#[derive(Deserialize)]
struct GladiaResult {
    transcription: Option<GladiaTranscription>,
}

#[derive(Deserialize)]
struct GladiaTranscription {
    full_transcript: Option<String>,
}

struct OpenAiProvider {
    client: Client,
    api_key: String,
}

impl CloudProvider for OpenAiProvider {
    fn transcribe(&self, wav: Vec<u8>, initial_prompt: Option<&str>) -> Result<String, String> {
        let part = wav_part(wav)?;
        let mut form = multipart::Form::new()
            .text("model", "whisper-1")
            .part("file", part);
        if let Some(prompt) = initial_prompt {
            form = form.text("prompt", prompt.to_string());
        }
        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .map_err(|e| classify_transport_error("OpenAI", e))?;
        parse_openai_like_response("OpenAI", response)
    }
}

struct GroqProvider {
    client: Client,
    api_key: String,
}

impl CloudProvider for GroqProvider {
    fn transcribe(&self, wav: Vec<u8>, initial_prompt: Option<&str>) -> Result<String, String> {
        let part = wav_part(wav)?;
        let mut form = multipart::Form::new()
            .text("model", "whisper-large-v3-turbo")
            .part("file", part);
        if let Some(prompt) = initial_prompt {
            form = form.text("prompt", prompt.to_string());
        }
        let response = self
            .client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .map_err(|e| classify_transport_error("Groq", e))?;
        parse_openai_like_response("Groq", response)
    }
}

struct DeepgramProvider {
    client: Client,
    api_key: String,
}

impl CloudProvider for DeepgramProvider {
    fn transcribe(&self, wav: Vec<u8>, initial_prompt: Option<&str>) -> Result<String, String> {
        let url = match initial_prompt {
            Some(prompt) => format!(
                "https://api.deepgram.com/v1/listen?model=nova-2&smart_format=true&keywords={}",
                prompt
                    .split(':')
                    .last()
                    .unwrap_or(prompt)
                    .replace('.', "")
                    .replace(", ", "&keywords=")
                    .replace(' ', "%20")
            ),
            None => "https://api.deepgram.com/v1/listen?model=nova-2&smart_format=true".to_string(),
        };
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "audio/wav")
            .body(wav)
            .send()
            .map_err(|e| classify_transport_error("Deepgram", e))?;
        parse_deepgram_response(response)
    }
}

#[derive(Deserialize)]
struct OpenAiLikeResponse {
    text: Option<String>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
    code: Option<String>,
}

fn parse_openai_like_response(provider: &str, response: Response) -> Result<String, String> {
    let status = response.status();
    let payload: OpenAiLikeResponse = response
        .json()
        .map_err(|e| format!("{provider} returned an unreadable response: {}", e))?;

    if !status.is_success() {
        return Err(classify_api_error(provider, status, payload.error));
    }

    payload
        .text
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("{provider} returned an empty transcript"))
}

#[derive(Deserialize)]
struct DeepgramResponse {
    results: Option<DeepgramResults>,
    err_msg: Option<String>,
}

#[derive(Deserialize)]
struct DeepgramResults {
    channels: Vec<DeepgramChannel>,
}

#[derive(Deserialize)]
struct DeepgramChannel {
    alternatives: Vec<DeepgramAlternative>,
}

#[derive(Deserialize)]
struct DeepgramAlternative {
    transcript: String,
}

fn parse_deepgram_response(response: Response) -> Result<String, String> {
    let status = response.status();
    let payload: DeepgramResponse = response
        .json()
        .map_err(|e| format!("Deepgram returned an unreadable response: {}", e))?;

    if !status.is_success() {
        let message = payload
            .err_msg
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        return Err(classify_status("Deepgram", status, message));
    }

    payload
        .results
        .and_then(|results| results.channels.into_iter().next())
        .and_then(|channel| channel.alternatives.into_iter().next())
        .map(|alternative| alternative.transcript)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "Deepgram returned an empty transcript".to_string())
}

fn classify_api_error(provider: &str, status: StatusCode, error: Option<ApiError>) -> String {
    let message = error
        .as_ref()
        .and_then(|e| e.message.clone())
        .or_else(|| error.as_ref().and_then(|e| e.code.clone()))
        .or_else(|| error.as_ref().and_then(|e| e.error_type.clone()))
        .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
    classify_status(provider, status, message)
}

fn classify_status(provider: &str, status: StatusCode, message: String) -> String {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            format!("{provider} rejected the API key: {message}")
        }
        StatusCode::TOO_MANY_REQUESTS | StatusCode::PAYMENT_REQUIRED => {
            format!("{provider} quota exceeded: {message}")
        }
        _ => format!("{provider} transcription failed: {message}"),
    }
}

fn classify_transport_error(provider: &str, error: reqwest::Error) -> String {
    if error.is_timeout() {
        format!("{provider} transcription timed out")
    } else {
        format!("{provider} transcription request failed: {error}")
    }
}

fn wav_part(wav: Vec<u8>) -> Result<multipart::Part, String> {
    multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Failed to prepare audio upload: {}", e))
}

fn extract_prompt_terms(prompt: &str) -> Vec<String> {
    prompt
        .split(':')
        .last()
        .unwrap_or(prompt)
        .trim_end_matches('.')
        .split(',')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_bytes = samples.len() as u32 * 2;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());

    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm = (clamped * i16::MAX as f32) as i16;
        wav.extend_from_slice(&pcm.to_le_bytes());
    }

    wav
}

#[cfg(windows)]
mod credential_manager {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use std::slice;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CREDENTIAL_ATTRIBUTEW,
        CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    pub fn read_secret(target: &str) -> Result<Option<String>, String> {
        let target = wide_null(target);
        let mut credential_ptr: *mut CREDENTIALW = null_mut();

        let read = unsafe {
            CredReadW(
                PCWSTR(target.as_ptr()),
                CRED_TYPE_GENERIC,
                0,
                &mut credential_ptr,
            )
        };

        if read.is_err() {
            return Ok(None);
        }

        if credential_ptr.is_null() {
            return Ok(None);
        }

        let result = unsafe {
            let credential = *credential_ptr;
            let bytes = slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            );
            let secret = String::from_utf8(bytes.to_vec())
                .map_err(|e| format!("Credential Manager secret is not UTF-8: {}", e));
            CredFree(credential_ptr as *const c_void);
            secret
        }?;

        Ok(Some(result))
    }

    pub fn write_secret(target: &str, secret: &str) -> Result<(), String> {
        let target = wide_null(target);
        let username = wide_null("wisprflow");
        let mut blob = secret.as_bytes().to_vec();

        let credential = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_ptr() as *mut _),
            Comment: PWSTR::null(),
            LastWritten: FILETIME::default(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: null_mut::<CREDENTIAL_ATTRIBUTEW>(),
            TargetAlias: PWSTR::null(),
            UserName: PWSTR(username.as_ptr() as *mut _),
        };

        unsafe { CredWriteW(&credential, 0) }.map_err(|e| {
            format!(
                "Failed to write API key to Windows Credential Manager: {}",
                e
            )
        })
    }

    pub fn delete_secret(target: &str) -> Result<(), String> {
        let target = wide_null(target);
        unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0) }.map_err(|e| {
            format!(
                "Failed to delete API key from Windows Credential Manager: {}",
                e
            )
        })
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod credential_manager {
    pub fn read_secret(_target: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn write_secret(_target: &str, _secret: &str) -> Result<(), String> {
        Err("Windows Credential Manager is only available on Windows".to_string())
    }

    pub fn delete_secret(_target: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_wav, CloudProviderKind};

    #[test]
    fn parses_cloud_provider_names() {
        assert_eq!(
            CloudProviderKind::parse("gladia"),
            CloudProviderKind::Gladia
        );
        assert_eq!(CloudProviderKind::parse("groq"), CloudProviderKind::Groq);
        assert_eq!(
            CloudProviderKind::parse("deepgram"),
            CloudProviderKind::Deepgram
        );
        assert_eq!(
            CloudProviderKind::parse("anything"),
            CloudProviderKind::OpenAi
        );
    }

    #[test]
    fn wav_encoding_stays_in_memory() {
        let wav = encode_wav(&[0.0, 1.0, -1.0], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 6);
    }
}
