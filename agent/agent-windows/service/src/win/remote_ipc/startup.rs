use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use windows_sys::Win32::{
    Foundation::{DuplicateHandle, ERROR_SUCCESS, GetLastError, LUID, SetLastError},
    Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
        SE_ASSIGNPRIMARYTOKEN_NAME, SE_INCREASE_QUOTA_NAME, SE_PRIVILEGE_ENABLED, SE_TCB_NAME,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::Threading::{
        GetCurrentProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

use super::{
    Peer, denied,
    identity::{Role, Token, luid, privileges},
    tokens::{current_token, no_impersonation},
};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct Broker {
    peer: Peer,
    adjustment: Adjustment,
    _exclusive: Exclusive,
}

impl Broker {
    pub fn initialize(installed_image: &Path) -> io::Result<Self> {
        no_impersonation()?;
        let token = current_token(TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES)?;
        let before = Token::read(&token)?;
        if !before.allows(&Role::Broker) {
            return Err(denied());
        }
        let exclusive = Exclusive::acquire(&ACTIVE)?;
        let required = [
            lookup(SE_TCB_NAME)?,
            lookup(SE_ASSIGNPRIMARYTOKEN_NAME)?,
            lookup(SE_INCREASE_QUOTA_NAME)?,
        ];
        let mut adjustment = Adjustment::enable(token, &required)?;
        let result = (|| {
            no_impersonation()?;
            if !before.same_primary_identity(&Token::read(&adjustment.token)?) {
                return Err(denied());
            }
            let peer = Peer::broker(current_process()?, installed_image)?;
            enabled(&adjustment.token, &required)?;
            if Token::read(&current_token(TOKEN_QUERY)?)? != Token::read(&adjustment.token)? {
                return Err(denied());
            }
            peer.check(std::process::id())?;
            Ok(peer)
        })();
        match result {
            Ok(peer) => Ok(Self {
                peer,
                adjustment,
                _exclusive: exclusive,
            }),
            Err(error) => {
                adjustment.restore()?;
                Err(error)
            }
        }
    }

    pub fn peer(&self) -> io::Result<&Peer> {
        no_impersonation()?;
        self.peer.check(std::process::id())?;
        Ok(&self.peer)
    }

    /// Call only after all workers and their IPC owners have stopped.
    pub fn finish(mut self) -> io::Result<()> {
        let identity = self.peer().map(|_| ());
        self.adjustment.restore()?;
        identity
    }
}

struct Exclusive(&'static AtomicBool);

impl Exclusive {
    fn acquire(active: &'static AtomicBool) -> io::Result<Self> {
        active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                io::Error::new(io::ErrorKind::AlreadyExists, "broker already initialized")
            })?;
        Ok(Self(active))
    }
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[repr(C)]
#[derive(Default)]
struct Privileges {
    count: u32,
    entries: [LUID_AND_ATTRIBUTES; 3],
}

const _: () = {
    assert!(align_of::<Privileges>() == align_of::<TOKEN_PRIVILEGES>());
    assert!(
        std::mem::offset_of!(Privileges, entries)
            == std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges)
    );
};

struct Adjustment {
    token: OwnedHandle,
    previous: Privileges,
    armed: bool,
}

impl Adjustment {
    fn enable(token: OwnedHandle, required: &[LUID]) -> io::Result<Self> {
        if required.is_empty()
            || required.len() > 3
            || required
                .iter()
                .enumerate()
                .any(|(i, id)| required[..i].iter().any(|other| luid(*id) == luid(*other)))
        {
            return Err(denied());
        }
        let mut requested = Privileges {
            count: required.len() as u32,
            ..Default::default()
        };
        for (entry, id) in requested.entries.iter_mut().zip(required) {
            *entry = LUID_AND_ATTRIBUTES {
                Luid: *id,
                Attributes: SE_PRIVILEGE_ENABLED,
            };
        }
        let mut guard = Self {
            token,
            previous: Privileges::default(),
            armed: true,
        };
        let mut returned = 0;
        // SAFETY: both buffers have native layout and room for every requested entry; the token is owned.
        let (ok, error) = unsafe {
            SetLastError(ERROR_SUCCESS);
            let ok = AdjustTokenPrivileges(
                guard.token.as_raw_handle(),
                0,
                ptr::from_ref(&requested).cast(),
                size_of::<Privileges>() as u32,
                ptr::from_mut(&mut guard.previous).cast(),
                &mut returned,
            );
            (ok, GetLastError())
        };
        if ok == 0 || error != ERROR_SUCCESS {
            guard.restore()?;
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        if let Err(error) = enabled(&guard.token, required) {
            guard.restore()?;
            return Err(error);
        }
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.armed {
            return Ok(());
        }
        if self.previous.count as usize > self.previous.entries.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid privilege rollback state",
            ));
        }
        if self.previous.count != 0 {
            // SAFETY: restore only entries returned by the successful native adjustment, on the same owned token.
            let (ok, error) = unsafe {
                SetLastError(ERROR_SUCCESS);
                let ok = AdjustTokenPrivileges(
                    self.token.as_raw_handle(),
                    0,
                    ptr::from_ref(&self.previous).cast(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                (ok, GetLastError())
            };
            if ok == 0 || error != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let actual = privileges(&self.token)?;
            if !self.previous.entries[..self.previous.count as usize]
                .iter()
                .all(|entry| {
                    actual.iter().any(|(id, attributes)| {
                        *id == luid(entry.Luid)
                            && attributes & SE_PRIVILEGE_ENABLED
                                == entry.Attributes & SE_PRIVILEGE_ENABLED
                    })
                })
            {
                return Err(io::Error::other(
                    "service privilege restoration was not confirmed",
                ));
            }
        }
        self.armed = false;
        Ok(())
    }
}

impl Drop for Adjustment {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            tracing::error!(%error, "service privilege rollback failed");
        }
    }
}

fn enabled(token: &OwnedHandle, required: &[LUID]) -> io::Result<()> {
    let actual = privileges(token)?;
    if required.iter().all(|id| {
        actual.iter().any(|(other, attributes)| {
            *other == luid(*id) && attributes & SE_PRIVILEGE_ENABLED != 0
        })
    }) {
        Ok(())
    } else {
        Err(denied())
    }
}

fn lookup(name: windows_sys::core::PCWSTR) -> io::Result<LUID> {
    let mut id = LUID::default();
    // SAFETY: callers use static native privilege names; id is a writable output.
    if unsafe { LookupPrivilegeValueW(ptr::null(), name, &mut id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(id)
}

fn current_process() -> io::Result<OwnedHandle> {
    let mut handle = ptr::null_mut();
    // SAFETY: duplicate this process's pseudo handle into a query/synchronize-only, non-inheritable handle.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            GetCurrentProcess(),
            GetCurrentProcess(),
            &mut handle,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: DuplicateHandle succeeded and returned one owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

#[cfg(test)]
mod tests;
