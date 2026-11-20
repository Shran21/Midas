use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use midas_protocol::{Body, Welcome, remote as wire, sessions::Presence as WirePresence};
use midas_remote_auth::{Connection, Error, Mode, Rights, SignedGrant, Target};
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

use super::{
    Cancellation, Channel, Consent, ConsentRequest, Ending, Lease, Observation, Pending, Presence,
    Renewer,
};

/// The local answer window and the names it shows stay within these limits.
pub const CONSENT_TIMEOUT: std::ops::RangeInclusive<u32> = 10..=300;
const NAME_LIMIT: usize = 200;

/// Implemented only by the protected native broker. It must check the lease at each
/// actual operation and serialize native observation, execution and stopping.
pub trait Native: Send + Sync {
    fn observe(&self, target: Target) -> Result<Observation, Error>;
    fn activate(&self, lease: Lease) -> Result<(), Error>;
    /// Asks the user signed in to the target session, and only that user, within the timeout.
    /// `stopped` turns true when the request is revoked; the window must then close.
    fn consent(&self, request: &ConsentRequest, stopped: &dyn Fn() -> bool) -> Consent {
        let _ = (request, stopped);
        Consent::Unavailable
    }
    /// Answers the browser's WebRTC offer for the session the last activated lease started; the
    /// relays come with short-lived credentials from the server.
    fn media(&self, offer: &str, relays: &[Relay]) -> Result<String, Error> {
        let _ = (offer, relays);
        Err(Error::Target)
    }
}

/// A TURN relay the server named for one session, with its short-lived credential.
#[derive(Clone, PartialEq, Eq)]
pub struct Relay {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

impl std::fmt::Debug for Relay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relay")
            .field("urls", &self.urls)
            .finish_non_exhaustive()
    }
}

/// Relays the server sent, within bounds; anything else in the list refuses the offer.
fn relays(wire: Vec<wire::Relay>) -> Result<Vec<Relay>, Error> {
    if wire.len() > 4 {
        return Err(Error::Invalid);
    }
    wire.into_iter()
        .map(|relay| {
            let bounded = |text: &str, limit: usize| {
                !text.is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
            };
            if relay.urls.is_empty()
                || relay.urls.len() > 4
                || !relay.urls.iter().all(|url| bounded(url, 256))
                || !bounded(&relay.username, 128)
                || !bounded(&relay.credential, 128)
            {
                return Err(Error::Invalid);
            }
            Ok(Relay {
                urls: relay.urls,
                username: relay.username,
                credential: relay.credential,
            })
        })
        .collect()
}

#[derive(Clone)]
pub struct Backend {
    tenant: Uuid,
    device: Uuid,
    native: Arc<dyn Native>,
    slot: Arc<Semaphore>,
}

impl Backend {
    pub fn new(tenant: Uuid, device: Uuid, native: Arc<dyn Native>) -> Result<Self, Error> {
        if tenant.is_nil() || device.is_nil() {
            return Err(Error::Invalid);
        }
        Ok(Self {
            tenant,
            device,
            native,
            slot: Arc::new(Semaphore::new(1)),
        })
    }
}

#[derive(Default)]
struct Revocation {
    stopped: AtomicBool,
    lease: Mutex<Option<Cancellation>>,
}

impl Revocation {
    fn revoke(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(lease) = self
            .lease
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            lease.revoke();
        }
    }
    fn install(&self, lease: Cancellation) {
        let mut current = self.lease.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopped.load(Ordering::Acquire) {
            lease.revoke();
        }
        *current = Some(lease);
    }
    fn check(&self) -> Result<(), Error> {
        if self.stopped.load(Ordering::Acquire) {
            Err(Error::Revoked)
        } else {
            Ok(())
        }
    }
}

struct Attended {
    operator: Uuid,
    operator_name: String,
    customer_name: String,
    rights: Rights,
    timeout: Duration,
}

enum Work {
    Prepare {
        request: Uuid,
        target: Target,
        pending: Pending,
        attended: Option<Box<Attended>>,
        stop: Arc<Revocation>,
    },
    Deliver {
        request: Uuid,
        grant: SignedGrant,
        stop: Arc<Revocation>,
    },
    Media {
        request: Uuid,
        offer: String,
        relays: Vec<Relay>,
        stop: Arc<Revocation>,
    },
    Renew {
        request: Uuid,
        grant: SignedGrant,
        stop: Arc<Revocation>,
    },
}

struct Prepared {
    request: Uuid,
    target: Target,
    pending: Pending,
    stop: Arc<Revocation>,
}

struct Engine {
    native: Arc<dyn Native>,
    pending: Option<Prepared>,
    /// The request whose grant the native backend accepted last.
    active: Option<Uuid>,
    /// Renews that request's lease while it lasts.
    renewer: Option<(Uuid, Renewer)>,
}

impl Engine {
    fn process(&mut self, work: Work, available: bool) -> Body {
        match work {
            Work::Prepare {
                request,
                target,
                pending,
                attended,
                stop,
            } => {
                self.pending = None;
                let observe = |native: &dyn Native| {
                    stop.check()?;
                    let observed = native.observe(target)?;
                    stop.check()?;
                    if observed.target != target || observed.desktop_epoch.is_nil() {
                        return Err(Error::Target);
                    }
                    Ok(observed)
                };
                let observed = available
                    .then(|| observe(self.native.as_ref()))
                    .transpose()
                    .ok()
                    .flatten();
                let presence = observed.map_or(Presence::Unknown, |o| o.presence);
                let asked = attended.is_some();
                let (pending, consent) = match (presence, attended) {
                    (Presence::Vacant, None) => (Some(pending), Consent::Unavailable),
                    (Presence::Occupied, Some(attended)) => {
                        let question = ConsentRequest {
                            request,
                            target,
                            operator: attended.operator,
                            operator_name: attended.operator_name,
                            customer_name: attended.customer_name,
                            rights: attended.rights,
                            timeout: attended.timeout,
                        };
                        let answer = self.native.consent(&question, &|| stop.check().is_err());
                        // The answer counts only for the same occupied desktop, still unrevoked.
                        let current = observe(self.native.as_ref()).ok().filter(|now| {
                            observed.is_some_and(|before| {
                                now.presence == Presence::Occupied
                                    && now.desktop_epoch == before.desktop_epoch
                            })
                        });
                        match answer {
                            Consent::Allowed if current.is_some() => {
                                (Some(pending.consented()), Consent::Allowed)
                            }
                            Consent::Allowed => (None, Consent::Cancelled),
                            other => (None, other),
                        }
                    }
                    (_, Some(_)) => (None, Consent::Unavailable),
                    (_, None) => (None, Consent::Unavailable),
                };
                let challenge = match pending {
                    Some(pending) => {
                        let challenge = pending.challenge().as_bytes().to_vec();
                        self.pending = Some(Prepared {
                            request,
                            target,
                            pending,
                            stop,
                        });
                        challenge
                    }
                    None => {
                        stop.revoke();
                        Vec::new()
                    }
                };
                Body::RemoteChallenge(wire::Challenge {
                    request: request.as_bytes().to_vec(),
                    challenge,
                    presence: match presence {
                        Presence::Vacant => WirePresence::Vacant,
                        Presence::Occupied => WirePresence::Occupied,
                        Presence::Unknown => WirePresence::Unknown,
                    } as i32,
                    consent: if asked {
                        match consent {
                            Consent::Allowed => wire::Consent::Allowed,
                            Consent::Declined => wire::Consent::Declined,
                            Consent::Expired => wire::Consent::Expired,
                            Consent::Unavailable => wire::Consent::Unavailable,
                            Consent::Cancelled => wire::Consent::Cancelled,
                        }
                    } else {
                        wire::Consent::Unspecified
                    } as i32,
                })
            }
            Work::Deliver {
                request,
                grant,
                stop,
            } => {
                let accepted = (|| {
                    stop.check()?;
                    let prepared = self.pending.take().ok_or(Error::Revoked)?;
                    if !available
                        || prepared.request != request
                        || !Arc::ptr_eq(&prepared.stop, &stop)
                    {
                        return Err(Error::Target);
                    }
                    let observed = self.native.observe(prepared.target)?;
                    stop.check()?;
                    let lease = prepared.pending.accept(&grant, observed)?;
                    stop.install(lease.cancellation());
                    stop.check()?;
                    let renewer = lease.renewer();
                    self.native.activate(lease)?;
                    stop.check()?;
                    Ok::<_, Error>(renewer)
                })();
                let accepted = accepted.map(|renewer| self.renewer = Some((request, renewer)));
                let accepted = accepted.is_ok();
                if accepted {
                    self.active = Some(request);
                } else {
                    stop.revoke();
                }
                Body::RemoteAck(wire::Ack {
                    request: request.as_bytes().to_vec(),
                    accepted,
                })
            }
            Work::Renew {
                request,
                grant,
                stop,
            } => {
                let accepted = (|| {
                    stop.check()?;
                    let (_, renewer) = self
                        .renewer
                        .as_ref()
                        .filter(|(current, _)| *current == request)
                        .ok_or(Error::Revoked)?;
                    if !available {
                        return Err(Error::Target);
                    }
                    let observed = self.native.observe(renewer.target())?;
                    stop.check()?;
                    renewer.renew(&grant, observed)
                })()
                .is_ok();
                Body::RemoteRenewed(wire::Ack {
                    request: request.as_bytes().to_vec(),
                    accepted,
                })
            }
            Work::Media {
                request,
                offer,
                relays,
                stop,
            } => {
                let answer = (self.active == Some(request) && stop.check().is_ok())
                    .then(|| self.native.media(&offer, &relays).ok())
                    .flatten()
                    .filter(|answer| answer.len() <= wire::MAX_SDP && stop.check().is_ok())
                    .unwrap_or_default();
                Body::MediaAnswer(wire::MediaAnswer {
                    request: request.as_bytes().to_vec(),
                    sdp: answer,
                })
            }
        }
    }
}

struct RevokeChannel(Arc<Channel>);
impl Drop for RevokeChannel {
    fn drop(&mut self) {
        self.0.revoke();
    }
}

enum Next {
    Work(Box<Work>),
    Ended(Ending),
    Closed,
}

async fn ended(watched: &Option<(Uuid, Renewer)>) -> Ending {
    match watched {
        Some((_, renewer)) => renewer.ended().await,
        None => std::future::pending().await,
    }
}

fn end_reason(ending: Ending) -> wire::EndReason {
    match ending {
        Ending::Local => wire::EndReason::Local,
        Ending::Desktop => wire::EndReason::Desktop,
        Ending::Expired => wire::EndReason::Expired,
        Ending::Stopped => wire::EndReason::Stopped,
    }
}

pub(crate) struct Transport {
    connection: Connection,
    channel: Arc<Channel>,
    operation: Option<(Uuid, Arc<Revocation>)>,
    commands: mpsc::Sender<Work>,
    results: mpsc::Receiver<Body>,
    task: tokio::task::JoinHandle<()>,
}

impl Transport {
    pub fn connect(backend: &Backend, welcome: &Welcome) -> Result<Option<Self>, Error> {
        let Some(context) = &welcome.remote else {
            return Ok(None);
        };
        let connection = Connection {
            tenant: id(&context.tenant)?,
            device: id(&context.device)?,
            id: id(&welcome.connection_id)?,
        };
        if connection.tenant != backend.tenant || connection.device != backend.device {
            return Err(Error::Target);
        }
        let key = context
            .public_key
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid)?;
        let channel = Arc::new(Channel::from_authenticated_server(connection, key)?);
        let (commands, mut inbox) = mpsc::channel(1);
        let (outbox, results) = mpsc::channel(1);
        let backend = backend.clone();
        let lifetime = RevokeChannel(channel.clone());
        let task = tokio::spawn(async move {
            let _lifetime = lifetime;
            let mut engine = Engine {
                native: backend.native,
                pending: None,
                active: None,
                renewer: None,
            };
            // The active lease is watched beside the work, so its end reaches the server even
            // when nothing else is asked.
            let mut watched: Option<(Uuid, Renewer)> = None;
            loop {
                let next = tokio::select! {
                    work = inbox.recv() => work.map_or(Next::Closed, |work| Next::Work(Box::new(work))),
                    ending = ended(&watched) => Next::Ended(ending),
                };
                let work = match next {
                    Next::Work(work) => *work,
                    Next::Closed => break,
                    Next::Ended(ending) => {
                        let Some((request, _)) = watched.take() else {
                            continue;
                        };
                        if engine
                            .renewer
                            .as_ref()
                            .is_some_and(|(current, _)| *current == request)
                        {
                            engine.renewer = None;
                        }
                        let ended = Body::RemoteEnded(wire::Ended {
                            request: request.as_bytes().to_vec(),
                            reason: end_reason(ending) as i32,
                        });
                        if outbox.send(ended).await.is_err() {
                            break;
                        }
                        continue;
                    }
                };
                let output = if let Ok(permit) = backend.slot.clone().try_acquire_owned() {
                    let result = tokio::task::spawn_blocking(move || {
                        let output = engine.process(work, true);
                        drop(permit);
                        (engine, output)
                    })
                    .await;
                    let Ok((next, output)) = result else {
                        break;
                    };
                    engine = next;
                    output
                } else {
                    engine.process(work, false)
                };
                if outbox.send(output).await.is_err() {
                    break;
                }
                if let Some((request, renewer)) = &engine.renewer
                    && watched
                        .as_ref()
                        .is_none_or(|(watching, _)| watching != request)
                {
                    watched = Some((*request, renewer.clone()));
                }
            }
        });
        Ok(Some(Self {
            connection,
            channel,
            operation: None,
            commands,
            results,
            task,
        }))
    }

    pub fn prepare(&mut self, request: wire::Prepare) -> Result<(), Error> {
        let request_id = id(&request.request)?;
        if self.operation.is_some() {
            return Err(Error::Invalid);
        }
        let target = Target {
            connection: self.connection,
            session: request.session,
            generation: id(&request.generation)?,
        };
        let rights = Rights {
            mode: match wire::Mode::try_from(request.mode) {
                Ok(wire::Mode::Normal) => Mode::Normal,
                Ok(wire::Mode::Administrative) => Mode::Administrative,
                _ => return Err(Error::Invalid),
            },
            uac: request.uac,
        };
        let operator = id(&request.operator)?;
        let attended = request
            .consent
            .map(|consent| {
                if !CONSENT_TIMEOUT.contains(&consent.timeout_seconds) {
                    return Err(Error::Invalid);
                }
                Ok(Box::new(Attended {
                    operator,
                    operator_name: display_name(&consent.operator_name)?,
                    customer_name: display_name(&consent.customer_name)?,
                    rights,
                    timeout: Duration::from_secs(consent.timeout_seconds.into()),
                }))
            })
            .transpose()?;
        let pending = self.channel.prepare(target, operator, rights)?;
        let stop = Arc::new(Revocation::default());
        self.commands
            .try_send(Work::Prepare {
                request: request_id,
                target,
                pending,
                attended,
                stop: stop.clone(),
            })
            .map_err(|_| Error::Revoked)?;
        self.operation = Some((request_id, stop));
        Ok(())
    }

    /// Passes a renewal on, only for the current request.
    pub fn renew(&mut self, renewal: wire::Deliver) -> Result<(), Error> {
        let request = id(&renewal.request)?;
        let Some((current, stop)) = &self.operation else {
            return Ok(());
        };
        if *current != request {
            return Ok(());
        }
        let grant = SignedGrant::decode_checked(&renewal.grant)?;
        self.commands
            .try_send(Work::Renew {
                request,
                grant,
                stop: stop.clone(),
            })
            .map_err(|_| Error::Revoked)
    }

    pub fn deliver(&mut self, delivery: wire::Deliver) -> Result<(), Error> {
        let request = id(&delivery.request)?;
        let Some((current, stop)) = &self.operation else {
            return Ok(());
        };
        if *current != request {
            return Ok(());
        }
        let grant = SignedGrant::decode_checked(&delivery.grant)?;
        self.commands
            .try_send(Work::Deliver {
                request,
                grant,
                stop: stop.clone(),
            })
            .map_err(|_| Error::Revoked)
    }

    /// Passes the browser's offer on, only for the current request; a busy engine refuses it.
    pub fn media(&mut self, offer: wire::MediaOffer) -> Result<(), Error> {
        let request = id(&offer.request)?;
        if offer.sdp.is_empty() || offer.sdp.len() > wire::MAX_SDP {
            return Err(Error::Invalid);
        }
        let Some((current, stop)) = &self.operation else {
            return Err(Error::Revoked);
        };
        if *current != request {
            return Err(Error::Target);
        }
        self.commands
            .try_send(Work::Media {
                request,
                relays: relays(offer.relays)?,
                offer: offer.sdp,
                stop: stop.clone(),
            })
            .map_err(|_| Error::Revoked)
    }

    pub fn revoke(&mut self, request: wire::Revoke) -> Result<(), Error> {
        let request = id(&request.request)?;
        if self
            .operation
            .as_ref()
            .is_some_and(|(current, _)| *current == request)
            && let Some((_, stop)) = self.operation.take()
        {
            stop.revoke();
        }
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.channel.revoke();
        if let Some((_, stop)) = &self.operation {
            stop.revoke();
        }
        self.task.abort();
    }
}

/// Drops control, direction and invisible formatting characters, so a name cannot reorder or
/// hide the text around it in the consent window.
fn display_name(text: &str) -> Result<String, Error> {
    let kept: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| {
            !c.is_control()
                && !matches!(
                    c,
                    '\u{00ad}'
                        | '\u{061c}'
                        | '\u{180e}'
                        | '\u{200b}'..='\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2060}'..='\u{206f}'
                        | '\u{feff}'
                        | '\u{fff9}'..='\u{fffb}'
                )
        })
        .collect();
    let name = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > NAME_LIMIT {
        return Err(Error::Invalid);
    }
    Ok(name)
}

fn id(bytes: &[u8]) -> Result<Uuid, Error> {
    Uuid::from_slice(bytes)
        .ok()
        .filter(|id| !id.is_nil())
        .ok_or(Error::Invalid)
}

pub(crate) async fn next(transport: &mut Option<Transport>) -> Option<Body> {
    match transport {
        Some(transport) => {
            let body = transport.results.recv().await;
            // A session the agent ended itself is over: the next request may start.
            if let Some(Body::RemoteEnded(ended)) = &body
                && transport
                    .operation
                    .as_ref()
                    .is_some_and(|(current, _)| current.as_bytes().as_slice() == ended.request)
                && let Some((_, stop)) = transport.operation.take()
            {
                stop.revoke();
            }
            body
        }
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
