use axum::{
    Extension, Router,
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

use crate::{
    enrollment::{Admission, AgentIdentity, Enrollment, Signal},
    transport::PeerCertificate,
};

#[derive(Clone)]
pub struct AgentProtocol {
    pub supported: VersionRange,
    pub keepalive: Duration,
    pub handshake_timeout: Duration,
    /// Device identities for the operational channel. Without them any certificate the
    /// TLS layer accepted may connect, which only protocol tests use; the server binary
    /// always sets them.
    pub identities: Option<Arc<Enrollment>>,
}

impl Default for AgentProtocol {
    fn default() -> Self {
        Self {
            supported: midas_protocol::SUPPORTED,
            keepalive: Duration::from_secs(30),
            handshake_timeout: Duration::from_secs(10),
            identities: None,
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
    peer: Option<Extension<PeerCertificate>>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Ok(upgrade) = upgrade else {
        return (
            StatusCode::UPGRADE_REQUIRED,
            [(header::UPGRADE, "websocket")],
        )
            .into_response();
    };
    let admission = match &protocol.identities {
        None => None,
        Some(identities) => {
            let certificate = peer.map(|Extension(peer)| peer.0);
            match identities
                .admit(certificate.as_deref().unwrap_or_default())
                .await
            {
                Some(admission) => Some(admission),
                None => {
                    tracing::warn!("agent certificate is not an active device credential");
                    return StatusCode::FORBIDDEN.into_response();
                }
            }
        }
    };
    upgrade
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve(socket, protocol, admission))
}

async fn serve(
    mut socket: WebSocket,
    protocol: Arc<AgentProtocol>,
    mut admission: Option<Admission>,
) {
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
    if send(&mut socket, welcome, protocol.keepalive)
        .await
        .is_err()
    {
        return;
    }
    let started = Instant::now();
    let identity = admission.as_ref().map(|admitted| admitted.identity);
    tracing::info!(
        version,
        agent_version = %hello.agent_version,
        platform = %hello.platform,
        tenant = ?identity.map(|i| i.tenant),
        device = ?identity.map(|i| i.device),
        "agent session started"
    );
    let mut assigned = None;
    seen(&protocol, identity).await;
    if !assign(&mut socket, &protocol, identity, &mut assigned).await {
        return;
    }
    let ending = loop {
        let next = tokio::select! {
            next = timeout(protocol.idle_limit(), receive(&mut socket)) => next,
            signal = changed(&mut admission) => {
                if signal.revoked {
                    let _ = timeout(protocol.handshake_timeout, socket.send(Message::Close(None))).await;
                    break "revoked";
                }
                if !assign(&mut socket, &protocol, identity, &mut assigned).await {
                    break "closed";
                }
                continue;
            }
        };
        match next {
            Err(_) => break "idle",
            Ok(Err(())) => break "closed",
            Ok(Ok(Body::Ping(ping))) => {
                seen(&protocol, identity).await;
                if send(
                    &mut socket,
                    Body::Pong(Pong { nonce: ping.nonce }),
                    protocol.keepalive,
                )
                .await
                .is_err()
                {
                    break "closed";
                }
            }
            Ok(Ok(Body::MetricsBatch(batch))) => {
                let (Some(identities), Some(identity)) = (&protocol.identities, identity) else {
                    continue;
                };
                // Without an acknowledgement the agent keeps the samples and resends them.
                if identities.ingest(identity, &batch).await.is_err() {
                    tracing::error!("metrics could not be stored; closing the session");
                    break "store";
                }
                let ack = Body::MetricsAck(midas_protocol::MetricsAck {
                    sequence: batch.sequence,
                });
                if send(&mut socket, ack, protocol.keepalive).await.is_err() {
                    break "closed";
                }
            }
            Ok(Ok(Body::DirectoryReport(report))) => {
                if let (Some(identities), Some(identity)) = (&protocol.identities, identity) {
                    identities.record(identity, &report).await;
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

async fn seen(protocol: &AgentProtocol, identity: Option<AgentIdentity>) {
    if let (Some(identities), Some(identity)) = (&protocol.identities, identity)
        && identities.seen(identity).await.is_err()
    {
        tracing::error!("device presence could not be recorded");
    }
}

async fn changed(admission: &mut Option<Admission>) -> Signal {
    match admission {
        Some(admitted) => match admitted.signal.changed().await {
            Ok(()) => *admitted.signal.borrow_and_update(),
            Err(_) => Signal {
                revoked: true,
                ..Signal::default()
            },
        },
        None => std::future::pending().await,
    }
}

/// Sends the directory assignment when the device is its tenant's connector and it changed.
async fn assign(
    socket: &mut WebSocket,
    protocol: &AgentProtocol,
    identity: Option<AgentIdentity>,
    assigned: &mut Option<u64>,
) -> bool {
    let (Some(identities), Some(identity)) = (&protocol.identities, identity) else {
        return true;
    };
    let Some(assignment) = identities.assignment(identity).await else {
        return true;
    };
    if *assigned == Some(assignment.revision) {
        return true;
    }
    *assigned = Some(assignment.revision);
    send(
        socket,
        Body::DirectoryAssignment(assignment),
        protocol.keepalive,
    )
    .await
    .is_ok()
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
    if send(socket, reject, protocol.handshake_timeout)
        .await
        .is_ok()
    {
        let _ = timeout(
            protocol.handshake_timeout,
            socket.send(Message::Close(None)),
        )
        .await;
    }
}

/// Bounded so an agent that stops reading cannot hold the session task.
async fn send(socket: &mut WebSocket, body: Body, limit: Duration) -> Result<(), ()> {
    let message = Message::Binary(midas_protocol::encode(body).into());
    match timeout(limit, socket.send(message)).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
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
