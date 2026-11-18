use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::Arc,
};

use midas_protocol::sessions::{OsSession, Presence, valid_sessions};
use windows_sys::Win32::{
    Foundation::{DuplicateHandle, ERROR_NO_TOKEN, HANDLE},
    Security::{
        DuplicateTokenEx, GetTokenInformation, SecurityIdentification, SetTokenInformation,
        TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_LINKED_TOKEN,
        TOKEN_QUERY, TokenLinkedToken, TokenPrimary, TokenSessionId,
    },
    System::{
        RemoteDesktop::WTSQueryUserToken,
        Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
    },
};

use super::super::sessions::{Binding, Observer};
use super::{
    Peer, Worker, denied,
    identity::{Role, Token},
};

const LAUNCH_ACCESS: u32 = TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY;

/// A native token pinned to an observed session generation, not a remote execution grant.
pub struct PreparedToken {
    handle: OwnedHandle,
    expected: Token,
    broker: Token,
    kind: Worker,
    target: OsSession,
    observer: Arc<Observer>,
    binding: Binding,
    source: Source,
}

impl PreparedToken {
    pub fn user(broker: &Peer, observer: Arc<Observer>, target: OsSession) -> io::Result<Self> {
        let broker_state = broker_token(broker)?;
        let binding = observer.bind(&target)?;
        check_target(&target, observer.collect(), true)?;
        let selected = select_user(wts_token(target.id)?, target.id)?;
        let prepared = Self {
            expected: Token::read(&selected.handle)?,
            handle: selected.handle,
            broker: broker_state,
            kind: selected.kind,
            target,
            observer,
            binding,
            source: Source::Normal(selected.binding),
        };
        prepared.check(broker)?;
        Ok(prepared)
    }

    pub fn privileged(
        broker: &Peer,
        observer: Arc<Observer>,
        target: OsSession,
    ) -> io::Result<Self> {
        let broker_state = broker_token(broker)?;
        let binding = observer.bind(&target)?;
        check_target(&target, observer.collect(), false)?;
        let source = if target.presence == Presence::Occupied as i32 {
            let state = Token::read(&wts_token(target.id)?)?;
            state.observed_user(target.id)?;
            Source::Occupied(state)
        } else {
            vacant(wts_token(target.id))?;
            Source::Vacant
        };
        let original = current_token(TOKEN_QUERY | TOKEN_DUPLICATE)?;
        if Token::read(&original)? != broker_state {
            return Err(denied());
        }
        let adjusted = duplicate(&original, LAUNCH_ACCESS | TOKEN_ADJUST_SESSIONID)?;
        // SAFETY: only the newly duplicated primary token is moved to this validated session.
        check(unsafe {
            SetTokenInformation(
                adjusted.as_raw_handle(),
                TokenSessionId,
                ptr::from_ref(&target.id).cast(),
                size_of::<u32>() as u32,
            )
        })?;
        let handle = reduce(&adjusted)?;
        let expected = Token::read(&handle)?;
        if !expected.allows(&Role::Worker {
            kind: Worker::Privileged,
            session: target.id,
        }) || Token::read(&original)? != broker_state
        {
            return Err(denied());
        }
        let prepared = Self {
            handle,
            expected,
            broker: broker_state,
            kind: Worker::Privileged,
            target,
            observer,
            binding,
            source,
        };
        prepared.check(broker)?;
        Ok(prepared)
    }

    pub(super) fn check(&self, broker: &Peer) -> io::Result<()> {
        self.binding.check()?;
        if broker_token(broker)? != self.broker || Token::read(&self.handle)? != self.expected {
            return Err(denied());
        }
        let user = matches!(self.kind, Worker::User { .. });
        check_target(&self.target, self.observer.collect(), user)?;
        match &self.source {
            Source::Normal(expected) => {
                let current = select_user(wts_token(self.target.id)?, self.target.id)?;
                if !expected.matches(&current.binding) {
                    return Err(denied());
                }
            }
            Source::Occupied(expected) => {
                let current = Token::read(&wts_token(self.target.id)?)?;
                current.observed_user(self.target.id)?;
                if !expected.same_logon(&current) {
                    return Err(denied());
                }
            }
            Source::Vacant => vacant(wts_token(self.target.id))?,
        }
        check_target(&self.target, self.observer.collect(), user)?;
        if broker_token(broker)? != self.broker || Token::read(&self.handle)? != self.expected {
            return Err(denied());
        }
        self.binding.check()
    }

    pub(super) fn into_binding(self) -> Binding {
        self.binding
    }

    pub(super) fn raw(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }
    pub(super) fn kind(&self) -> Worker {
        self.kind.clone()
    }
    pub(super) fn session(&self) -> u32 {
        self.target.id
    }
}

enum Source {
    Normal(UserBinding),
    Occupied(Token),
    Vacant,
}

struct UserBinding {
    source: Token,
    selected: Token,
}

impl UserBinding {
    fn matches(&self, other: &Self) -> bool {
        // Fresh WTS/linked-token queries can return new token objects for the same logon.
        self.source.same_logon(&other.source) && self.selected.same_logon(&other.selected)
    }
}

struct SelectedUser {
    handle: OwnedHandle,
    kind: Worker,
    binding: UserBinding,
}

fn select_user(source: OwnedHandle, session: u32) -> io::Result<SelectedUser> {
    let before = Token::read(&source)?;
    let linked = if before.needs_linked(session)? {
        Some(linked_token(&source)?)
    } else {
        None
    };
    let selected = linked.as_ref().unwrap_or(&source);
    let selected_before = Token::read(selected)?;
    let kind = if linked.is_some() {
        before.linked_worker(&selected_before, session)?
    } else {
        selected_before.user_worker(session)?
    };
    let handle = duplicate(selected, LAUNCH_ACCESS)?;
    let copied = Token::read(&handle)?;
    if !copied.same_logon(&selected_before)
        || Token::read(&source)? != before
        || Token::read(selected)? != selected_before
    {
        return Err(denied());
    }
    Ok(SelectedUser {
        handle,
        kind,
        binding: UserBinding {
            source: before,
            selected: selected_before,
        },
    })
}

fn check_target(target: &OsSession, current: Option<Vec<OsSession>>, user: bool) -> io::Result<()> {
    let sessions = current.ok_or_else(denied)?;
    if !valid_sessions(std::slice::from_ref(target))
        || !valid_sessions(&sessions)
        || target.locked.is_none()
        || !matches!(
            Presence::try_from(target.presence),
            Ok(Presence::Occupied | Presence::Vacant)
        )
        || (target.presence == Presence::Occupied as i32 && target.account.is_empty())
        || (user && target.presence != Presence::Occupied as i32)
        || sessions.iter().find(|session| session.id == target.id) != Some(target)
    {
        return Err(denied());
    }
    Ok(())
}

fn broker_token(broker: &Peer) -> io::Result<Token> {
    if broker.role != Role::Broker {
        return Err(denied());
    }
    no_impersonation()?;
    broker.check(std::process::id())?;
    let token = Token::read(&current_token(TOKEN_QUERY)?)?;
    if !token.allows(&Role::Broker) {
        return Err(denied());
    }
    Ok(token)
}

fn vacant(result: io::Result<OwnedHandle>) -> io::Result<()> {
    match result {
        Err(error) if error.raw_os_error() == Some(ERROR_NO_TOKEN as i32) => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(denied()),
    }
}

fn no_impersonation() -> io::Result<()> {
    let mut handle = ptr::null_mut();
    // SAFETY: query this thread as the process identity; any successful handle is owned below.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut handle) } != 0 {
        // SAFETY: a successful OpenThreadToken returns one owned token handle.
        drop(unsafe { OwnedHandle::from_raw_handle(handle) });
        return Err(denied());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_NO_TOKEN as i32) {
        Ok(())
    } else {
        Err(error)
    }
}

fn current_token(access: u32) -> io::Result<OwnedHandle> {
    let mut handle = ptr::null_mut();
    // SAFETY: the current-process pseudo handle is borrowed, and handle is a writable output.
    check(unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut handle) })?;
    // SAFETY: the successful query transferred one token handle to this owner.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn wts_token(session: u32) -> io::Result<OwnedHandle> {
    let mut handle = ptr::null_mut();
    // SAFETY: session is selected from the native observer; handle is an owned output on success.
    check(unsafe { WTSQueryUserToken(session, &mut handle) })?;
    // SAFETY: WTSQueryUserToken returned one owned primary-token handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn linked_token(source: &OwnedHandle) -> io::Result<OwnedHandle> {
    let mut linked = TOKEN_LINKED_TOKEN::default();
    let mut length = 0;
    // SAFETY: the output is an aligned TOKEN_LINKED_TOKEN of the stated size.
    check(unsafe {
        GetTokenInformation(
            source.as_raw_handle(),
            TokenLinkedToken,
            ptr::from_mut(&mut linked).cast(),
            size_of_val(&linked) as u32,
            &mut length,
        )
    })?;
    if linked.LinkedToken.is_null() {
        return Err(denied());
    }
    // SAFETY: this successful information class returns a new owned token handle.
    let token = unsafe { OwnedHandle::from_raw_handle(linked.LinkedToken) };
    if length as usize != size_of::<TOKEN_LINKED_TOKEN>() {
        return Err(denied());
    }
    Ok(token)
}

fn duplicate(source: &OwnedHandle, access: u32) -> io::Result<OwnedHandle> {
    let mut handle = ptr::null_mut();
    // SAFETY: duplicates the owned source into a distinct, non-inheritable primary token.
    check(unsafe {
        DuplicateTokenEx(
            source.as_raw_handle(),
            access,
            ptr::null(),
            SecurityIdentification,
            TokenPrimary,
            &mut handle,
        )
    })?;
    // SAFETY: DuplicateTokenEx returned one owned handle to the new token object.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn reduce(source: &OwnedHandle) -> io::Result<OwnedHandle> {
    let mut handle = ptr::null_mut();
    // SAFETY: narrows the owned token's rights in this process, without handle inheritance.
    check(unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source.as_raw_handle(),
            GetCurrentProcess(),
            &mut handle,
            LAUNCH_ACCESS,
            0,
            0,
        )
    })?;
    // SAFETY: DuplicateHandle returned one new owned handle in this process.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
