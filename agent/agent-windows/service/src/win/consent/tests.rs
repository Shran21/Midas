use std::{
    sync::{Mutex, MutexGuard, mpsc},
    thread,
    time::Duration,
};

use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::IsWindowEnabled,
        WindowsAndMessaging::{
            BM_CLICK, EnumChildWindows, EnumThreadWindows, GUITHREADINFO, GetGUIThreadInfo,
            GetWindowTextW, PostMessageW,
        },
    },
};

use super::*;

/// One window at a time, as in the service.
static WINDOW: Mutex<()> = Mutex::new(());

/// Runs the window on its own thread and finds its two buttons by their labels.
struct Shown {
    thread: u32,
    allow: HWND,
    decline: HWND,
    answer: mpsc::Receiver<io::Result<Answer>>,
}

unsafe extern "system" fn every(window: HWND, found: LPARAM) -> i32 {
    // SAFETY: `found` points to the caller's list for the duration of the enumeration.
    unsafe { (*(found as *mut Vec<HWND>)).push(window) };
    1
}

fn label(window: HWND) -> String {
    let mut text = [0u16; 64];
    // SAFETY: the buffer is writable for its whole length.
    let length = unsafe { GetWindowTextW(window, text.as_mut_ptr(), text.len() as i32) };
    String::from_utf16_lossy(&text[..length.max(0) as usize])
}

fn show(seconds: u32) -> Shown {
    let question = Question {
        request: Uuid::new_v4(),
        operator: "Test Operator".into(),
        customer: "Test Customer".into(),
        administrative: true,
        uac: true,
        transfer: midas_remote_auth::Transfer::NONE,
        seconds,
    };
    // SAFETY: a plain query of the user's interface language.
    let hungarian = unsafe { GetUserDefaultUILanguage() } & 0x3ff == LANG_HUNGARIAN;
    let labels = texts(&question, hungarian);
    let (thread_id, thread) = mpsc::channel();
    let (sender, answer) = mpsc::channel();
    thread::spawn(move || {
        // SAFETY: reads the calling thread's own ID.
        thread_id.send(unsafe { GetCurrentThreadId() }).unwrap();
        let _ = sender.send(ask(&question));
    });
    let thread = thread.recv().unwrap();
    for _ in 0..250 {
        // The thread also owns input method windows; only the dialog has the two buttons.
        let mut windows: Vec<HWND> = Vec::new();
        // SAFETY: the callback writes only to `windows`, which outlives the call.
        unsafe { EnumThreadWindows(thread, Some(every), &raw mut windows as LPARAM) };
        for window in windows {
            let mut children: Vec<HWND> = Vec::new();
            // SAFETY: the callback writes only to `children`, which outlives the call.
            unsafe { EnumChildWindows(window, Some(every), &raw mut children as LPARAM) };
            let find = |text: &str| children.iter().copied().find(|&c| label(c) == text);
            if let (Some(allow), Some(decline)) = (find(&labels.allow), find(&labels.decline)) {
                return Shown {
                    thread,
                    allow,
                    decline,
                    answer,
                };
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("the consent window did not open: {:?}", answer.try_recv());
}

fn one_window() -> MutexGuard<'static, ()> {
    WINDOW.lock().unwrap_or_else(|e| e.into_inner())
}

fn enabled(window: HWND) -> bool {
    // SAFETY: queries a window of this test process.
    unsafe { IsWindowEnabled(window) != 0 }
}

fn click(window: HWND) {
    // SAFETY: posts a click to a button of this test process.
    unsafe { PostMessageW(window, BM_CLICK, 0, 0) };
}

#[test]
fn decline_has_the_focus_and_allow_waits_one_second() {
    let _window = one_window();
    let shown = show(30);
    assert!(!enabled(shown.allow));
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a correctly sized, writable structure.
    assert_ne!(unsafe { GetGUIThreadInfo(shown.thread, &mut info) }, 0);
    assert_eq!(info.hwndFocus, shown.decline);
    click(shown.allow);
    thread::sleep(Duration::from_millis(1300));
    assert!(shown.answer.try_recv().is_err(), "allowed while disabled");
    assert!(enabled(shown.allow));
    click(shown.allow);
    let answer = shown.answer.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(answer.unwrap(), Answer::Allow);
}

#[test]
fn decline_and_expiry_never_allow() {
    let _window = one_window();
    let shown = show(30);
    click(shown.decline);
    let answer = shown.answer.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(answer.unwrap(), Answer::Decline);

    let shown = show(2);
    let answer = shown.answer.recv_timeout(Duration::from_secs(6)).unwrap();
    assert_eq!(answer.unwrap(), Answer::Expired);
}
