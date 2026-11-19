use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Security::{
        DACL_SECURITY_INFORMATION, DuplicateTokenEx, GetKernelObjectSecurity,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, IsValidAcl,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
        SecurityIdentification, SetKernelObjectSecurity, TOKEN_ALL_ACCESS, TOKEN_ASSIGN_PRIMARY,
        TOKEN_DUPLICATE, TOKEN_QUERY, TokenPrimary,
    },
    System::Threading::{
        CreateProcessAsUserW, OpenProcessToken, PROCESS_ALL_ACCESS,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

use super::{
    super::{SecurityAttributes, sessions::Binding},
    Peer,
    bootstrap::delivery,
    denied,
    identity::{Role, Token},
    launch::{Process, RunningWorker, SuspendedWorker, image_path},
    tokens::{current_token, no_impersonation},
};

/// A session-zero companion whose identity is queryable only by the selected user logon.
pub struct SuspendedBroker {
    process: Process,
    identity: Peer,
    preparation: Preparation,
    bootstrap_id: uuid::Uuid,
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
        let bootstrap_id = uuid::Uuid::new_v4();
        let environment = delivery::broker_environment(bootstrap_id)?;
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
        preparation.configure_actual_token(&process, image)?;
        let identity = Peer::broker(process.query_handle()?, image)?;
        Ok(Self {
            process,
            identity,
            preparation,
            bootstrap_id,
        })
    }

    pub fn peer(&self) -> io::Result<Peer> {
        self.preparation.check()?;
        self.identity.duplicate()
    }

    pub(super) fn check_worker(&self, parent: &Peer, worker: &Peer) -> io::Result<()> {
        self.preparation.check()?;
        self.preparation.parent.same_process(parent)?;
        self.preparation.user.same_process(worker)
    }

    pub(super) async fn listen(
        mut self,
        endpoint: uuid::Uuid,
        deadline: tokio::time::Instant,
    ) -> io::Result<RunningWorker> {
        let sender = delivery::Sender::for_broker(self.bootstrap_id, self.peer()?)?;
        let worker = self.preparation.user.duplicate()?;
        let binding = self.preparation.binding.clone();
        let handle;
        (self.process, handle) = self.process.transfer_peer(&worker)?;
        if tokio::time::Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let running = self.resume()?;
        sender
            .send(handle, endpoint, &binding, &worker, deadline)
            .await?;
        Ok(running)
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
        let mut attributes = access(sid, TOKEN_QUERY, TOKEN_ALL_ACCESS)?;
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
            PROCESS_ALL_ACCESS,
        )
    }

    fn configure_actual_token(&self, process: &Process, image: &Path) -> io::Result<()> {
        self.check()?;
        let peer = Peer::broker(process.query_handle()?, image)?;
        let handle = process.query_handle()?;
        let mut raw = ptr::null_mut();
        // SAFETY: opens only this retained, suspended child; READ_CONTROL/WRITE_DAC configure its token object.
        let ok = unsafe {
            OpenProcessToken(handle.as_raw_handle(), TOKEN_QUERY | 0x0006_0000, &mut raw)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful OpenProcessToken returns one owned token handle.
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let before = Token::read(&token)?;
        if before.same_primary_identity(&self.source) || !before.same_logon(&self.expected) {
            return Err(denied());
        }
        let attributes = access(self.user.user_logon_sid()?, TOKEN_QUERY, TOKEN_ALL_ACCESS)?;
        replace_dacl(&token, &attributes)?;
        if Token::read(&token)? != before {
            return Err(denied());
        }
        peer.recheck()?;
        self.check()
    }
}

fn access(logon_sid: &str, rights: u32, system_rights: u32) -> io::Result<SecurityAttributes> {
    SecurityAttributes::from_sddl(&format!(
        "D:P(A;;0x{system_rights:08x};;;SY)(A;;0x{rights:08x};;;{logon_sid})"
    ))
}

fn replace_dacl(token: &OwnedHandle, attributes: &SecurityAttributes) -> io::Result<()> {
    // SAFETY: the converted descriptor remains owned and live for this call.
    let expected = unsafe { descriptor_dacl(attributes.descriptor) }?;
    // SAFETY: changes only DACL protection on the explicitly opened token object, not token privileges or integrity.
    if unsafe {
        SetKernelObjectSecurity(
            token.as_raw_handle(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            attributes.descriptor,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if read_dacl(token)? != expected {
        return Err(denied());
    }
    Ok(())
}

fn read_dacl(token: &OwnedHandle) -> io::Result<(Vec<u8>, bool)> {
    let mut length = 0;
    // SAFETY: asks for the required size without an output buffer.
    unsafe {
        GetKernelObjectSecurity(
            token.as_raw_handle(),
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            0,
            &mut length,
        );
    }
    if length == 0 || length > 65_536 {
        return Err(denied());
    }
    let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    let capacity = length;
    // SAFETY: the aligned buffer has capacity bytes and length is a writable output.
    if unsafe {
        GetKernelObjectSecurity(
            token.as_raw_handle(),
            DACL_SECURITY_INFORMATION,
            buffer.as_mut_ptr().cast(),
            capacity,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if length > capacity {
        return Err(denied());
    }
    // SAFETY: the successful native query populated this live security descriptor.
    unsafe { descriptor_dacl(buffer.as_mut_ptr().cast()) }
}

unsafe fn descriptor_dacl(descriptor: PSECURITY_DESCRIPTOR) -> io::Result<(Vec<u8>, bool)> {
    let mut present = 0;
    let mut defaulted = 0;
    let mut acl = ptr::null_mut();
    // SAFETY: caller provides a complete live descriptor from a successful Windows query/conversion.
    if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) } == 0
        || present == 0
        || acl.is_null()
    {
        return Err(denied());
    }
    // SAFETY: acl points into that live native descriptor.
    if unsafe { IsValidAcl(acl) } == 0 {
        return Err(denied());
    }
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: all outputs are valid and the descriptor remains alive.
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: IsValidAcl verified the native ACL, whose length is its bounded u16 AclSize field.
    let bytes =
        unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize as usize) }.to_vec();
    Ok((bytes, control & SE_DACL_PROTECTED != 0))
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
