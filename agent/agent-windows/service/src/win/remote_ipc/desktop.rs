//! The input desktop as the privileged worker sees it: whether the secure desktop is shown, and
//! whether a window the user worker cannot reach (above medium integrity) is in front or under
//! the pointer.

use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

use midas_agent_core::media::link::Desktop;
use windows_sys::Win32::{
    Foundation::{GENERIC_ALL, HWND, POINT},
    Security::Authentication::Identity::SendSAS,
    Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
        TOKEN_QUERY, TokenIntegrityLevel,
    },
    System::{
        Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW},
        RemoteDesktop::WTSGetActiveConsoleSessionId,
    },
    System::{
        StationsAndDesktops::{
            CloseDesktop, GetUserObjectInformationW, HDESK, OpenInputDesktop, SetThreadDesktop,
            UOI_NAME,
        },
        Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
    },
    UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId,
        WindowFromPoint,
    },
};

/// Ctrl+Alt+Del for `session`: only the console session, and only where the Windows policy
/// (`SoftwareSASGeneration`) lets services generate it. Nothing is sent otherwise.
pub(super) fn secure_attention(session: u32) -> bool {
    // SAFETY: a plain query of the console session.
    let console = unsafe { WTSGetActiveConsoleSessionId() };
    if console != session || !allowed(policy()) {
        return false;
    }
    // SAFETY: SendSAS takes no pointers; as a SYSTEM process it signals the console session.
    unsafe { SendSAS(0) };
    true
}

/// Services may generate it with policy 1 (services) or 3 (services and Ease of Access).
fn allowed(policy: Option<u32>) -> bool {
    matches!(policy, Some(1 | 3))
}

fn policy() -> Option<u32> {
    let key = super::super::wide(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System");
    let name = super::super::wide("SoftwareSASGeneration");
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: both names are NUL-terminated; `value` is a writable DWORD of `size` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    (status == 0).then_some(value)
}

/// `SECURITY_MANDATORY_MEDIUM_RID`, the integrity of the user worker.
const MEDIUM: u32 = 0x2000;

pub(super) struct InputDesktop {
    handle: HDESK,
    name: String,
}

// SAFETY: a desktop handle may be used and closed from any thread of the owning process.
unsafe impl Send for InputDesktop {}

impl InputDesktop {
    /// The desktop that receives input now.
    pub(super) fn open() -> io::Result<Self> {
        // SAFETY: no inheritance; the returned handle is owned by the value below.
        let handle = unsafe { OpenInputDesktop(0, 0, GENERIC_ALL) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let desktop = Self {
            handle,
            name: String::new(),
        };
        let mut name = [0u16; 64];
        let mut needed = 0;
        // SAFETY: the buffer is writable for its byte length; `needed` is a valid output.
        if unsafe {
            GetUserObjectInformationW(
                desktop.handle,
                UOI_NAME,
                name.as_mut_ptr().cast(),
                std::mem::size_of_val(&name) as u32,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        Ok(Self {
            name: String::from_utf16_lossy(&name[..length]),
            ..desktop
        })
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// Moves the calling thread onto this desktop; the thread must own no windows or hooks.
    /// The value must outlive the thread's use of the desktop.
    pub(super) fn attach(&self) -> io::Result<()> {
        // SAFETY: the handle is open for the lifetime of `self`.
        if unsafe { SetThreadDesktop(self.handle) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// What the calling thread, attached to this desktop, sees now.
    pub(super) fn state(&self) -> Desktop {
        state(&self.name, elevated_window)
    }
}

impl Drop for InputDesktop {
    fn drop(&mut self) {
        // SAFETY: closes the handle this value owns; a desktop still used by a thread stays open.
        unsafe { CloseDesktop(self.handle) };
    }
}

fn state(name: &str, elevated: impl FnOnce() -> bool) -> Desktop {
    if !name.eq_ignore_ascii_case("Default") {
        Desktop::Secure
    } else if elevated() {
        Desktop::Elevated
    } else {
        Desktop::User
    }
}

/// Whether the window in front, or the top-level window under the pointer, belongs to a process
/// above medium integrity, which the user worker's input cannot reach.
fn elevated_window() -> bool {
    // SAFETY: plain queries of the calling thread's desktop; null results are handled.
    let front = unsafe { GetForegroundWindow() };
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: `point` is a valid output.
    let under = if unsafe { GetCursorPos(&mut point) } != 0 {
        // SAFETY: window handles are only queried; a null or stale one yields null.
        unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) }
    } else {
        ptr::null_mut()
    };
    [front, under]
        .into_iter()
        .any(|window| !window.is_null() && integrity(window).is_some_and(|rid| rid > MEDIUM))
}

fn integrity(window: HWND) -> Option<u32> {
    let mut pid = 0;
    // SAFETY: `pid` is a valid output for the queried window.
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid == 0 {
        return None;
    }
    // SAFETY: query-only access without inheritance; the handle is owned below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    // SAFETY: OpenProcess returned one owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut token = ptr::null_mut();
    // SAFETY: the process handle is open; the token handle is owned below.
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    // SAFETY: OpenProcessToken returned one owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut buffer = [0u64; 16];
    let mut length = 0;
    // SAFETY: the aligned buffer is writable for its byte length.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenIntegrityLevel,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
            &mut length,
        )
    } == 0
    {
        return None;
    }
    // SAFETY: the call filled the buffer with a TOKEN_MANDATORY_LABEL whose SID lies inside it.
    unsafe {
        let label = &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        (count > 0).then(|| *GetSidSubAuthority(label.Label.Sid, u32::from(count) - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_default_desktop_is_the_users_own() {
        assert_eq!(state("Default", || false), Desktop::User);
        assert_eq!(state("default", || true), Desktop::Elevated);
        assert_eq!(state("Winlogon", || false), Desktop::Secure);
        assert_eq!(state("Screen-saver", || false), Desktop::Secure);
        assert_eq!(state("Winlogon", || panic!("not asked")), Desktop::Secure);
    }

    #[test]
    fn only_a_policy_for_services_allows_ctrl_alt_del() {
        assert!(
            !allowed(None),
            "not configured: Windows default, not allowed"
        );
        assert!(!allowed(Some(0)));
        assert!(allowed(Some(1)));
        assert!(!allowed(Some(2)), "Ease of Access applications only");
        assert!(allowed(Some(3)));
        // This test process is not the console's service; nothing is sent for another session.
        assert!(!secure_attention(u32::MAX));
    }

    #[test]
    fn the_input_desktop_has_a_name() {
        // The test runs on an interactive desktop; without one there is nothing to check.
        let Ok(desktop) = InputDesktop::open() else {
            return;
        };
        assert!(!desktop.name().is_empty());
        let _ = desktop.state();
    }
}
