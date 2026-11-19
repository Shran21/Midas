use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Foundation::DuplicateHandle,
    System::Threading::{
        GetCurrentProcess, GetProcessId, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

use super::{
    Peer,
    broker::SuspendedBroker,
    denied,
    launch::{RunningWorker, SuspendedWorker},
    tokens::no_impersonation,
};

pub(super) const QUERY_ACCESS: u32 = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;

pub mod delivery;

/// Starts the matching pair under one deadline. A failed startup drops both job owners.
pub async fn start_pair(
    parent: &Peer,
    broker: SuspendedBroker,
    worker: SuspendedWorker,
    deadline: tokio::time::Instant,
) -> io::Result<[RunningWorker; 2]> {
    let worker = Prepared::new(parent, &broker, worker)?;
    let endpoint = uuid::Uuid::new_v4();
    let listening = broker.listen(endpoint, deadline).await?;
    let running_worker = worker.deliver_to(parent, endpoint, deadline).await?;
    let running_broker = listening.connected().await?;
    Ok([running_broker, running_worker])
}

/// Owns the suspended worker and its single query-only companion handle transfer.
pub struct Prepared {
    worker: SuspendedWorker,
    broker: Peer,
    handle_value: u64,
}

impl Prepared {
    /// Resumes the worker and returns it only after it acknowledges the verified metadata.
    pub async fn deliver(
        self,
        parent: &Peer,
        endpoint: &str,
        deadline: tokio::time::Instant,
    ) -> io::Result<RunningWorker> {
        self.deliver_to(parent, super::endpoint_id(endpoint)?, deadline)
            .await
    }

    async fn deliver_to(
        self,
        parent: &Peer,
        endpoint: uuid::Uuid,
        deadline: tokio::time::Instant,
    ) -> io::Result<RunningWorker> {
        let id = self.worker.bootstrap_id.ok_or_else(denied)?;
        let binding = self.worker.binding(parent)?;
        let value = self.handle_value;
        let sender = delivery::Sender::bind(id, self.worker.peer()?)?;
        let broker = self.broker.duplicate()?;
        if tokio::time::Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let running = self.resume(parent)?;
        sender
            .send(value, endpoint, &binding, &broker, deadline)
            .await?;
        Ok(running)
    }

    pub fn new(
        parent: &Peer,
        broker: &SuspendedBroker,
        worker: SuspendedWorker,
    ) -> io::Result<Self> {
        worker.binding(parent)?;
        broker.check_worker(parent, &worker.peer()?)?;
        let peer = broker.peer()?;
        let (worker, handle_value) = worker.transfer_broker(parent, &peer)?;
        broker.check_worker(parent, &worker.peer()?)?;
        Ok(Self {
            worker,
            broker: peer,
            handle_value,
        })
    }

    /// Valid only in this worker's handle table; protected metadata delivery is still required.
    pub fn handle_value(&self, parent: &Peer) -> io::Result<u64> {
        self.worker.binding(parent)?;
        self.broker.recheck()?;
        Ok(self.handle_value)
    }

    pub fn peer(&self, parent: &Peer) -> io::Result<Peer> {
        self.handle_value(parent)?;
        self.worker.peer()
    }

    pub fn resume(self, parent: &Peer) -> io::Result<RunningWorker> {
        self.handle_value(parent)?;
        let running = self.worker.resume(parent)?;
        self.broker.recheck()?;
        Ok(running)
    }
}

/// Clones a delivered handle; never owns or closes the supplied numeric value.
/// Protected delivery and the pipe's pinned peer checks remain required.
/// The image must come from trusted installation configuration, never the received metadata.
pub fn import_broker(handle_value: u64, installed_image: &Path) -> io::Result<Peer> {
    Peer::broker(import_process(handle_value)?, installed_image)
}

fn import_process(handle_value: u64) -> io::Result<OwnedHandle> {
    no_impersonation()?;
    let value = usize::try_from(handle_value).map_err(|_| denied())?;
    if value == 0 || value > isize::MAX as usize {
        return Err(denied());
    }
    let source = value as *mut std::ffi::c_void;
    // SAFETY: the kernel validates this borrowed numeric handle; it is never closed or dereferenced here.
    let pid = unsafe { GetProcessId(source) };
    if pid == 0 || pid == std::process::id() {
        return Err(denied());
    }
    let mut raw = ptr::null_mut();
    // SAFETY: the kernel creates a separate non-inherited handle; CLOSE_SOURCE and SAME_ACCESS are absent.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source,
            GetCurrentProcess(),
            &mut raw,
            QUERY_ACCESS,
            0,
            0,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: only the successful duplication's new owned handle is adopted.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    // SAFETY: verify the duplicated object still matches the process inspected before duplication.
    if unsafe { GetProcessId(process.as_raw_handle()) } != pid {
        return Err(denied());
    }
    Ok(process)
}

#[cfg(test)]
mod tests;
