//! Suspended, job-contained workers with session-bound tokens. Bootstrap and execution grants are separate.

use std::{
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, Prefix},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{DuplicateHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        JobObjects::{
            CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        },
        SystemInformation::GetSystemWindowsDirectoryW,
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
            GetProcessId, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
            PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, ResumeThread, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess,
            UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

use super::super::sessions::Binding;
use super::{Peer, denied, tokens::PreparedToken};

mod wait;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopped {
    Requested,
    WorkerExited,
    SessionChanged,
}

/// Owns all workers until stop, exit or cancellation; this does not authorize their operations.
pub async fn supervise(
    workers: Vec<RunningWorker>,
    stop: impl Future<Output = ()>,
) -> io::Result<Stopped> {
    if workers.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no workers to supervise",
        ));
    }
    let mut waits = tokio::task::JoinSet::new();
    let mut sessions = tokio::task::JoinSet::new();
    for worker in &workers {
        let wait = wait::ExitWait::new(worker.process.query_handle()?)?;
        waits.spawn(wait.wait());
        let binding = worker.session.clone();
        sessions.spawn(async move { binding.ended().await });
    }
    let stopped = tokio::select! {
        biased;
        _ = stop => Ok(Stopped::Requested),
        ended = sessions.join_next() => ended
            .ok_or_else(|| io::Error::other("session wait disappeared"))
            .and_then(|result| result
                .map(|()| Stopped::SessionChanged)
                .map_err(|_| io::Error::other("session wait failed"))),
        exited = waits.join_next() => wait_result(exited).map(|()| Stopped::WorkerExited),
    };
    let mut failure = None;
    for worker in &workers {
        if let Err(error) = worker.terminate() {
            failure.get_or_insert(error);
        }
    }
    let cleanup: io::Result<()> = tokio::time::timeout(super::IO_TIMEOUT, async {
        while !waits.is_empty() {
            wait_result(waits.join_next().await)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "worker shutdown was not confirmed"))?;
    cleanup?;
    if let Some(error) = failure {
        return Err(error);
    }
    stopped
}

fn wait_result(result: Option<Result<io::Result<()>, tokio::task::JoinError>>) -> io::Result<()> {
    result
        .ok_or_else(|| io::Error::other("worker exit wait disappeared"))?
        .map_err(|_| io::Error::other("worker exit wait failed"))?
}

pub struct SuspendedWorker {
    process: Process,
    token: PreparedToken,
    identity: Peer,
}

impl SuspendedWorker {
    /// The service supplies a prepared native token and an administrator-protected installed image.
    /// No PID, token handle, image path or command line may come from a remote request.
    pub fn spawn(broker: &Peer, token: PreparedToken, installed_image: &Path) -> io::Result<Self> {
        token.check(broker)?;
        let application = image_path(installed_image)?;
        let directory = image_path(installed_image.parent().ok_or_else(denied)?)?;
        let mut command = vec![b'"' as u16];
        command.extend_from_slice(&application[..application.len() - 1]);
        command.extend("\" remote-worker".encode_utf16());
        command.push(0);
        if command.len() > 32_767 {
            return Err(denied());
        }
        let environment = environment()?;
        let mut desktop = super::super::wide("winsta0\\default");
        let process = Process::create(Some(&mut desktop), |startup, flags, information| {
            // SAFETY: all buffers remain alive; the token is owned, handles are not inherited,
            // and startup points to the extended structure with its live job attribute.
            unsafe {
                CreateProcessAsUserW(
                    token.raw(),
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    ptr::null(),
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
        token.check(broker)?;
        let identity = Peer::worker(
            process.query_handle()?,
            installed_image,
            token.kind(),
            token.session(),
        )?;
        Ok(Self {
            process,
            token,
            identity,
        })
    }

    /// Query-only process access for IPC identity; no process-control handle leaves this owner.
    pub fn peer(&self) -> io::Result<Peer> {
        self.identity.duplicate()
    }

    pub fn resume(self, broker: &Peer) -> io::Result<RunningWorker> {
        self.token.check(broker)?;
        let peer = self.peer()?;
        let process = self.process.resume()?;
        self.token.check(broker)?;
        peer.check(process.pid()?)?;
        let session = self.token.into_binding();
        session.check()?;
        Ok(RunningWorker { process, session })
    }
}

/// Dropping this owner closes the sole job handle and terminates the worker.
pub struct RunningWorker {
    process: Process,
    session: Binding,
}

impl RunningWorker {
    pub fn has_exited(&self) -> io::Result<bool> {
        self.process.has_exited()
    }

    /// Requests termination; `has_exited` separately confirms that the process has stopped.
    pub fn terminate(&self) -> io::Result<()> {
        // SAFETY: this owner retains the job handle; no job handle is exported or inherited.
        check(unsafe { TerminateJobObject(self.process.job.as_raw_handle(), 1) })
    }
}

struct Process {
    job: OwnedHandle,
    process: OwnedHandle,
    thread: Option<OwnedHandle>,
}

impl Process {
    fn create(
        desktop: Option<&mut [u16]>,
        create: impl FnOnce(&STARTUPINFOW, u32, &mut PROCESS_INFORMATION) -> i32,
    ) -> io::Result<Self> {
        // SAFETY: an unnamed job with default security and non-inheritable ownership.
        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned one owned non-null handle.
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        // SAFETY: limits has exactly the structure and size selected by the information class.
        check(unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                size_of_val(&limits) as u32,
            )
        })?;
        let job_handle = job.as_raw_handle();
        let mut attributes = Attributes::new()?;
        // SAFETY: job_handle stays at this address until attributes is destroyed, including errors.
        check(unsafe {
            UpdateProcThreadAttribute(
                attributes.raw(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                ptr::from_ref(&job_handle).cast(),
                size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        })?;
        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: size_of::<STARTUPINFOEXW>() as u32,
                lpDesktop: desktop.map_or(ptr::null_mut(), |value| value.as_mut_ptr()),
                ..Default::default()
            },
            lpAttributeList: attributes.raw(),
        };
        let flags = CREATE_SUSPENDED
            | CREATE_NO_WINDOW
            | CREATE_UNICODE_ENVIRONMENT
            | EXTENDED_STARTUPINFO_PRESENT;
        let mut information = PROCESS_INFORMATION::default();
        check(create(&startup.StartupInfo, flags, &mut information))?;
        drop(attributes);
        // SAFETY: successful process creation returns two distinct owned handles.
        let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess) };
        // SAFETY: the successful creation's primary thread handle is owned exactly once.
        let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread) };
        let child = Self {
            job,
            process,
            thread: Some(thread),
        };
        let mut member = 0;
        // SAFETY: both handles remain owned; member is a writable BOOL output.
        check(unsafe {
            IsProcessInJob(
                child.process.as_raw_handle(),
                child.job.as_raw_handle(),
                &mut member,
            )
        })?;
        if member == 0 {
            return Err(denied());
        }
        Ok(child)
    }

    fn query_handle(&self) -> io::Result<OwnedHandle> {
        let mut result = ptr::null_mut();
        // SAFETY: duplicate only query/synchronize access into this process, without inheritance.
        check(unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.process.as_raw_handle(),
                GetCurrentProcess(),
                &mut result,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                0,
            )
        })?;
        // SAFETY: DuplicateHandle returned one owned handle in this process.
        Ok(unsafe { OwnedHandle::from_raw_handle(result) })
    }

    fn pid(&self) -> io::Result<u32> {
        // SAFETY: the retained handle always identifies the same process object.
        match unsafe { GetProcessId(self.process.as_raw_handle()) } {
            0 => Err(io::Error::last_os_error()),
            pid => Ok(pid),
        }
    }

    fn resume(mut self) -> io::Result<Self> {
        let thread = self.thread.take().ok_or_else(denied)?;
        // SAFETY: only this owner can resume the retained primary thread.
        if unsafe { ResumeThread(thread.as_raw_handle()) } != 1 {
            return Err(denied());
        }
        Ok(self)
    }

    fn has_exited(&self) -> io::Result<bool> {
        // SAFETY: nonblocking query of the retained process object, with SYNCHRONIZE access.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: this is the retained creation handle, never a looked-up or reused process ID.
        // Also covers a failed membership check; job close separately covers owner process death.
        unsafe {
            TerminateProcess(self.process.as_raw_handle(), 1);
        }
    }
}

struct Attributes(Vec<usize>);

impl Attributes {
    fn new() -> io::Result<Self> {
        let mut length = 0;
        // SAFETY: null buffer queries the size required for one attribute.
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut length);
        }
        if length == 0 || length > 65_536 {
            return Err(denied());
        }
        let mut words = vec![0usize; length.div_ceil(size_of::<usize>())];
        // SAFETY: the allocation is pointer-aligned and covers the requested size.
        check(unsafe {
            InitializeProcThreadAttributeList(words.as_mut_ptr().cast(), 1, 0, &mut length)
        })?;
        Ok(Self(words))
    }

    fn raw(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.0.as_mut_ptr().cast()
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: only successfully initialized lists reach this owner; storage remains allocated.
        unsafe {
            DeleteProcThreadAttributeList(self.raw());
        }
    }
}

fn image_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)))
        || components.next() != Some(Component::RootDir)
        || components.any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(denied());
    }
    let text: Vec<u16> = path.as_os_str().encode_wide().collect();
    if text.is_empty()
        || text.len() >= 32_767
        || text
            .iter()
            .enumerate()
            .any(|(i, c)| *c == 0 || *c == b'"' as u16 || (*c == b':' as u16 && i != 1))
    {
        return Err(denied());
    }
    Ok(text.into_iter().chain(Some(0)).collect())
}

fn environment() -> io::Result<Vec<u16>> {
    let mut directory = vec![0u16; 32_768];
    // SAFETY: directory has the stated writable capacity; this reads the OS system directory.
    let length =
        unsafe { GetSystemWindowsDirectoryW(directory.as_mut_ptr(), directory.len() as u32) }
            as usize;
    if length == 0 || length >= directory.len() {
        return Err(io::Error::last_os_error());
    }
    directory.truncate(length);
    let mut block = Vec::new();
    for (key, suffix) in [
        ("PATH=", "\\System32"),
        ("SystemRoot=", ""),
        ("WINDIR=", ""),
    ] {
        block.extend(key.encode_utf16());
        block.extend_from_slice(&directory);
        block.extend(suffix.encode_utf16());
        block.push(0);
    }
    block.push(0);
    Ok(block)
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
