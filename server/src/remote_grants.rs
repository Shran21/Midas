use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use midas_protocol::{Body, remote as wire, sessions::Presence as WirePresence};
use midas_remote_auth::verify;
use prost::Message;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch};
use uuid::Uuid;

use crate::{
    audit::{Action, Actor, Event as AuditEvent, Outcome},
    auth::sessions::{SessionInfo, SessionToken, Sessions},
    authorization::{
        AccessError, RemoteMode, Target,
        unattended::{
            Presence, SessionObservation, SessionSelection, UnattendedRequest,
            grants::{GrantAuthority, GrantDecision, GrantRequest},
        },
    },
    enrollment::{Admission, AgentIdentity, Enrollment},
};

type Result<T> = std::result::Result<T, AccessError>;
const REPLY_LIMIT: Duration = Duration::from_secs(8);
const LIFETIME: Duration = Duration::from_secs(midas_remote_auth::MAX_LIFETIME_SECONDS);

#[derive(Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<HashMap<Uuid, Entry>>>);

struct Entry {
    identity: AgentIdentity,
    authority: Arc<GrantAuthority>,
    sender: mpsc::Sender<Command>,
    slot: Arc<Semaphore>,
}

pub(crate) enum Command {
    Prepare {
        message: wire::Prepare,
        stop: watch::Receiver<bool>,
        active: Arc<AtomicBool>,
        reply: oneshot::Sender<Result<Challenge>>,
    },
    Deliver {
        message: wire::Deliver,
        lifetime: Duration,
        reply: oneshot::Sender<Result<()>>,
    },
}

pub(crate) enum Event {
    Command(Command),
    Cancel,
}

pub(crate) struct Challenge {
    nonce: Uuid,
    observation: SessionObservation,
    started: Instant,
}

pub(crate) struct Registration {
    registry: Registry,
    connection: Uuid,
    pub authority: Arc<GrantAuthority>,
    requests: mpsc::Receiver<Command>,
    flow: Option<Flow>,
}

struct Flow {
    request: Vec<u8>,
    selection: SessionSelection,
    started: Instant,
    deadline: Instant,
    stop: watch::Receiver<bool>,
    active: Arc<AtomicBool>,
    challenge: Option<oneshot::Sender<Result<Challenge>>>,
    delivery: Option<oneshot::Sender<Result<()>>>,
    prepared: bool,
}

impl Drop for Flow {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

/// Owned by the protected connection/session coordinator, never returned to a browser.
pub struct Session {
    request: Uuid,
    deadline: Instant,
    active: Arc<AtomicBool>,
    stop: watch::Sender<bool>,
    _slot: OwnedSemaphorePermit,
}

impl Session {
    pub fn id(&self) -> Uuid {
        self.request
    }
    /// Acknowledged, unexpired grant ownership; not proof of an active media channel.
    pub fn is_active(&self) -> bool {
        Instant::now() < self.deadline
            && self.active.load(Ordering::Acquire)
            && !*self.stop.borrow()
    }
    pub fn revoke(&self) {
        self.active.store(false, Ordering::Release);
        self.stop.send_replace(true);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.revoke();
    }
}

pub enum Decision {
    LocalConsentRequired,
    Granted(Session),
}

impl Registry {
    pub fn register(&self, admission: &Admission) -> Result<Registration> {
        let authority = Arc::new(GrantAuthority::for_agent(admission)?);
        let (sender, requests) = mpsc::channel(1);
        let connection = admission.connection_id();
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(
            connection,
            Entry {
                identity: admission.identity,
                authority: authority.clone(),
                sender,
                slot: Arc::new(Semaphore::new(1)),
            },
        );
        Ok(Registration {
            registry: self.clone(),
            connection,
            authority,
            requests,
            flow: None,
        })
    }

    fn acquire(
        &self,
        identity: AgentIdentity,
        connection: Uuid,
    ) -> Result<(
        Arc<GrantAuthority>,
        mpsc::Sender<Command>,
        OwnedSemaphorePermit,
    )> {
        let entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = entries
            .get(&connection)
            .filter(|entry| entry.identity == identity)
            .ok_or(AccessError::Offline)?;
        if entries
            .values()
            .filter(|entry| entry.identity == identity)
            .count()
            != 1
        {
            return Err(AccessError::Conflict);
        }
        let slot = entry
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| AccessError::Conflict)?;
        Ok((entry.authority.clone(), entry.sender.clone(), slot))
    }
}

impl Registration {
    pub fn context(&self, admission: &Admission) -> wire::Context {
        wire::Context {
            tenant: admission.identity.tenant.as_bytes().to_vec(),
            device: admission.identity.device.as_bytes().to_vec(),
            public_key: self.authority.public_key().to_vec(),
        }
    }

    pub fn command(&mut self, command: Command, admission: &Admission) -> Option<Body> {
        match command {
            Command::Prepare {
                message,
                stop,
                active,
                reply,
            } => {
                if self.flow.is_some()
                    || *stop.borrow()
                    || stop.has_changed().is_err()
                    || reply.is_closed()
                {
                    let _ = reply.send(Err(AccessError::Conflict));
                    return None;
                }
                let now = Instant::now();
                self.flow = Some(Flow {
                    request: message.request.clone(),
                    selection: SessionSelection {
                        connection_id: admission.connection_id(),
                        os_session_id: message.session.to_string(),
                        generation: Uuid::from_slice(&message.generation)
                            .expect("validated generation"),
                    },
                    started: now,
                    deadline: now + REPLY_LIMIT,
                    stop,
                    active,
                    challenge: Some(reply),
                    delivery: None,
                    prepared: false,
                });
                Some(Body::RemotePrepare(message))
            }
            Command::Deliver {
                message,
                lifetime,
                reply,
            } => {
                let Some(flow) = self.flow.as_mut().filter(|flow| {
                    flow.request == message.request
                        && flow.prepared
                        && flow.delivery.is_none()
                        && !flow.active.load(Ordering::Acquire)
                        && !*flow.stop.borrow()
                        && flow.stop.has_changed().is_ok()
                        && Instant::now() < flow.deadline
                        && !reply.is_closed()
                }) else {
                    let _ = reply.send(Err(AccessError::Offline));
                    return None;
                };
                flow.deadline = flow.started + lifetime.min(LIFETIME);
                if Instant::now() >= flow.deadline {
                    let _ = reply.send(Err(AccessError::Offline));
                    return None;
                }
                flow.prepared = false;
                flow.delivery = Some(reply);
                Some(Body::RemoteDeliver(message))
            }
        }
    }

    pub fn challenge(&mut self, message: wire::Challenge, admission: &Admission) {
        let Some(flow) = self
            .flow
            .as_mut()
            .filter(|flow| flow.request == message.request)
        else {
            return;
        };
        let Some(reply) = flow.challenge.take() else {
            return;
        };
        let result = (|| {
            if Instant::now() >= flow.deadline
                || *flow.stop.borrow()
                || flow.stop.has_changed().is_err()
            {
                return Err(AccessError::Offline);
            }
            let (presence, nonce) = match WirePresence::try_from(message.presence) {
                Ok(WirePresence::Vacant) => (
                    Presence::Vacant,
                    Uuid::from_slice(&message.challenge)
                        .ok()
                        .filter(|id| !id.is_nil())
                        .ok_or(AccessError::InvalidInput)?,
                ),
                Ok(WirePresence::Occupied) if message.challenge.is_empty() => {
                    (Presence::Occupied, Uuid::nil())
                }
                _ => return Err(AccessError::Offline),
            };
            let observation = SessionObservation::from_request(
                admission,
                flow.selection.clone(),
                presence,
                flow.started,
            )?;
            flow.prepared = true;
            flow.deadline = flow.started + LIFETIME;
            Ok(Challenge {
                nonce,
                observation,
                started: flow.started,
            })
        })();
        let _ = reply.send(result);
    }

    pub fn acknowledged(&mut self, message: wire::Ack) {
        let Some(flow) = self
            .flow
            .as_mut()
            .filter(|flow| flow.request == message.request)
        else {
            return;
        };
        let Some(reply) = flow.delivery.take() else {
            return;
        };
        let accepted = message.accepted
            && Instant::now() < flow.deadline
            && !*flow.stop.borrow()
            && flow.stop.has_changed().is_ok()
            && !reply.is_closed();
        flow.active.store(accepted, Ordering::Release);
        let _ = reply.send(if accepted {
            Ok(())
        } else {
            Err(AccessError::Offline)
        });
    }

    pub fn cancel(&mut self) -> Option<Body> {
        self.flow.take().map(|flow| {
            Body::RemoteRevoke(wire::Revoke {
                request: flow.request.clone(),
            })
        })
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.registry
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.connection);
    }
}

pub(crate) async fn next(registration: &mut Option<Registration>) -> Option<Event> {
    match registration {
        Some(registration) => tokio::select! {
            biased;
            () = cancelled(&mut registration.flow) => Some(Event::Cancel),
            command = registration.requests.recv() => command.map(Event::Command),
        },
        None => std::future::pending().await,
    }
}

async fn cancelled(flow: &mut Option<Flow>) {
    let Some(flow) = flow.as_mut() else {
        return std::future::pending().await;
    };
    if *flow.stop.borrow() {
        return;
    }
    tokio::select! {
        _ = tokio::time::sleep_until(flow.deadline.into()) => {},
        _ = flow.stop.changed() => {},
    }
}

impl Enrollment {
    pub async fn authorize_remote(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
        request: &UnattendedRequest,
    ) -> Result<Decision> {
        let target = Target {
            tenant,
            device: Some(device),
        };
        let operator = self
            .authorizer
            .prepare_unattended_request(sessions, token, target, request)
            .await?;
        let id = Uuid::new_v4();
        let event = |action, outcome, result| {
            let mut event = AuditEvent::new(action, outcome, Actor::session(&operator))
                .detail("request_id", id.to_string())
                .detail("session", serde_json::json!(request.session))
                .detail("mode", serde_json::json!(request.mode))
                .detail("uac", request.uac)
                .detail("reason", request.reason.trim())
                .detail("result", result);
            event.tenant = Some(tenant);
            event.device = Some(device);
            event
        };
        self.authorizer
            .record_decision(&event(
                Action::RemoteGrantPrepare,
                Outcome::Success,
                "requested",
            ))
            .await?;
        let result = self
            .deliver_remote(sessions, token, target, request, &operator, id)
            .await;
        let (outcome, label) = match &result {
            Ok(Decision::Granted(_)) => (Outcome::Success, "acknowledged"),
            Ok(Decision::LocalConsentRequired) => (Outcome::Denied, "local_consent_required"),
            Err(_) => (Outcome::Failure, "not_delivered"),
        };
        self.authorizer
            .record_decision(&event(Action::RemoteGrantDelivery, outcome, label))
            .await?;
        if let Ok(Decision::Granted(session)) = &result
            && !session.is_active()
        {
            return Err(AccessError::Offline);
        }
        result
    }

    async fn deliver_remote(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: &UnattendedRequest,
        operator: &SessionInfo,
        id: Uuid,
    ) -> Result<Decision> {
        let (tenant, device) = (
            target.tenant,
            target.device.ok_or(AccessError::InvalidInput)?,
        );
        let (authority, sender, slot) = match self.remote_grants.acquire(
            AgentIdentity { tenant, device },
            request.session.connection_id,
        ) {
            Ok(entry) => entry,
            Err(error) => {
                self.authorizer
                    .record_unattended_refusal(operator, target, request, &error)
                    .await?;
                return Err(error);
            }
        };
        let os_session: u32 = request
            .session
            .os_session_id
            .parse()
            .map_err(|_| AccessError::InvalidInput)?;
        if os_session == 0
            || os_session == u32::MAX
            || request.session.os_session_id != os_session.to_string()
        {
            return Err(AccessError::InvalidInput);
        }
        let (stop, stopped) = watch::channel(false);
        let mut session = Session {
            request: id,
            deadline: Instant::now() + LIFETIME,
            active: Arc::new(AtomicBool::new(false)),
            stop,
            _slot: slot,
        };
        let (reply, result) = oneshot::channel();
        sender
            .try_send(Command::Prepare {
                message: wire::Prepare {
                    request: id.as_bytes().to_vec(),
                    session: os_session,
                    generation: request.session.generation.as_bytes().to_vec(),
                    operator: operator.operator_id.as_bytes().to_vec(),
                    mode: match request.mode {
                        RemoteMode::Normal => wire::Mode::Normal,
                        RemoteMode::Administrative => wire::Mode::Administrative,
                    } as i32,
                    uac: request.uac,
                },
                stop: stopped,
                active: session.active.clone(),
                reply,
            })
            .map_err(|_| AccessError::Offline)?;
        let challenge = wait(result).await?;
        let decision = self
            .authorizer
            .issue_unattended_grant(
                sessions,
                token,
                target,
                request,
                Some(&challenge.observation),
                GrantRequest {
                    authority: &authority,
                    challenge: challenge.nonce,
                },
            )
            .await?;
        let GrantDecision::Issued(grant) = decision else {
            return Ok(Decision::LocalConsentRequired);
        };
        let checked =
            verify(&authority.public_key(), &grant).map_err(|_| AccessError::Unavailable)?;
        let lifetime = Duration::from_secs(
            checked
                .claims()
                .lifetime_seconds()
                .map_err(|_| AccessError::Unavailable)?,
        );
        session.deadline = challenge.started + lifetime;
        let (reply, result) = oneshot::channel();
        sender
            .try_send(Command::Deliver {
                message: wire::Deliver {
                    request: id.as_bytes().to_vec(),
                    grant: grant.encode_to_vec(),
                },
                lifetime,
                reply,
            })
            .map_err(|_| AccessError::Offline)?;
        wait(result).await?;
        if !session.is_active() {
            return Err(AccessError::Offline);
        }
        Ok(Decision::Granted(session))
    }
}

async fn wait<T>(result: oneshot::Receiver<Result<T>>) -> Result<T> {
    tokio::time::timeout(REPLY_LIMIT + Duration::from_secs(2), result)
        .await
        .map_err(|_| AccessError::Offline)?
        .map_err(|_| AccessError::Offline)?
}
