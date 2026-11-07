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
    if send(&mut socket, hello).await.is_err() {
        return Outcome::Failed(Reason::Handshake);
    }
    let welcome = match timeout(settings.handshake_timeout, receive(&mut socket)).await {
        Ok(Ok(Body::Welcome(welcome))) if settings.supported.contains(welcome.version) => welcome,
        Ok(Ok(Body::Reject(reject)))
            if reject.reason == RejectReason::UnsupportedVersion as i32 =>
        {
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
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if pending.is_some() {
                    tracing::warn!("agent keepalive unanswered");
                    return Outcome::Ended { connected };
                }
                let nonce = random();
                if send(&mut socket, Body::Ping(Ping { nonce })).await.is_err() {
                    return Outcome::Ended { connected };
                }
                pending = Some(nonce);
            }
            frame = receive(&mut socket) => match frame {
                Ok(Body::Pong(pong)) if Some(pong.nonce) == pending => pending = None,
                Ok(Body::Ping(ping)) => {
                    if send(&mut socket, Body::Pong(Pong { nonce: ping.nonce })).await.is_err() {
                        return Outcome::Ended { connected };
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

/// Each resolved address gets its own time limit: Windows retries a refused
/// connection for about two seconds, which would otherwise starve the next address.
async fn reach(host: &str, port: u16, limit: Duration) -> Option<TcpStream> {
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

async fn send(socket: &mut Socket, body: Body) -> Result<(), ()> {
    socket
        .send(Message::Binary(midas_protocol::encode(body).into()))
        .await
        .map_err(|_| ())
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
