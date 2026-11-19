//! Process-bound control IPC. A verified peer is not a remote-session grant or desktop authorization.

pub mod bootstrap;
pub mod broker;
mod identity;
pub mod launch;
pub mod participant;
pub mod startup;
pub mod tokens;
pub use identity::{Peer, Worker};

use std::{
    fmt,
    fs::OpenOptions,
    io,
    ops::Deref,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions},
    time::{Instant, timeout_at},
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_WRITE_DATA, SECURITY_IDENTIFICATION,
    },
    System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
};
use zeroize::Zeroizing;

use identity::Role;

const PREFIX: &str = r"\\.\pipe\midas-remote-";
const MAX_FRAME: usize = 65_536;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// A freshly resumed process may need longer than one I/O step to start and connect, for example
/// while the executable is scanned on its first run.
pub(in crate::win::remote_ipc) const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const MAGIC: &[u8; 4] = b"MRC1";
const CLIENT_ACCESS: u32 = FILE_GENERIC_READ | FILE_WRITE_DATA;

pub struct Frame(Zeroizing<Vec<u8>>);

impl Deref for Frame {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Frame { .. }")
    }
}

pub struct Listener {
    name: String,
    pipe: NamedPipeServer,
    peer: Peer,
}

impl Listener {
    /// The listener belongs to the pinned local broker and accepts only the launcher's worker.
    pub fn bind(broker: &Peer, worker: Peer) -> io::Result<Self> {
        Self::bind_at(broker, worker, uuid::Uuid::new_v4())
    }

    fn bind_at(broker: &Peer, worker: Peer, endpoint: uuid::Uuid) -> io::Result<Self> {
        if broker.role != Role::Broker {
            return Err(denied());
        }
        broker.check(std::process::id())?;
        Self::create(worker, endpoint)
    }

    fn create(peer: Peer, endpoint: uuid::Uuid) -> io::Result<Self> {
        let name = format!("{PREFIX}{endpoint}");
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1)
            .in_buffer_size(4096)
            .out_buffer_size(4096);
        let pipe = super::create_pipe(&options, &name, &peer.acl()?)?;
        Ok(Self { name, pipe, peer })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn accept(self, deadline: Instant) -> io::Result<Connection<NamedPipeServer>> {
        until(deadline, self.pipe.connect()).await?;
        self.peer.check(peer_pid(&self.pipe, true)?)?;
        Ok(Connection {
            pipe: Some(self.pipe),
            peer: self.peer,
            server: true,
        })
    }
}

/// The broker handle must arrive through protected bootstrap, not a PID supplied by the pipe.
pub fn connect(name: &str, broker: Peer) -> io::Result<Connection<NamedPipeClient>> {
    if broker.role != Role::Broker {
        return Err(denied());
    }
    open(name, broker)
}

fn open(name: &str, peer: Peer) -> io::Result<Connection<NamedPipeClient>> {
    endpoint_id(name)?;
    let pipe = OpenOptions::new()
        .access_mode(CLIENT_ACCESS)
        .custom_flags(FILE_FLAG_OVERLAPPED)
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(name)
        .map_err(|error| io::Error::new(error.kind(), format!("open control pipe: {error}")))?;
    peer.check(peer_pid(&pipe, false)?)?;
    let pipe = super::into_pipe_client(pipe)
        .map_err(|error| io::Error::new(error.kind(), format!("register control pipe: {error}")))?;
    Ok(Connection {
        pipe: Some(pipe),
        peer,
        server: false,
    })
}

fn endpoint_id(name: &str) -> io::Result<uuid::Uuid> {
    let suffix = name
        .strip_prefix(PREFIX)
        .filter(|s| s.len() == 36)
        .ok_or_else(denied)?;
    uuid::Uuid::parse_str(suffix).map_err(|_| denied())
}

/// Serialized, bounded control frames. Timeout, cancellation or failed verification closes the channel.
pub struct Connection<S> {
    pipe: Option<S>,
    peer: Peer,
    server: bool,
}

impl<S: AsyncRead + AsyncWrite + AsRawHandle + Unpin> Connection<S> {
    pub async fn send(&mut self, payload: &[u8], deadline: Instant) -> io::Result<()> {
        let mut pipe = self.pipe.take().ok_or_else(denied)?;
        if payload.is_empty() || payload.len() > MAX_FRAME {
            return Err(denied());
        }
        self.peer.check(peer_pid(&pipe, self.server)?)?;
        bounded(deadline, async {
            pipe.write_all(MAGIC).await?;
            pipe.write_u32_le(payload.len() as u32).await?;
            pipe.write_all(payload).await
        })
        .await?;
        self.peer.check(peer_pid(&pipe, self.server)?)?;
        self.pipe = Some(pipe);
        Ok(())
    }

    pub async fn receive(&mut self, deadline: Instant) -> io::Result<Frame> {
        let mut pipe = self.pipe.take().ok_or_else(denied)?;
        self.peer.check(peer_pid(&pipe, self.server)?)?;
        let payload = bounded(deadline, async {
            let mut magic = [0u8; 4];
            pipe.read_exact(&mut magic).await?;
            if &magic != MAGIC {
                return Err(denied());
            }
            let length = pipe.read_u32_le().await? as usize;
            if length == 0 || length > MAX_FRAME {
                return Err(denied());
            }
            let mut payload = Zeroizing::new(vec![0; length]);
            pipe.read_exact(&mut payload).await?;
            Ok(payload)
        })
        .await?;
        self.peer.check(peer_pid(&pipe, self.server)?)?;
        self.pipe = Some(pipe);
        Ok(Frame(payload))
    }
}

fn peer_pid(pipe: &impl AsRawHandle, server: bool) -> io::Result<u32> {
    let mut pid = 0;
    let handle = pipe.as_raw_handle() as HANDLE;
    // SAFETY: the borrowed pipe remains open and pid is a valid output parameter.
    let ok = unsafe {
        if server {
            GetNamedPipeClientProcessId(handle, &mut pid)
        } else {
            GetNamedPipeServerProcessId(handle, &mut pid)
        }
    };
    if ok == 0 || pid == 0 {
        return Err(denied());
    }
    Ok(pid)
}

async fn bounded<T>(
    deadline: Instant,
    operation: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    let now = Instant::now();
    if deadline <= now {
        return Err(io::ErrorKind::TimedOut.into());
    }
    timeout_at(deadline.min(now + IO_TIMEOUT), operation)
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

/// Waits for a peer to arrive until the caller's deadline; established-channel I/O uses `bounded`.
async fn until<T>(
    deadline: Instant,
    operation: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    if deadline <= Instant::now() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    timeout_at(deadline, operation)
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "remote control channel refused",
    )
}

#[cfg(test)]
mod tests;
