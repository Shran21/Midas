//! The local consent window in the signed-in user's session. Decline has the initial focus, Allow
//! stays disabled for the first second so a stray key press cannot approve, and the window declines
//! by itself when the time runs out.

use std::{
    io,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, S_FALSE, S_OK, WPARAM},
    Globalization::GetUserDefaultUILanguage,
    System::{
        ApplicationInstallationAndServicing::{
            ACTCTXW, ActivateActCtx, CreateActCtxW, DeactivateActCtx, ReleaseActCtx,
        },
        LibraryLoader::GetModuleHandleW,
    },
    UI::{
        Controls::{
            TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TD_INFORMATION_ICON, TD_SHIELD_ICON, TDE_FOOTER,
            TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER, TDM_CLICK_BUTTON, TDM_ENABLE_BUTTON,
            TDM_UPDATE_ELEMENT_TEXT, TDN_BUTTON_CLICKED, TDN_CREATED, TDN_TIMER,
            TaskDialogIndirect,
        },
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
        WindowsAndMessaging::{
            HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetForegroundWindow, SetWindowPos,
        },
    },
};

use super::wide;
use crate::consent::{Answer, Question, remaining, texts};

pub(crate) const ALLOW: i32 = 100;
pub(crate) const DECLINE: i32 = 101;
const ARMING: Duration = Duration::from_secs(1);
const LANG_HUNGARIAN: u16 = 0x0e;
/// The program's own Common Controls 6 manifest, embedded as resource 2 (see build.rs).
const MANIFEST: usize = 2;
const ACTCTX_FLAG_RESOURCE_NAME_VALID: u32 = 0x8;
const ACTCTX_FLAG_HMODULE_VALID: u32 = 0x80;

/// Selects Common Controls 6 for this thread while the window is open.
pub(super) struct Controls {
    context: HANDLE,
    cookie: usize,
}

impl Controls {
    pub(super) fn activate() -> io::Result<Self> {
        let request = ACTCTXW {
            cbSize: size_of::<ACTCTXW>() as u32,
            dwFlags: ACTCTX_FLAG_RESOURCE_NAME_VALID | ACTCTX_FLAG_HMODULE_VALID,
            lpResourceName: MANIFEST as *const u16,
            // SAFETY: the handle of this program's own module, which stays loaded.
            hModule: unsafe { GetModuleHandleW(std::ptr::null()) },
            ..Default::default()
        };
        // SAFETY: `request` is fully initialized and names a resource of this module.
        let context = unsafe { CreateActCtxW(&request) };
        if context == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut cookie = 0;
        // SAFETY: activates the context just created on this thread.
        if unsafe { ActivateActCtx(context, &mut cookie) } == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: releases the context this function created.
            unsafe { ReleaseActCtx(context) };
            return Err(error);
        }
        Ok(Self { context, cookie })
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        // SAFETY: deactivates and releases the context this guard activated on this thread.
        unsafe {
            DeactivateActCtx(0, self.cookie);
            ReleaseActCtx(self.context);
        }
    }
}

struct State {
    shown: Instant,
    limit: Duration,
    armed: bool,
    expired: bool,
    seconds: u64,
    hungarian: bool,
    footer: Vec<u16>,
}

impl State {
    fn left(&self) -> u64 {
        self.limit
            .saturating_sub(self.shown.elapsed())
            .as_millis()
            .div_ceil(1000) as u64
    }
}

unsafe extern "system" fn callback(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    _: LPARAM,
    data: isize,
) -> i32 {
    // SAFETY: `data` is the `State` that `ask` owns for the whole TaskDialogIndirect call, and the
    // dialog calls back only on that thread.
    let state = unsafe { &mut *(data as *mut State) };
    let send = |message: i32, wparam: usize, lparam: isize| {
        // SAFETY: `window` is the live task dialog that called back.
        unsafe { SendMessageW(window, message as u32, wparam, lparam) };
    };
    match message as i32 {
        TDN_CREATED => {
            state.shown = Instant::now();
            send(TDM_ENABLE_BUTTON, ALLOW as usize, 0);
            // SAFETY: plain window positioning of the dialog itself.
            unsafe {
                SetWindowPos(window, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                SetForegroundWindow(window);
            }
        }
        TDN_TIMER => {
            if !state.armed && state.shown.elapsed() >= ARMING {
                state.armed = true;
                send(TDM_ENABLE_BUTTON, ALLOW as usize, 1);
            }
            let left = state.left();
            if left == 0 && !state.expired {
                state.expired = true;
                send(TDM_CLICK_BUTTON, DECLINE as usize, 0);
            } else if left != state.seconds {
                state.seconds = left;
                state.footer = wide(&remaining(left, state.hungarian));
                send(
                    TDM_UPDATE_ELEMENT_TEXT,
                    TDE_FOOTER as usize,
                    state.footer.as_ptr() as isize,
                );
            }
        }
        // Allow counts only once armed and before expiry, whoever sends the click.
        TDN_BUTTON_CLICKED
            if wparam == ALLOW as usize && (!state.armed || state.expired || state.left() == 0) =>
        {
            return S_FALSE;
        }
        _ => {}
    }
    S_OK
}

/// Shows the question and waits for the user's answer or the end of its time.
pub fn ask(question: &Question) -> io::Result<Answer> {
    // Only this thread is DPI aware: a process-wide setting would make every participant
    // initialize its desktop connection at start, which fails for tokens without desktop access.
    // SAFETY: changes the DPI awareness of the calling thread only.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let _controls = Controls::activate()?;
    // SAFETY: a plain query of the user's interface language.
    let hungarian = unsafe { GetUserDefaultUILanguage() } & 0x3ff == LANG_HUNGARIAN;
    let text = texts(question, hungarian);
    let title = wide(&text.title);
    let instruction = wide(&text.instruction);
    let content = wide(&text.content);
    let allow = wide(&text.allow);
    let decline = wide(&text.decline);
    let seconds = u64::from(question.seconds);
    let mut state = State {
        shown: Instant::now(),
        limit: Duration::from_secs(seconds),
        armed: false,
        expired: false,
        seconds,
        hungarian,
        footer: wide(&remaining(seconds, hungarian)),
    };
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: ALLOW,
            pszButtonText: allow.as_ptr(),
        },
        TASKDIALOG_BUTTON {
            nButtonID: DECLINE,
            pszButtonText: decline.as_ptr(),
        },
    ];
    let mut config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_CALLBACK_TIMER,
        pszWindowTitle: title.as_ptr(),
        pszMainInstruction: instruction.as_ptr(),
        pszContent: content.as_ptr(),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: DECLINE,
        pfCallback: Some(callback),
        ..Default::default()
    };
    config.Anonymous1.pszMainIcon = if question.administrative || question.uac {
        TD_SHIELD_ICON
    } else {
        TD_INFORMATION_ICON
    };
    config.pszFooter = state.footer.as_ptr();
    config.lpCallbackData = &raw mut state as isize;
    let mut button = 0;
    // SAFETY: every string, the buttons and the state outlive this modal call; the callback only
    // touches the state on this thread.
    let result = unsafe {
        TaskDialogIndirect(
            &config,
            &mut button,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(if state.expired {
        Answer::Expired
    } else if button == ALLOW {
        Answer::Allow
    } else {
        Answer::Decline
    })
}

#[cfg(test)]
mod tests;
