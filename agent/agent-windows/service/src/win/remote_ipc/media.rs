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
    link::{
        Command, Desktop, KEEPALIVE_MESSAGE, command_message, desktop_message, read_command,
        read_desktop,
    },
    peer::{Answered, answer},
    session::{Input, Transport},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::windows::named_pipe::NamedPipeServer,
    sync::mpsc,
};

use super::{
    Connection, IO_TIMEOUT, Peer, STARTUP_TIMEOUT, Writer,
    bootstrap::{delivery::Launched, import_privileged},
    denied,
    desktop::InputDesktop,
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
const LISTEN: u8 = 4;
const LISTENING: u8 = 5;
/// How often the privileged worker looks at the input desktop.
const POLL: Duration = Duration::from_millis(100);

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

fn release(held: &mut Held) {
    let _ = inject::send(&held.release());
}

type Thread = Box<
    dyn FnOnce(blocking::Receiver<Command>, mpsc::Sender<Vec<Vec<u8>>>) -> io::Result<()> + Send,
>;

/// A worker's side of the control channel. Its thread captures and, without `inline`, also
/// executes input; with `inline`, input runs here against the view the thread keeps.
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
                    (Some(command), _) => {
                        if commands.send(command).is_err() {
                            break Ok(());
                        }
                    }
                    (None, _) => break Err(denied()),
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
    let control_task = tokio::spawn(async move {
        let mut started = false;
        let mut listening = false;
        while let Ok(Some(message)) = control.next().await {
            let reply = if let Some((handle, endpoint)) = read_listen(&message) {
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
                match read_offer(&message).filter(|_| !started) {
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
                if !route(desktop, privileged.is_some()).frames_privileged {
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
                let (incoming, writer) = connection.split()?;
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
                running = Some(std::thread::spawn(move || {
                    self::transport(transport, receiver, commands, stop)
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
        assert_eq!(read_listen(&offer_message(&[], "v=0")), None);
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
