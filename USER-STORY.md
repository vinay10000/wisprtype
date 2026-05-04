## Problem Statement
Windows users lack a high-performance, privacy-first, system-wide voice-to-text tool that provides a seamless "hold-to-type" experience with intelligent auto-cleanup and custom dictionary support.

## Solution
WisprType for Windows: A low-latency application utilizing a Client-Core architecture (Tauri + Rust) that allows users to hold a hotkey, speak, and have their words instantly transcribed, refined by a local SLM, and injected into any active application.

## User Stories
1. As a user, I want to hold a customizable hotkey (e.g., Right Alt) to start recording, so that I can dictate text intuitively.
2. As a user, I want the system to process the audio immediately upon releasing the hotkey, so that the workflow is fluid.
3. As a user, I want a visual "Aurora Pill" overlay to indicate the current state (Recording $\rightarrow$ Transcribing $\rightarrow$ Cleaning $\rightarrow$ Inserting), so that I have real-time feedback.
4. As a user, I want transcription to happen locally on my device, so that my audio data remains private.
5. As a user, I want the AI to automatically remove filler words ("um", "uh") and fix punctuation, so that the output is professional and polished.
6. As a user, I want to maintain a custom dictionary for technical jargon, so that specialized terms are transcribed accurately.
7. As a user, I want the final text to be injected directly into the active application, so that I avoid manual copy-pasting.
8. As a user, I want to choose between different model sizes (Tiny to Large) to optimize for either speed or accuracy based on my hardware.
9. As a user, I want the option to use cloud-based engines (OpenAI, Groq, Deepgram) via API keys for maximum accuracy.
10. As a user, I want the application to launch at system startup, so that it is always ready for use.
11. As a user, I want the application to have a minimal memory footprint, so that it does not impact system performance.

## Implementation Decisions
- **Architecture:** Client-Core model with a Tauri (React/TS) shell and a Rust native core.
- **UI Overlay:** Borderless, transparent Tauri window using `set_ignore_cursor_events` for the Aurora Pill.
- **Audio Pipeline:** Use WASAPI via `cpal` for low-latency 16kHz Mono Float32 PCM streaming.
- **STT Engine:** `whisper.cpp` integration with CUDA (NVIDIA) and OpenVINO (Intel) acceleration.
- **Post-Processing:** Local quantized SLM (Phi-3 Mini or Gemma 2B) via `llama.cpp` for "Smart Typing" cleanup.
- **Text Injection:** Win32 `SendInput` API simulating `Ctrl+V` for paragraphs and keystrokes for short snippets.
- **Hotkey Listening:** `global-hotkey` crate to manage the press-and-hold state.
- **Storage:** SQLite database for custom dictionaries and user settings.
- **Model Management:** Local storage of `.bin` models in `%AppData%/WisprWin/models`.

## Testing Decisions
- **Deep Module Testing:** Isolated testing for the Audio Pipeline, Inference Engine, and Injection modules.
- **End-to-End Validation:** Testing the full pipeline from hotkey press to text appearance in various Windows apps (Notepad, Chrome, IDEs).
- **Latency Benchmarking:** Measuring "time-to-text" across different hardware to ensure "blazingly fast" performance.
- **Privacy Audit:** Verifying that audio buffers are held in RAM and never written to disk.

## Out of Scope
- Cloud-synced collaborative dictionaries.
- General-purpose voice assistant functionality.
- Support for non-English languages in the initial MVP.

## Further Notes
- Integration of NVIDIA TensorRT is recommended for RTX users to further reduce inference latency.
- Ensure the Tauri shell is optimized for zero-impact on system resources when idle.