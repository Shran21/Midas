use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Security::{
        DuplicateTokenEx, SecurityIdentification, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
        TOKEN_QUERY, TokenPrimary,
    },
    System::Threading::{
        CreateProcessAsUserW, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

use super::{
    super::{SecurityAttributes, sessions::Binding},
    Peer, denied,
    identity::{Role, Token},
    launch::{Process, RunningWorker, SuspendedWorker, environment, image_path},
    tokens::{current_token, no_impersonation},
};

/// A session-zero companion whose identity is queryable only by the selected user logon.
pub struct SuspendedBroker {
    process: Process,
    identity: Peer,
    preparation: Preparation,
}

impl SuspendedBroker {
    /// Both images and the suspended user must come from the trusted installed launcher.
    pub fn spawn(
        parent: &Peer,
        user: &SuspendedWorker,
        installed_image: &Path,
    ) -> io::Result<Self> {
        let binding = user.binding(parent)?;
        Self::create(
            parent,
            user.peer()?,
            binding,
            installed_image,
            "remote-broker",
        )
    }

    fn create(
        parent: &Peer,
        user: Peer,
        binding: Binding,
        image: &Path,
        arguments: &str,
    ) -> io::Result<Self> {
        let preparation = Preparation::new(parent, user, binding)?;
        let application = image_path(image)?;
        let directory = image_path(image.parent().ok_or_else(denied)?)?;
        let mut command = vec![b'"' as u16];
        command.extend_from_slice(&application[..application.len() - 1]);
        command.extend(format!("\" {arguments}").encode_utf16());
        command.push(0);
        if command.len() > 32_767 {
            return Err(denied());
        }
        let environment = environment()?;
        let mut attributes = preparation.process_attributes()?;
        let process = Process::create(None, |startup, flags, information| {
            // SAFETY: the token, descriptor, buffers and extended job attributes stay alive; no inheritance.
            unsafe {
                CreateProcessAsUserW(
                    preparation.token.as_raw_handle(),
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    attributes.as_mut_ptr().cast(),
                    ptr::null(),
                    0,
                    flags,
                    environment.as_ptr().cast(),
                    directory.as_ptr(),
                    startup,
                    information,
                )
            }
        })?;
        preparation.check()?;
        let identity = Peer::broker(process.query_handle()?, image)?;
        Ok(Self {
            process,
            identity,
            preparation,
        })
    }

    pub fn peer(&self) -> io::Result<Peer> {
        self.preparation.check()?;
        self.identity.duplicate()
    }

    pub fn resume(self) -> io::Result<RunningWorker> {
        let peer = self.peer()?;
        let process = self.process.resume()?;
        self.preparation.check()?;
        peer.check(process.pid()?)?;
        RunningWorker::bound(process, self.preparation.binding)
    }
}

struct Preparation {
    token: OwnedHandle,
    expected: Token,
    source: Token,
    parent: Peer,
    user: Peer,
    binding: Binding,
}

impl Preparation {
    fn new(parent: &Peer, user: Peer, binding: Binding) -> io::Result<Self> {
        no_impersonation()?;
        binding.check()?;
        if parent.role != Role::Broker {
            return Err(denied());
        }
        parent.check(std::process::id())?;
        let sid = user.user_logon_sid()?;
        let mut attributes = access(sid, TOKEN_QUERY)?;
        let original = current_token(TOKEN_QUERY | TOKEN_DUPLICATE)?;
        let source = Token::read(&original)?;
        let token = copy(&original, &mut attributes)?;
        let expected = Token::read(&token)?;
        if !expected.allows(&Role::Broker)
            || !source.same_logon(&expected)
            || source.same_primary_identity(&expected)
            || Token::read(&original)? != source
        {
            return Err(denied());
        }
        let prepared = Self {
            token,
            expected,
            source,
            parent: parent.duplicate()?,
            user,
            binding,
        };
        prepared.check()?;
        Ok(prepared)
    }

    fn check(&self) -> io::Result<()> {
        no_impersonation()?;
        self.binding.check()?;
        self.parent.check(std::process::id())?;
        self.user.recheck()?;
        if Token::read(&self.token)? != self.expected
            || Token::read(&current_token(TOKEN_QUERY)?)? != self.source
        {
            return Err(denied());
        }
        self.binding.check()
    }

    fn process_attributes(&self) -> io::Result<SecurityAttributes> {
        self.check()?;
        access(
            self.user.user_logon_sid()?,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
        )
    }
}

fn access(logon_sid: &str, rights: u32) -> io::Result<SecurityAttributes> {
    SecurityAttributes::from_sddl(&format!("D:P(A;;GA;;;SY)(A;;0x{rights:08x};;;{logon_sid})"))
}

fn copy(source: &OwnedHandle, attributes: &mut SecurityAttributes) -> io::Result<OwnedHandle> {
    let mut token = ptr::null_mut();
    // SAFETY: the descriptor applies to this distinct token object, not the source or TokenDefaultDacl.
    let ok = unsafe {
        DuplicateTokenEx(
            source.as_raw_handle(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            attributes.as_mut_ptr().cast(),
            SecurityIdentification,
            TokenPrimary,
            &mut token,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful duplication returns one owned primary token handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

#[cfg(test)]
mod tests;
