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
    identity::Token,
    launch::environment,
    tokens::{current_token, no_impersonation},
};
use super::{Peer, denied, import_broker};
use crate::win::{self, sessions::Binding};

const ENV: &str = "MIDAS_WORKER_BOOTSTRAP";
const MAGIC: &[u8; 4] = b"MBT1";
const ACK: &[u8; 4] = b"MBA1";

pub(in crate::win::remote_ipc) fn worker_environment(id: Uuid) -> io::Result<Vec<u16>> {
    let mut block: Vec<_> = format!("{ENV}={}:{}\0", std::process::id(), id)
        .encode_utf16()
        .collect();
    block.extend(environment()?);
    Ok(block)
}

fn name(id: Uuid) -> String {
    format!(r"\\.\pipe\midas-bootstrap-{id}")
}

pub(in crate::win::remote_ipc) struct Sender {
    pipe: NamedPipeServer,
    worker: Peer,
}

impl Sender {
    pub(in crate::win::remote_ipc) fn bind(id: Uuid, worker: Peer) -> io::Result<Self> {
        no_impersonation()?;
        worker.user_logon_sid()?;
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1);
        let pipe = win::create_pipe(&options, &name(id), &format!("O:SY{}", worker.acl()?))?;
        Ok(Self { pipe, worker })
    }

    pub(in crate::win::remote_ipc) async fn send(
        mut self,
        handle: u64,
        endpoint: Uuid,
        binding: &Binding,
        broker: &Peer,
        deadline: Instant,
    ) -> io::Result<()> {
        let check = |pipe: &NamedPipeServer| {
            binding.check()?;
            broker.recheck()?;
            self.worker.check(ipc::peer_pid(pipe, true)?)
        };
        let exchange = async {
            self.pipe.connect().await?;
            check(&self.pipe)?;
            let mut message = [0; 28];
            message[..4].copy_from_slice(MAGIC);
            message[4..12].copy_from_slice(&handle.to_le_bytes());
            message[12..].copy_from_slice(endpoint.as_bytes());
            self.pipe.write_all(&message).await?;
            let mut ack = [0; 4];
            self.pipe.read_exact(&mut ack).await?;
            if &ack != ACK {
                return Err(denied());
            }
            check(&self.pipe)?;
            self.pipe.write_all(b"OK").await
        };
        tokio::select! {
            biased;
            _ = binding.ended() => Err(denied()),
            result = ipc::bounded(deadline, exchange) => result,
        }
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
    no_impersonation()?;
    Token::read(&current_token(TOKEN_QUERY)?)?.user_worker(win::current_session()?)?;
    let locator = std::env::var(ENV).map_err(|_| denied())?;
    let (pid, id) = locator.split_once(':').ok_or_else(denied)?;
    let launcher = pid.parse::<u32>().map_err(|_| denied())?;
    let id = Uuid::parse_str(id).map_err(|_| denied())?;
    ipc::bounded(deadline, async {
        let file = OpenOptions::new()
            .access_mode(ipc::CLIENT_ACCESS)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(name(id))?;
        verify_sender(&file, launcher)?;
        let mut pipe = win::into_pipe_client(file)?;
        let mut message = [0; 28];
        pipe.read_exact(&mut message).await?;
        if &message[..4] != MAGIC {
            return Err(denied());
        }
        let handle = u64::from_le_bytes(message[4..12].try_into().map_err(|_| denied())?);
        let endpoint = Uuid::from_slice(&message[12..]).map_err(|_| denied())?;
        let broker = import_broker(handle, installed_image)?;
        verify_sender(&pipe, launcher)?;
        pipe.write_all(ACK).await?;
        let mut confirmed = [0; 2];
        pipe.read_exact(&mut confirmed).await?;
        if &confirmed != b"OK" {
            return Err(denied());
        }
        Ok(Destination { broker, endpoint })
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

    fn ordinary_listener() -> (Sender, String) {
        let worker = local();
        let name = name(Uuid::new_v4());
        let pipe = win::create_pipe(&ServerOptions::new(), &name, &worker.acl().unwrap()).unwrap();
        (Sender { pipe, worker }, name)
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
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
}
