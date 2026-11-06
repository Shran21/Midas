use std::{
    ptr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, HBRUSH},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem, GetMessageW,
        HMENU, IDC_ARROW, LoadCursorW, MSG, RegisterClassW, SPI_GETWORKAREA, SendMessageW,
        SetTimer, SetWindowTextW, SystemParametersInfoW, TranslateMessage, WM_CLOSE, WM_COMMAND,
        WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_SYSMENU, WS_VISIBLE,
    },
};

use super::wide;

/// Exit code of the helper when the local user ended the session.
pub const ENDED_LOCALLY: i32 = 0;

const END: i32 = 1;
const STATE: i32 = 2;
const IDLE: &str = "Nincs kapcsolat / Not connected";
const CONNECTED: &str = "Kapcsolat aktív / Remote session active";

static ACTIVE: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static SHOWN: AtomicU8 = AtomicU8::new(0);

fn end() -> ! {
    tracing::info!("session ended locally");
    std::process::exit(ENDED_LOCALLY)
}

extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND if (wparam & 0xFFFF) as i32 == END => end(),
        // Closing the window must not leave the user without a way to stop the session.
        WM_CLOSE => end(),
        WM_TIMER => {
            let active = ACTIVE.get().is_some_and(|a| a.load(Ordering::Acquire));
            if SHOWN.swap(u8::from(active) + 1, Ordering::AcqRel) != u8::from(active) + 1 {
                let text = wide(if active { CONNECTED } else { IDLE });
                // SAFETY: the window and its child exist while messages arrive; the text is NUL-terminated.
                unsafe { SetWindowTextW(GetDlgItem(window, STATE), text.as_ptr()) };
            }
            0
        }
        // SAFETY: default handling for a window this thread owns.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Shows the always-on-top window with the pairing code and the local End button, and runs its message loop.
pub fn run(code: &str, active: &Arc<AtomicBool>) {
    let _ = ACTIVE.set(active.clone());
    let class = wide("MidasUacPrototype");
    let title = wide("MIDAS UAC-próba / UAC proof");
    let (label, button) = (wide("STATIC"), wide("BUTTON"));
    let code = wide(&format!("Kód / Code: {code}"));
    let (idle, end) = (wide(IDLE), wide("Befejezés / End"));
    let (width, height) = (360, 150);
    // SAFETY: every string outlives the calls using it; handles belong to this thread's message loop.
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let registration = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&registration) == 0 {
            tracing::error!("cannot register the local window");
            return;
        }
        let mut area = RECT::default();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, ptr::from_mut(&mut area).cast(), 0);
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            area.right - width - 16,
            area.bottom - height - 16,
            width,
            height,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if window.is_null() {
            tracing::error!("cannot create the local window");
            return;
        }
        let font = GetStockObject(DEFAULT_GUI_FONT) as usize;
        let children = [
            (label.as_ptr(), code.as_ptr(), 0, 12, 10, 30, 0),
            (label.as_ptr(), idle.as_ptr(), STATE, 12, 36, 30, 0),
            (
                button.as_ptr(),
                end.as_ptr(),
                END,
                12,
                66,
                34,
                BS_PUSHBUTTON as u32,
            ),
        ];
        for (kind, text, id, x, y, h, style) in children {
            let child = CreateWindowExW(
                0,
                kind,
                text,
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                width - 40,
                h,
                window,
                id as usize as HMENU,
                instance,
                ptr::null(),
            );
            SendMessageW(child, WM_SETFONT, font, 1);
        }
        SetTimer(window, 1, 500, None);
        let mut message = MSG::default();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
