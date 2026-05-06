# Resource Optimization Notes

Issue #8 adds the following low-footprint changes:

- the core engine now blocks on the global hotkey receiver instead of busy polling every 10ms
- worker processes are still spawned lazily and are released on app shutdown
- audio capture buffers stay in RAM and are cleared after each transcription pass
- closing the main window hides to tray instead of tearing the app down and rebuilding state
- app shutdown now unregisters the hotkey and joins the engine thread before exit

## How To Sample

Use Task Manager or PowerShell while WisprType is idle in the tray:

```powershell
Get-Process wisprtype | Select-Object ProcessName, CPU, WS, PM
```

Repeat once during an active transcription to compare idle and active footprints.

## Expected Outcome

- near-idle CPU usage while waiting for the hotkey
- stable background RAM usage without growth across repeated dictation cycles
- no orphaned worker processes after quitting from the tray
