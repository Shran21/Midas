use std::{
    ffi::OsString,
    io,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{HANDLE, LUID, LocalFree, WAIT_TIMEOUT},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, PSID, TOKEN_ELEVATION,
        TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
        TOKEN_STATISTICS, TOKEN_USER, TokenElevation, TokenIntegrityLevel, TokenIsAppContainer,
        TokenLogonSid, TokenPrimary, TokenSessionId, TokenStatistics, TokenUIAccess, TokenUser,
    },
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetProcessId, OpenProcessToken, QueryFullProcessImageNameW, WaitForSingleObject,
        },
    },
};

use super::denied;

const SYSTEM: &str = "S-1-5-18";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Worker {
    User { user_sid: String, logon_id: u64 },
    Privileged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Broker,
    Worker { kind: Worker, session: u32 },
}

/// A process handle supplied by the trusted launcher, never looked up from a pipe's claimed PID.
pub struct Peer {
    process: OwnedHandle,
    snapshot: Snapshot,
    pub(super) role: Role,
}

impl Peer {
    pub fn broker(process: OwnedHandle, installed_image: &Path) -> io::Result<Self> {
        Self::pin(process, installed_image, Role::Broker)
    }

    pub fn worker(
        process: OwnedHandle,
        installed_image: &Path,
        kind: Worker,
        session: u32,
    ) -> io::Result<Self> {
        Self::pin(process, installed_image, Role::Worker { kind, session })
    }

    fn pin(process: OwnedHandle, image: &Path, role: Role) -> io::Result<Self> {
        let snapshot = Snapshot::read(&process)?;
        if snapshot.image != image || !snapshot.allows(&role) {
            return Err(denied());
        }
        Ok(Self {
            process,
            snapshot,
            role,
        })
    }

    pub(super) fn check(&self, pid: u32) -> io::Result<()> {
        if pid != self.snapshot.pid || Snapshot::read(&self.process)? != self.snapshot {
            return Err(denied());
        }
        Ok(())
    }

    pub(super) fn acl(&self) -> io::Result<String> {
        let sid = match &self.role {
            Role::Worker {
                kind: Worker::User { .. },
                ..
            } => self
                .snapshot
                .token
                .logon_sid
                .as_deref()
                .ok_or_else(denied)?,
            Role::Worker {
                kind: Worker::Privileged,
                ..
            } => SYSTEM,
            Role::Broker => return Err(denied()),
        };
        // FILE_GENERIC_WRITE also grants FILE_CREATE_PIPE_INSTANCE; grant only its data-write bit.
        Ok(format!(
            "D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;0x{:08x};;;{sid})",
            super::CLIENT_ACCESS
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    pid: u32,
    image: PathBuf,
    token: Token,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Token {
    id: u64,
    modified: u64,
    logon: u64,
    session: u32,
    user: String,
    logon_sid: Option<String>,
    integrity: String,
    elevated: bool,
    ui_access: bool,
    app_container: bool,
}

impl Snapshot {
    fn read(process: &OwnedHandle) -> io::Result<Self> {
        let handle = process.as_raw_handle() as HANDLE;
        alive(handle)?;
        // SAFETY: the borrowed process handle stays open throughout these queries.
        let pid = unsafe { GetProcessId(handle) };
        if pid == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut image = vec![0u16; 32_768];
        let mut length = image.len() as u32;
        // SAFETY: image has length writable UTF-16 elements; length is an in/out parameter.
        check(unsafe { QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut length) })?;
        image.truncate(length as usize);
        let mut token = ptr::null_mut();
        // SAFETY: the process is live and token is a valid output; ownership is taken below.
        check(unsafe { OpenProcessToken(handle, TOKEN_QUERY, &mut token) })?;
        // SAFETY: OpenProcessToken succeeded, giving this function one owned token handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let token = Token::read(&token)?;
        let mut session = 0;
        // SAFETY: pid belongs to the retained process handle and session is a valid output.
        check(unsafe { ProcessIdToSessionId(pid, &mut session) })?;
        if session != token.session {
            return Err(denied());
        }
        alive(handle)?;
        Ok(Self {
            pid,
            image: PathBuf::from(OsString::from_wide(&image)),
            token,
        })
    }

    fn allows(&self, role: &Role) -> bool {
        self.token.allows(role)
    }
}

impl Token {
    pub(super) fn allows(&self, role: &Role) -> bool {
        let token = self;
        if token.ui_access || token.app_container {
            return false;
        }
        let system = token.user == SYSTEM && token.integrity == "S-1-16-16384";
        match role {
            Role::Broker => system && token.session == 0,
            Role::Worker { kind, session } => {
                if *session == 0 || *session == u32::MAX || *session != token.session {
                    return false;
                }
                match kind {
                    Worker::Privileged => system,
                    Worker::User { user_sid, logon_id } => {
                        !system
                            && !token.elevated
                            && token.integrity == "S-1-16-8192"
                            && token.user == *user_sid
                            && token.logon == *logon_id
                            && token.logon_sid.is_some()
                    }
                }
            }
        }
    }
}

fn luid(value: LUID) -> u64 {
    (u64::from(value.HighPart as u32) << 32) | u64::from(value.LowPart)
}

impl Token {
    pub(super) fn read(token: &OwnedHandle) -> io::Result<Self> {
        let token = token.as_raw_handle() as HANDLE;
        let before = statistics(token)?;
        if before.TokenType != TokenPrimary {
            return Err(denied());
        }
        let user = Buffer::query(token, TokenUser)?;
        let integrity = Buffer::query(token, TokenIntegrityLevel)?;
        let groups = Buffer::query(token, TokenLogonSid)?;
        // SAFETY: each buffer is aligned, size checked and filled by the corresponding token query.
        let (user, integrity, group_count) = unsafe {
            (
                sid(user.read::<TOKEN_USER>()?.User.Sid)?,
                sid(integrity.read::<TOKEN_MANDATORY_LABEL>()?.Label.Sid)?,
                groups.read::<u32>()?,
            )
        };
        let logon_sid = match group_count {
            0 => None,
            // SAFETY: TokenLogonSid returned one group and its backing buffer remains alive.
            1 => Some(unsafe { sid(groups.read::<TOKEN_GROUPS>()?.Groups[0].Sid)? }),
            _ => return Err(denied()),
        };
        let value = Self {
            id: luid(before.TokenId),
            modified: luid(before.ModifiedId),
            logon: luid(before.AuthenticationId),
            user,
            integrity,
            logon_sid,
            session: scalar(token, TokenSessionId)?,
            elevated: scalar(token, TokenElevation)? != 0,
            ui_access: scalar(token, TokenUIAccess)? != 0,
            app_container: scalar(token, TokenIsAppContainer)? != 0,
        };
        let after = statistics(token)?;
        if value.id != luid(after.TokenId) || value.modified != luid(after.ModifiedId) {
            return Err(denied());
        }
        Ok(value)
    }
}

fn statistics(token: HANDLE) -> io::Result<TOKEN_STATISTICS> {
    // SAFETY: TokenStatistics returns TOKEN_STATISTICS, with size checked by read.
    unsafe { Buffer::query(token, TokenStatistics)?.read() }
}

fn scalar(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<u32> {
    const {
        assert!(size_of::<TOKEN_ELEVATION>() == size_of::<u32>());
    }
    // SAFETY: callers only pass DWORD-valued classes, including TOKEN_ELEVATION's single DWORD.
    unsafe { Buffer::query(token, class)?.read() }
}

struct Buffer {
    words: Vec<usize>,
    length: usize,
}

impl Buffer {
    fn query(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Self> {
        let mut length = 0;
        // SAFETY: a null/zero buffer queries the required length of this valid token.
        unsafe {
            GetTokenInformation(token, class, ptr::null_mut(), 0, &mut length);
        }
        if length == 0 || length > 65_536 {
            return Err(denied());
        }
        let mut words = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        let capacity = length;
        // SAFETY: words is aligned for Windows token structures and has at least capacity bytes.
        check(unsafe {
            GetTokenInformation(
                token,
                class,
                words.as_mut_ptr().cast(),
                capacity,
                &mut length,
            )
        })?;
        if length > capacity {
            return Err(denied());
        }
        Ok(Self {
            words,
            length: length as usize,
        })
    }

    unsafe fn read<T: Copy>(&self) -> io::Result<T> {
        if size_of::<T>() > self.length || align_of::<T>() > align_of::<usize>() {
            return Err(denied());
        }
        // SAFETY: the caller supplies the queried token type; size and alignment are checked above.
        Ok(unsafe { self.words.as_ptr().cast::<T>().read() })
    }
}

unsafe fn sid(value: PSID) -> io::Result<String> {
    let mut text = ptr::null_mut();
    // SAFETY: caller supplies a SID inside a live, successful Windows token query buffer.
    check(unsafe { ConvertSidToStringSidW(value, &mut text) })?;
    // SAFETY: conversion returns an allocated NUL-terminated string, freed once after copying.
    let result = unsafe { super::super::from_wide(text) };
    // SAFETY: ConvertSidToStringSidW documents LocalFree for this owned allocation.
    unsafe {
        LocalFree(text.cast());
    }
    Ok(result)
}

fn alive(handle: HANDLE) -> io::Result<()> {
    // SAFETY: the caller holds an open process handle with SYNCHRONIZE access; no wait is performed.
    if unsafe { WaitForSingleObject(handle, 0) } != WAIT_TIMEOUT {
        return Err(denied());
    }
    Ok(())
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests;
