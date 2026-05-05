use clipboard_win::{raw, Clipboard};
use std::thread;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_CONTROL,
    VK_V,
};

const PASTE_THRESHOLD_CHARS: usize = 100;

struct ClipboardSnapshot {
    formats: Vec<(u32, Vec<u8>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InjectionMode {
    Typing,
    ClipboardPaste,
}

pub struct TextInjector;

impl TextInjector {
    pub fn inject(text: String) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }

        let preferred = Self::preferred_mode(&text);
        match Self::inject_with_mode(&text, preferred) {
            Ok(()) => Ok(()),
            Err(primary_error) => {
                let fallback = Self::fallback_mode(preferred);
                Self::inject_with_mode(&text, fallback).map_err(|fallback_error| {
                    format!(
                        "Primary {:?} injection failed: {}; fallback {:?} injection failed: {}",
                        preferred, primary_error, fallback, fallback_error
                    )
                })
            }
        }
    }

    fn preferred_mode(text: &str) -> InjectionMode {
        if text.chars().count() < PASTE_THRESHOLD_CHARS {
            InjectionMode::Typing
        } else {
            InjectionMode::ClipboardPaste
        }
    }

    fn fallback_mode(mode: InjectionMode) -> InjectionMode {
        match mode {
            InjectionMode::Typing => InjectionMode::ClipboardPaste,
            InjectionMode::ClipboardPaste => InjectionMode::Typing,
        }
    }

    fn inject_with_mode(text: &str, mode: InjectionMode) -> Result<(), String> {
        match mode {
            InjectionMode::Typing => Self::simulate_typing(text),
            InjectionMode::ClipboardPaste => Self::paste_text(text),
        }
    }

    fn simulate_typing(text: &str) -> Result<(), String> {
        let mut inputs: Vec<INPUT> = Vec::new();

        for c in text.encode_utf16() {
            // Key down
            let mut down = INPUT {
                r#type: INPUT_KEYBOARD,
                ..Default::default()
            };
            down.Anonymous.ki = KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: c,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: 0,
            };
            inputs.push(down);

            // Key up
            let mut up = INPUT {
                r#type: INPUT_KEYBOARD,
                ..Default::default()
            };
            up.Anonymous.ki = KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: c,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            };
            inputs.push(up);
        }

        unsafe {
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent != inputs.len() as u32 {
                return Err(format!(
                    "SendInput sent {} of {} inputs",
                    sent,
                    inputs.len()
                ));
            }
        }

        Ok(())
    }

    fn paste_text(text: &str) -> Result<(), String> {
        let previous_content = Self::save_clipboard()?;

        {
            let _clipboard = Clipboard::new_attempts(10).map_err(|e| e.to_string())?;
            raw::empty().map_err(|e| e.to_string())?;
            raw::set_string(text).map_err(|e| e.to_string())?;
        }

        let mut inputs: Vec<INPUT> = Vec::new();

        // Ctrl down
        let mut ctrl_down = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        ctrl_down.Anonymous.ki = KEYBDINPUT {
            wVk: VK_CONTROL,
            wScan: 0,
            dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
            time: 0,
            dwExtraInfo: 0,
        };
        inputs.push(ctrl_down);

        // V down
        let mut v_down = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        v_down.Anonymous.ki = KEYBDINPUT {
            wVk: VK_V,
            wScan: 0,
            dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
            time: 0,
            dwExtraInfo: 0,
        };
        inputs.push(v_down);

        // V up
        let mut v_up = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        v_up.Anonymous.ki = KEYBDINPUT {
            wVk: VK_V,
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        };
        inputs.push(v_up);

        // Ctrl up
        let mut ctrl_up = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        ctrl_up.Anonymous.ki = KEYBDINPUT {
            wVk: VK_CONTROL,
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        };
        inputs.push(ctrl_up);

        unsafe {
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent != inputs.len() as u32 {
                let _ = Self::restore_clipboard(previous_content);
                return Err(format!(
                    "SendInput sent {} of {} paste inputs",
                    sent,
                    inputs.len()
                ));
            }
        }

        // Yield briefly to let the OS process the Ctrl+V before restoring clipboard
        thread::sleep(Duration::from_millis(150));

        Self::restore_clipboard(previous_content)
    }

    fn save_clipboard() -> Result<ClipboardSnapshot, String> {
        let _clipboard = Clipboard::new_attempts(10).map_err(|e| e.to_string())?;
        let mut formats = Vec::new();

        for format in raw::EnumFormats::new() {
            let mut data = Vec::new();
            if raw::get_vec(format, &mut data).is_ok() {
                formats.push((format, data));
            }
        }

        Ok(ClipboardSnapshot { formats })
    }

    fn restore_clipboard(snapshot: ClipboardSnapshot) -> Result<(), String> {
        let _clipboard = Clipboard::new_attempts(10).map_err(|e| e.to_string())?;
        raw::empty().map_err(|e| e.to_string())?;

        for (format, data) in snapshot.formats {
            raw::set_without_clear(format, &data).map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{InjectionMode, TextInjector};

    #[test]
    fn short_text_prefers_typing() {
        assert_eq!(
            TextInjector::preferred_mode("short note"),
            InjectionMode::Typing
        );
    }

    #[test]
    fn long_text_prefers_clipboard_paste() {
        assert_eq!(
            TextInjector::preferred_mode(&"a".repeat(100)),
            InjectionMode::ClipboardPaste
        );
    }

    #[test]
    fn fallback_mode_switches_to_the_other_injection_path() {
        assert_eq!(
            TextInjector::fallback_mode(InjectionMode::Typing),
            InjectionMode::ClipboardPaste
        );
        assert_eq!(
            TextInjector::fallback_mode(InjectionMode::ClipboardPaste),
            InjectionMode::Typing
        );
    }
}
