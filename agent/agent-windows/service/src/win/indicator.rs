//! The session indicator in the signed-in user's session: a small window at the top of the screen
//! that stays above other windows without taking the focus, names the technician, and ends the
//! session with one button. Remote input may press it too; ending is always allowed.

use std::{
    io, ptr,
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Globalization::GetUserDefaultUILanguage,
    Graphics::Gdi::{CreateFontIndirectW, DeleteObject, HGDIOBJ},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
            SetThreadDpiAwarenessContext, SystemParametersInfoForDpi,
        },
        WindowsAndMessaging::{
            BN_CLICKED, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow,
            DispatchMessageW, GetMessageW, IsDialogMessageW, MSG, NONCLIENTMETRICSW, PostMessageW,
            PostQuitMessage, RegisterClassW, SPI_GETNONCLIENTMETRICS, SPI_GETWORKAREA,
            SW_SHOWNOACTIVATE, SendMessageW, ShowWindow, SystemParametersInfoW, TranslateMessage,
            WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION,
            WS_CHILD, WS_EX_CONTROLPARENT, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP,
            WS_VISIBLE,
        },
    },
};

use super::{consent::Controls, wide};
use crate::consent::{Question, indicator};

const LANG_HUNGARIAN: u16 = 0x0e;
const END: usize = 200;
/// Closes the window from the controlling thread, without counting as the user's End.
const DISMISS: u32 = WM_APP + 1;
/// Asked to close before the window existed.
const CLOSED: isize = -1;
/// `SS_NOPREFIX`: an ampersand in a name stays visible.
const SS_NOPREFIX: u32 = 0x80;

static PRESSED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND if wparam & 0xffff == END && (wparam >> 16) as u32 == BN_CLICKED => {
            PRESSED.store(true, Ordering::Release);
            // SAFETY: destroys the window this procedure belongs to, on its own thread.
            unsafe { DestroyWindow(window) };
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
    let (width, height) = (scale(520), scale(64));
    let x = area.left + (area.right - area.left - width) / 2;
    let y = area.top + scale(8);
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
            height + scale(24),
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    if window.is_null() {
        return Err(io::Error::last_os_error());
    }
    if handle
        .compare_exchange(0, window as isize, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // SAFETY: the window was never shown; the controller already asked it to go.
        unsafe { DestroyWindow(window) };
        return Ok(false);
    }
    let line = wide(&text.line);
    let end = wide(&text.end);
    let (button_width, margin) = (scale(110), scale(12));
    let child = |class: &str, label: &[u16], style: u32, rect: (i32, i32, i32, i32), id: usize| {
        let class = wide(class);
        // SAFETY: the strings outlive the call; the parent window is alive on this thread.
        unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                label.as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
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
    let text_window = child(
        "STATIC",
        &line,
        SS_NOPREFIX,
        (
            margin,
            margin,
            width - button_width - 3 * margin,
            height - 2 * margin,
        ),
        0,
    );
    let button = child(
        "BUTTON",
        &end,
        BS_PUSHBUTTON as u32 | WS_TABSTOP,
        (
            width - button_width - margin,
            (height - scale(32)) / 2,
            button_width,
            scale(32),
        ),
        END,
    );
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
        for control in [text_window, button] {
            // SAFETY: both controls are alive; the font outlives them.
            unsafe { SendMessageW(control, WM_SETFONT, font as WPARAM, 1) };
        }
    }
    // SAFETY: shows the window without taking the focus from the user's own program.
    unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
    let mut message = MSG::default();
    // SAFETY: a standard message loop for the windows of this thread.
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        // SAFETY: as above; Tab, Enter and Space reach the button through the dialog manager.
        if unsafe { IsDialogMessageW(window, &message) } == 0 {
            // SAFETY: the message came from this thread's own queue.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    handle.store(CLOSED, Ordering::Release);
    if !font.is_null() {
        // SAFETY: the controls using the font are destroyed.
        unsafe { DeleteObject(font as HGDIOBJ) };
    }
    Ok(PRESSED.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use windows_sys::Win32::UI::WindowsAndMessaging::{BM_CLICK, GetDlgItem};

    use super::*;

    fn question() -> Question {
        Question {
            request: uuid::Uuid::new_v4(),
            operator: "Trial Operator".into(),
            customer: "Trial Customer".into(),
            administrative: true,
            uac: false,
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

    #[test]
    fn end_reports_the_users_choice_and_dismiss_does_not() {
        let handle = Arc::new(AtomicIsize::new(0));
        let running = {
            let handle = handle.clone();
            std::thread::spawn(move || indicate(&question(), &handle))
        };
        let window = shown(&handle);
        // SAFETY: the button of the window this test opened; a click is posted to it.
        unsafe {
            let button = GetDlgItem(window, END as i32);
            assert!(!button.is_null());
            // A close request from the keyboard leaves the indicator in place.
            PostMessageW(window, WM_CLOSE, 0, 0);
            PostMessageW(button, BM_CLICK, 0, 0);
        }
        assert!(running.join().unwrap().unwrap());

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
