use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use midas_protocol::{
    Body, CONNECT_PATH, Hello, MAX_AGENT_VERSION_BYTES, MAX_MESSAGE_BYTES, MAX_PLATFORM_BYTES,
    Pong, Reject, RejectReason, VersionRange, Welcome,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::timeout;

#[derive(Clone, Debug)]
pub struct AgentProtocol {
    pub supported: VersionRange,
    pub keepalive: Duration,
    pub handshake_timeout: Duration,
}

impl Default for AgentProtocol {
    fn default() -> Self {
        Self {
            supported: midas_protocol::SUPPORTED,
            keepalive: Duration::from_secs(30),
            handshake_timeout: Duration::from_secs(10),
        }
    }
}

impl AgentProtocol {
    /// Silence after which the server drops a session; three missed keepalives.
    fn idle_limit(&self) -> Duration {
        self.keepalive.saturating_mul(3)
    }
}

pub fn router(protocol: AgentProtocol) -> Router {
    Router::new()
        .route(CONNECT_PATH, get(connect))
        .with_state(Arc::new(protocol))
}

async fn connect(
    State(protocol): State<Arc<AgentProtocol>>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Ok(upgrade) = upgrade else {
        return (
            StatusCode::UPGRADE_REQUIRED,
            [(header::UPGRADE, "websocket")],
        )
            .into_response();
    };
    upgrade
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve(socket, protocol))
}

async fn serve(mut socket: WebSocket, protocol: Arc<AgentProtocol>) {
    let hello = match timeout(protocol.handshake_timeout, receive(&mut socket)).await {
        Ok(Ok(Body::Hello(hello))) => hello,
        _ => {
            tracing::warn!("agent handshake failed: no valid hello");
            refuse(&mut socket, RejectReason::InvalidHello, &protocol).await;
            return;
        }
    };
    let Some(peer) = valid_range(&hello) else {
        tracing::warn!("agent handshake failed: invalid hello");
        refuse(&mut socket, RejectReason::InvalidHello, &protocol).await;
        return;
    };
    let Some(version) = protocol.supported.negotiate(peer) else {
        tracing::warn!(
            agent_min = peer.min,
            agent_max = peer.max,
            server_min = protocol.supported.min,
            server_max = protocol.supported.max,
            "agent protocol versions unsupported"
        );
        refuse(&mut socket, RejectReason::UnsupportedVersion, &protocol).await;
        return;
    };
    let welcome = Body::Welcome(Welcome {
        version,
        keepalive_seconds: u32::try_from(protocol.keepalive.as_secs()).unwrap_or(u32::MAX),
        server_version: env!("CARGO_PKG_VERSION").into(),
    });
    if send(&mut socket, welcome).await.is_err() {
        return;
    }
    let started = Instant::now();
    tracing::info!(
        version,
        agent_version = %hello.agent_version,
        platform = %hello.platform,
        "agent session started"
    );
    let ending = loop {
        match timeout(protocol.idle_limit(), receive(&mut socket)).await {
            Err(_) => break "idle",
            Ok(Err(())) => break "closed",
            Ok(Ok(Body::Ping(ping))) => {
                if send(&mut socket, Body::Pong(Pong { nonce: ping.nonce }))
                    .await
                    .is_err()
                {
                    break "closed";
                }
            }
            Ok(Ok(_)) => {}
        }
    };
    tracing::info!(
        ending,
        duration_s = started.elapsed().as_secs(),
        "agent session ended"
    );
}

fn valid_range(hello: &Hello) -> Option<VersionRange> {
    (hello.agent_version.len() <= MAX_AGENT_VERSION_BYTES
        && hello.platform.len() <= MAX_PLATFORM_BYTES)
        .then(|| VersionRange::new(hello.min_version, hello.max_version))
        .flatten()
}

async fn refuse(socket: &mut WebSocket, reason: RejectReason, protocol: &AgentProtocol) {
    let reject = Body::Reject(Reject {
        reason: reason as i32,
        min_version: protocol.supported.min,
        max_version: protocol.supported.max,
    });
    if send(socket, reject).await.is_ok() {
        let _ = socket.send(Message::Close(None)).await;
    }
}

async fn send(socket: &mut WebSocket, body: Body) -> Result<(), axum::Error> {
    socket
        .send(Message::Binary(midas_protocol::encode(body).into()))
        .await
}

/// Next known message; unknown message types from newer agents are skipped.
async fn receive(socket: &mut WebSocket) -> Result<Body, ()> {
    loop {
        match socket.recv().await {
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
