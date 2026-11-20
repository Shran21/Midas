use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use midas_remote_auth::{Claims, Connection, Error, Mode, Rights, SignedGrant, Target, verify};
use tokio::sync::Notify;
use uuid::Uuid;

pub mod transport;

struct ChannelState {
    identity: Connection,
    key: [u8; 32],
    live: Signal,
}

struct Signal {
    active: AtomicBool,
    changed: Notify,
}

impl Signal {
    fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
            changed: Notify::new(),
        }
    }

    fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn revoke(&self) {
        self.active.store(false, Ordering::Release);
        self.changed.notify_waiters();
    }

    async fn ended(&self) {
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if self.active() {
            changed.await;
        }
    }
}

/// The protected transport must obtain the identity and key from its authenticated server channel.
pub struct Channel(Arc<ChannelState>);
impl Channel {
    pub fn from_authenticated_server(identity: Connection, key: [u8; 32]) -> Result<Self, Error> {
        if !identity.valid() {
            return Err(Error::Invalid);
        }
        Ok(Self(Arc::new(ChannelState {
            identity,
            key,
            live: Signal::new(),
        })))
    }
    pub fn prepare(
        &self,
        target: Target,
        operator: Uuid,
        rights: Rights,
    ) -> Result<Pending, Error> {
        if !self.0.live.active() {
            return Err(Error::Revoked);
        }
        if !target.valid() || target.connection != self.0.identity || operator.is_nil() {
            return Err(Error::Target);
        }
        Ok(Pending {
            channel: Arc::downgrade(&self.0),
            challenge: Uuid::new_v4(),
            target,
            operator,
            rights,
            started: Instant::now(),
            attended: false,
        })
    }
    pub fn revoke(&self) {
        self.0.live.revoke();
    }
}
impl Drop for Channel {
    fn drop(&mut self) {
        self.revoke();
    }
}

pub struct Pending {
    channel: Weak<ChannelState>,
    challenge: Uuid,
    target: Target,
    operator: Uuid,
    rights: Rights,
    started: Instant,
    attended: bool,
}
impl Pending {
    pub fn challenge(&self) -> Uuid {
        self.challenge
    }
    /// Only after the local user allowed this request: the challenge and the grant lifetime start
    /// now, and the lease then serves only the occupied desktop that answered.
    pub(crate) fn consented(self) -> Self {
        Self {
            challenge: Uuid::new_v4(),
            started: Instant::now(),
            attended: true,
            ..self
        }
    }
    pub fn accept(self, grant: &SignedGrant, observed: Observation) -> Result<Lease, Error> {
        self.accept_at(grant, observed, Instant::now())
    }
    fn accept_at(
        self,
        grant: &SignedGrant,
        observed: Observation,
        now: Instant,
    ) -> Result<Lease, Error> {
        let channel = live(&self.channel)?;
        let claims = verify(&channel.key, grant)?.into_claims();
        if claims.challenge != self.challenge
            || claims.target != self.target
            || claims.operator != self.operator
            || claims.rights != self.rights
        {
            return Err(Error::Target);
        }
        let deadline = self
            .started
            .checked_add(Duration::from_secs(claims.lifetime_seconds()?))
            .ok_or(Error::Invalid)?;
        if now < self.started || now >= deadline {
            return Err(Error::Expired);
        }
        let lease = Lease {
            channel: self.channel,
            life: Arc::new(Life {
                current: Mutex::new(Current {
                    id: claims.id,
                    deadline,
                }),
                ending: Mutex::new(None),
                renewed: Notify::new(),
            }),
            claims,
            started: self.started,
            attended: self.attended,
            active: Arc::new(Signal::new()),
        };
        lease.validate(observed, now)?;
        Ok(lease)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Vacant,
    Occupied,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopKind {
    User,
    Secure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Worker {
    User,
    Privileged,
}

/// Fresh native state from the protected broker; never populate this from browser or worker claims.
#[derive(Clone, Copy, Debug)]
pub struct Observation {
    pub target: Target,
    pub presence: Presence,
    pub desktop: DesktopKind,
    pub desktop_epoch: Uuid,
}

pub struct Lease {
    channel: Weak<ChannelState>,
    /// The first grant; renewals keep its target, operator and rights.
    claims: Claims,
    life: Arc<Life>,
    started: Instant,
    attended: bool,
    active: Arc<Signal>,
}

/// Why a lease ended on the agent's side; the server learns it, since nothing else tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The local user stopped the session.
    Local,
    /// The target session or desktop changed.
    Desktop,
    /// No renewal arrived within the grant's lifetime.
    Expired,
    /// The workers stopped, or the server revoked the grant.
    Stopped,
}

struct Current {
    /// The last applied grant; only a renewal naming it as its challenge extends the lease.
    id: Uuid,
    deadline: Instant,
}

struct Life {
    current: Mutex<Current>,
    ending: Mutex<Option<Ending>>,
    renewed: Notify,
}

impl Life {
    fn deadline(&self) -> Instant {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .deadline
    }

    fn record(&self, ending: Ending) {
        self.ending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(ending);
    }

    fn ending(&self) -> Ending {
        self.ending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unwrap_or(Ending::Stopped)
    }
}

/// An attended request as the local consent window shows it. The names come from the
/// authenticated server and are already stripped of control and direction characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentRequest {
    pub request: Uuid,
    pub target: Target,
    pub operator: Uuid,
    pub operator_name: String,
    pub customer_name: String,
    pub rights: Rights,
    pub timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consent {
    Allowed,
    Declined,
    Expired,
    Unavailable,
    Cancelled,
}
#[derive(Clone)]
pub struct Cancellation(Arc<Signal>, Arc<Life>);
impl Cancellation {
    pub fn revoke(&self) {
        self.0.revoke();
    }
    /// The local user's End: capture and input stop at once, and the server learns why.
    pub fn end_locally(&self) {
        self.1.record(Ending::Local);
        self.0.revoke();
    }
}

/// Extends one lease with renewals the server signs while the session lasts.
#[derive(Clone)]
pub struct Renewer {
    channel: Weak<ChannelState>,
    claims: Claims,
    life: Arc<Life>,
    attended: bool,
    active: Arc<Signal>,
}

impl Renewer {
    /// Applies a renewal: it must name the last applied grant as its challenge, keep the
    /// target, operator and rights, and the session must still be the one observed.
    pub fn renew(&self, grant: &SignedGrant, observed: Observation) -> Result<(), Error> {
        self.renew_at(grant, observed, Instant::now())
    }

    fn renew_at(
        &self,
        grant: &SignedGrant,
        observed: Observation,
        now: Instant,
    ) -> Result<(), Error> {
        let channel = live(&self.channel)?;
        if !self.active.active() {
            return Err(Error::Revoked);
        }
        let claims = verify(&channel.key, grant)?.into_claims();
        let lifetime = Duration::from_secs(claims.lifetime_seconds()?);
        if claims.target != self.claims.target
            || claims.operator != self.claims.operator
            || claims.rights != self.claims.rights
        {
            return Err(Error::Target);
        }
        let presence_ok = matches!(
            (observed.presence, self.attended),
            (Presence::Vacant, false) | (Presence::Occupied, true)
        );
        if observed.target != self.claims.target || observed.desktop_epoch.is_nil() || !presence_ok
        {
            self.life.record(Ending::Desktop);
            self.active.revoke();
            return Err(Error::Target);
        }
        let mut current = self.life.current.lock().unwrap_or_else(|e| e.into_inner());
        if claims.challenge != current.id || claims.id == current.id {
            return Err(Error::Target);
        }
        if now >= current.deadline {
            return Err(Error::Expired);
        }
        current.id = claims.id;
        current.deadline = now.checked_add(lifetime).ok_or(Error::Invalid)?;
        drop(current);
        self.life.renewed.notify_waiters();
        Ok(())
    }

    /// The session the lease serves; native code observes it before each renewal.
    pub fn target(&self) -> Target {
        self.claims.target
    }

    /// Waits until the lease ends for any reason, and says why.
    pub async fn ended(&self) -> Ending {
        self.active.ended().await;
        self.life.ending()
    }
}

pub struct InputPermit {
    grant: Uuid,
    desktop: Uuid,
    kind: DesktopKind,
    worker: Worker,
}

fn live(channel: &Weak<ChannelState>) -> Result<Arc<ChannelState>, Error> {
    channel
        .upgrade()
        .filter(|c| c.live.active())
        .ok_or(Error::Revoked)
}

impl Lease {
    pub fn id(&self) -> Uuid {
        self.claims.id
    }
    /// The session the grant names; native code must still observe it before acting.
    pub fn target(&self) -> Target {
        self.claims.target
    }
    pub fn rights(&self) -> Rights {
        self.claims.rights
    }
    /// Whether the local user consented; such a lease never serves a vacant session.
    pub fn attended(&self) -> bool {
        self.attended
    }
    pub fn cancellation(&self) -> Cancellation {
        Cancellation(self.active.clone(), self.life.clone())
    }
    pub fn renewer(&self) -> Renewer {
        Renewer {
            channel: self.channel.clone(),
            claims: self.claims.clone(),
            life: self.life.clone(),
            attended: self.attended,
            active: self.active.clone(),
        }
    }

    /// Wakes native supervision on revocation, channel loss or the current deadline, which
    /// each applied renewal moves.
    /// Execution still requires fresh observation and the per-operation checks below.
    pub async fn ended(&self) {
        if let Some(channel) = self.channel.upgrade() {
            loop {
                let renewed = self.life.renewed.notified();
                tokio::pin!(renewed);
                renewed.as_mut().enable();
                let deadline = self.life.deadline();
                if Instant::now() >= deadline {
                    self.life.record(Ending::Expired);
                    break;
                }
                tokio::select! {
                    _ = self.active.ended() => break,
                    _ = channel.live.ended() => break,
                    _ = tokio::time::sleep_until(deadline.into()) => {},
                    _ = &mut renewed => {},
                }
            }
        }
        self.active.revoke();
    }
    fn validate(&self, observed: Observation, now: Instant) -> Result<(), Error> {
        let result = self.validate_inner(observed, now);
        if let Err(error) = &result {
            self.life.record(match error {
                Error::Expired => Ending::Expired,
                Error::Revoked => Ending::Stopped,
                _ => Ending::Desktop,
            });
            self.active.revoke();
        }
        result
    }
    fn validate_inner(&self, observed: Observation, now: Instant) -> Result<(), Error> {
        live(&self.channel)?;
        if !self.active.active() {
            return Err(Error::Revoked);
        }
        if now < self.started || now >= self.life.deadline() {
            return Err(Error::Expired);
        }
        if observed.target != self.claims.target || observed.desktop_epoch.is_nil() {
            return Err(Error::Target);
        }
        match (observed.presence, self.attended) {
            (Presence::Vacant, false) | (Presence::Occupied, true) => Ok(()),
            (Presence::Occupied, false) => Err(Error::ConsentRequired),
            (Presence::Vacant | Presence::Unknown, _) => Err(Error::Target),
        }
    }
    fn route(&self, observed: Observation, worker: Worker) -> Result<(), Error> {
        self.validate(observed, Instant::now())?;
        if worker == Worker::Privileged
            && self.claims.rights.mode == Mode::Normal
            && !self.claims.rights.uac
        {
            return Err(Error::Desktop);
        }
        if observed.desktop == DesktopKind::Secure
            && (!self.claims.rights.uac || worker != Worker::Privileged)
        {
            return Err(Error::Desktop);
        }
        Ok(())
    }
    pub fn capture(&self, observed: Observation, worker: Worker) -> Result<(), Error> {
        self.route(observed, worker)
    }
    pub fn prepare_input(
        &self,
        observed: Observation,
        worker: Worker,
    ) -> Result<InputPermit, Error> {
        self.route(observed, worker)?;
        Ok(InputPermit {
            grant: self.claims.id,
            desktop: observed.desktop_epoch,
            kind: observed.desktop,
            worker,
        })
    }
    pub fn commit_input(&self, permit: InputPermit, observed: Observation) -> Result<(), Error> {
        self.route(observed, permit.worker)?;
        if permit.grant != self.claims.id
            || permit.desktop != observed.desktop_epoch
            || permit.kind != observed.desktop
        {
            return Err(Error::Desktop);
        }
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.active.revoke();
    }
}

#[cfg(test)]
mod tests;
