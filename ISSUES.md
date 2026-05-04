# Project Issues - WisprType for Windows

## Parent PRD
Reference the Technical Specification in the project root.

---

## Issue #1: Tracer Bullet: Core Path
**Type:** AFK | **Status:** pending | **Blocked by:** None

### What to Build
Implement the minimum viable path through the system:
`Hotkey Press` $\rightarrow$ `Audio Capture (WASAPI)` $\rightarrow$ `Basic Transcription (whisper.cpp)` $\rightarrow$ `System Text Injection (SendInput)`.
This slice proves the core native architecture connects end-to-end.

### Boundary Map
#### Produces
- `src/core/engine.rs` $\rightarrow$ `CoreEngine::run()`
- `src/core/audio.rs` $\rightarrow$ `AudioCapturer` (PCM stream)
- `src/core/stt.rs` $\rightarrow$ `BasicTranscriber` (Interface for transcription)
- `src/core/injection.rs` $\rightarrow$ `TextInjector::inject(text: String)`

#### Consumes
- Nothing — this is a leaf node.

### Acceptance Criteria
- [ ] Holding a hotkey starts audio recording.
- [ ] Releasing the hotkey triggers transcription.
- [ ] **Injection:** For short strings (<100 chars), use `SendInput` keystrokes. For longer strings, use `Ctrl+V` (must restore original clipboard content after injection).
- [ ] Audio buffers are handled in RAM and not written to disk.
- [ ] **Error Handling:** Gracefully handle and report (via Tauri event) if microphone access is denied.
- [ ] **Error Handling:** Handle STT engine crashes without taking down the entire app.

### Assumptions from Parent PRD
- [ ] `cpal` or `wasapi` crates provide sufficient low-latency access on Windows.
- [ ] `whisper.cpp` Rust bindings can be compiled for Windows.
- [ ] `SendInput` is effective for the majority of Windows applications.

### User Stories Addressed
User story 1, 2, 4, 7

---

## Issue #2: The Aurora Pill UI
**Type:** HITL | **Status:** pending | **Blocked by:** #1

### What to Build
Implement the "Aurora Pill" visual overlay using Tauri and React. The UI should be a borderless, transparent window that stays on top and doesn't intercept mouse events.

### Boundary Map
#### Produces
- `src-tauri/src/main.rs` $\rightarrow$ Window configuration (`set_ignore_cursor_events`)
- `src/components/Pill.tsx` $\rightarrow$ Visual state animations (Glow/Pulse)
- `src/hooks/useCoreEvents.ts` $\rightarrow$ Tauri event listener for core states

#### Consumes
- From #1: `src/core/engine.rs` $\rightarrow$ Events: `recording`, `transcribing`, `cleaning`, `inserting`.

### Acceptance Criteria
- [ ] The Pill is visible, always-on-top, and does not block clicks.
- [ ] The Pill changes color/animation based on the current state emitted by the core.
- [ ] Animations are fluid (60fps) and match the high-end macOS aesthetic.
- [ ] **Error State:** The Pill displays a distinct "Error" state/color when a failure event is received from the core.

### Assumptions from Parent PRD
- [ ] Tauri's window API allows for the required transparency and "click-through" behavior on Windows.

### User Stories Addressed
User story 3

---

## Issue #3: Intelligent Refinement
**Type:** AFK | **Status:** pending | **Blocked by:** #1

### What to Build
Integrate a local SLM (Phi-3 Mini or Gemma 2B) via `llama.cpp` to clean up the raw transcript.

### Boundary Map
#### Produces
- `src/core/refinement.rs` $\rightarrow$ `RefinementEngine::clean(raw_text: String) -> String`

#### Consumes
- From #1: `src/core/stt.rs` $\rightarrow$ Raw transcription output.

### Acceptance Criteria
- [ ] Filler words ("um", "uh") are removed.
- [ ] Punctuation and capitalization are correctly applied.
- [ ] The original meaning is preserved without introducing hallucinations.
- [ ] **Error Handling:** If the SLM fails to load or crashes, the system falls back to the raw transcript without blocking the injection pipeline.

### Assumptions from Parent PRD
- [ ] Local SLMs can run with acceptable latency (<500ms) on target hardware.

### User Stories Addressed
User story 5

---

## Issue #4: Advanced STT & GPU Acceleration
**Type:** AFK | **Status:** pending | **Blocked by:** #1

### What to Build
Replace the basic transcriber with a production-grade implementation supporting CUDA (NVIDIA) and OpenVINO (Intel) acceleration and multiple model sizes.

### Boundary Map
#### Produces
- `src/core/stt/model_manager.rs` $\rightarrow$ `ModelManager` (Download/Load/Swap models)
- `src/core/stt/backends.rs` $\rightarrow$ CUDA and OpenVINO optimized implementations.

#### Consumes
- From #1: `src/core/stt.rs` $\rightarrow$ Transcription trait/interface.
- From #7: `SettingsUI` $\rightarrow$ Model selection preference.

### Acceptance Criteria
- [ ] GPU acceleration significantly reduces transcription time compared to CPU.
- [ ] Users can switch between model sizes (Tiny, Base, Small, Medium, Large) via settings.
- [ ] Models are stored correctly in `%AppData%/WisprWin/models`.
- [ ] **Error Handling:** Handle "Model Not Found" by triggering an automatic download or prompting the user via the UI.
- [ ] **Error Handling:** Fall back to CPU inference if GPU initialization fails.

### Assumptions from Parent PRD
- [ ] CUDA/OpenVINO drivers are available on the user's system.

### User Stories Addressed
User story 4, 8

---

## Issue #5: Cloud Engine Integration
**Type:** AFK | **Status:** pending | **Blocked by:** #1

### What to Build
Implement a provider interface to allow the use of cloud-based STT engines (OpenAI, Groq, Deepgram) when local inference is not desired.

### Boundary Map
#### Produces
- `src/core/stt/cloud.rs` $\rightarrow$ `CloudProvider` trait and implementations for OpenAI, Groq, Deepgram.

#### Consumes
- From #1: `src/core/stt.rs` $\rightarrow$ Transcription trait/interface.
- From #7: `SettingsUI` $\rightarrow$ API keys and provider selection.

### Acceptance Criteria
- [ ] API keys are stored securely in Windows Credential Manager.
- [ ] Transcription is successfully routed to the selected cloud provider.
- [ ] Cloud mode provides a fallback if local inference fails or is disabled.
- [ ] **Error Handling:** Detect and report invalid API keys or "Quota Exceeded" errors immediately.
- [ ] **Error Handling:** Implement a timeout for cloud requests to prevent the UI from hanging.

### Assumptions from Parent PRD
- [ ] API latency for Groq/Deepgram is low enough to maintain a fluid UX.

### User Stories Addressed
User story 9

---

## Issue #6: Custom Dictionary
**Type:** AFK | **Status:** pending | **Blocked by:** #1

### What to Build
Implement an SQLite-backed dictionary that allows users to define jargon, which is then used to bias the STT engine.

### Boundary Map
#### Produces
- `src/core/dictionary.rs` $\rightarrow$ `DictionaryStore` (CRUD for terms)
- `src/core/stt/prompting.rs` $\rightarrow$ Logic to inject dictionary terms into STT initial prompts.

#### Consumes
- From #1/4/5: Prompt interfaces of the active STT engine.
- From #7: `SettingsUI` $\rightarrow$ Dictionary management interface.

### Acceptance Criteria
- [ ] Users can add/remove custom terms via the UI.
- [ ] Specialized terms are transcribed with higher accuracy when present in the dictionary.
- [ ] **Error Handling:** Handle SQLite database corruption or "Disk Full" errors during save.

### Assumptions from Parent PRD
- [ ] `whisper.cpp` and cloud APIs support initial prompts/biasing for transcription.

### User Stories Addressed
User story 6

---

## Issue #7: Settings UI
**Type:** HITL | **Status:** pending | **Blocked by:** #2

### What to Build
Implement a comprehensive settings panel within the Tauri app. This is the primary control center for the application's behavior.

### Boundary Map
#### Produces
- `src/pages/Settings.tsx` $\rightarrow$ Settings view.
- `src/store/settingsStore.ts` $\rightarrow$ Global state for user preferences (model, provider, hotkeys, API keys).
- `src-tauri/src/settings.rs` $\rightarrow$ Persistence layer for settings.

#### Consumes
- From #2: `PillWindow` $\rightarrow$ Toggle for UI visibility/style.

### Acceptance Criteria
- [ ] Users can select STT model size (Tiny $\rightarrow$ Large).
- [ ] Users can switch between Local and Cloud engines.
- [ ] Secure input fields for API keys (password masking).
- [ ] Interface for adding/removing dictionary terms.
- [ ] Hotkey re-mapping interface.

### Assumptions from Parent PRD
- [ ] Tauri's state management can be synchronized efficiently between the frontend and the Rust backend.

### User Stories Addressed
User story 6, 8, 9

---

## Issue #8: System Integration & Optimization
**Type:** AFK | **Status:** pending | **Blocked by:** All Previous Slices

### What to Build
Finalize the Windows integration, including startup behavior, system tray presence, and resource optimization.

### Boundary Map
#### Produces
- `src-tauri/src/system.rs` $\rightarrow$ Registry keys for startup, Tray icon logic.
- Memory profiling report and optimizations.

#### Consumes
- All previous modules.

### Acceptance Criteria
- [ ] App launches automatically on system login.
- [ ] App runs in the background (tray) with minimal CPU/RAM usage.
- [ ] Application closes cleanly and releases all resources.
- [ ] **Error Handling:** Detect and warn the user if the chosen hotkey is currently registered by another high-priority application.

### Assumptions from Parent PRD
- [ ] Registry modifications are permitted by the user's system permissions.

### User Stories Addressed
User story 10, 11

---

## Issue #9: Final QA & Validation
**Type:** HITL | **Status:** pending | **Blocked by:** All Previous Slices

### What to Build
Execute a comprehensive manual and automated QA plan across various Windows environments.

### Acceptance Criteria
- [ ] E2E path (Hotkey $\rightarrow$ Text) works in Notepad, Chrome, and VS Code.
- [ ] "Aurora Pill" animations are smooth and correctly synchronized.
- [ ] Local and Cloud engines are both verified for accuracy and speed.
- [ ] Custom dictionary biasing is demonstrably effective.
- [ ] No audio data is found on disk after recording.
- [ ] All documented failure modes (mic denied, invalid API key, etc.) are handled gracefully without app crashes.

### User Stories Addressed
All (1-11)

---

## Summary & Coverage

### Dependency Graph
#1 $\rightarrow$ #2 $\rightarrow$ #7 $\rightarrow$ #4, #5, #6 $\rightarrow$ #8 $\rightarrow$ #9
#1 $\rightarrow$ #3 $\rightarrow$ #8

### Coverage Matrix
| User Story | Slice |
| :--- | :--- |
| 1. Hotkey Recording | #1 |
| 2. Fluid Processing | #1 |
| 3. Aurora Pill UI | #2 |
| 4. Local Privacy/Speed | #1, #4 |
| 5. Smart Cleanup | #3 |
| 6. Custom Dictionary | #6, #7 |
| 7. Text Injection | #1 |
| 8. Model Sizes | #4, #7 |
| 9. Cloud Engines | #5, #7 |
| 10. Startup | #8 |
| 11. Low Footprint | #8 |
