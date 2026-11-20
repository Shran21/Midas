use std::{
    fs::OpenOptions,
    io,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    ptr,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions},
    time::Instant,
};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
        IsWellKnownSid, OWNER_SECURITY_INFORMATION, TOKEN_QUERY, WinLocalSystemSid,
    },
    Storage::FileSystem::{FILE_FLAG_OVERLAPPED, SECURITY_IDENTIFICATION},
};

use super::super::{
    self as ipc,
    identity::{Role, Token},
    launch::environment,
    startup::current_process,
    tokens::{current_token, no_impersonation},
};
use super::{Peer, denied, import_broker, import_process};
use crate::win::remote_ipc::participant::{Stage, reached};
use crate::win::{self, sessions::Binding};

const MAGIC: &[u8; 4] = b"MBT1";
const ACK: &[u8; 4] = b"MBA1";
const CONNECTED: &[u8; 4] = b"MBC1";

#[derive(Clone, Copy)]
enum Recipient {
    Worker,
    Broker,
    Privileged,
    Consent,
}

impl Recipient {
    fn env(self) -> &'static str {
        match self {
            Self::Worker => "MIDAS_WORKER_BOOTSTRAP",
            Self::Broker => "MIDAS_BROKER_BOOTSTRAP",
            Self::Privileged => "MIDAS_PRIVILEGED_BOOTSTRAP",
            Self::Consent => "MIDAS_CONSENT_BOOTSTRAP",
        }
    }

    fn markers(self) -> (&'static [u8; 4], &'static [u8; 4]) {
        match self {
            Self::Worker => (MAGIC, ACK),
            Self::Broker => (b"MBL1", b"MBR1"),
            Self::Privileged => (b"MBP1", b"MBQ1"),
            Self::Consent => (b"MBW1", b"MBX1"),
        }
    }

    fn environment(self, id: Uuid) -> io::Result<Vec<u16>> {
        let mut block: Vec<_> = format!("{}={}:{}\0", self.env(), std::process::id(), id)
            .encode_utf16()
            .collect();
        block.extend(environment()?);
        Ok(block)
    }
}

pub(in crate::win::remote_ipc) fn worker_environment(id: Uuid) -> io::Result<Vec<u16>> {
    Recipient::Worker.environment(id)
}

pub(in crate::win::remote_ipc) fn broker_environment(id: Uuid) -> io::Result<Vec<u16>> {
    Recipient::Broker.environment(id)
}

pub(in crate::win::remote_ipc) fn privileged_environment(id: Uuid) -> io::Result<Vec<u16>> {
    Recipient::Privileged.environment(id)
}

pub(in crate::win::remote_ipc) fn consent_environment(id: Uuid) -> io::Result<Vec<u16>> {
    Recipient::Consent.environment(id)
}

fn name(id: Uuid) -> String {
    format!(r"\\.\pipe\midas-bootstrap-{id}")
}

pub(in crate::win::remote_ipc) struct Sender {
    pipe: NamedPipeServer,
    peer: Peer,
    recipient: Recipient,
}

impl Sender {
    pub(in crate::win::remote_ipc) fn bind(id: Uuid, worker: Peer) -> io::Result<Self> {
        no_impersonation()?;
        worker.user_logon_sid()?;
        let acl = format!("O:SY{}", worker.acl()?);
        Self::create(id, worker, Recipient::Worker, &acl)
    }

    /// The consent window cannot read the service's token, so it keeps this startup pipe, whose
    /// SYSTEM owner, session and server PID it verifies on every message, as its only channel.
    pub(in crate::win::remote_ipc) fn for_consent(id: Uuid, worker: Peer) -> io::Result<Self> {
        no_impersonation()?;
        if !matches!(
            worker.role,
            Role::Worker {
                kind: super::super::Worker::User { .. },
                ..
            }
        ) {
            return Err(denied());
        }
        worker.user_logon_sid()?;
        let acl = format!("O:SY{}", worker.acl()?);
        Self::create(id, worker, Recipient::Consent, &acl)
    }

    pub(in crate::win::remote_ipc) fn for_broker(id: Uuid, broker: Peer) -> io::Result<Self> {
        no_impersonation()?;
        if broker.role != Role::Broker {
            return Err(denied());
        }
        broker.recheck()?;
        Self::create(
            id,
            broker,
            Recipient::Broker,
            "O:SYD:P(D;;GA;;;NU)(A;;GA;;;SY)",
        )
    }

    /// A privileged worker runs as SYSTEM in the target session; only SYSTEM may open its pipe.
    pub(in crate::win::remote_ipc) fn for_privileged(id: Uuid, worker: Peer) -> io::Result<Self> {
        no_impersonation()?;
        if !matches!(
            worker.role,
            Role::Worker {
                kind: super::super::Worker::Privileged,
                ..
            }
        ) {
            return Err(denied());
        }
        worker.recheck()?;
        Self::create(
            id,
            worker,
            Recipient::Privileged,
            "O:SYD:P(D;;GA;;;NU)(A;;GA;;;SY)",
        )
    }

    fn create(id: Uuid, peer: Peer, recipient: Recipient, acl: &str) -> io::Result<Self> {
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1);
        let pipe = win::create_pipe(&options, &name(id), acl)?;
        Ok(Self {
            pipe,
            peer,
            recipient,
        })
    }

    pub(in crate::win::remote_ipc) async fn send(
        mut self,
        handle: u64,
        endpoint: Uuid,
        binding: &Binding,
        counterpart: &Peer,
        deadline: Instant,
    ) -> io::Result<Sent> {
        let check = |pipe: &NamedPipeServer| {
            binding.check()?;
            counterpart.recheck()?;
            self.peer.check(ipc::peer_pid(pipe, true)?)
        };
        let (magic, acknowledgement) = self.recipient.markers();
        let mut phase = "startup connection";
        let exchange = async {
            ipc::until(deadline, self.pipe.connect()).await?;
            check(&self.pipe)?;
            let mut message = [0; 28];
            message[..4].copy_from_slice(magic);
            message[4..12].copy_from_slice(&handle.to_le_bytes());
            message[12..].copy_from_slice(endpoint.as_bytes());
            phase = "startup record";
            ipc::bounded(deadline, self.pipe.write_all(&message)).await?;
            phase = "startup receipt";
            let mut ack = [0; 4];
            ipc::bounded(deadline, self.pipe.read_exact(&mut ack)).await?;
            if &ack != acknowledgement {
                return Err(denied());
            }
            check(&self.pipe)?;
            ipc::bounded(deadline, self.pipe.write_all(b"OK")).await
        };
        let result = tokio::select! {
            biased;
            _ = binding.ended() => Err(denied()),
            result = exchange => result,
        };
        result.map_err(|error| match error.kind() {
            io::ErrorKind::TimedOut => io::Error::new(error.kind(), format!("timed out: {phase}")),
            _ => error,
        })?;
        Ok(Sent {
            pipe: self.pipe,
            peer: self.peer,
            recipient: self.recipient,
        })
    }
}

pub(in crate::win::remote_ipc) struct Sent {
    pipe: NamedPipeServer,
    peer: Peer,
    recipient: Recipient,
}

impl Sent {
    /// After the companion or the consent window confirms, its startup pipe stays open as the
    /// service's control channel.
    pub(in crate::win::remote_ipc) async fn connected(
        mut self,
        deadline: Instant,
    ) -> io::Result<Control> {
        let allowed = match self.recipient {
            Recipient::Broker => self.peer.role == Role::Broker,
            Recipient::Consent => matches!(
                self.peer.role,
                Role::Worker {
                    kind: super::super::Worker::User { .. },
                    ..
                }
            ),
            Recipient::Worker | Recipient::Privileged => false,
        };
        if !allowed {
            return Err(denied());
        }
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        let mut reply = [0u8; 4];
        ipc::bounded(deadline, self.pipe.read_exact(&mut reply)).await?;
        if &reply != CONNECTED {
            return Err(denied());
        }
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        ipc::bounded(deadline, self.pipe.write_all(b"OK")).await?;
        Ok(Control {
            pipe: self.pipe,
            peer: self.peer,
        })
    }
}

const CONTROL_LIMIT: usize = 65_536;

async fn write_message(pipe: &mut (impl AsyncWriteExt + Unpin), payload: &[u8]) -> io::Result<()> {
    if payload.is_empty() || payload.len() > CONTROL_LIMIT {
        return Err(denied());
    }
    pipe.write_u32_le(payload.len() as u32).await?;
    pipe.write_all(payload).await
}

async fn read_message(pipe: &mut (impl AsyncReadExt + Unpin)) -> io::Result<Vec<u8>> {
    let length = pipe.read_u32_le().await? as usize;
    if length == 0 || length > CONTROL_LIMIT {
        return Err(denied());
    }
    let mut payload = vec![0; length];
    pipe.read_exact(&mut payload).await?;
    Ok(payload)
}

/// The service's channel to the session companion: a request out, its reply back. Closing it
/// ends the companion's media session.
pub struct Control {
    pipe: NamedPipeServer,
    peer: Peer,
}

impl Control {
    pub async fn exchange(&mut self, request: &[u8], deadline: Instant) -> io::Result<Vec<u8>> {
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        let pipe = &mut self.pipe;
        let reply = ipc::bounded(deadline, async {
            write_message(pipe, request).await?;
            read_message(pipe).await
        })
        .await?;
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        Ok(reply)
    }

    /// A message the other side does not answer, such as the start of the session indicator.
    pub async fn send(&mut self, message: &[u8], deadline: Instant) -> io::Result<()> {
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        ipc::bounded(deadline, write_message(&mut self.pipe, message)).await?;
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)
    }

    /// The next message the other side sends by itself; `None` once it closed. Waits as long
    /// as needed.
    pub async fn receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        match read_message(&mut self.pipe).await {
            Ok(message) => {
                self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
                Ok(Some(message))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::BrokenPipe
                        | io::ErrorKind::NotConnected
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// A request a person answers: written within the I/O limit, its reply awaited until the
    /// caller's deadline.
    pub async fn ask(&mut self, request: &[u8], deadline: Instant) -> io::Result<Vec<u8>> {
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        ipc::bounded(deadline, write_message(&mut self.pipe, request)).await?;
        let reply = ipc::until(deadline, read_message(&mut self.pipe)).await?;
        self.peer.check(ipc::peer_pid(&self.pipe, true)?)?;
        Ok(reply)
    }
}

/// The companion's end of the control channel, from the verified launching service.
pub(in crate::win::remote_ipc) struct Launched {
    pipe: NamedPipeClient,
    pid: u32,
}

impl Launched {
    /// The next request; `None` once the service closed the channel. Waits as long as needed.
    pub(in crate::win::remote_ipc) async fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        verify_sender(&self.pipe, self.pid)?;
        match read_message(&mut self.pipe).await {
            Ok(message) => {
                verify_sender(&self.pipe, self.pid)?;
                Ok(Some(message))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::BrokenPipe
                        | io::ErrorKind::NotConnected
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub(in crate::win::remote_ipc) async fn reply(
        &mut self,
        payload: &[u8],
        deadline: Instant,
    ) -> io::Result<()> {
        verify_sender(&self.pipe, self.pid)?;
        ipc::bounded(deadline, write_message(&mut self.pipe, payload)).await
    }
}

pub struct Destination {
    broker: Peer,
    endpoint: Uuid,
}

impl Destination {
    pub fn connect(self) -> io::Result<ipc::Connection<NamedPipeClient>> {
        ipc::connect(&format!("{}{}", ipc::PREFIX, self.endpoint), self.broker)
    }
}

/// Reads the locator from the service-created worker environment.
pub async fn receive(installed_image: &Path, deadline: Instant) -> io::Result<Destination> {
    receive_worker(installed_image, deadline, async |destination| {
        Ok(destination)
    })
    .await
}

pub(in crate::win::remote_ipc) async fn receive_worker<T>(
    installed_image: &Path,
    deadline: Instant,
    ready: impl AsyncFnOnce(Destination) -> io::Result<T>,
) -> io::Result<T> {
    no_impersonation()?;
    Token::read(&current_token(TOKEN_QUERY)?)?.user_worker(win::current_session()?)?;
    reached(Stage::Identity);
    receive_record(Recipient::Worker, deadline, async |handle, endpoint| {
        ready(Destination {
            broker: import_broker(handle, installed_image)?,
            endpoint,
        })
        .await
    })
    .await
    .map(|(worker, _)| worker)
}

/// The consent window receives no handle and no endpoint: the verified startup pipe to the
/// service is its channel.
pub(in crate::win::remote_ipc) async fn receive_consent(deadline: Instant) -> io::Result<Launched> {
    no_impersonation()?;
    Token::read(&current_token(TOKEN_QUERY)?)?.user_worker(win::current_session()?)?;
    reached(Stage::Identity);
    let ((), launcher) = receive_record(Recipient::Consent, deadline, async |handle, endpoint| {
        if handle != 0 || !endpoint.is_nil() {
            return Err(denied());
        }
        Ok(())
    })
    .await?;
    launcher.connected(deadline).await
}

/// Reads the locator from the service-created privileged worker environment; the control
/// endpoint belongs to the service itself.
pub(in crate::win::remote_ipc) async fn receive_privileged<T>(
    installed_image: &Path,
    deadline: Instant,
    ready: impl AsyncFnOnce(Destination) -> io::Result<T>,
) -> io::Result<T> {
    no_impersonation()?;
    Token::read(&current_token(TOKEN_QUERY)?)?.privileged_worker(win::current_session()?)?;
    reached(Stage::Identity);
    receive_record(Recipient::Privileged, deadline, async |handle, endpoint| {
        ready(Destination {
            broker: import_broker(handle, installed_image)?,
            endpoint,
        })
        .await
    })
    .await
    .map(|(worker, _)| worker)
}

/// Confirms readiness only after the companion has bound the ordinary worker's control pipe.
pub async fn receive_listener(
    installed_image: &Path,
    deadline: Instant,
) -> io::Result<ipc::Listener> {
    receive_companion(installed_image, deadline)
        .await
        .map(|(listener, _)| listener)
}

pub(in crate::win::remote_ipc) async fn receive_companion(
    installed_image: &Path,
    deadline: Instant,
) -> io::Result<(ipc::Listener, Launcher)> {
    no_impersonation()?;
    let broker = Peer::broker(current_process()?, installed_image)?;
    reached(Stage::Identity);
    receive_record(Recipient::Broker, deadline, async |handle, endpoint| {
        let worker = Peer::ordinary(import_process(handle)?, installed_image)?;
        ipc::Listener::bind_at(&broker, worker, endpoint)
    })
    .await
}

pub(in crate::win::remote_ipc) struct Launcher {
    pipe: NamedPipeClient,
    pid: u32,
}

impl Launcher {
    pub(in crate::win::remote_ipc) async fn connected(
        mut self,
        deadline: Instant,
    ) -> io::Result<Launched> {
        verify_sender(&self.pipe, self.pid)?;
        ipc::bounded(deadline, async {
            self.pipe.write_all(CONNECTED).await?;
            let mut accepted = [0; 2];
            self.pipe.read_exact(&mut accepted).await?;
            if &accepted != b"OK" {
                return Err(denied());
            }
            Ok(())
        })
        .await?;
        Ok(Launched {
            pipe: self.pipe,
            pid: self.pid,
        })
    }
}

async fn receive_record<T>(
    recipient: Recipient,
    deadline: Instant,
    prepare: impl AsyncFnOnce(u64, Uuid) -> io::Result<T>,
) -> io::Result<(T, Launcher)> {
    let (magic, acknowledgement) = recipient.markers();
    let locator = std::env::var(recipient.env()).map_err(|_| denied())?;
    let (pid, id) = locator.split_once(':').ok_or_else(denied)?;
    let launcher = pid.parse::<u32>().map_err(|_| denied())?;
    let id = Uuid::parse_str(id).map_err(|_| denied())?;
    reached(Stage::Locator);
    ipc::bounded(deadline, async {
        let file = OpenOptions::new()
            .access_mode(ipc::CLIENT_ACCESS)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(name(id))?;
        verify_sender(&file, launcher)?;
        let mut pipe = win::into_pipe_client(file)?;
        reached(Stage::Bootstrap);
        let mut message = [0; 28];
        pipe.read_exact(&mut message).await?;
        if &message[..4] != magic {
            return Err(denied());
        }
        reached(Stage::Record);
        let handle = u64::from_le_bytes(message[4..12].try_into().map_err(|_| denied())?);
        let endpoint = Uuid::from_slice(&message[12..]).map_err(|_| denied())?;
        let result = prepare(handle, endpoint).await?;
        reached(Stage::Prepared);
        verify_sender(&pipe, launcher)?;
        pipe.write_all(acknowledgement).await?;
        let mut confirmed = [0; 2];
        pipe.read_exact(&mut confirmed).await?;
        if &confirmed != b"OK" {
            return Err(denied());
        }
        reached(Stage::Acknowledged);
        Ok((
            result,
            Launcher {
                pipe,
                pid: launcher,
            },
        ))
    })
    .await
}

#[cfg(test)]
impl Destination {
    pub(in crate::win::remote_ipc) fn verify_fixture(
        &self,
        broker: &Peer,
        endpoint: Uuid,
    ) -> io::Result<()> {
        if self.endpoint != endpoint {
            return Err(denied());
        }
        self.broker.same_process(broker)
    }
}

fn verify_sender(pipe: &impl AsRawHandle, launcher: u32) -> io::Result<()> {
    if launcher == 0 || ipc::peer_pid(pipe, false)? != launcher {
        return Err(denied());
    }
    let mut owner = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    // SAFETY: queries the retained pipe. The owner SID belongs to the returned descriptor.
    let error = unsafe {
        GetSecurityInfo(
            pipe.as_raw_handle(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    // SAFETY: the descriptor remains live while checking its owner and is freed exactly once.
    let system = unsafe {
        let system = !owner.is_null() && IsWellKnownSid(owner, WinLocalSystemSid) != 0;
        LocalFree(descriptor);
        system
    };
    if !system {
        return Err(denied());
    }
    let mut session = u32::MAX;
    // SAFETY: uses the same open pipe instance whose owner and server PID were verified.
    if unsafe {
        windows_sys::Win32::System::Pipes::GetNamedPipeServerSessionId(
            pipe.as_raw_handle(),
            &mut session,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if session == 0 { Ok(()) } else { Err(denied()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc::identity::tests::local;
    use win::sessions::tests::Fixture;

    #[test]
    fn privileged_delivery_requires_a_privileged_worker() {
        assert!(Sender::for_privileged(Uuid::new_v4(), local()).is_err());
    }

    fn ordinary_listener() -> (Sender, String) {
        listener(Recipient::Worker)
    }

    fn listener(recipient: Recipient) -> (Sender, String) {
        let worker = local();
        let name = name(Uuid::new_v4());
        let pipe = win::create_pipe(&ServerOptions::new(), &name, &worker.acl().unwrap()).unwrap();
        (
            Sender {
                pipe,
                peer: worker,
                recipient,
            },
            name,
        )
    }

    fn client(name: &str) -> NamedPipeClient {
        let file = OpenOptions::new()
            .access_mode(ipc::CLIENT_ACCESS)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(name)
            .unwrap();
        win::into_pipe_client(file).unwrap()
    }

    async fn acknowledge(client: &mut NamedPipeClient, recipient: Recipient, deadline: Instant) {
        let mut message = [0; 28];
        ipc::bounded(deadline, client.read_exact(&mut message))
            .await
            .unwrap();
        assert_eq!(&message[..4], recipient.markers().0);
        client.write_all(recipient.markers().1).await.unwrap();
        let mut confirmed = [0; 2];
        ipc::bounded(deadline, client.read_exact(&mut confirmed))
            .await
            .unwrap();
        assert_eq!(&confirmed, b"OK");
    }

    #[tokio::test]
    async fn an_ordinary_worker_never_becomes_a_control_channel() {
        let (sender, name) = ordinary_listener();
        let mut client = client(&name);
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let peer = local();
        let deadline = Instant::now() + ipc::IO_TIMEOUT;
        let (sent, ()) = tokio::join!(
            sender.send(4, Uuid::new_v4(), &binding, &peer, deadline),
            acknowledge(&mut client, Recipient::Worker, deadline)
        );
        client.write_all(CONNECTED).await.unwrap();
        assert!(
            matches!(sent.unwrap().connected(deadline).await, Err(e) if e.kind() == io::ErrorKind::PermissionDenied)
        );
    }

    #[tokio::test]
    async fn the_consent_window_may_answer_after_the_io_limit() {
        let (sender, name) = listener(Recipient::Consent);
        let mut client = client(&name);
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let peer = local();
        let deadline = Instant::now() + ipc::IO_TIMEOUT;
        let (sent, ()) = tokio::join!(
            sender.send(0, Uuid::nil(), &binding, &peer, deadline),
            async {
                acknowledge(&mut client, Recipient::Consent, deadline).await;
                client.write_all(CONNECTED).await.unwrap();
            }
        );
        let mut control = sent.unwrap().connected(deadline).await.unwrap();
        let mut accepted = [0; 2];
        ipc::bounded(deadline, client.read_exact(&mut accepted))
            .await
            .unwrap();
        assert_eq!(&accepted, b"OK");
        let answer_by = Instant::now() + ipc::IO_TIMEOUT * 2;
        let (reply, ()) = tokio::join!(control.ask(b"question", answer_by), async {
            assert_eq!(read_message(&mut client).await.unwrap(), b"question");
            tokio::time::sleep(ipc::IO_TIMEOUT + std::time::Duration::from_millis(500)).await;
            write_message(&mut client, b"answer").await.unwrap();
        });
        assert_eq!(reply.unwrap(), b"answer");
        let late = Instant::now() + std::time::Duration::from_millis(300);
        assert!(
            matches!(control.ask(b"again", late).await, Err(e) if e.kind() == io::ErrorKind::TimedOut)
        );
    }

    #[tokio::test]
    async fn matching_pid_on_an_ordinary_owned_pipe_is_insufficient() {
        let (_server, name) = ordinary_listener();
        let client = win::open_pipe(&name).unwrap();
        assert_eq!(ipc::peer_pid(&client, false).unwrap(), std::process::id());
        assert_eq!(
            verify_sender(&client, std::process::id())
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[tokio::test]
    async fn worker_cannot_acknowledge_as_the_companion() {
        let (sender, name) = ordinary_listener();
        let file = OpenOptions::new()
            .access_mode(ipc::CLIENT_ACCESS)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(name)
            .unwrap();
        let mut client = win::into_pipe_client(file).unwrap();
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let peer = local();
        let deadline = Instant::now() + ipc::IO_TIMEOUT;
        assert!(Sender::for_broker(Uuid::new_v4(), local()).is_err());
        let (sent, ()) = tokio::join!(
            sender.send(4, Uuid::new_v4(), &binding, &peer, deadline),
            async {
                let mut message = [0; 28];
                ipc::bounded(deadline, client.read_exact(&mut message))
                    .await
                    .unwrap();
                assert_eq!(&message[..4], Recipient::Worker.markers().0);
                client
                    .write_all(Recipient::Broker.markers().1)
                    .await
                    .unwrap();
            }
        );
        assert!(matches!(sent, Err(e) if e.kind() == io::ErrorKind::PermissionDenied));
    }

    #[tokio::test]
    async fn session_loss_while_waiting_for_ack_prevents_confirmation() {
        let (sender, name) = ordinary_listener();
        let file = OpenOptions::new()
            .access_mode(ipc::CLIENT_ACCESS)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(name)
            .unwrap();
        let mut client = win::into_pipe_client(file).unwrap();
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let peer = local();
        let deadline = Instant::now() + ipc::IO_TIMEOUT;
        let (result, ()) = tokio::join!(
            sender.send(4, Uuid::new_v4(), &binding, &peer, deadline),
            async {
                let mut message = [0; 28];
                ipc::bounded(deadline, client.read_exact(&mut message))
                    .await
                    .unwrap();
                assert_eq!(&message[..4], MAGIC);
                fixture.unavailable();
                let mut confirmation = [0; 2];
                let read = ipc::bounded(deadline, client.read(&mut confirmation)).await;
                assert!(
                    matches!(read, Ok(0))
                        || read.is_err_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
                );
            }
        );
        assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::PermissionDenied));
    }
}
