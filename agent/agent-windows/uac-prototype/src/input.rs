//! Remote input events and the rules for admitting them, independent of how they are injected.

use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// Pointer position as a fraction of the captured screen.
    Move {
        x: f64,
        y: f64,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// Wheel delta in Windows units: 120 per notch, positive scrolls up.
    Wheel {
        delta: i32,
    },
    /// `code` is the physical key as the browser reports it (`KeyboardEvent.code`).
    Key {
        code: String,
        down: bool,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// Set-1 scan code plus the extended-key flag.
pub type Scan = (u16, bool);

/// Maps a fraction of the screen to the 0..=65535 range of absolute pointer input.
pub fn absolute(fraction: f64) -> i32 {
    if fraction.is_nan() {
        return 0;
    }
    (fraction.clamp(0.0, 1.0) * 65535.0).round() as i32
}

/// At most three notches per event, so one message cannot scroll a page away.
pub fn wheel(delta: i32) -> i32 {
    delta.clamp(-360, 360)
}

/// Scan codes follow the physical key, so the target's own layout (for example Hungarian) decides the character.
pub fn scancode(code: &str) -> Option<Scan> {
    if let Some(letter) = code.strip_prefix("Key")
        && let [c @ b'A'..=b'Z'] = letter.as_bytes()
    {
        const LETTERS: [u16; 26] = [
            0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31,
            0x18, 0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
        ];
        return Some((LETTERS[usize::from(c - b'A')], false));
    }
    if let Some(digit) = code.strip_prefix("Digit")
        && let [d @ b'0'..=b'9'] = digit.as_bytes()
    {
        return Some((
            if *d == b'0' {
                0x0B
            } else {
                u16::from(d - b'0') + 1
            },
            false,
        ));
    }
    if let Some(number) = code.strip_prefix('F')
        && let Ok(n @ 1..=12) = number.parse::<u16>()
    {
        return Some((if n <= 10 { 0x3A + n } else { 0x4C + n }, false));
    }
    let scan = match code {
        "Escape" => (0x01, false),
        "Minus" => (0x0C, false),
        "Equal" => (0x0D, false),
        "Backspace" => (0x0E, false),
        "Tab" => (0x0F, false),
        "BracketLeft" => (0x1A, false),
        "BracketRight" => (0x1B, false),
        "Enter" => (0x1C, false),
        "ControlLeft" => (0x1D, false),
        "Semicolon" => (0x27, false),
        "Quote" => (0x28, false),
        "Backquote" => (0x29, false),
        "ShiftLeft" => (0x2A, false),
        "Backslash" => (0x2B, false),
        "Comma" => (0x33, false),
        "Period" => (0x34, false),
        "Slash" => (0x35, false),
        "ShiftRight" => (0x36, false),
        "NumpadMultiply" => (0x37, false),
        "AltLeft" => (0x38, false),
        "Space" => (0x39, false),
        "CapsLock" => (0x3A, false),
        "NumLock" => (0x45, true),
        "ScrollLock" => (0x46, false),
        "Numpad7" => (0x47, false),
        "Numpad8" => (0x48, false),
        "Numpad9" => (0x49, false),
        "NumpadSubtract" => (0x4A, false),
        "Numpad4" => (0x4B, false),
        "Numpad5" => (0x4C, false),
        "Numpad6" => (0x4D, false),
        "NumpadAdd" => (0x4E, false),
        "Numpad1" => (0x4F, false),
        "Numpad2" => (0x50, false),
        "Numpad3" => (0x51, false),
        "Numpad0" => (0x52, false),
        "NumpadDecimal" => (0x53, false),
        "IntlBackslash" => (0x56, false),
        "NumpadEnter" => (0x1C, true),
        "ControlRight" => (0x1D, true),
        "NumpadDivide" => (0x35, true),
        "PrintScreen" => (0x37, true),
        "AltRight" => (0x38, true),
        "Home" => (0x47, true),
        "ArrowUp" => (0x48, true),
        "PageUp" => (0x49, true),
        "ArrowLeft" => (0x4B, true),
        "ArrowRight" => (0x4D, true),
        "End" => (0x4F, true),
        "ArrowDown" => (0x50, true),
        "PageDown" => (0x51, true),
        "Insert" => (0x52, true),
        "Delete" => (0x53, true),
        "MetaLeft" => (0x5B, true),
        "MetaRight" => (0x5C, true),
        "ContextMenu" => (0x5D, true),
        _ => return None,
    };
    Some(scan)
}

/// What still has to be released on the desktop the injector is attached to.
#[derive(Debug, Default, PartialEq)]
pub struct Release {
    pub keys: Vec<Scan>,
    pub buttons: Vec<Button>,
}

pub struct Admission {
    /// The input desktop changed: attach to the new one, then release these there.
    pub switched: Option<Release>,
    /// False when the viewer produced the event while it still saw an earlier desktop.
    pub inject: bool,
}

/// Admits events only for the desktop the viewer actually saw, and tracks what is held down.
#[derive(Default)]
pub struct Gate {
    attached: Option<u64>,
    keys: BTreeSet<Scan>,
    buttons: BTreeSet<Button>,
}

impl Gate {
    pub fn admit(&mut self, surface: u64, current: u64) -> Admission {
        let switched = (self.attached != Some(current)).then(|| {
            self.attached = Some(current);
            self.release()
        });
        Admission {
            switched,
            inject: surface == current,
        }
    }

    pub fn key(&mut self, scan: Scan, down: bool) {
        if down {
            self.keys.insert(scan);
        } else {
            self.keys.remove(&scan);
        }
    }

    pub fn button(&mut self, button: Button, down: bool) {
        if down {
            self.buttons.insert(button);
        } else {
            self.buttons.remove(&button);
        }
    }

    pub fn release(&mut self) -> Release {
        Release {
            keys: std::mem::take(&mut self.keys).into_iter().collect(),
            buttons: std::mem::take(&mut self.buttons).into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_viewer_events() {
        let parse = |v: &str| serde_json::from_str::<Event>(v).unwrap();
        assert_eq!(
            parse(r#"{"kind":"move","x":0.5,"y":1}"#),
            Event::Move { x: 0.5, y: 1.0 }
        );
        assert_eq!(
            parse(r#"{"kind":"button","button":"right","down":true}"#),
            Event::Button {
                button: Button::Right,
                down: true
            }
        );
        assert_eq!(
            parse(r#"{"kind":"key","code":"KeyA","down":false}"#),
            Event::Key {
                code: "KeyA".into(),
                down: false
            }
        );
        assert!(serde_json::from_str::<Event>(r#"{"kind":"sas"}"#).is_err());
    }

    #[test]
    fn maps_pointer_and_wheel_into_bounds() {
        assert_eq!(absolute(0.0), 0);
        assert_eq!(absolute(1.0), 65535);
        assert_eq!(absolute(0.5), 32768);
        assert_eq!(absolute(-3.0), 0);
        assert_eq!(absolute(7.0), 65535);
        assert_eq!(absolute(f64::NAN), 0);
        assert_eq!(wheel(-120), -120);
        assert_eq!(wheel(100_000), 360);
    }

    #[test]
    fn scan_codes_follow_physical_keys() {
        assert_eq!(scancode("KeyA"), Some((0x1E, false)));
        assert_eq!(scancode("KeyZ"), Some((0x2C, false)));
        assert_eq!(scancode("KeyQ"), Some((0x10, false)));
        assert_eq!(scancode("Digit1"), Some((0x02, false)));
        assert_eq!(scancode("Digit0"), Some((0x0B, false)));
        assert_eq!(scancode("F1"), Some((0x3B, false)));
        assert_eq!(scancode("F10"), Some((0x44, false)));
        assert_eq!(scancode("F11"), Some((0x57, false)));
        assert_eq!(scancode("F12"), Some((0x58, false)));
        assert_eq!(scancode("Enter"), Some((0x1C, false)));
        assert_eq!(scancode("NumpadEnter"), Some((0x1C, true)));
        assert_eq!(scancode("AltRight"), Some((0x38, true)));
        assert_eq!(scancode("IntlBackslash"), Some((0x56, false)));
        assert_eq!(scancode("Delete"), Some((0x53, true)));
        assert_eq!(scancode("F13"), None);
        assert_eq!(scancode("KeyAA"), None);
        assert_eq!(scancode("Unidentified"), None);
    }

    #[test]
    fn drops_stale_input_and_releases_held_keys_on_a_desktop_switch() {
        let mut gate = Gate::default();
        let first = gate.admit(1, 1);
        assert_eq!(first.switched, Some(Release::default()));
        assert!(first.inject);
        gate.key((0x2A, false), true);
        gate.button(Button::Left, true);
        let same = gate.admit(1, 1);
        assert!(same.switched.is_none() && same.inject);

        let switched = gate.admit(1, 2);
        assert!(!switched.inject, "input produced for the old desktop");
        assert_eq!(
            switched.switched,
            Some(Release {
                keys: vec![(0x2A, false)],
                buttons: vec![Button::Left],
            })
        );
        let fresh = gate.admit(2, 2);
        assert!(fresh.switched.is_none() && fresh.inject);

        gate.key((0x1D, false), true);
        gate.key((0x1D, false), false);
        assert_eq!(gate.release(), Release::default());
    }
}
