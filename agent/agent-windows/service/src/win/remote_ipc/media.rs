//! The video path of a consented session. The user worker captures, encodes and injects input;
//! the SYSTEM companion runs the WebRTC transport; the service hands the browser's offer to the
//! companion. With UAC rights a privileged worker in the same session joins the companion: it
//! streams the secure desktop and delivers input the user worker cannot. Each side sends a
//! keepalive every second, as every read is time-bounded.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    os::windows::io::AsRawHandle,
    path::Path,
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
    files::Files,
    link::{
        Command, Desktop, KEEPALIVE_MESSAGE, command_message, desktop_message, read_command,
        read_desktop, read_transfer_reply, sas_result_message, transfer_reply_message,
    },
    peer::{Answered, answer},
    session::{Input, Transport},
    transfer::{REPORTS_LIMIT, Report, read_reports, read_request, reply_message, reports_message},
    turn::{Server, allocate},
};
use midas_remote_auth::Transfer;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::windows::named_pipe::NamedPipeServer,
    sync::mpsc,
};

use super::{
    Connection, IO_TIMEOUT, Peer, STARTUP_TIMEOUT, Writer,
    bootstrap::{delivery::Launched, import_privileged},
    denied,
    desktop::{InputDesktop, secure_attention},
    identity::Role,
    participant::{MEDIA_HELLO, WORKER_HELLO, expect},
    startup::current_process,
};
use crate::{
    capture::{Rect, Selection},
    input::Held,
    win::{inject, stream::Stream},
};

const KEEPALIVE: Duration = Duration::from_secs(1);
const OFFER: u8 = 1;
const ANSWER: u8 = 2;
const REFUSED: u8 = 3;
/// The service collects the transfers the companion's transport reported for the audit.
pub const REPORTS: u8 = 6;
const REPORTED: u8 = 7;
const LISTEN: u8 = 4;
const LISTENING: u8 = 5;
/// How often the privileged worker looks at the input desktop.
const POLL: Duration = Duration::from_millis(100);

/// The service's request to the companion: candidate addresses, the relay the agent may
/// allocate on with its short-lived credential, the transfer the grant allows and the browser's
/// offer.
pub fn offer_message(
    hosts: &[IpAddr],
    relay: Option<&Server>,
    sdp: &str,
    transfer: Transfer,
) -> Vec<u8> {
    let mut message = vec![OFFER, hosts.len().min(16) as u8];
    let address = |message: &mut Vec<u8>, host: &IpAddr| match host {
        IpAddr::V4(v4) => {
            message.push(4);
            message.extend_from_slice(&v4.octets());
        }
        IpAddr::V6(v6) => {
            message.push(6);
            message.extend_from_slice(&v6.octets());
        }
    };
    for host in hosts.iter().take(16) {
        address(&mut message, host);
    }
    match relay.filter(|relay| relay.username.len() <= 255 && relay.password.len() <= 255) {
        Some(relay) => {
            message.push(1);
            address(&mut message, &relay.address.ip());
            message.extend_from_slice(&relay.address.port().to_be_bytes());
            for text in [&relay.username, &relay.password] {
                message.push(text.len() as u8);
                message.extend_from_slice(text.as_bytes());
            }
        }
        None => message.push(0),
    }
    message.push(
        u8::from(transfer.clipboard)
            | (u8::from(transfer.upload) << 1)
            | (u8::from(transfer.download) << 2),
    );
    message.extend_from_slice(sdp.as_bytes());
    message
}

fn read_address(rest: &mut &[u8]) -> Option<IpAddr> {
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
    *rest = tail;
    Some(host)
}

fn read_text(rest: &mut &[u8]) -> Option<String> {
    let (&[length], tail) = rest.split_first_chunk::<1>()?;
    let (text, tail) = tail.split_at_checked(usize::from(length))?;
    *rest = tail;
    String::from_utf8(text.to_vec())
        .ok()
        .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
}

/// What the service asks the companion to answer.
#[derive(Debug, PartialEq)]
struct Offer {
    hosts: Vec<IpAddr>,
    relay: Option<Server>,
    transfer: Transfer,
    sdp: String,
}

fn read_offer(message: &[u8]) -> Option<Offer> {
    let (&[kind, count], mut rest) = message.split_first_chunk::<2>()?;
    if kind != OFFER || count > 16 {
        return None;
    }
    let mut hosts = Vec::new();
    for _ in 0..count {
        hosts.push(read_address(&mut rest)?);
    }
    let (&[flag], tail) = rest.split_first_chunk::<1>()?;
    rest = tail;
    let relay = match flag {
        0 => None,
        1 => {
            let host = read_address(&mut rest)?;
            let (port, tail) = rest.split_first_chunk::<2>()?;
            rest = tail;
            let address = SocketAddr::new(host, u16::from_be_bytes(*port));
            let username = read_text(&mut rest)?;
            let password = read_text(&mut rest)?;
            Some(Server {
                address,
                username,
                password,
            })
        }
        _ => return None,
    };
    let (&[flags], rest) = rest.split_first_chunk::<1>()?;
    if flags >= 8 {
        return None;
    }
    let transfer = Transfer {
        clipboard: flags & 1 != 0,
        upload: flags & 2 != 0,
        download: flags & 4 != 0,
    };
    let sdp = String::from_utf8(rest.to_vec()).ok()?;
    (!sdp.is_empty() && sdp.len() <= midas_protocol::remote::MAX_SDP).then_some(Offer {
        hosts,
        relay,
        transfer,
        sdp,
    })
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

/// The service's request that the companion accept the session's privileged worker: a
/// query-only handle to that worker, valid in the companion, and the endpoint to listen on.
pub fn listen_message(handle: u64, endpoint: uuid::Uuid) -> Vec<u8> {
    let mut message = vec![LISTEN];
    message.extend_from_slice(&handle.to_le_bytes());
    message.extend_from_slice(endpoint.as_bytes());
    message
}

fn read_listen(message: &[u8]) -> Option<(u64, uuid::Uuid)> {
    let (&[kind], rest) = message.split_first_chunk::<1>()?;
    let (handle, endpoint) = rest.split_first_chunk::<8>()?;
    let handle = u64::from_le_bytes(*handle);
    let endpoint = uuid::Uuid::from_slice(endpoint).ok()?;
    (kind == LISTEN && handle != 0 && !endpoint.is_nil()).then_some((handle, endpoint))
}

pub fn listening(reply: &[u8]) -> bool {
    reply == [LISTENING]
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
        Input::Pointer {
            x,
            y,
            buttons,
            modifiers,
        } => held.pointer(
            x,
            y,
            buttons,
            modifiers,
            (view.x, view.y, view.width, view.height),
        ),
        Input::Wheel { dx, dy } => vec![held.wheel(dx, dy)],
        Input::Key { code, down } => held.key(&code, down).into_iter().collect(),
        Input::Text { text } => held.text(&text),
    };
    if let Err(error) = inject::send(&events) {
        tracing::debug!(%error, "input not injected");
    }
}

fn release(held: &mut Held) {
    let _ = inject::send(&held.release());
}

type Thread = Box<
    dyn FnOnce(blocking::Receiver<Command>, mpsc::Sender<Vec<Vec<u8>>>) -> io::Result<()> + Send,
>;

/// A worker's side of the control channel. Its thread captures and, without `inline`, also
/// executes input; with `inline`, input runs here against the view the thread keeps.
/// Clipboard and files on their own thread with the user's token, so hashing a large file never
/// stalls the picture. The transport has checked each message against the grant already.
fn transfers(replies: mpsc::Sender<Vec<u8>>) -> blocking::Sender<Vec<u8>> {
    let (sender, requests) = blocking::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let Some((home, folder)) = crate::win::transfer::folders() else {
            tracing::warn!("the user's folders are not known; no transfer");
            return;
        };
        let platform = match crate::win::transfer::Windows::new() {
            Ok(platform) => platform,
            Err(error) => {
                tracing::warn!(%error, "clipboard window not created; no transfer");
                return;
            }
        };
        let all = Transfer {
            clipboard: true,
            upload: true,
            download: true,
        };
        let mut files = Files::new(platform, home, folder);
        while let Ok(request) = requests.recv() {
            let Some(request) = read_request(&request, all) else {
                continue;
            };
            for reply in files.handle(request) {
                if replies.blocking_send(reply_message(&reply)).is_err() {
                    return;
                }
            }
        }
    });
    sender
}

async fn serve<S>(
    connection: Connection<S>,
    inline: Option<Arc<Mutex<Option<Rect>>>>,
    thread: Thread,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + AsRawHandle + Unpin + Send + 'static,
{
    let (incoming, mut writer) = connection.split()?;
    let mut incoming = reader(incoming);
    let (out_sender, mut out) = mpsc::channel::<Vec<Vec<u8>>>(2);
    let (commands, thread_commands) = blocking::channel();
    let running = std::thread::spawn(move || thread(thread_commands, out_sender));
    let mut held = Held::default();
    let mut tick = tokio::time::interval(KEEPALIVE);
    let (replies_sender, mut replies) = mpsc::channel::<Vec<u8>>(64);
    let mut files: Option<blocking::Sender<Vec<u8>>> = None;
    let result = loop {
        tokio::select! {
            message = incoming.recv() => {
                let Some(message) = message else { break Ok(()) };
                if message == KEEPALIVE_MESSAGE {
                    continue;
                }
                match (read_command(&message), &inline) {
                    (Some(Command::Stop), _) => break Ok(()),
                    (Some(Command::Input(input)), Some(view)) => inject(&mut held, input, view),
                    (Some(Command::Release), Some(_)) => release(&mut held),
                    (Some(Command::Transfer(request)), Some(_)) => {
                        let sender = files.get_or_insert_with(|| transfers(replies_sender.clone()));
                        let _ = sender.send(request);
                    }
                    // Only the user worker handles the user's files and clipboard.
                    (Some(Command::Transfer(_)), None) => {}
                    (Some(command), _) => {
                        if commands.send(command).is_err() {
                            break Ok(());
                        }
                    }
                    (None, _) => break Err(denied()),
                }
            }
            Some(reply) = replies.recv() => {
                if let Err(error) = send_all(&mut writer, &transfer_reply_message(&reply)).await {
                    break Err(error);
                }
            }
            sent = out.recv() => {
                let Some(messages) = sent else { break Ok(()) };
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
    release(&mut held);
    let _ = commands.send(Command::Stop);
    drop(commands);
    drop(out);
    match tokio::task::spawn_blocking(move || running.join()).await {
        Ok(Ok(Err(error))) => tracing::warn!(%error, "worker thread stopped"),
        Ok(Err(_)) | Err(_) => tracing::warn!("worker thread failed"),
        Ok(Ok(Ok(()))) => {}
    }
    result
}

/// The user worker's session: capture on its own thread, input from the transport, keepalives.
pub(in crate::win::remote_ipc) async fn worker<S>(connection: Connection<S>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + AsRawHandle + Unpin + Send + 'static,
{
    let view = Arc::new(Mutex::new(None));
    let shared = view.clone();
    serve(
        connection,
        Some(view),
        Box::new(move |commands, frames| capture(commands, frames, shared)),
    )
    .await
}

/// The privileged worker of a session with UAC rights. It follows the input desktop, streams
/// only the secure desktop and executes the input routed to it.
pub(in crate::win::remote_ipc) async fn privileged<S>(connection: Connection<S>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + AsRawHandle + Unpin + Send + 'static,
{
    serve(connection, None, Box::new(follow)).await
}

/// The privileged worker's desktop thread. It reports every change of desktop state before any
/// frame of the new state, and input runs on this thread, attached to the input desktop.
fn follow(
    commands: blocking::Receiver<Command>,
    out: mpsc::Sender<Vec<Vec<u8>>>,
) -> io::Result<()> {
    let view = Arc::new(Mutex::new(None));
    let set_view = |rect: Option<Rect>| *view.lock().unwrap_or_else(|e| e.into_inner()) = rect;
    let combined = || {
        crate::win::capture::monitors()
            .ok()
            .and_then(|monitors| crate::capture::view(&monitors, &Selection::Combined).ok())
    };
    let mut held = Held::default();
    let mut settings = None;
    let mut attached: Option<InputDesktop> = None;
    let mut reported = None;
    let mut stream: Option<Stream> = None;
    let mut looked: Option<Instant> = None;
    let mut retry = Instant::now();
    loop {
        let wait = match (&stream, looked) {
            (Some(_), _) | (None, None) => Duration::ZERO,
            (None, Some(at)) => POLL.saturating_sub(at.elapsed()),
        };
        let first = match commands.recv_timeout(wait) {
            Ok(command) => Some(command),
            Err(blocking::RecvTimeoutError::Timeout) => None,
            Err(blocking::RecvTimeoutError::Disconnected) => break,
        };
        for command in first
            .into_iter()
            .chain(std::iter::from_fn(|| commands.try_recv().ok()))
        {
            match command {
                Command::Stop => {
                    release(&mut held);
                    return Ok(());
                }
                Command::Input(input) => inject(&mut held, input, &view),
                Command::Release => release(&mut held),
                // The transport side answers Ctrl+Alt+Del; a worker never acts on it.
                Command::Sas | Command::Transfer(_) => {}
                Command::Settings(changed) => {
                    settings = Some(changed);
                    if let Some(active) = &mut stream
                        && active.command(Command::Settings(changed)).is_err()
                    {
                        stream = None;
                    }
                }
                Command::Keyframe => {
                    if let Some(active) = &mut stream {
                        let _ = active.command(Command::Keyframe);
                    }
                }
            }
        }
        if looked.is_none_or(|at| at.elapsed() >= POLL) {
            looked = Some(Instant::now());
            match InputDesktop::open() {
                Ok(input) => {
                    if attached
                        .as_ref()
                        .is_none_or(|current| current.name() != input.name())
                    {
                        // Keys held on the old desktop are let go; its capture ends.
                        release(&mut held);
                        stream = None;
                        match input.attach() {
                            Ok(()) => attached = Some(input),
                            Err(error) => tracing::debug!(%error, "input desktop not attached"),
                        }
                    }
                }
                Err(error) => tracing::debug!(%error, "input desktop not readable"),
            }
            if let Some(current) = &attached {
                let state = current.state();
                if reported != Some(state) {
                    if out
                        .blocking_send(vec![desktop_message(state).to_vec()])
                        .is_err()
                    {
                        break;
                    }
                    reported = Some(state);
                    if state != Desktop::Secure {
                        stream = None;
                        set_view(combined());
                    }
                }
            }
        }
        if reported != Some(Desktop::Secure) {
            continue;
        }
        if stream.is_none()
            && let Some(settings) = settings
            && Instant::now() >= retry
        {
            match Stream::open(&Selection::Combined, settings) {
                Ok(opened) => {
                    set_view(Some(opened.view()));
                    stream = Some(opened);
                }
                Err(error) => {
                    tracing::debug!(%error, "secure desktop not captured");
                    retry = Instant::now() + RETRY;
                }
            }
        }
        if let Some(active) = &mut stream {
            match active.next_frame() {
                Ok(Some(messages)) => {
                    set_view(Some(active.view()));
                    if let Err(mpsc::error::TrySendError::Closed(_)) = out.try_send(messages) {
                        break;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%error, "secure desktop capture interrupted");
                    stream = None;
                    retry = Instant::now() + RETRY;
                }
            }
        }
    }
    release(&mut held);
    Ok(())
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
/// Transfers waiting for the service, which collects them every few seconds.
type Reports = Arc<Mutex<std::collections::VecDeque<Report>>>;

fn keep_reports(transport: &mut Transport, reports: &Reports) {
    let mut queue = reports.lock().unwrap_or_else(|e| e.into_inner());
    while let Some(report) = transport.next_report() {
        if queue.len() < REPORTS_LIMIT {
            queue.push_back(report);
        }
    }
}

/// The companion's answer to a collection: the reports since the last one.
fn reports_reply(reports: &Reports) -> Vec<u8> {
    let taken: Vec<Report> = reports
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
        .collect();
    [vec![REPORTED], reports_message(&taken)].concat()
}

/// The reports in the companion's answer, or `None` when it is not one.
pub fn read_reports_reply(message: &[u8]) -> Option<Vec<Report>> {
    match message {
        [REPORTED, rest @ ..] => read_reports(rest),
        _ => None,
    }
}

fn transport(
    mut transport: Transport,
    frames: blocking::Receiver<Vec<u8>>,
    commands: mpsc::Sender<Command>,
    stop: Arc<AtomicBool>,
    reports: Reports,
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
        keep_reports(&mut transport, &reports);
        for command in pending {
            if commands.blocking_send(command).is_err() {
                transport.stop();
                keep_reports(&mut transport, &reports);
                return;
            }
        }
        if transport.is_stopped() {
            break;
        }
    }
    transport.stop();
    keep_reports(&mut transport, &reports);
    while let Some(command) = transport.next_command() {
        let _ = commands.blocking_send(command);
    }
}

/// How long the companion waits for the relay before answering with direct candidates only.
const RELAY_WAIT: Duration = Duration::from_secs(3);

/// Answers the offer and starts the transport thread; `None` refuses it. With a relay, the
/// answer also carries the relayed and server-reflexive addresses.
fn start(offer: &Offer) -> Option<(String, Transport)> {
    let Offer {
        hosts,
        relay,
        transfer,
        sdp,
    } = offer;
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).ok()?;
    let allocation = relay.as_ref().and_then(|relay| {
        allocate(&socket, relay, Instant::now() + RELAY_WAIT)
            .inspect_err(|error| tracing::warn!(?error, "relay not allocated"))
            .ok()
    });
    let mut hosts: Vec<IpAddr> = hosts.iter().copied().filter(IpAddr::is_ipv4).collect();
    if hosts.is_empty() {
        hosts = crate::win::network::local_addresses()
            .ok()?
            .into_iter()
            .filter(IpAddr::is_ipv4)
            .collect();
    }
    let Answered { answer, peer } =
        answer(sdp, socket, &hosts, allocation, software(), Instant::now()).ok()?;
    let mut transport = Transport::new(peer, Profile::Text, (1920, 1080), Instant::now());
    transport.allow_transfer(*transfer);
    Some((answer, transport))
}

/// Which worker serves the browser for a desktop state: frames come from the privileged worker
/// only on the secure desktop; input goes to it also for elevated windows the user worker cannot
/// reach. Without a privileged worker everything stays with the user worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Route {
    frames_privileged: bool,
    input_privileged: bool,
}

fn route(desktop: Desktop, privileged: bool) -> Route {
    Route {
        frames_privileged: privileged && desktop == Desktop::Secure,
        input_privileged: privileged && desktop != Desktop::User,
    }
}

/// Binds the endpoint for the privileged worker the service handed over and accepts it in the
/// background; only that worker, SYSTEM in this companion's session, may connect.
fn accept_privileged(
    handle: u64,
    endpoint: uuid::Uuid,
    image: &Path,
    session: u32,
    attached: mpsc::Sender<Connection<NamedPipeServer>>,
) -> io::Result<()> {
    let own = Peer::broker(current_process()?, image)?;
    let worker = import_privileged(handle, image, session)?;
    let listener = super::Listener::bind_at(&own, worker, endpoint)?;
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        let accepted = async {
            let mut connection = listener.accept(deadline).await?;
            expect(&mut connection, WORKER_HELLO, deadline).await?;
            connection.send(MEDIA_HELLO, deadline).await?;
            Ok::<_, io::Error>(connection)
        }
        .await;
        match accepted {
            Ok(connection) => {
                let _ = attached.send(connection).await;
            }
            Err(error) => tracing::warn!(%error, "privileged worker did not connect"),
        }
    });
    Ok(())
}

struct Privileged {
    incoming: mpsc::Receiver<Vec<u8>>,
    writer: Writer<NamedPipeServer>,
}

async fn next(privileged: &mut Option<Privileged>) -> Option<Vec<u8>> {
    match privileged {
        Some(privileged) => privileged.incoming.recv().await,
        None => std::future::pending().await,
    }
}

/// Sends to the privileged worker; a failure drops it, as its exit ends the session anyway.
async fn send_privileged(privileged: &mut Option<Privileged>, message: &[u8]) {
    if let Some(attached) = privileged
        && send_all(&mut attached.writer, message).await.is_err()
    {
        *privileged = None;
    }
}

/// The SYSTEM companion's session: it answers the service's offer and runs the transport
/// between the browser and the workers; the service closing the control channel ends it.
pub(in crate::win::remote_ipc) async fn companion<S>(
    connection: Connection<S>,
    mut control: Launched,
    image: &Path,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + AsRawHandle + Unpin + Send + 'static,
{
    let Role::Worker { session, .. } = connection.peer.role else {
        return Err(denied());
    };
    let image = image.to_path_buf();
    let (incoming, mut writer) = connection.split()?;
    let mut incoming = reader(incoming);
    let (offers_sender, mut offers) = mpsc::channel::<Transport>(1);
    let (attached_sender, mut attached) = mpsc::channel::<Connection<NamedPipeServer>>(1);
    let reports: Reports = Arc::default();
    let collected = reports.clone();
    let control_task = tokio::spawn(async move {
        let mut started = false;
        let mut listening = false;
        while let Ok(Some(message)) = control.next().await {
            let reply = if message == [REPORTS] {
                reports_reply(&collected)
            } else if let Some((handle, endpoint)) = read_listen(&message) {
                let accepted = !listening
                    && accept_privileged(
                        handle,
                        endpoint,
                        &image,
                        session,
                        attached_sender.clone(),
                    )
                    .inspect_err(|error| {
                        tracing::warn!(%error, "privileged worker refused");
                    })
                    .is_ok();
                listening |= accepted;
                vec![if accepted { LISTENING } else { REFUSED }]
            } else {
                let offer = read_offer(&message).filter(|_| !started);
                // Allocating on the relay waits on the network, so it runs off this thread.
                let answered = match offer {
                    Some(offer) => tokio::task::spawn_blocking(move || start(&offer))
                        .await
                        .ok()
                        .flatten(),
                    None => None,
                };
                match answered {
                    Some((answer, transport)) => {
                        started = true;
                        let _ = offers_sender.send(transport).await;
                        let mut reply = vec![ANSWER];
                        reply.extend_from_slice(answer.as_bytes());
                        reply
                    }
                    None => vec![REFUSED],
                }
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
    let mut privileged: Option<Privileged> = None;
    let mut desktop = Desktop::User;
    let mut settings: Option<Command> = None;
    let mut tick = tokio::time::interval(KEEPALIVE);
    let forward = |frames: &Option<blocking::SyncSender<Vec<u8>>>, message: Vec<u8>| {
        if let Some(frames) = frames
            && frames.try_send(message).is_err()
        {
            tracing::debug!("message dropped before the transport");
        }
    };
    let result = loop {
        tokio::select! {
            message = incoming.recv() => {
                let Some(message) = message else { break Ok(()) };
                if message == KEEPALIVE_MESSAGE || read_desktop(&message).is_some() {
                    continue;
                }
                // Frames and the encoder report follow the picture's source; the user's transfer
                // answers pass whichever desktop is shown.
                if read_transfer_reply(&message).is_some()
                    || !route(desktop, privileged.is_some()).frames_privileged
                {
                    forward(&frames, message);
                }
            }
            message = next(&mut privileged) => {
                let Some(message) = message else {
                    privileged = None;
                    if desktop != Desktop::User {
                        desktop = Desktop::User;
                        forward(&frames, desktop_message(desktop).to_vec());
                    }
                    continue;
                };
                if message == KEEPALIVE_MESSAGE {
                    continue;
                }
                if let Some(state) = read_desktop(&message) {
                    let before = route(desktop, true);
                    desktop = state;
                    let after = route(desktop, true);
                    // Keys and buttons held through the old route are let go there.
                    if before.input_privileged && !after.input_privileged {
                        send_privileged(&mut privileged, &command_message(&Command::Release)).await;
                    } else if !before.input_privileged
                        && after.input_privileged
                        && let Err(error) = send_all(&mut writer, &command_message(&Command::Release)).await
                    {
                        break Err(error);
                    }
                    forward(&frames, message);
                } else if route(desktop, true).frames_privileged {
                    forward(&frames, message);
                }
            }
            connection = attached.recv(), if privileged.is_none() => {
                let Some(connection) = connection else { continue };
                let (incoming, writer) = match connection.split() {
                    Ok(halves) => halves,
                    Err(error) => {
                        tracing::warn!(%error, "privileged worker channel not split");
                        continue;
                    }
                };
                privileged = Some(Privileged { incoming: reader(incoming), writer });
                if let Some(settings) = &settings {
                    send_privileged(&mut privileged, &command_message(settings)).await;
                }
            }
            transport = offers.recv(), if running.is_none() => {
                let Some(transport) = transport else { continue };
                let (sender, receiver) = blocking::sync_channel(64);
                if desktop != Desktop::User {
                    let _ = sender.try_send(desktop_message(desktop).to_vec());
                }
                frames = Some(sender);
                let (stop, commands) = (stop.clone(), commands_sender.clone());
                let reports = reports.clone();
                running = Some(std::thread::spawn(move || {
                    self::transport(transport, receiver, commands, stop, reports)
                }));
            }
            command = commands.recv() => {
                let Some(command) = command else { break Ok(()) };
                let message = command_message(&command);
                let current = route(desktop, privileged.is_some());
                let (ordinary, to_privileged) = match &command {
                    Command::Input(_) | Command::Release => {
                        (!current.input_privileged, current.input_privileged)
                    }
                    Command::Settings(_) => {
                        settings = Some(command.clone());
                        (true, true)
                    }
                    Command::Keyframe | Command::Stop => (true, true),
                    // Files and the clipboard belong to the user; never to the privileged worker.
                    Command::Transfer(_) => (true, false),
                    // Only with UAC rights, whose privileged worker handles the secure desktop
                    // that Ctrl+Alt+Del opens; the console session and Windows policy decide too.
                    Command::Sas => {
                        let sent = privileged.is_some() && secure_attention(session);
                        tracing::info!(sent, "Ctrl+Alt+Del requested");
                        forward(&frames, sas_result_message(sent).to_vec());
                        (false, false)
                    }
                };
                if to_privileged {
                    send_privileged(&mut privileged, &message).await;
                }
                if ordinary && let Err(error) = send_all(&mut writer, &message).await {
                    break Err(error);
                }
                if command == Command::Stop {
                    break Ok(());
                }
            }
            _ = tick.tick() => {
                if control_task.is_finished() {
                    let stopping = command_message(&Command::Stop);
                    send_privileged(&mut privileged, &stopping).await;
                    let _ = send_all(&mut writer, &stopping).await;
                    break Ok(());
                }
                send_privileged(&mut privileged, &KEEPALIVE_MESSAGE).await;
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
        let download = Transfer {
            download: true,
            ..Transfer::NONE
        };
        let message = offer_message(&hosts, None, "v=0", download);
        assert_eq!(
            read_offer(&message),
            Some(Offer {
                hosts: hosts.to_vec(),
                relay: None,
                transfer: download,
                sdp: "v=0".into()
            })
        );
        assert_eq!(
            read_offer(&offer_message(&hosts, None, "", Transfer::NONE)),
            None
        );
        let relay = Server {
            address: "[2001:db8::7]:3478".parse().unwrap(),
            username: "1700000300:request".into(),
            password: "credential".into(),
        };
        let with_relay = offer_message(&hosts, Some(&relay), "v=0", Transfer::NONE);
        assert_eq!(
            read_offer(&with_relay),
            Some(Offer {
                hosts: hosts.to_vec(),
                relay: Some(relay.clone()),
                transfer: Transfer::NONE,
                sdp: "v=0".into()
            })
        );
        // An unknown transfer flag is refused.
        let mut unknown = message.clone();
        let flags = unknown.len() - 4;
        unknown[flags] = 8;
        assert_eq!(read_offer(&unknown), None);
        assert_eq!(read_offer(&with_relay[..with_relay.len() - 8]), None);
        assert_eq!(read_offer(&message[..5]), None);
        assert_eq!(read_offer(&[OFFER, 17]), None);
        assert_eq!(read_offer(&[ANSWER, 0]), None);
        assert_eq!(read_answer(&[ANSWER, b'v']), Some("v".into()));
        assert_eq!(read_answer(&[ANSWER]), None);
        assert_eq!(read_answer(&[REFUSED]), None);
    }

    #[test]
    fn collected_reports_are_taken_once() {
        use midas_agent_core::media::transfer::{Kind, Outcome};
        let reports: Reports = Arc::default();
        let report = Report {
            kind: Kind::ClipboardFromDevice,
            name: None,
            size: None,
            sha256: None,
            outcome: Outcome::Completed,
        };
        reports.lock().unwrap().push_back(report.clone());
        assert_eq!(
            read_reports_reply(&reports_reply(&reports)),
            Some(vec![report])
        );
        assert_eq!(read_reports_reply(&reports_reply(&reports)), Some(vec![]));
        assert_eq!(read_reports_reply(&[ANSWER]), None);
    }

    #[test]
    fn listen_requests_carry_a_handle_and_an_endpoint() {
        let endpoint = uuid::Uuid::new_v4();
        let message = listen_message(0x1234, endpoint);
        assert_eq!(read_listen(&message), Some((0x1234, endpoint)));
        assert_eq!(read_listen(&listen_message(0, endpoint)), None);
        assert_eq!(read_listen(&listen_message(4, uuid::Uuid::nil())), None);
        assert_eq!(read_listen(&message[..20]), None);
        let mut longer = message.clone();
        longer.push(0);
        assert_eq!(read_listen(&longer), None);
        assert_eq!(
            read_listen(&offer_message(&[], None, "v=0", Transfer::NONE)),
            None
        );
        assert!(listening(&[LISTENING]) && !listening(&[REFUSED]));
    }

    #[test]
    fn the_secure_desktop_and_elevated_windows_go_to_the_privileged_worker_only() {
        let both = |frames_privileged, input_privileged| Route {
            frames_privileged,
            input_privileged,
        };
        assert_eq!(route(Desktop::User, true), both(false, false));
        assert_eq!(route(Desktop::Elevated, true), both(false, true));
        assert_eq!(route(Desktop::Secure, true), both(true, true));
        for desktop in [Desktop::User, Desktop::Elevated, Desktop::Secure] {
            assert_eq!(route(desktop, false), both(false, false));
        }
    }
}
