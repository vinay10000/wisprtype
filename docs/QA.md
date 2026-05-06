# WisprType QA Plan

This project now includes two layers of final validation:

1. Automated runtime checks from the Settings app under `Validation`.
2. A manual Windows sweep for host apps and failure modes.

## Automated Checks

Run `Validation -> Run checks` inside the app to verify:

- launch-at-login registry state matches the saved setting
- tray icon is registered
- Aurora Pill window is available
- worker executables are present next to the app binary
- dictionary seeding is ready for bias testing
- the selected cloud provider has an API key available
- the app data directory is free of persisted audio artifacts

## Manual Sweep

Use this checklist on a Windows machine:

1. End-to-end dictation
   - Hold the hotkey in Notepad, Chrome, and VS Code.
   - Confirm text is inserted after release in each target app.
2. Aurora Pill synchronization
   - Verify `Idle -> Recording -> Transcribing -> Cleaning -> Inserting`.
   - Confirm error state styling appears after a forced failure.
3. Local and cloud engines
   - Run one sample with the local engine.
   - Run one sample with the configured cloud provider.
4. Dictionary biasing
   - Add a unique jargon term in Settings.
   - Speak the term and confirm the transcript preserves the custom spelling.
5. No audio on disk
   - Review `%APPDATA%\\WisprWin`.
   - Confirm no `.wav`, `.mp3`, `.pcm`, `.raw`, or related audio artifacts appear.
6. Failure modes
   - deny microphone permission
   - save an invalid cloud API key
   - configure a hotkey already owned by another application
   - confirm each case surfaces a recoverable error without crashing the app

## Exit Criteria

The slice is ready when the automated checks pass or are intentionally understood, and the manual sweep succeeds across the supported host apps.
