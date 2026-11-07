#![allow(unsafe_code)]
//! Reviewed Win32 calls. Each unsafe block states why its pointers and handles are valid.

pub mod service;

use std::{ffi::c_void, io, os::windows::io::RawHandle, path::PathBuf, ptr};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
    System::{
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
        RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId},
        Threading::{
            GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    },
};

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Security attributes built from SDDL, freed on drop.
pub struct SecurityAttributes {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

// SAFETY: the descriptor is an owned LocalAlloc allocation that is never shared between
// threads concurrently; moving it to another thread is sound.
unsafe impl Send for SecurityAttributes {}

impl SecurityAttributes {
    pub fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text = wide(sddl);
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `text` is a NUL-terminated UTF-16 string that outlives the call, and
        // `descriptor` is a valid out pointer; the size output is optional.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }

    pub fn as_mut_ptr(&mut self) -> *mut c_void {
        (&raw mut self.attributes).cast()
    }
}

impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        // SAFETY: the descriptor came from ConvertStringSecurityDescriptorToSecurityDescriptorW,
        // which documents LocalFree as the release function, and is freed exactly once.
        unsafe {
            LocalFree(self.descriptor);
        }
    }
}

/// Creates a pipe instance whose access is limited by `sddl`.
pub fn create_pipe(
    options: &tokio::net::windows::named_pipe::ServerOptions,
    name: &str,
    sddl: &str,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    let mut attributes = SecurityAttributes::from_sddl(sddl)?;
    // SAFETY: `attributes` points to a SECURITY_ATTRIBUTES with a valid descriptor and stays
    // alive until the call returns; Windows copies the descriptor into the new pipe.
    unsafe { options.create_with_security_attributes_raw(name, attributes.as_mut_ptr()) }
}

fn session_of(pid: u32) -> io::Result<u32> {
    let mut session = 0u32;
    // SAFETY: `session` is a valid out pointer for the duration of the call.
    if unsafe { ProcessIdToSessionId(pid, &mut session) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(session)
}

fn image_of(pid: u32) -> io::Result<PathBuf> {
    // SAFETY: OpenProcess takes plain values; a null result is checked below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u16; 32_768];
    let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: `process` is an open handle, `buffer` holds `length` UTF-16 units and
    // `length` is a valid in/out pointer; the handle is closed exactly once.
    let queried = unsafe {
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        );
        CloseHandle(process);
        queried
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(usize::try_from(length).unwrap_or(0));
    Ok(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

/// Session and executable of the process at the other end of a server pipe.
pub fn pipe_client(pipe: RawHandle) -> io::Result<(u32, PathBuf)> {
    let mut pid = 0u32;
    // SAFETY: `pipe` is a live named pipe handle owned by the caller and `pid` is a valid
    // out pointer.
    if unsafe { GetNamedPipeClientProcessId(pipe as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((session_of(pid)?, image_of(pid)?))
}

/// Session of the process that created a client pipe's server end.
pub fn pipe_server_session(pipe: RawHandle) -> io::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: `pipe` is a live named pipe handle owned by the caller and `pid` is a valid
    // out pointer.
    if unsafe { GetNamedPipeServerProcessId(pipe as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    session_of(pid)
}

pub fn console_session() -> u32 {
    // SAFETY: no arguments; returns 0xFFFFFFFF when no console session is attached.
    unsafe { WTSGetActiveConsoleSessionId() }
}

pub fn current_session() -> io::Result<u32> {
    // SAFETY: no arguments.
    session_of(unsafe { GetCurrentProcessId() })
}
