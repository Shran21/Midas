//! The video path of a consented session. The user worker captures, encodes and injects input;
//! the SYSTEM companion runs the WebRTC transport; the service hands the browser's offer to the
//! companion. Each side sends a keepalive every second, as every read is time-bounded.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    os::windows::io::AsRawHandle,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc as blocking,
    },
    time::{Duration, Instant},
};

use midas_agent_core::media::{
    adapt::{Limits, Profile, Settings},
    codec::{Encoder, Kind, Profile as H264Profile},
    link::{Command, KEEPALIVE_MESSAGE, command_message, read_command},
    peer::{Answered, answer},
    session::{Input, Transport},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};

use super::{Connection, IO_TIMEOUT, Writer, bootstrap::delivery::Launched, denied};
use crate::{
    capture::{Rect, Selection},
    input::Held,
    win::{inject, stream::Stream},
};

const KEEPALIVE: Duration = Duration::from_secs(1);
const OFFER: u8 = 1;
const ANSWER: u8 = 2;
const REFUSED: u8 = 3;

/// The service's request to the companion: candidate addresses and the browser's offer.
pub fn offer_message(hosts: &[IpAddr], sdp: &str) -> Vec<u8> {
    let mut message = vec![OFFER, hosts.len().min(16) as u8];
    for host in hosts.iter().take(16) {
        match host {
            IpAddr::V4(v4) => {
                message.push(4);
                message.extend_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                message.push(6);
                message.extend_from_slice(&v6.octets());
            }
        }
    }
    message.extend_from_slice(sdp.as_bytes());
    message
}

fn read_offer(message: &[u8]) -> Option<(Vec<IpAddr>, String)> {
    let (&[kind, count], mut rest) = message.split_first_chunk::<2>()?;
    if kind != OFFER || count > 16 {
        return None;
    }
    let mut hosts = Vec::new();
    for _ in 0..count {
        let (&[family], tail) = rest.split_first_chunk::<1>()?;
        let (host, tail) = match family {
            4 => {
                let (octets, tail) = tail.split_first_chunk::<4>()?;
                (IpAddr::V4(Ipv4Addr::from(*octets)), tail)
            }
            6 => {
                let (octets, tail) = tail.split_first_chunk::<16>()?;
                (IpAddr::V6(Ipv6Addr::from(*octets)), tail)
            }
            _ => return None,
        };
        hosts.push(host);
        rest = tail;
    }
    let sdp = String::from_utf8(rest.to_vec()).ok()?;
    (!sdp.is_empty() && sdp.len() <= midas_protocol::remote::MAX_SDP).then_some((hosts, sdp))
}

/// The companion's answer, or `None` when it refused the offer.
pub fn read_answer(message: &[u8]) -> Option<String> {
    match message {
        [ANSWER, sdp @ ..] => String::from_utf8(sdp.to_vec())
            .ok()
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

async fn send_all<S: AsyncRead + AsyncWrite + Unpin>(
    writer: &mut Writer<S>,
    message: &[u8],
) -> io::Result<()> {
    writer
        .send(message, tokio::time::Instant::now() + IO_TIMEOUT)
        .await
}

/// Reads every message of a split connection into a channel, so no read is ever cancelled half
/// way; the channel closes when the other side goes away or misbehaves.
fn reader<S: AsyncRead + AsyncWrite + AsRawHandle + Unpin + Send + 'static>(
    mut reader: super::Reader<S>,
) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Ok(frame) = reader
            .receive(tokio::time::Instant::now() + IO_TIMEOUT)
            .await
        {
            if sender.send(frame.to_vec()).await.is_err() {
                break;
            }
        }
    });
    receiver
}

/// How long capture waits before trying again while the desktop cannot be captured.
const RETRY: Duration = Duration::from_millis(500);

/// The capture thread: it waits for the first settings, then streams until the stop. Capture can
/// be unavailable for a while (a secure desktop, a display change); the session then stays and
/// capture is tried again.
fn capture(
    commands: blocking::Receiver<Command>,
    frames: mpsc::Sender<Vec<Vec<u8>>>,
    view: Arc<Mutex<Option<Rect>>>,
) -> io::Result<()> {
    let mut settings = loop {
        match commands.recv() {
            Ok(Command::Settings(settings)) => break settings,
            Ok(Command::Stop) | Err(_) => return Ok(()),
            Ok(_) => {}
        }
    };
    loop {
        match Stream::open(&Selection::Combined, settings) {
            Ok(mut stream) => {
                *view.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream.view());
                match run(&mut stream, &mut settings, &commands, &frames, &view) {
                    Ok(()) => return Ok(()),
                    Err(error) => tracing::debug!(%error, "capture interrupted"),
                }
            }
            Err(error) => tracing::debug!(%error, "capture unavailable"),
        }
        match commands.recv_timeout(RETRY) {
            Ok(Command::Settings(changed)) => settings = changed,
            Ok(Command::Stop) | Err(blocking::RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(_) | Err(blocking::RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Streams until the stop (`Ok`) or a capture failure (`Err`).
fn run(
    stream: &mut Stream,
    settings: &mut Settings,
    commands: &blocking::Receiver<Command>,
    frames: &mpsc::Sender<Vec<Vec<u8>>>,
    view: &Arc<Mutex<Option<Rect>>>,
) -> io::Result<()> {
    loop {
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    if let Command::Settings(changed) = command {
                        *settings = changed;
                    }
                    if !stream.command(command)? {
                        return Ok(());
                    }
                }
                Err(blocking::TryRecvError::Empty) => break,
                Err(blocking::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if let Some(messages) = stream.next_frame()? {
            *view.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream.view());
            match frames.try_send(messages) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
            }
        }
    }
}

fn inject(held: &mut Held, input: Input, view: &Arc<Mutex<Option<Rect>>>) {
    let Some(view) = *view.lock().unwrap_or_else(|e| e.into_inner()) else {
        return;
    };
    let events = match input {
        Input::Pointer { x, y, buttons } => {
            held.pointer(x, y, buttons, (view.x, view.y, view.width, view.height))
        }
        Input::Wheel { dx, dy } => vec![held.wheel(dx, dy)],
        Input::Key { code, down } => held.key(&code, down).into_iter().collect(),
    };
    if let Err(error) = inject::send(&events) {
        tracing::debug!(%error, "input not injected");
    }
}

/// The user worker's session: capture on its own thread, input from the transport, keepalives.
pub(in crate::win::remote_ipc) async fn worker<S>(connection: Connection<S>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + AsRawHandle + Unpin + Send + 'static,
{
    let (incoming, mut writer) = connection.split()?;
    let mut incoming = reader(incoming);
    let (frames_sender, mut frames) = mpsc::channel::<Vec<Vec<u8>>>(2);
    let (commands, capture_commands) = blocking::channel();
    let view = Arc::new(Mutex::new(None));
    let shared = view.clone();
    let capturing = std::thread::spawn(move || capture(capture_commands, frames_sender, shared));
    let mut held = Held::default();
    let mut tick = tokio::time::interval(KEEPALIVE);
    let result = loop {
        tokio::select! {
            message = incoming.recv() => {
                let Some(message) = message else { break Ok(()) };
                if message == KEEPALIVE_MESSAGE {
                    continue;
                }
                match read_command(&message) {
                    Some(Command::Stop) => break Ok(()),
                    Some(Command::Input(input)) => inject(&mut held, input, &view),
                    Some(command) => {
                        if commands.send(command).is_err() {
                            break Ok(());
                        }
                    }
                    None => break Err(denied()),
                }
            }
            encoded = frames.recv() => {
                let Some(messages) = encoded else { break Ok(()) };
                let mut sent = Ok(());
                for message in messages {
                    sent = send_all(&mut writer, &message).await;
                    if sent.is_err() {
                        break;
                    }
                }
                if let Err(error) = sent {
                    break Err(error);
                }
            }
            _ = tick.tick() => {
                if let Err(error) = send_all(&mut writer, &KEEPALIVE_MESSAGE).await {
                    break Err(error);
                }
            }
        }
    };
    let _ = inject::send(&held.release());
    let _ = commands.send(Command::Stop);
    drop(commands);
    drop(frames);
    match capturing.join() {
        Ok(Err(error)) => tracing::warn!(%error, "capture stopped"),
        Err(_) => tracing::warn!("capture thread failed"),
        Ok(Ok(())) => {}
    }
    result
}

fn software() -> Vec<Encoder> {
    vec![Encoder {
        name: "Media Foundation H.264".into(),
        kind: Kind::Software,
        profiles: vec![H264Profile::ConstrainedBaseline, H264Profile::Baseline],
        limits: Limits::SOFTWARE,
    }]
}

/// The WebRTC transport on its own thread: worker frames in, worker commands out.
fn transport(
    mut transport: Transport,
    frames: blocking::Receiver<Vec<u8>>,
    commands: mpsc::Sender<Command>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        if transport
            .drive(Instant::now() + Duration::from_millis(5))
            .is_err()
        {
            break;
        }
        loop {
            match frames.try_recv() {
                Ok(message) => {
                    if transport.worker_message(&message).is_err() {
                        break;
                    }
                }
                Err(blocking::TryRecvError::Empty) => break,
                Err(blocking::TryRecvError::Disconnected) => {
                    transport.stop();
                    break;
                }
            }
        }
        let mut pending: Vec<Command> = std::iter::from_fn(|| transport.next_command()).collect();
        pending.extend(std::iter::from_fn(|| transport.next_input()).map(Command::Input));
        for command in pending {
            if commands.blocking_send(command).is_err() {
                return;
            }
        }
        if transport.is_stopped() {
            break;
        }
    }
    transport.stop();
    while let Some(command) = transport.next_command() {
        let _ = commands.blocking_send(command);
    }
}

/// Answers the offer and starts the transport thread; `None` refuses it.
fn start(hosts: &[IpAddr], sdp: &str) -> Option<(String, Transport)> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).ok()?;
    let mut hosts: Vec<IpAddr> = hosts.iter().copied().filter(IpAddr::is_ipv4).collect();
    if hosts.is_empty() {
        hosts = crate::win::network::local_addresses()
            .ok()?
            .into_iter()
            .filter(IpAddr::is_ipv4)
            .collect();
    }
    let Answered { answer, peer } = answer(sdp, socket, &hosts, software(), Instant::now()).ok()?;
    Some((
        answer,
        Transport::new(peer, Profile::Text, (1920, 1080), Instant::now()),
    ))
}

/// The SYSTEM companion's session: it answers the service's offer and runs the transport
/// between the browser and the worker; the service closing the control channel ends it.
pub(in crate::win::remote_ipc) async fn companion<S>(
    connection: Connection<S>,
    mut control: Launched,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + AsRawHandle + Unpin + Send + 'static,
{
    let (incoming, mut writer) = connection.split()?;
    let mut incoming = reader(incoming);
    let (offers_sender, mut offers) = mpsc::channel::<Transport>(1);
    let control_task = tokio::spawn(async move {
        let mut started = false;
        while let Ok(Some(message)) = control.next().await {
            let reply = match read_offer(&message).filter(|_| !started) {
                Some((hosts, sdp)) => match start(&hosts, &sdp) {
                    Some((answer, transport)) => {
                        started = true;
                        let _ = offers_sender.send(transport).await;
                        let mut reply = vec![ANSWER];
                        reply.extend_from_slice(answer.as_bytes());
                        reply
                    }
                    None => vec![REFUSED],
                },
                None => vec![REFUSED],
            };
            if control
                .reply(&reply, tokio::time::Instant::now() + IO_TIMEOUT)
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let stop = Arc::new(AtomicBool::new(false));
    let (commands_sender, mut commands) = mpsc::channel::<Command>(64);
    let mut frames: Option<blocking::SyncSender<Vec<u8>>> = None;
    let mut running = None;
    let mut tick = tokio::time::interval(KEEPALIVE);
    let result = loop {
        tokio::select! {
            message = incoming.recv() => {
                let Some(message) = message else { break Ok(()) };
                if message == KEEPALIVE_MESSAGE {
                    continue;
                }
                if let Some(frames) = &frames
                    && frames.try_send(message).is_err()
                {
                    tracing::debug!("frame dropped before the transport");
                }
            }
            transport = offers.recv(), if running.is_none() => {
                let Some(transport) = transport else { continue };
                let (sender, receiver) = blocking::sync_channel(64);
                frames = Some(sender);
                let (stop, commands) = (stop.clone(), commands_sender.clone());
                running = Some(std::thread::spawn(move || {
                    self::transport(transport, receiver, commands, stop)
                }));
            }
            command = commands.recv() => {
                let Some(command) = command else { break Ok(()) };
                let stopping = command == Command::Stop;
                if let Err(error) = send_all(&mut writer, &command_message(&command)).await {
                    break Err(error);
                }
                if stopping {
                    break Ok(());
                }
            }
            _ = tick.tick() => {
                if control_task.is_finished() {
                    let _ = send_all(&mut writer, &command_message(&Command::Stop)).await;
                    break Ok(());
                }
                if let Err(error) = send_all(&mut writer, &KEEPALIVE_MESSAGE).await {
                    break Err(error);
                }
            }
        }
    };
    stop.store(true, Ordering::Release);
    drop(frames);
    control_task.abort();
    if let Some(running) = running {
        let _ = tokio::task::spawn_blocking(move || running.join()).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_carry_hosts_and_sdp_and_answers_are_checked() {
        let hosts = [
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            IpAddr::V6("2001:db8::1".parse().unwrap()),
        ];
        let message = offer_message(&hosts, "v=0");
        assert_eq!(read_offer(&message), Some((hosts.to_vec(), "v=0".into())));
        assert_eq!(read_offer(&offer_message(&hosts, "")), None);
        assert_eq!(read_offer(&message[..5]), None);
        assert_eq!(read_offer(&[OFFER, 17]), None);
        assert_eq!(read_offer(&[ANSWER, 0]), None);
        assert_eq!(read_answer(&[ANSWER, b'v']), Some("v".into()));
        assert_eq!(read_answer(&[ANSWER]), None);
        assert_eq!(read_answer(&[REFUSED]), None);
    }
}
