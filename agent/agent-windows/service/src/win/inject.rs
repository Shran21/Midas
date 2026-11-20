//! Injects the worker's input into its own desktop with SendInput. Windows keeps a user-token
//! worker away from elevated windows and the secure desktop (UIPI), so the privileged component
//! alone may serve those.

use std::io;

use windows_sys::Win32::UI::{
    HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
    Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
        MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput,
    },
    WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, XBUTTON1, XBUTTON2,
    },
};

use crate::input::{Button, Event};

/// SendInput's absolute coordinates: 0..=65535 across the virtual desktop.
fn absolute(point: (i32, i32), desktop: (i32, i32, i32, i32)) -> (i32, i32) {
    let (left, top, width, height) = desktop;
    let scale = |value: i32, start: i32, size: i32| {
        let offset = i64::from(value - start).clamp(0, i64::from(size.max(2) - 1));
        (offset * 65_535 / i64::from(size.max(2) - 1)) as i32
    };
    (scale(point.0, left, width), scale(point.1, top, height))
}

fn mouse(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn inputs(events: &[Event], desktop: (i32, i32, i32, i32)) -> Vec<INPUT> {
    events
        .iter()
        .map(|event| match *event {
            Event::Move { x, y } => {
                let (dx, dy) = absolute((x, y), desktop);
                mouse(
                    dx,
                    dy,
                    0,
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                )
            }
            Event::Button { button, down } => {
                let (flags, data) = match (button, down) {
                    (Button::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                    (Button::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
                    (Button::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                    (Button::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
                    (Button::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                    (Button::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                    (Button::Back, true) => (MOUSEEVENTF_XDOWN, u32::from(XBUTTON1)),
                    (Button::Back, false) => (MOUSEEVENTF_XUP, u32::from(XBUTTON1)),
                    (Button::Forward, true) => (MOUSEEVENTF_XDOWN, u32::from(XBUTTON2)),
                    (Button::Forward, false) => (MOUSEEVENTF_XUP, u32::from(XBUTTON2)),
                };
                mouse(0, 0, data, flags)
            }
            Event::Wheel {
                vertical,
                horizontal,
            } => {
                if horizontal != 0 && vertical == 0 {
                    mouse(0, 0, horizontal as u32, MOUSEEVENTF_HWHEEL)
                } else {
                    mouse(0, 0, vertical as u32, MOUSEEVENTF_WHEEL)
                }
            }
            Event::Key { scan, down } => {
                let mut flags = KEYEVENTF_SCANCODE;
                if scan.extended {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: 0,
                            wScan: scan.code,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                }
            }
        })
        .collect()
}

/// Injects `events` in order; coordinates are physical pixels of the virtual desktop.
pub fn send(events: &[Event]) -> io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    // SAFETY: plain queries and a per-thread DPI setting, so the metrics are physical pixels
    // like the captured desktop.
    let desktop = unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let inputs = inputs(events, desktop);
    // SAFETY: `inputs` is a live array of fully initialized INPUT structures of the stated size.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != inputs.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Scan;

    #[test]
    fn points_scale_across_the_virtual_desktop() {
        let desktop = (-1920, 0, 3840, 1080);
        assert_eq!(absolute((-1920, 0), desktop), (0, 0));
        assert_eq!(absolute((1919, 1079), desktop), (65_535, 65_535));
        assert_eq!(absolute((5000, -5), desktop), (65_535, 0));
        let (x, _) = absolute((0, 0), desktop);
        assert!((x - 32_776).abs() <= 8, "{x}");
    }

    #[test]
    fn events_become_scan_code_keys_and_mouse_actions() {
        let events = [
            Event::Key {
                scan: Scan {
                    code: 0x4b,
                    extended: true,
                },
                down: false,
            },
            Event::Button {
                button: Button::Back,
                down: true,
            },
            Event::Wheel {
                vertical: 0,
                horizontal: 120,
            },
        ];
        let inputs = inputs(&events, (0, 0, 1920, 1080));
        // SAFETY: each union field read matches the input type written above.
        unsafe {
            assert_eq!(inputs[0].r#type, INPUT_KEYBOARD);
            assert_eq!(inputs[0].Anonymous.ki.wScan, 0x4b);
            assert_eq!(
                inputs[0].Anonymous.ki.dwFlags,
                KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
            );
            assert_eq!(inputs[1].Anonymous.mi.dwFlags, MOUSEEVENTF_XDOWN);
            assert_eq!(inputs[1].Anonymous.mi.mouseData, u32::from(XBUTTON1));
            assert_eq!(inputs[2].Anonymous.mi.dwFlags, MOUSEEVENTF_HWHEEL);
        }
        assert!(send(&[]).is_ok());
    }
}
