//! The session indicator in the signed-in user's session: a small window at the top of the screen
//! that stays above other windows without taking the focus, names the technician, and ends the
//! session with one button. Remote input may press it too; ending is always allowed. A Chat
//! button opens the conversation with the technician; a message from the technician opens it.

use std::{
    collections::VecDeque,
    io, ptr,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
};

use midas_protocol::remote::{MAX_CHAT_CHARS, chat_valid};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Globalization::GetUserDefaultUILanguage,
    Graphics::Gdi::{CreateFontIndirectW, DeleteObject, HGDIOBJ},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{EM_REPLACESEL, EM_SETLIMITTEXT, EM_SETSEL},
        HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
            SetThreadDpiAwarenessContext, SystemParametersInfoForDpi,
        },
        WindowsAndMessaging::{
            BN_CLICKED, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW,
            DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE,
            ES_READONLY, FLASHW_CAPTION, FLASHWINFO, FlashWindowEx, GetDlgItem, GetMessageW,
            GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, MSG, NONCLIENTMETRICSW,
            PostMessageW, PostQuitMessage, RegisterClassW, SPI_GETNONCLIENTMETRICS,
            SPI_GETWORKAREA, SW_HIDE, SW_SHOWNA, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOZORDER, SendMessageW, SetWindowPos, SetWindowTextW, ShowWindow,
            SystemParametersInfoW, TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY,
            WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use super::{consent::Controls, wide};
use crate::consent::{Question, indicator};

const LANG_HUNGARIAN: u16 = 0x0e;
const END: usize = 200;
const CHAT: usize = 201;
const SEND: usize = 202;
const HISTORY: usize = 203;
const INPUT: usize = 204;
const LABEL: usize = 205;
/// Closes the window from the controlling thread, without counting as the user's End.
const DISMISS: u32 = WM_APP + 1;
/// A technician's message waits in `INCOMING`.
const RECEIVED: u32 = WM_APP + 2;
/// Asked to close before the window existed.
const CLOSED: isize = -1;
/// `SS_NOPREFIX`: an ampersand in a name stays visible.
const SS_NOPREFIX: u32 = 0x80;
/// Messages either side may have waiting; beyond this they are dropped.
const QUEUE: usize = 50;

static PRESSED: AtomicBool = AtomicBool::new(false);
/// The technician's messages for the window, from the controlling thread.
static INCOMING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// The user's messages, which the controlling thread collects.
static OUTGOING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// What the chat needs while the window lives.
static CONVERSATION: Mutex<Option<Conversation>> = Mutex::new(None);

struct Conversation {
    technician: String,
    you: String,
    open: bool,
    /// Window heights with the chat closed and open.
    heights: (i32, i32),
    width: i32,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Shows the technician's message in the open indicator; dropped when none is open.
pub fn deliver(handle: &AtomicIsize, text: String) {
    let window = handle.load(Ordering::Acquire);
    if window == 0 || window == CLOSED {
        return;
    }
    let mut incoming = lock(&INCOMING);
    if incoming.len() < QUEUE {
        incoming.push_back(text);
    }
    drop(incoming);
    // SAFETY: posting to a window that may already be gone is harmless.
    unsafe { PostMessageW(window as HWND, RECEIVED, 0, 0) };
}

/// The user's messages since the last call.
pub fn said() -> Vec<String> {
    lock(&OUTGOING).drain(..).collect()
}

fn append(window: HWND, line: &str) {
    // SAFETY: the history is a child of this thread's window; the text outlives the calls.
    unsafe {
        let history = GetDlgItem(window, HISTORY as i32);
        let text = wide(&format!("{line}\r\n"));
        let end = GetWindowTextLengthW(history).max(0) as usize;
        SendMessageW(history, EM_SETSEL, end, end as LPARAM);
        SendMessageW(history, EM_REPLACESEL, 0, text.as_ptr() as LPARAM);
    }
}

/// Opens or closes the chat part below the indicator, never taking the focus.
fn show_chat(window: HWND, open: bool) {
    let mut conversation = lock(&CONVERSATION);
    let Some(conversation) = conversation.as_mut() else {
        return;
    };
    conversation.open = open;
    let height = if open {
        conversation.heights.1
    } else {
        conversation.heights.0
    };
    // SAFETY: the window and its children belong to this thread.
    unsafe {
        SetWindowPos(
            window,
            ptr::null_mut(),
            0,
            0,
            conversation.width,
            height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        for id in [HISTORY, LABEL, INPUT, SEND] {
            ShowWindow(
                GetDlgItem(window, id as i32),
                if open { SW_SHOWNA } else { SW_HIDE },
            );
        }
    }
}

fn received(window: HWND) {
    let messages: Vec<String> = lock(&INCOMING).drain(..).collect();
    let (technician, open) = match lock(&CONVERSATION).as_ref() {
        Some(conversation) => (conversation.technician.clone(), conversation.open),
        None => return,
    };
    for text in messages {
        append(window, &format!("{technician}: {text}"));
    }
    if !open {
        show_chat(window, true);
    }
    let flash = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd: window,
        dwFlags: FLASHW_CAPTION,
        uCount: 3,
        dwTimeout: 0,
    };
    // SAFETY: draws attention to this thread's window without activating it.
    unsafe { FlashWindowEx(&flash) };
}

fn send(window: HWND) {
    // SAFETY: the input is a child of this thread's window; the buffer is writable for its length.
    let text = unsafe {
        let input = GetDlgItem(window, INPUT as i32);
        let length = GetWindowTextLengthW(input).max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let copied = GetWindowTextW(input, buffer.as_mut_ptr(), buffer.len() as i32).max(0);
        SetWindowTextW(input, wide("").as_ptr());
        String::from_utf16_lossy(&buffer[..copied as usize])
    };
    let text = text.trim().to_owned();
    if !chat_valid(&text) {
        return;
    }
    let you = lock(&CONVERSATION)
        .as_ref()
        .map(|conversation| conversation.you.clone())
        .unwrap_or_default();
    let mut outgoing = lock(&OUTGOING);
    if outgoing.len() < QUEUE {
        outgoing.push_back(text.clone());
        drop(outgoing);
        append(window, &format!("{you}: {text}"));
    }
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let clicked =
        |id| message == WM_COMMAND && wparam & 0xffff == id && (wparam >> 16) as u32 == BN_CLICKED;
    match message {
        _ if clicked(END) => {
            PRESSED.store(true, Ordering::Release);
            // SAFETY: destroys the window this procedure belongs to, on its own thread.
            unsafe { DestroyWindow(window) };
            0
        }
        _ if clicked(CHAT) => {
            let open = lock(&CONVERSATION).as_ref().is_some_and(|c| c.open);
            show_chat(window, !open);
            0
        }
        _ if clicked(SEND) => {
            send(window);
            0
        }
        RECEIVED => {
            received(window);
            0
        }
        DISMISS => {
            // SAFETY: as above; the controller asked the window to go away.
            unsafe { DestroyWindow(window) };
            0
        }
        // The indicator stays until the session ends or the user presses End; closing it with
        // a keyboard shortcut must not leave the session without its control.
        WM_CLOSE => 0,
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: default handling for every other message of this window.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Requests the window to close; safe before, during and after its creation.
pub fn dismiss(handle: &AtomicIsize) {
    let window = handle.swap(CLOSED, Ordering::AcqRel);
    if window != 0 && window != CLOSED {
        // SAFETY: posting to a window that may already be gone is harmless.
        unsafe { PostMessageW(window as HWND, DISMISS, 0, 0) };
    }
}

/// Shows the indicator until the user presses End (`true`) or `dismiss` closes it (`false`).
pub fn indicate(question: &Question, handle: &AtomicIsize) -> io::Result<bool> {
    // SAFETY: changes the DPI awareness of the calling thread only.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let _controls = Controls::activate()?;
    // SAFETY: a plain query of the user's interface language.
    let hungarian = unsafe { GetUserDefaultUILanguage() } & 0x3ff == LANG_HUNGARIAN;
    let text = indicator(question, hungarian);
    PRESSED.store(false, Ordering::Release);
    lock(&INCOMING).clear();
    lock(&OUTGOING).clear();
    // SAFETY: the module handle of this program.
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let class = wide("MidasRemoteIndicator");
    let registration = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        ..Default::default()
    };
    // SAFETY: the class name outlives the call; an existing registration is fine to reuse.
    unsafe { RegisterClassW(&registration) };
    // SAFETY: a plain system query.
    let dpi = unsafe { GetDpiForSystem() }.max(96) as i32;
    let scale = |value: i32| value * dpi / 96;
    let mut area = RECT::default();
    // SAFETY: `area` is a valid RECT output.
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) };
    let (width, height) = (scale(560), scale(64));
    let panel = scale(200);
    let caption = scale(24);
    let x = area.left + (area.right - area.left - width) / 2;
    let y = area.top + scale(8);
    *lock(&CONVERSATION) = Some(Conversation {
        technician: question.operator.clone(),
        you: text.you.clone(),
        open: false,
        heights: (height + caption, height + panel + caption),
        width,
    });
    let title = wide(&text.title);
    // SAFETY: every string outlives the call; the window belongs to this thread.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_CAPTION | WS_BORDER,
            x,
            y,
            width,
            height + caption,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    if window.is_null() {
        return Err(io::Error::last_os_error());
    }
    let (button_width, margin) = (scale(110), scale(12));
    let child = |class: &str,
                 label: &str,
                 style: u32,
                 visible: bool,
                 rect: (i32, i32, i32, i32),
                 id: usize| {
        let class = wide(class);
        let label = wide(label);
        // SAFETY: the strings outlive the call; the parent window is alive on this thread.
        unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                label.as_ptr(),
                WS_CHILD | if visible { WS_VISIBLE } else { 0 } | style,
                rect.0,
                rect.1,
                rect.2,
                rect.3,
                window,
                id as _,
                instance,
                ptr::null(),
            )
        }
    };
    let button_y = (height - scale(32)) / 2;
    let controls = [
        child(
            "STATIC",
            &text.line,
            SS_NOPREFIX,
            true,
            (
                margin,
                margin,
                width - 2 * button_width - 4 * margin,
                height - 2 * margin,
            ),
            0,
        ),
        child(
            "BUTTON",
            &text.chat,
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            true,
            (
                width - 2 * button_width - 2 * margin,
                button_y,
                button_width,
                scale(32),
            ),
            CHAT,
        ),
        child(
            "BUTTON",
            &text.end,
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            true,
            (
                width - button_width - margin,
                button_y,
                button_width,
                scale(32),
            ),
            END,
        ),
        child(
            "EDIT",
            "",
            (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32
                | WS_VSCROLL
                | WS_BORDER
                | WS_TABSTOP,
            false,
            (margin, height, width - 2 * margin, panel - scale(56)),
            HISTORY,
        ),
        child(
            "STATIC",
            &text.message,
            SS_NOPREFIX,
            false,
            (margin, height + panel - scale(44), scale(70), scale(28)),
            LABEL,
        ),
        child(
            "EDIT",
            "",
            ES_AUTOHSCROLL as u32 | WS_BORDER | WS_TABSTOP,
            false,
            (
                margin + scale(76),
                height + panel - scale(46),
                width - button_width - scale(76) - 3 * margin,
                scale(28),
            ),
            INPUT,
        ),
        child(
            "BUTTON",
            &text.send,
            BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
            false,
            (
                width - button_width - margin,
                height + panel - scale(48),
                button_width,
                scale(32),
            ),
            SEND,
        ),
    ];
    // SAFETY: the input is a child of this window.
    unsafe {
        SendMessageW(
            GetDlgItem(window, INPUT as i32),
            EM_SETLIMITTEXT,
            MAX_CHAT_CHARS,
            0,
        )
    };
    // Published only once the window is complete, so whoever holds it finds the End button.
    if handle
        .compare_exchange(0, window as isize, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // SAFETY: the window was never shown; the controller already asked it to go.
        unsafe { DestroyWindow(window) };
        return Ok(false);
    }
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    // SAFETY: `metrics` is a valid output of the size it declares.
    let font = if unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            (&raw mut metrics).cast(),
            0,
            dpi as u32,
        )
    } != 0
    {
        // SAFETY: the font description comes from the system.
        unsafe { CreateFontIndirectW(&metrics.lfMessageFont) }
    } else {
        ptr::null_mut()
    };
    if !font.is_null() {
        for control in controls {
            // SAFETY: the controls are alive; the font outlives them.
            unsafe { SendMessageW(control, WM_SETFONT, font as WPARAM, 1) };
        }
    }
    // SAFETY: shows the window without taking the focus from the user's own program.
    unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
    let mut message = MSG::default();
    // SAFETY: a standard message loop for the windows of this thread.
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        // SAFETY: as above; Tab, Enter and Space reach the buttons through the dialog manager.
        if unsafe { IsDialogMessageW(window, &message) } == 0 {
            // SAFETY: the message came from this thread's own queue.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    handle.store(CLOSED, Ordering::Release);
    *lock(&CONVERSATION) = None;
    if !font.is_null() {
        // SAFETY: the controls using the font are destroyed.
        unsafe { DeleteObject(font as HGDIOBJ) };
    }
    Ok(PRESSED.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use windows_sys::Win32::UI::WindowsAndMessaging::{BM_CLICK, IsWindowVisible};

    use super::*;

    fn question() -> Question {
        Question {
            request: uuid::Uuid::new_v4(),
            operator: "Trial Operator".into(),
            customer: "Trial Customer".into(),
            administrative: true,
            uac: false,
            transfer: midas_remote_auth::Transfer::NONE,
            seconds: 30,
        }
    }

    fn shown(handle: &AtomicIsize) -> HWND {
        for _ in 0..100 {
            let window = handle.load(Ordering::Acquire);
            if window != 0 && window != CLOSED {
                return window as HWND;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the indicator did not open");
    }

    fn eventually(check: impl Fn() -> bool) {
        for _ in 0..100 {
            if check() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("not reached");
    }

    #[test]
    fn end_reports_the_users_choice_dismiss_does_not_and_the_chat_goes_both_ways() {
        let handle = Arc::new(AtomicIsize::new(0));
        let running = {
            let handle = handle.clone();
            std::thread::spawn(move || indicate(&question(), &handle))
        };
        let window = shown(&handle);
        // SAFETY: controls of the window this test opened; messages are posted or sent to them.
        unsafe {
            let history = GetDlgItem(window, HISTORY as i32);
            assert_eq!(IsWindowVisible(history), 0, "the chat starts closed");
            // A technician's message opens the chat.
            deliver(&handle, "Mindjárt nézem.".into());
            eventually(|| IsWindowVisible(history) != 0);
            eventually(|| GetWindowTextLengthW(history) > 0);
            let input = GetDlgItem(window, INPUT as i32);
            SetWindowTextW(input, wide("  Köszönöm!  ").as_ptr());
            PostMessageW(GetDlgItem(window, SEND as i32), BM_CLICK, 0, 0);
            eventually(|| !lock(&OUTGOING).is_empty());
            assert_eq!(said(), ["Köszönöm!"]);
            // Blank text is not sent.
            PostMessageW(GetDlgItem(window, SEND as i32), BM_CLICK, 0, 0);
            std::thread::sleep(Duration::from_millis(100));
            assert!(said().is_empty());
            let button = GetDlgItem(window, END as i32);
            assert!(!button.is_null());
            // A close request from the keyboard leaves the indicator in place.
            PostMessageW(window, WM_CLOSE, 0, 0);
            PostMessageW(button, BM_CLICK, 0, 0);
        }
        assert!(running.join().unwrap().unwrap());
        // Nothing is shown once the window is gone.
        deliver(&handle, "late".into());
        assert!(lock(&INCOMING).is_empty());

        let handle = Arc::new(AtomicIsize::new(0));
        let running = {
            let handle = handle.clone();
            std::thread::spawn(move || indicate(&question(), &handle))
        };
        shown(&handle);
        dismiss(&handle);
        assert!(!running.join().unwrap().unwrap());

        // Dismissed before it opened: it never shows.
        let handle = AtomicIsize::new(0);
        dismiss(&handle);
        assert!(!indicate(&question(), &handle).unwrap());
    }
}
