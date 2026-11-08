//! A resident tray icon without a WebView: it reads the service status from the protected pipe
//! every few seconds and explains in plain words what, if anything, the user can do.

use std::{
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicIsize, Ordering},
    },
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM},
    Globalization::GetUserDefaultUILanguage,
    System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
    UI::{
        Shell::{
            NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO, NIIF_WARNING, NIM_ADD,
            NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
            DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, IDI_ERROR, IDI_INFORMATION,
            IDI_WARNING, LoadIconW, MB_ICONINFORMATION, MB_OK, MF_GRAYED, MF_SEPARATOR, MF_STRING,
            MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
            RegisterWindowMessageW, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON,
            TrackPopupMenu, TranslateMessage, WM_APP, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP,
            WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
        },
    },
};

use super::wide;
use crate::status::{Tone, View, view};

const CALLBACK: u32 = WM_APP + 1;
const UPDATED: u32 = WM_APP + 2;
const DETAILS: usize = 1;
const QUIT: usize = 2;
const POLL: Duration = Duration::from_secs(5);
const LANG_HUNGARIAN: u16 = 0x0e;

/// The latest status from the poller and what the icon shows.
struct Shown {
    status: Option<String>,
    view: Option<View>,
    hungarian: bool,
}

static SHOWN: Mutex<Shown> = Mutex::new(Shown {
    status: None,
    view: None,
    hungarian: false,
});
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: Mutex<u32> = Mutex::new(0);

fn shown() -> std::sync::MutexGuard<'static, Shown> {
    SHOWN.lock().unwrap_or_else(|e| e.into_inner())
}

fn copy(target: &mut [u16], text: &str) {
    let units: Vec<u16> = text.encode_utf16().take(target.len() - 1).collect();
    target[..units.len()].copy_from_slice(&units);
    target[units.len()] = 0;
}

fn icon_data(window: HWND) -> NOTIFYICONDATAW {
    // SAFETY: NOTIFYICONDATAW is plain data; all-zero is a valid empty value.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = 1;
    data
}

fn apply(window: HWND, current: View, previous: Option<View>, add: bool) {
    let mut data = icon_data(window);
    data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
    data.uCallbackMessage = CALLBACK;
    let resource = match current.tone {
        Tone::Ok | Tone::Working => IDI_INFORMATION,
        Tone::Attention => IDI_WARNING,
        Tone::Problem => IDI_ERROR,
    };
    // SAFETY: a null instance with a predefined id loads a shared system icon.
    data.hIcon = unsafe { LoadIconW(ptr::null_mut(), resource) };
    copy(&mut data.szTip, &format!("MIDAS: {}", current.title));
    let worse = matches!(current.tone, Tone::Attention | Tone::Problem)
        && previous.is_none_or(|before| before.title != current.title);
    if worse && let Some(advice) = current.advice {
        data.uFlags |= NIF_INFO;
        copy(&mut data.szInfoTitle, current.title);
        copy(&mut data.szInfo, advice);
        data.dwInfoFlags = if current.tone == Tone::Problem {
            NIIF_ERROR
        } else {
            NIIF_WARNING
        };
    } else if !add
        && previous.is_some_and(|before| before.tone != Tone::Ok)
        && current.tone == Tone::Ok
    {
        data.uFlags |= NIF_INFO;
        copy(&mut data.szInfoTitle, "MIDAS");
        copy(&mut data.szInfo, current.title);
        data.dwInfoFlags = NIIF_INFO;
    }
    // SAFETY: `data` is fully initialized and lives for the call.
    unsafe {
        Shell_NotifyIconW(if add { NIM_ADD } else { NIM_MODIFY }, &data);
    }
}

fn refresh(window: HWND, add: bool) {
    let (current, previous) = {
        let mut state = shown();
        let current = view(state.status.as_deref(), state.hungarian);
        let previous = state.view.replace(current);
        (current, previous)
    };
    if add || previous != Some(current) {
        apply(window, current, previous, add);
    }
}

fn details(window: HWND) {
    let (current, hungarian) = {
        let state = shown();
        (
            view(state.status.as_deref(), state.hungarian),
            state.hungarian,
        )
    };
    let mut text = current.title.to_owned();
    if let Some(advice) = current.advice {
        text.push_str("\n\n");
        text.push_str(advice);
    }
    let caption = wide(if hungarian {
        "MIDAS agent állapota"
    } else {
        "MIDAS agent status"
    });
    let body = wide(&text);
    // SAFETY: both strings are NUL-terminated and outlive the modal call.
    unsafe {
        MessageBoxW(
            window,
            body.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn menu(window: HWND) {
    let (title, hungarian) = {
        let state = shown();
        (
            view(state.status.as_deref(), state.hungarian).title,
            state.hungarian,
        )
    };
    let labels = (
        wide(title),
        wide(if hungarian {
            "Részletek…"
        } else {
            "Details…"
        }),
        wide(if hungarian {
            "Ikon elrejtése"
        } else {
            "Hide icon"
        }),
    );
    // SAFETY: the menu is created, shown and destroyed here; strings outlive each call.
    unsafe {
        let popup = CreatePopupMenu();
        if popup.is_null() {
            return;
        }
        AppendMenuW(popup, MF_STRING | MF_GRAYED, 0, labels.0.as_ptr());
        AppendMenuW(popup, MF_SEPARATOR, 0, ptr::null());
        AppendMenuW(popup, MF_STRING, DETAILS, labels.1.as_ptr());
        AppendMenuW(popup, MF_STRING, QUIT, labels.2.as_ptr());
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        SetForegroundWindow(window);
        let chosen = TrackPopupMenu(
            popup,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            window,
            ptr::null(),
        );
        DestroyMenu(popup);
        match usize::try_from(chosen).unwrap_or(0) {
            DETAILS => details(window),
            QUIT => {
                DestroyWindow(window);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        CALLBACK => {
            match u32::try_from(lparam & 0xffff).unwrap_or(0) {
                WM_LBUTTONUP => details(window),
                WM_RBUTTONUP | WM_CONTEXTMENU => menu(window),
                _ => {}
            }
            0
        }
        UPDATED => {
            refresh(window, false);
            0
        }
        WM_DESTROY => {
            let data = icon_data(window);
            // SAFETY: removes the icon this window added; then ends the message loop.
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &data);
                PostQuitMessage(0);
            }
            0
        }
        // Explorer restarted: the icon must be added again.
        other
            if other != 0
                && other == *TASKBAR_CREATED.lock().unwrap_or_else(|e| e.into_inner()) =>
        {
            refresh(window, true);
            0
        }
        // SAFETY: unhandled messages go to the default procedure with their original arguments.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn poll() {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    loop {
        let status = runtime.block_on(crate::ipc::query(crate::ipc::PIPE)).ok();
        let changed = {
            let mut state = shown();
            let changed = state.status != status;
            state.status = status;
            changed
        };
        let window = WINDOW.load(Ordering::Acquire);
        if changed && window != 0 {
            // SAFETY: the handle belongs to this process; posting to a destroyed window is harmless.
            unsafe {
                PostMessageW(window as HWND, UPDATED, 0, 0);
            }
        }
        std::thread::sleep(POLL);
    }
}

/// Runs the tray icon until the user hides it; a second instance in the session exits at once.
pub fn run() -> i32 {
    let name = wide("Local\\MidasAgentTray");
    // SAFETY: the name is NUL-terminated; the handle stays open for the process lifetime.
    let single = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
    // SAFETY: reads the error of the CreateMutexW call above.
    if single.is_null() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return 0;
    }
    // SAFETY: a plain query of the user's interface language.
    shown().hungarian = unsafe { GetUserDefaultUILanguage() } & 0x3ff == LANG_HUNGARIAN;
    let class = wide("MidasAgentTray");
    let taskbar = wide("TaskbarCreated");
    // SAFETY: the class and window use this module's instance and a 'static procedure.
    let window = unsafe {
        *TASKBAR_CREATED.lock().unwrap_or_else(|e| e.into_inner()) =
            RegisterWindowMessageW(taskbar.as_ptr());
        let instance = GetModuleHandleW(ptr::null());
        let mut definition: WNDCLASSW = std::mem::zeroed();
        definition.lpfnWndProc = Some(procedure);
        definition.hInstance = instance;
        definition.lpszClassName = class.as_ptr();
        if RegisterClassW(&definition) == 0 {
            return 1;
        }
        CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    if window.is_null() {
        return 1;
    }
    WINDOW.store(window as isize, Ordering::Release);
    refresh(window, true);
    std::thread::spawn(poll);
    // SAFETY: a standard message loop for this thread's window.
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    0
}
