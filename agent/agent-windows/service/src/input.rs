//! Turns browser input into the events the worker injects. Keys travel as physical scan codes, so
//! the target's own keyboard layout decides the character; held keys and buttons are tracked so
//! everything still pressed can be released when the session ends.

use std::collections::BTreeSet;

/// A physical key: set-1 scan code and whether it needs the extended prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scan {
    pub code: u16,
    pub extended: bool,
}

const fn key(code: u16) -> Scan {
    Scan {
        code,
        extended: false,
    }
}

const fn extended(code: u16) -> Scan {
    Scan {
        code,
        extended: true,
    }
}

/// The scan code for a `KeyboardEvent.code` name; unknown names are not injected.
pub fn scan(code: &str) -> Option<Scan> {
    const LETTERS: [u16; 26] = [
        0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
    ];
    const FUNCTION: [u16; 12] = [
        0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x57, 0x58,
    ];
    if let Some(letter) = code.strip_prefix("Key")
        && let [byte @ b'A'..=b'Z'] = letter.as_bytes()
    {
        return Some(key(LETTERS[usize::from(byte - b'A')]));
    }
    if let Some(digit) = code.strip_prefix("Digit")
        && let [byte @ b'0'..=b'9'] = digit.as_bytes()
    {
        let index = u16::from(byte - b'0');
        return Some(key(if index == 0 { 0x0b } else { 0x01 + index }));
    }
    if let Some(number) = code.strip_prefix('F')
        && let Ok(index @ 1..=12) = number.parse::<usize>()
    {
        return Some(key(FUNCTION[index - 1]));
    }
    Some(match code {
        "Escape" => key(0x01),
        "Minus" => key(0x0c),
        "Equal" => key(0x0d),
        "Backspace" => key(0x0e),
        "Tab" => key(0x0f),
        "BracketLeft" => key(0x1a),
        "BracketRight" => key(0x1b),
        "Enter" => key(0x1c),
        "ControlLeft" => key(0x1d),
        "Semicolon" => key(0x27),
        "Quote" => key(0x28),
        "Backquote" => key(0x29),
        "ShiftLeft" => key(0x2a),
        "Backslash" => key(0x2b),
        "Comma" => key(0x33),
        "Period" => key(0x34),
        "Slash" => key(0x35),
        "ShiftRight" => key(0x36),
        "NumpadMultiply" => key(0x37),
        "AltLeft" => key(0x38),
        "Space" => key(0x39),
        "CapsLock" => key(0x3a),
        "NumLock" => key(0x45),
        "ScrollLock" => key(0x46),
        "Numpad7" => key(0x47),
        "Numpad8" => key(0x48),
        "Numpad9" => key(0x49),
        "NumpadSubtract" => key(0x4a),
        "Numpad4" => key(0x4b),
        "Numpad5" => key(0x4c),
        "Numpad6" => key(0x4d),
        "NumpadAdd" => key(0x4e),
        "Numpad1" => key(0x4f),
        "Numpad2" => key(0x50),
        "Numpad3" => key(0x51),
        "Numpad0" => key(0x52),
        "NumpadDecimal" => key(0x53),
        // The extra key next to the left Shift on ISO keyboards, such as the Hungarian í.
        "IntlBackslash" => key(0x56),
        "NumpadEnter" => extended(0x1c),
        "ControlRight" => extended(0x1d),
        "NumpadDivide" => extended(0x35),
        "AltRight" => extended(0x38),
        "Home" => extended(0x47),
        "ArrowUp" => extended(0x48),
        "PageUp" => extended(0x49),
        "ArrowLeft" => extended(0x4b),
        "ArrowRight" => extended(0x4d),
        "End" => extended(0x4f),
        "ArrowDown" => extended(0x50),
        "PageDown" => extended(0x51),
        "Insert" => extended(0x52),
        "Delete" => extended(0x53),
        "MetaLeft" => extended(0x5b),
        "MetaRight" => extended(0x5c),
        "ContextMenu" => extended(0x5d),
        _ => return None,
    })
}

/// One mouse button as the browser's `buttons` bitmask numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

const BUTTONS: [(u8, Button); 5] = [
    (1, Button::Left),
    (2, Button::Right),
    (4, Button::Middle),
    (8, Button::Back),
    (16, Button::Forward),
];

/// What to inject next, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Moves to a point of the virtual desktop, in pixels.
    Move {
        x: i32,
        y: i32,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// Wheel steps in Windows units; positive is up or right.
    Wheel {
        vertical: i32,
        horizontal: i32,
    },
    Key {
        scan: Scan,
        down: bool,
    },
    /// One UTF-16 unit typed as a character, independent of the keyboard layout.
    Unicode {
        unit: u16,
        down: bool,
    },
}

/// The modifier bits a pointer event carries, with the keys that stand for each.
const MODIFIERS: [(u8, &str, &str); 4] = [
    (1, "ControlLeft", "ControlRight"),
    (2, "ShiftLeft", "ShiftRight"),
    (4, "AltLeft", "AltRight"),
    (8, "MetaLeft", "MetaRight"),
];

/// Characters of one text input that are typed; the rest is ignored.
const TEXT_LIMIT: usize = 64;

/// The pressed state, so every press is released once and only known keys are injected.
#[derive(Default)]
pub struct Held {
    buttons: u8,
    keys: BTreeSet<Scan>,
}

impl Held {
    /// The events for pointer input at `(x, y)` of the video, mapped onto `view` (left, top,
    /// width, height of the captured desktop area).
    pub fn pointer(
        &mut self,
        x: f64,
        y: f64,
        buttons: u8,
        modifiers: u8,
        view: (i32, i32, u32, u32),
    ) -> Vec<Event> {
        let mut events = self.modifiers(modifiers);
        let (left, top, width, height) = view;
        let at = |unit: f64, start: i32, size: u32| {
            start + (unit.clamp(0.0, 1.0) * f64::from(size.saturating_sub(1))).round() as i32
        };
        events.push(Event::Move {
            x: at(x, left, width),
            y: at(y, top, height),
        });
        let changed = self.buttons ^ buttons;
        for (bit, button) in BUTTONS {
            if changed & bit != 0 {
                events.push(Event::Button {
                    button,
                    down: buttons & bit != 0,
                });
            }
        }
        self.buttons = buttons & 0x1f;
        events
    }

    /// Brings the held modifier keys in line with what the pointer event says is held, so a
    /// Ctrl+click works without keyboard focus and a modifier missed on release does not stick.
    fn modifiers(&mut self, modifiers: u8) -> Vec<Event> {
        let mut events = Vec::new();
        for (bit, left, right) in MODIFIERS {
            let (Some(left), Some(right)) = (scan(left), scan(right)) else {
                continue;
            };
            let held = self.keys.contains(&left) || self.keys.contains(&right);
            if modifiers & bit != 0 && !held {
                self.keys.insert(left);
                events.push(Event::Key {
                    scan: left,
                    down: true,
                });
            } else if modifiers & bit == 0 && held {
                for scan in [left, right] {
                    if self.keys.remove(&scan) {
                        events.push(Event::Key { scan, down: false });
                    }
                }
            }
        }
        events
    }

    /// Text typed as characters: each UTF-16 unit pressed and released. Line breaks become Enter
    /// and tabs Tab, as character input; nothing else outside printable text is typed.
    pub fn text(&self, text: &str) -> Vec<Event> {
        let mut events = Vec::new();
        for character in text.chars().take(TEXT_LIMIT) {
            let character = match character {
                '\n' => '\r',
                '\t' => '\t',
                c if c.is_control() => continue,
                c => c,
            };
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units) {
                events.push(Event::Unicode {
                    unit: *unit,
                    down: true,
                });
                events.push(Event::Unicode {
                    unit: *unit,
                    down: false,
                });
            }
        }
        events
    }

    /// Browser wheel deltas point down and right; Windows wheel units point up and right.
    pub fn wheel(&self, dx: i16, dy: i16) -> Event {
        Event::Wheel {
            vertical: -i32::from(dy),
            horizontal: i32::from(dx),
        }
    }

    /// A key event, or `None` for an unknown key or a release of a key that is not held.
    pub fn key(&mut self, code: &str, down: bool) -> Option<Event> {
        let scan = scan(code)?;
        if down {
            self.keys.insert(scan);
        } else if !self.keys.remove(&scan) {
            return None;
        }
        Some(Event::Key { scan, down })
    }

    /// Releases everything still pressed, keys first.
    pub fn release(&mut self) -> Vec<Event> {
        let mut events: Vec<_> = std::mem::take(&mut self.keys)
            .into_iter()
            .map(|scan| Event::Key { scan, down: false })
            .collect();
        for (bit, button) in BUTTONS {
            if self.buttons & bit != 0 {
                events.push(Event::Button {
                    button,
                    down: false,
                });
            }
        }
        self.buttons = 0;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_keys_map_to_scan_codes() {
        assert_eq!(scan("KeyA"), Some(key(0x1e)));
        assert_eq!(scan("KeyZ"), Some(key(0x2c)));
        assert_eq!(scan("Digit0"), Some(key(0x0b)));
        assert_eq!(scan("Digit1"), Some(key(0x02)));
        assert_eq!(scan("F12"), Some(key(0x58)));
        assert_eq!(scan("IntlBackslash"), Some(key(0x56)));
        assert_eq!(scan("ArrowLeft"), Some(extended(0x4b)));
        assert_eq!(scan("AltRight"), Some(extended(0x38)));
        for unknown in ["", "Keya", "KeyAA", "F0", "F13", "Digit10", "Power"] {
            assert_eq!(scan(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn pointer_modifiers_are_pressed_and_released_with_the_pointer() {
        let mut held = Held::default();
        let view = (0, 0, 1920, 1080);
        // Ctrl+click without keyboard focus: Ctrl goes down before the button.
        let events = held.pointer(0.5, 0.5, 1, 1, view);
        assert_eq!(
            events[0],
            Event::Key {
                scan: key(0x1d),
                down: true
            }
        );
        assert!(matches!(events[1], Event::Move { .. }));
        // Still held: nothing new; then released with the next pointer event.
        assert!(matches!(
            held.pointer(0.5, 0.5, 0, 1, view)[0],
            Event::Move { .. }
        ));
        assert_eq!(
            held.pointer(0.5, 0.5, 0, 0, view)[0],
            Event::Key {
                scan: key(0x1d),
                down: false
            }
        );
        // A right Shift pressed by key counts as Shift; a missed release does not stick.
        assert!(held.key("ShiftRight", true).is_some());
        assert!(matches!(
            held.pointer(0.5, 0.5, 0, 2, view)[0],
            Event::Move { .. }
        ));
        let released = held.pointer(0.5, 0.5, 0, 0, view);
        assert_eq!(
            released[0],
            Event::Key {
                scan: Scan {
                    code: 0x36,
                    extended: false
                },
                down: false
            }
        );
        assert!(held.release().is_empty());
    }

    #[test]
    fn text_is_typed_as_characters_whatever_the_layout() {
        let held = Held::default();
        let events = held.text("\u{151}\u{170}\n\u{1f600}\u{7}");
        let units: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Unicode { unit, down: true } => Some(*unit),
                _ => None,
            })
            .collect();
        // Emoji outside the basic plane travel as a surrogate pair; control characters are not typed.
        assert_eq!(units, [0x0151, 0x0170, 0x000d, 0xd83d, 0xde00]);
        assert_eq!(events.len(), 10, "each unit pressed and released");
        assert_eq!(held.text(&"x".repeat(100)).len(), 2 * TEXT_LIMIT);
    }

    #[test]
    fn pointer_input_lands_on_the_view_and_presses_are_released_once() {
        let mut held = Held::default();
        let view = (-1920, 0, 1920, 1080);
        let events = held.pointer(0.0, 1.0, 1, 0, view);
        assert_eq!(events[0], Event::Move { x: -1920, y: 1079 });
        assert_eq!(
            events[1],
            Event::Button {
                button: Button::Left,
                down: true
            }
        );
        assert_eq!(
            held.pointer(1.0, 0.0, 1 | 2, 0, view)[1..],
            [Event::Button {
                button: Button::Right,
                down: true
            }]
        );
        assert_eq!(held.pointer(0.5, 0.5, 2, 0, view).len(), 2);
        assert_eq!(
            held.key("KeyA", true),
            Some(Event::Key {
                scan: key(0x1e),
                down: true
            })
        );
        assert_eq!(held.key("KeyB", false), None, "never pressed");
        assert_eq!(held.key("Bogus", true), None);
        let released = held.release();
        assert_eq!(
            released,
            [
                Event::Key {
                    scan: key(0x1e),
                    down: false
                },
                Event::Button {
                    button: Button::Right,
                    down: false
                }
            ]
        );
        assert!(held.release().is_empty());
        assert_eq!(
            held.wheel(5, 120),
            Event::Wheel {
                vertical: -120,
                horizontal: 5
            }
        );
    }
}
