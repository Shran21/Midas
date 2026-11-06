use std::{
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
};

use midas_media_prototype::session::InputSink;
use serde::Deserialize;
use serde_json::Value;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    SendInput,
};

use super::desktop::{Attachment, Tracker};
use crate::input::{Button, Event, Gate, Release, Scan, absolute, scancode, wheel};

/// Injects viewer input on its own thread, attached to the desktop the input is meant for.
pub struct Injector {
    sender: Option<mpsc::Sender<(u64, Event)>>,
    thread: Option<JoinHandle<()>>,
}

impl Injector {
    pub fn start(tracker: Arc<Tracker>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || run(&tracker, &receiver));
        Self {
            sender: Some(sender),
            thread: Some(thread),
        }
    }
}

impl InputSink for Injector {
    fn input(&mut self, surface: u64, event: &Value) {
        if let Ok(event) = Event::deserialize(event)
            && let Some(sender) = &self.sender
        {
            let _ = sender.send((surface, event));
        }
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        self.sender = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(tracker: &Tracker, events: &mpsc::Receiver<(u64, Event)>) {
    let mut attachment = Attachment::new();
    let mut gate = Gate::default();
    while let Ok((surface, event)) = events.recv() {
        let (current, _) = tracker.current();
        let admission = gate.admit(surface, current);
        if let Some(release) = admission.switched {
            attachment.follow();
            send_release(&release);
        }
        if admission.inject {
            inject(&mut gate, event);
        }
    }
    send_release(&gate.release());
}

fn inject(gate: &mut Gate, event: Event) {
    match event {
        Event::Move { x, y } => mouse(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
            absolute(x),
            absolute(y),
            0,
        ),
        Event::Button { button, down } => {
            mouse(button_flag(button, down), 0, 0, 0);
            gate.button(button, down);
        }
        Event::Wheel { delta } => mouse(MOUSEEVENTF_WHEEL, 0, 0, wheel(delta) as u32),
        Event::Key { code, down } => {
            if let Some(scan) = scancode(&code) {
                key(scan, down);
                gate.key(scan, down);
            }
        }
    }
}

fn send_release(release: &Release) {
    for &scan in &release.keys {
        key(scan, false);
    }
    for &button in &release.buttons {
        mouse(button_flag(button, false), 0, 0, 0);
    }
}

fn button_flag(button: Button, down: bool) -> MOUSE_EVENT_FLAGS {
    match (button, down) {
        (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
        (Button::Left, false) => MOUSEEVENTF_LEFTUP,
        (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
        (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
    }
}

fn send(input: INPUT) {
    // SAFETY: one fully initialised INPUT with its exact size.
    unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) };
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: u32) {
    send(INPUT {
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
    });
}

fn key((scan, extended): Scan, down: bool) {
    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    send(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
}
