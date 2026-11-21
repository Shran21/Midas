use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    time::{Duration, Instant},
};

use midas_protocol::{Body, remote as wire, sessions::Presence as WirePresence};
use midas_remote_auth::{SignedGrant, verify};
use prost::Message;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch};
use uuid::Uuid;

use crate::{
    audit::{Action, Actor, Event as AuditEvent, Outcome},
    auth::sessions::{SessionInfo, SessionToken, Sessions},
    authorization::{
        AccessError, RemoteMode, Target,
        attended::{Asked, CONSENT_TIMEOUT, Requester},
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

/// Messages from the agent one session may have waiting; beyond this they are dropped.
const INBOUND: usize = 64;

/// What the agent sends for an active session besides its lifecycle.
#[derive(Debug)]
pub(crate) enum FromAgent {
    Transfer(wire::TransferReport),
    /// The signed-in user's chat message.
    Chat(String),
}

pub(crate) enum Command {
    Prepare {
        message: wire::Prepare,
        stop: watch::Receiver<bool>,
        active: Arc<AtomicBool>,
        ended: Arc<AtomicI32>,
        /// Where the session's transfer reports and chat go.
        inbound: Option<mpsc::Sender<FromAgent>>,
        reply: oneshot::Sender<Result<Challenge>>,
    },
    Deliver {
        message: wire::Deliver,
        lifetime: Duration,
        reply: oneshot::Sender<Result<()>>,
    },
    Media {
        message: wire::MediaOffer,
        reply: oneshot::Sender<Result<String>>,
    },
    /// A renewal chained to the last grant of the active request.
    Renew {
        message: wire::Deliver,
        lifetime: Duration,
        reply: oneshot::Sender<Result<()>>,
    },
    /// The operator's chat message for the active request.
    Chat { message: wire::Chat },
}

pub(crate) enum Event {
    Command(Command),
    Cancel,
}

pub(crate) struct Challenge {
    nonce: Uuid,
    observation: Option<SessionObservation>,
    started: Instant,
    /// The local answer to an attended request; `Unspecified` for an unattended one.
    consent: wire::Consent,
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
    /// Why the agent ended the session itself, as `wire::EndReason`.
    ended: Arc<AtomicI32>,
    inbound: Option<mpsc::Sender<FromAgent>>,
    challenge: Option<oneshot::Sender<Result<Challenge>>>,
    delivery: Option<oneshot::Sender<Result<()>>>,
    media: Option<oneshot::Sender<Result<String>>>,
    renewal: Option<(oneshot::Sender<Result<()>>, Duration)>,
    prepared: bool,
    attended: bool,
}

impl Drop for Flow {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

/// Owned by the protected connection/session coordinator, never returned to a browser.
pub struct Session {
    request: Uuid,
    deadline: Mutex<Instant>,
    active: Arc<AtomicBool>,
    ended: Arc<AtomicI32>,
    stop: watch::Sender<bool>,
    sender: mpsc::Sender<Command>,
    /// What renewals of an attended session are signed for, once its grant is acknowledged.
    renewal: Option<Box<Renewal>>,
    /// The agent's transfer reports and chat, for the request that handles them.
    inbound: Mutex<Option<mpsc::Receiver<FromAgent>>>,
    _slot: OwnedSemaphorePermit,
}

pub(crate) struct Renewal {
    pub(crate) authority: Arc<GrantAuthority>,
    pub(crate) observation: SessionObservation,
    /// The last grant the agent applied; the next renewal names it as its challenge.
    grant: Mutex<Uuid>,
}

impl Session {
    pub fn id(&self) -> Uuid {
        self.request
    }
    /// Acknowledged, unexpired grant ownership; not proof of an active media channel.
    pub fn is_active(&self) -> bool {
        Instant::now() < *self.deadline.lock().unwrap_or_else(|e| e.into_inner())
            && self.active.load(Ordering::Acquire)
            && !*self.stop.borrow()
    }
    pub fn revoke(&self) {
        self.active.store(false, Ordering::Release);
        self.stop.send_replace(true);
    }
    /// Why the agent ended the session itself, if it did.
    /// The agent's transfer reports and chat; only the first caller gets them.
    pub(crate) fn take_inbound(&self) -> Option<mpsc::Receiver<FromAgent>> {
        self.inbound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Passes the operator's chat message to the agent of the active session.
    pub(crate) fn chat(&self, text: String) -> Result<()> {
        if !self.is_active() {
            return Err(AccessError::Offline);
        }
        self.sender
            .try_send(Command::Chat {
                message: wire::Chat {
                    request: self.request.as_bytes().to_vec(),
                    text,
                },
            })
            .map_err(|_| AccessError::Offline)
    }

    pub fn ended_by_agent(&self) -> Option<wire::EndReason> {
        wire::EndReason::try_from(self.ended.load(Ordering::Acquire))
            .ok()
            .filter(|reason| *reason != wire::EndReason::Unspecified)
    }

    pub(crate) fn renewal(&self) -> Option<(&Renewal, Uuid)> {
        let renewal = self.renewal.as_ref()?;
        let grant = *renewal.grant.lock().unwrap_or_else(|e| e.into_inner());
        Some((renewal, grant))
    }

    /// Delivers a renewal; once the agent applied it, the session lives one more lifetime.
    pub(crate) async fn renew(&self, grant: SignedGrant) -> Result<()> {
        let renewal = self.renewal.as_ref().ok_or(AccessError::Offline)?;
        if !self.is_active() {
            return Err(AccessError::Offline);
        }
        let claims = verify(&renewal.authority.public_key(), &grant)
            .map_err(|_| AccessError::Unavailable)?
            .into_claims();
        let lifetime = Duration::from_secs(
            claims
                .lifetime_seconds()
                .map_err(|_| AccessError::Unavailable)?,
        );
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(Command::Renew {
                message: wire::Deliver {
                    request: self.request.as_bytes().to_vec(),
                    grant: grant.encode_to_vec(),
                },
                lifetime,
                reply,
            })
            .map_err(|_| AccessError::Offline)?;
        wait(result, REPLY_LIMIT).await?;
        if !self.active.load(Ordering::Acquire) {
            return Err(AccessError::Offline);
        }
        *renewal.grant.lock().unwrap_or_else(|e| e.into_inner()) = claims.id;
        *self.deadline.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now() + lifetime;
        Ok(())
    }

    pub fn remaining(&self) -> Duration {
        self.deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .saturating_duration_since(Instant::now())
    }

    /// Passes the browser's WebRTC offer to the agent and returns its answer.
    /// Passes the browser's offer on, with the relay credentials the agent may allocate on.
    pub async fn media(&self, sdp: String, relays: Vec<wire::Relay>) -> Result<String> {
        if !self.is_active() || sdp.is_empty() || sdp.len() > wire::MAX_SDP {
            return Err(AccessError::Offline);
        }
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(Command::Media {
                message: wire::MediaOffer {
                    request: self.request.as_bytes().to_vec(),
                    sdp,
                    relays,
                },
                reply,
            })
            .map_err(|_| AccessError::Offline)?;
        let answer = wait(result, REPLY_LIMIT).await?;
        if !self.is_active() {
            return Err(AccessError::Offline);
        }
        Ok(answer)
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

/// The result of an attended request: a granted session, or the local answer that refused it.
pub enum Attended {
    Granted(Session),
    Answered(wire::Consent),
}

/// What an accepted request reserved before it starts: the grant authority, the agent's command
/// channel and the device's single remote-support slot.
pub(crate) struct Reserved {
    authority: Arc<GrantAuthority>,
    sender: mpsc::Sender<Command>,
    slot: OwnedSemaphorePermit,
    os_session: u32,
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
                ended,
                inbound,
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
                let answer = message
                    .consent
                    .as_ref()
                    .map(|consent| Duration::from_secs(consent.timeout_seconds.into()));
                self.flow = Some(Flow {
                    attended: answer.is_some(),
                    request: message.request.clone(),
                    selection: SessionSelection {
                        connection_id: admission.connection_id(),
                        os_session_id: message.session.to_string(),
                        generation: Uuid::from_slice(&message.generation)
                            .expect("validated generation"),
                    },
                    started: now,
                    deadline: now + REPLY_LIMIT + answer.unwrap_or_default(),
                    stop,
                    active,
                    ended,
                    inbound,
                    challenge: Some(reply),
                    delivery: None,
                    media: None,
                    renewal: None,
                    prepared: false,
                });
                Some(Body::RemotePrepare(message))
            }
            Command::Chat { message } => self
                .flow
                .as_ref()
                .filter(|flow| {
                    flow.request == message.request && flow.active.load(Ordering::Acquire)
                })
                .map(|_| Body::RemoteChat(message)),
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
            Command::Media { message, reply } => {
                // Only for the acknowledged, unexpired grant of this request, one offer at a time.
                let Some(flow) = self.flow.as_mut().filter(|flow| {
                    flow.request == message.request
                        && flow.active.load(Ordering::Acquire)
                        && flow.media.is_none()
                        && !*flow.stop.borrow()
                        && flow.stop.has_changed().is_ok()
                        && Instant::now() < flow.deadline
                        && !reply.is_closed()
                }) else {
                    let _ = reply.send(Err(AccessError::Offline));
                    return None;
                };
                flow.media = Some(reply);
                Some(Body::MediaOffer(message))
            }
            Command::Renew {
                message,
                lifetime,
                reply,
            } => {
                let Some(flow) = self.flow.as_mut().filter(|flow| {
                    flow.request == message.request
                        && flow.active.load(Ordering::Acquire)
                        && flow.renewal.is_none()
                        && !*flow.stop.borrow()
                        && flow.stop.has_changed().is_ok()
                        && Instant::now() < flow.deadline
                        && !reply.is_closed()
                }) else {
                    let _ = reply.send(Err(AccessError::Offline));
                    return None;
                };
                flow.renewal = Some((reply, lifetime.min(LIFETIME)));
                Some(Body::RemoteRenew(message))
            }
        }
    }

    /// The agent applied a renewal or refused it; only an applied one moves the deadline.
    pub fn renewed(&mut self, message: wire::Ack) {
        let Some(flow) = self
            .flow
            .as_mut()
            .filter(|flow| flow.request == message.request)
        else {
            return;
        };
        let Some((reply, lifetime)) = flow.renewal.take() else {
            return;
        };
        let applied = message.accepted
            && flow.active.load(Ordering::Acquire)
            && !*flow.stop.borrow()
            && flow.stop.has_changed().is_ok()
            && Instant::now() < flow.deadline
            && !reply.is_closed();
        if applied {
            flow.deadline = Instant::now() + lifetime;
        }
        let _ = reply.send(if applied {
            Ok(())
        } else {
            Err(AccessError::Offline)
        });
    }

    /// The agent ended the session itself; the flow closes without a revocation.
    pub fn ended(&mut self, message: wire::Ended) {
        if let Some(flow) = self
            .flow
            .take_if(|flow| flow.request == message.request && flow.active.load(Ordering::Acquire))
        {
            let reason =
                wire::EndReason::try_from(message.reason).unwrap_or(wire::EndReason::Stopped);
            flow.ended.store(reason as i32, Ordering::Release);
        }
    }

    /// A transfer the agent reports for the active request; anything else is dropped. A session
    /// that reports faster than its audit is written loses the excess, never the connection.
    pub fn transfer(&mut self, message: wire::TransferReport) {
        let request = message.request.clone();
        self.inbound(&request, FromAgent::Transfer(message));
    }

    /// The signed-in user's chat message for the active request; anything else is dropped.
    pub fn chat(&mut self, message: wire::Chat) {
        if wire::chat_valid(&message.text) {
            self.inbound(&message.request, FromAgent::Chat(message.text));
        }
    }

    fn inbound(&self, request: &[u8], message: FromAgent) {
        if let Some(sender) = self
            .flow
            .as_ref()
            .filter(|flow| flow.request == request && flow.active.load(Ordering::Acquire))
            .and_then(|flow| flow.inbound.as_ref())
        {
            let _ = sender.try_send(message);
        }
    }

    pub fn media_answer(&mut self, message: wire::MediaAnswer) {
        let Some(flow) = self
            .flow
            .as_mut()
            .filter(|flow| flow.request == message.request)
        else {
            return;
        };
        let Some(reply) = flow.media.take() else {
            return;
        };
        let _ = reply.send(
            if message.sdp.is_empty() || message.sdp.len() > wire::MAX_SDP {
                Err(AccessError::Unavailable)
            } else {
                Ok(message.sdp)
            },
        );
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
            let consent =
                wire::Consent::try_from(message.consent).map_err(|_| AccessError::Offline)?;
            let nonce = || {
                Uuid::from_slice(&message.challenge)
                    .ok()
                    .filter(|id| !id.is_nil())
                    .ok_or(AccessError::InvalidInput)
            };
            let presence = WirePresence::try_from(message.presence);
            let (presence, nonce) = match (presence, flow.attended, consent) {
                (Ok(WirePresence::Vacant), false, wire::Consent::Unspecified) => {
                    (Presence::Vacant, nonce()?)
                }
                (Ok(WirePresence::Occupied), false, wire::Consent::Unspecified)
                    if message.challenge.is_empty() =>
                {
                    (Presence::Occupied, Uuid::nil())
                }
                (Ok(WirePresence::Occupied), true, wire::Consent::Allowed) => {
                    // The grant lifetime starts with the local answer, not with the request.
                    flow.started = Instant::now();
                    (Presence::Occupied, nonce()?)
                }
                (_, true, answer)
                    if answer != wire::Consent::Allowed && message.challenge.is_empty() =>
                {
                    return Ok(Challenge {
                        nonce: Uuid::nil(),
                        observation: None,
                        started: flow.started,
                        // An agent without consent support ignores the request's consent part.
                        consent: match answer {
                            wire::Consent::Unspecified => wire::Consent::Unavailable,
                            answer => answer,
                        },
                    });
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
                observation: Some(observation),
                started: flow.started,
                consent,
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
        let os_session = os_session(&request.session)?;
        let (stop, stopped) = watch::channel(false);
        let mut session = Session {
            request: id,
            deadline: Mutex::new(Instant::now() + LIFETIME),
            active: Arc::new(AtomicBool::new(false)),
            ended: Arc::new(AtomicI32::new(0)),
            stop,
            sender: sender.clone(),
            renewal: None,
            inbound: Mutex::new(None),
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
                    consent: None,
                    clipboard: request.transfer.clipboard,
                    upload: request.transfer.upload,
                    download: request.transfer.download,
                },
                stop: stopped,
                active: session.active.clone(),
                ended: session.ended.clone(),
                inbound: None,
                reply,
            })
            .map_err(|_| AccessError::Offline)?;
        let challenge = wait(result, REPLY_LIMIT).await?;
        let decision = self
            .authorizer
            .issue_unattended_grant(
                sessions,
                token,
                target,
                request,
                challenge.observation.as_ref(),
                GrantRequest {
                    authority: &authority,
                    challenge: challenge.nonce,
                },
            )
            .await?;
        let GrantDecision::Issued(grant) = decision else {
            return Ok(Decision::LocalConsentRequired);
        };
        send_grant(
            &authority,
            &sender,
            id,
            grant,
            challenge.started,
            &mut session,
        )
        .await?;
        Ok(Decision::Granted(session))
    }

    /// Reserves the device's remote-support slot on the selected agent connection.
    pub(crate) fn reserve(&self, target: Target, selection: &SessionSelection) -> Result<Reserved> {
        let device = target.device.ok_or(AccessError::InvalidInput)?;
        let os_session = os_session(selection)?;
        let (authority, sender, slot) = self.remote_grants.acquire(
            AgentIdentity {
                tenant: target.tenant,
                device,
            },
            selection.connection_id,
        )?;
        Ok(Reserved {
            authority,
            sender,
            slot,
            os_session,
        })
    }

    /// Asks the signed-in user through the agent, and only after an allowed answer signs and
    /// delivers a grant bound to that answer's challenge.
    pub(crate) async fn deliver_attended(
        &self,
        asked: &Asked<'_>,
        requester: &Requester,
        reserved: Reserved,
    ) -> Result<Attended> {
        let Asked {
            target,
            request,
            id,
            ..
        } = *asked;
        let Reserved {
            authority,
            sender,
            slot,
            os_session,
        } = reserved;
        let (stop, stopped) = watch::channel(false);
        // Reports beyond this wait are dropped rather than slowing the agent connection.
        let (inbound_sender, inbound) = mpsc::channel(INBOUND);
        let mut session = Session {
            request: id,
            deadline: Mutex::new(Instant::now() + CONSENT_TIMEOUT + REPLY_LIMIT + LIFETIME),
            active: Arc::new(AtomicBool::new(false)),
            ended: Arc::new(AtomicI32::new(0)),
            stop,
            sender: sender.clone(),
            renewal: None,
            inbound: Mutex::new(Some(inbound)),
            _slot: slot,
        };
        let (reply, result) = oneshot::channel();
        sender
            .try_send(Command::Prepare {
                message: wire::Prepare {
                    request: id.as_bytes().to_vec(),
                    session: os_session,
                    generation: request.session.generation.as_bytes().to_vec(),
                    operator: requester.identity.operator_id.as_bytes().to_vec(),
                    mode: match request.mode {
                        RemoteMode::Normal => wire::Mode::Normal,
                        RemoteMode::Administrative => wire::Mode::Administrative,
                    } as i32,
                    uac: request.uac,
                    consent: Some(wire::ConsentRequest {
                        operator_name: requester.operator_name.clone(),
                        customer_name: requester.customer_name.clone(),
                        timeout_seconds: CONSENT_TIMEOUT.as_secs() as u32,
                    }),
                    clipboard: request.transfer.clipboard,
                    upload: request.transfer.upload,
                    download: request.transfer.download,
                },
                stop: stopped,
                active: session.active.clone(),
                ended: session.ended.clone(),
                inbound: Some(inbound_sender),
                reply,
            })
            .map_err(|_| AccessError::Offline)?;
        let challenge = wait(result, CONSENT_TIMEOUT + REPLY_LIMIT).await?;
        let (outcome, label) = match challenge.consent {
            wire::Consent::Allowed => (Outcome::Success, "allowed"),
            wire::Consent::Declined => (Outcome::Denied, "declined"),
            wire::Consent::Expired => (Outcome::Denied, "expired"),
            wire::Consent::Cancelled => (Outcome::Denied, "cancelled"),
            wire::Consent::Unavailable | wire::Consent::Unspecified => {
                (Outcome::Failure, "unavailable")
            }
        };
        self.authorizer
            .record_attended(
                &requester.identity,
                target,
                request,
                id,
                (Action::RemoteConsent, outcome, label),
            )
            .await?;
        let Some(observation) = challenge
            .observation
            .filter(|_| challenge.consent == wire::Consent::Allowed)
        else {
            return Ok(Attended::Answered(challenge.consent));
        };
        let grant = self
            .authorizer
            .issue_attended_grant(
                asked,
                &observation,
                GrantRequest {
                    authority: &authority,
                    challenge: challenge.nonce,
                },
            )
            .await?;
        let first = verify(&authority.public_key(), &grant)
            .map_err(|_| AccessError::Unavailable)?
            .claims()
            .id;
        let delivered = send_grant(
            &authority,
            &sender,
            id,
            grant,
            challenge.started,
            &mut session,
        )
        .await;
        session.renewal = Some(Box::new(Renewal {
            authority: authority.clone(),
            observation,
            grant: Mutex::new(first),
        }));
        self.authorizer
            .record_attended(
                &requester.identity,
                target,
                request,
                id,
                match delivered {
                    Ok(()) => (
                        Action::RemoteGrantDelivery,
                        Outcome::Success,
                        "acknowledged",
                    ),
                    Err(_) => (
                        Action::RemoteGrantDelivery,
                        Outcome::Failure,
                        "not_delivered",
                    ),
                },
            )
            .await?;
        delivered?;
        Ok(Attended::Granted(session))
    }
}

impl Enrollment {
    /// Renews an active attended session after a fresh check of the operator's authorization.
    pub(crate) async fn renew_attended(&self, asked: &Asked<'_>, session: &Session) -> Result<()> {
        let (renewal, previous) = session.renewal().ok_or(AccessError::Offline)?;
        let grant = self
            .authorizer
            .renew_attended_grant(asked, &renewal.authority, &renewal.observation, previous)
            .await?;
        session.renew(grant).await
    }
}

fn os_session(selection: &SessionSelection) -> Result<u32> {
    let os_session: u32 = selection
        .os_session_id
        .parse()
        .map_err(|_| AccessError::InvalidInput)?;
    if os_session == 0
        || os_session == u32::MAX
        || selection.os_session_id != os_session.to_string()
    {
        return Err(AccessError::InvalidInput);
    }
    Ok(os_session)
}

/// Delivers a signed grant and waits for the agent's acknowledgement within its lifetime.
async fn send_grant(
    authority: &GrantAuthority,
    sender: &mpsc::Sender<Command>,
    id: Uuid,
    grant: SignedGrant,
    started: Instant,
    session: &mut Session,
) -> Result<()> {
    let checked = verify(&authority.public_key(), &grant).map_err(|_| AccessError::Unavailable)?;
    let lifetime = Duration::from_secs(
        checked
            .claims()
            .lifetime_seconds()
            .map_err(|_| AccessError::Unavailable)?,
    );
    *session
        .deadline
        .get_mut()
        .unwrap_or_else(|e| e.into_inner()) = started + lifetime;
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
    wait(result, REPLY_LIMIT).await?;
    if !session.is_active() {
        return Err(AccessError::Offline);
    }
    Ok(())
}

async fn wait<T>(result: oneshot::Receiver<Result<T>>, limit: Duration) -> Result<T> {
    tokio::time::timeout(limit + Duration::from_secs(2), result)
        .await
        .map_err(|_| AccessError::Offline)?
        .map_err(|_| AccessError::Offline)?
}
