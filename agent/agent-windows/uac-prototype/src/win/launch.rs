use std::{io, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    Security::{
        DuplicateTokenEx, SecurityIdentification, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
        TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY, TokenPrimary,
        TokenSessionId,
    },
    System::{
        RemoteDesktop::WTSGetActiveConsoleSessionId,
        Threading::{
            CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess,
            GetExitCodeProcess, OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW,
            TerminateProcess, WaitForSingleObject,
        },
    },
};

use super::wide;

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned open by Windows and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn active_console_session() -> Option<u32> {
    // SAFETY: plain query.
    let session = unsafe { WTSGetActiveConsoleSessionId() };
    (session != u32::MAX).then_some(session)
}

/// A helper process started by the service.
pub struct Child {
    process: Handle,
    pub session: u32,
}

// SAFETY: a process handle may be waited on, terminated and closed from any thread.
unsafe impl Send for Child {}

impl Child {
    pub fn exited(&self) -> Option<u32> {
        let mut code = 0;
        // SAFETY: the process handle is open for the lifetime of `self`.
        unsafe {
            (WaitForSingleObject(self.process.0, 0) == WAIT_OBJECT_0
                && GetExitCodeProcess(self.process.0, &mut code) != 0)
                .then_some(code)
        }
    }

    pub fn terminate(&self) {
        // SAFETY: the process handle is open; the wait bounds how long the service blocks.
        unsafe {
            TerminateProcess(self.process.0, 1);
            WaitForSingleObject(self.process.0, 5_000);
        }
    }
}

/// Starts `arguments` with the service's LocalSystem token inside `session`, on its interactive desktop.
/// Needs SeTcbPrivilege to move the token into another session, which only LocalSystem holds.
pub fn spawn_in_session(session: u32, program: &Path, arguments: &str) -> io::Result<Child> {
    let mut own = ptr::null_mut();
    // SAFETY: the out pointer is valid; the handle is closed by `Handle`.
    check(unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut own)
    })?;
    let own = Handle(own);
    let mut token = ptr::null_mut();
    // SAFETY: duplicates an open token into a new primary token owned by `Handle`.
    check(unsafe {
        DuplicateTokenEx(
            own.0,
            TOKEN_ASSIGN_PRIMARY
                | TOKEN_DUPLICATE
                | TOKEN_QUERY
                | TOKEN_ADJUST_DEFAULT
                | TOKEN_ADJUST_SESSIONID,
            ptr::null(),
            SecurityIdentification,
            TokenPrimary,
            &mut token,
        )
    })?;
    let token = Handle(token);
    // SAFETY: the value is a u32 session identifier of the stated length.
    check(unsafe {
        SetTokenInformation(
            token.0,
            TokenSessionId,
            ptr::from_ref(&session).cast(),
            size_of::<u32>() as u32,
        )
    })?;
    let application: Vec<u16> = program.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut line = wide(&format!("\"{}\" {arguments}", program.display()));
    let mut desktop = wide("winsta0\\default");
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        lpDesktop: desktop.as_mut_ptr(),
        ..Default::default()
    };
    let mut information = PROCESS_INFORMATION::default();
    // SAFETY: all strings are NUL-terminated and outlive the call; the command line buffer is mutable.
    check(unsafe {
        CreateProcessAsUserW(
            token.0,
            application.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            ptr::null(),
            ptr::null(),
            &startup,
            &mut information,
        )
    })?;
    drop(Handle(information.hThread));
    Ok(Child {
        process: Handle(information.hProcess),
        session,
    })
}
