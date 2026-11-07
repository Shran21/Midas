use crate::backoff::{Backoff, BackoffPolicy};
use futures_util::{SinkExt, StreamExt};
use midas_protocol::{
    Body, CONNECT_PATH, Hello, MAX_MESSAGE_BYTES, Ping, Pong, RejectReason, VersionRange,
};
use rustls::pki_types::ServerName;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, sync::watch, time::timeout};
use tokio_rustls::{TlsConnector, client::TlsStream};
use tokio_tungstenite::{
    WebSocketStream, client_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

const KEEPALIVE_LIMITS: (u32, u32) = (1, 300);

#[derive(Clone)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    /// Must carry the agent's client certificate; the server only accepts mTLS.
    pub tls: Arc<rustls::ClientConfig>,
    pub supported: VersionRange,
    pub agent_version: String,
    pub platform: String,
    pub handshake_timeout: Duration,
    pub backoff: BackoffPolicy,
    /// A session lasting this long resets the backoff.
    pub stable_after: Duration,
    /// Present only on the agent the tenant designated as its directory connector.
    pub directory: Option<crate::directory::Credentials>,
    /// Samples waiting for upload; they stay until the server acknowledges them.
    pub metrics: Option<crate::metrics::Shared>,
}

impl Settings {
    pub fn new(host: String, port: u16, tls: Arc<rustls::ClientConfig>, platform: &str) -> Self {
        Self {
            host,
            port,
            tls,
            supported: midas_protocol::SUPPORTED,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            platform: platform.into(),
            handshake_timeout: Duration::from_secs(10),
            backoff: BackoffPolicy::default(),
            stable_after: Duration::from_secs(60),
            directory: None,
            metrics: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting { attempt: u32 },
    Connected { version: u32, keepalive: Duration },
    Waiting { retry_in: Duration, reason: Reason },
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Unreachable,
    Handshake,
    Incompatible { server: VersionRange },
    Lost,
}

enum Outcome {
    Failed(Reason),
    Ended { connected: Instant },
    Shutdown,
}

/// Keeps one outbound session open until `shutdown` becomes true.
pub async fn run(
    settings: Settings,
    status: watch::Sender<Status>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = Backoff::new(settings.backoff);
    let mut attempt = 0u32;
    loop {
        attempt = attempt.saturating_add(1);
        status.send_replace(Status::Connecting { attempt });
        let outcome = session(&settings, &status, &mut shutdown).await;
        let (reason, delay) = match outcome {
            Outcome::Shutdown => break,
            Outcome::Ended { connected } => {
                if connected.elapsed() >= settings.stable_after {
                    backoff.reset();
                }
                (Reason::Lost, backoff.next_delay(random()))
            }
            Outcome::Failed(reason @ Reason::Incompatible { .. }) => {
                (reason, backoff.longest_delay(random()))
            }
            Outcome::Failed(reason) => (reason, backoff.next_delay(random())),
        };
        tracing::info!(
            ?reason,
            retry_in_ms = delay.as_millis() as u64,
            "agent connection waiting"
        );
        status.send_replace(Status::Waiting {
            retry_in: delay,
            reason,
        });
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = stopped(&mut shutdown) => break,
        }
    }
    status.send_replace(Status::Stopped);
}

async fn session(
    settings: &Settings,
    status: &watch::Sender<Status>,
    shutdown: &mut watch::Receiver<bool>,
) -> Outcome {
    let mut socket = tokio::select! {
        socket = open(settings) => match socket {
            Ok(socket) => socket,
            Err(reason) => return Outcome::Failed(reason),
        },
        () = stopped(shutdown) => return Outcome::Shutdown,
    };
    let hello = Body::Hello(Hello {
        min_version: settings.supported.min,
        max_version: settings.supported.max,
        agent_version: settings.agent_version.clone(),
        platform: settings.platform.clone(),
    });
    let handshake = async {
        send(&mut socket, hello, settings.handshake_timeout).await?;
        timeout(settings.handshake_timeout, receive(&mut socket))
            .await
            .map_err(|_| ())?
    };
    let answer = tokio::select! {
        answer = handshake => answer,
        () = stopped(shutdown) => return Outcome::Shutdown,
    };
    let welcome = match answer {
        Ok(Body::Welcome(welcome)) if settings.supported.contains(welcome.version) => welcome,
        Ok(Body::Reject(reject)) if reject.reason == RejectReason::UnsupportedVersion as i32 => {
            let server = VersionRange::new(reject.min_version, reject.max_version)
                .unwrap_or(VersionRange { min: 0, max: 0 });
            tracing::warn!(?server, supported = ?settings.supported, "server refused agent protocol versions");
            return Outcome::Failed(Reason::Incompatible { server });
        }
        _ => return Outcome::Failed(Reason::Handshake),
    };
    let keepalive = Duration::from_secs(
        welcome
            .keepalive_seconds
            .clamp(KEEPALIVE_LIMITS.0, KEEPALIVE_LIMITS.1)
            .into(),
    );
    let connected = Instant::now();
    tracing::info!(version = welcome.version, server_version = %welcome.server_version, "agent connected");
    status.send_replace(Status::Connected {
        version: welcome.version,
        keepalive,
    });
    let mut tick = tokio::time::interval(keepalive);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await;
    let mut pending: Option<u64> = None;
    let (reports, mut finished) = tokio::sync::mpsc::channel(1);
    // One directory check at a time; a newer assignment waits for the running one.
    let mut checking = false;
    let mut queued: Option<midas_protocol::DirectoryAssignment> = None;
    let mut upload = Upload::default();
    if upload
        .offer(&mut socket, settings, keepalive)
        .await
        .is_err()
    {
        return Outcome::Ended { connected };
    }
    loop {
        tokio::select! {
            Some(report) = finished.recv() => {
                checking = false;
                if let Some(next) = queued.take() {
                    checking = true;
                    directory_check(next, settings.directory.clone(), reports.clone());
                }
                if send(&mut socket, Body::DirectoryReport(report), keepalive).await.is_err() {
                    return Outcome::Ended { connected };
                }
            }
            _ = tick.tick() => {
                if pending.is_some() {
                    tracing::warn!("agent keepalive unanswered");
                    return Outcome::Ended { connected };
                }
                let nonce = random();
                if send(&mut socket, Body::Ping(Ping { nonce }), keepalive).await.is_err() {
                    return Outcome::Ended { connected };
                }
                pending = Some(nonce);
                if upload.offer(&mut socket, settings, keepalive).await.is_err() {
                    return Outcome::Ended { connected };
                }
            }
            frame = receive(&mut socket) => match frame {
                Ok(Body::Pong(pong)) if Some(pong.nonce) == pending => pending = None,
                Ok(Body::Ping(ping)) => {
                    if send(&mut socket, Body::Pong(Pong { nonce: ping.nonce }), keepalive).await.is_err() {
                        return Outcome::Ended { connected };
                    }
                }
                Ok(Body::MetricsAck(ack)) => {
                    upload.acknowledged(&ack, settings);
                    if upload.offer(&mut socket, settings, keepalive).await.is_err() {
                        return Outcome::Ended { connected };
                    }
                }
                Ok(Body::DirectoryAssignment(assignment)) => {
                    if checking {
                        queued = Some(assignment);
                    } else {
                        checking = true;
                        directory_check(assignment, settings.directory.clone(), reports.clone());
                    }
                }
                Ok(_) => {}
                Err(()) => return Outcome::Ended { connected },
            },
            () = stopped(shutdown) => {
                let _ = socket.close(None).await;
                return Outcome::Shutdown;
            }
        }
    }
}

type Socket = WebSocketStream<TlsStream<TcpStream>>;

async fn open(settings: &Settings) -> Result<Socket, Reason> {
    let limit = settings.handshake_timeout;
    let tcp = reach(&settings.host, settings.port, limit)
        .await
        .ok_or(Reason::Unreachable)?;
    let _ = tcp.set_nodelay(true);
    let name = ServerName::try_from(settings.host.clone()).map_err(|_| Reason::Handshake)?;
    let tls = match timeout(
        limit,
        TlsConnector::from(settings.tls.clone()).connect(name, tcp),
    )
    .await
    {
        Ok(Ok(tls)) => tls,
        _ => return Err(Reason::Handshake),
    };
    let request = format!("wss://{}:{}{CONNECT_PATH}", settings.host, settings.port)
        .into_client_request()
        .map_err(|_| Reason::Handshake)?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    match timeout(limit, client_async_with_config(request, tls, Some(config))).await {
        Ok(Ok((socket, _))) => Ok(socket),
        _ => Err(Reason::Handshake),
    }
}

/// At most one metrics batch in flight; the next waits for its acknowledgement.
#[derive(Default)]
struct Upload {
    sequence: u64,
    in_flight: Option<(u64, usize, u64)>,
}

impl Upload {
    async fn offer(
        &mut self,
        socket: &mut Socket,
        settings: &Settings,
        limit: Duration,
    ) -> Result<(), ()> {
        let Some(buffer) = &settings.metrics else {
            return Ok(());
        };
        if self.in_flight.is_some() {
            return Ok(());
        }
        let (samples, dropped) = crate::metrics::lock(buffer).peek();
        if samples.is_empty() && dropped == 0 {
            return Ok(());
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.in_flight = Some((self.sequence, samples.len(), dropped));
        let batch = midas_protocol::MetricsBatch {
            sequence: self.sequence,
            samples,
            dropped,
        };
        send(socket, Body::MetricsBatch(batch), limit).await
    }

    fn acknowledged(&mut self, ack: &midas_protocol::MetricsAck, settings: &Settings) {
        if let (Some((sequence, count, dropped)), Some(buffer)) =
            (self.in_flight, &settings.metrics)
            && sequence == ack.sequence
        {
            crate::metrics::lock(buffer).acknowledge(count, dropped);
            self.in_flight = None;
        }
    }
}

fn directory_check(
    assignment: midas_protocol::DirectoryAssignment,
    credentials: Option<crate::directory::Credentials>,
    reports: tokio::sync::mpsc::Sender<midas_protocol::DirectoryReport>,
) {
    tokio::spawn(async move {
        let report = match credentials {
            Some(credentials) => crate::directory::check(&assignment, &credentials).await,
            None => midas_protocol::DirectoryReport {
                revision: assignment.revision,
                ok: false,
                objects: 0,
                error: "unconfigured".into(),
            },
        };
        let _ = reports.send(report).await;
    });
}

/// Each resolved address gets its own time limit: Windows retries a refused
/// connection for about two seconds, which would otherwise starve the next address.
pub(crate) async fn reach(host: &str, port: u16, limit: Duration) -> Option<TcpStream> {
    let addresses = timeout(limit, tokio::net::lookup_host((host, port)))
        .await
        .ok()?
        .ok()?;
    for address in addresses {
        if let Ok(Ok(tcp)) = timeout(limit, TcpStream::connect(address)).await {
            return Some(tcp);
        }
    }
    None
}

/// Bounded so a stalled send buffer counts as a lost connection instead of hanging.
async fn send(socket: &mut Socket, body: Body, limit: Duration) -> Result<(), ()> {
    let message = Message::Binary(midas_protocol::encode(body).into());
    match timeout(limit, socket.send(message)).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
}

/// Next known protocol message; unknown message types from newer servers are skipped.
async fn receive(socket: &mut Socket) -> Result<Body, ()> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Binary(bytes))) => match midas_protocol::decode(&bytes) {
                Ok(Some(body)) => return Ok(body),
                Ok(None) => {}
                Err(_) => return Err(()),
            },
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Err(()),
            Some(Ok(_)) => {}
        }
    }
}

async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|stop| *stop).await;
}

fn random() -> u64 {
    getrandom::u64().unwrap_or_default()
}
