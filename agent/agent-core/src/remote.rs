use std::{
    sync::{
        Arc, Weak,
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
}
impl Pending {
    pub fn challenge(&self) -> Uuid {
        self.challenge
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
            claims,
            deadline,
            started: self.started,
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
    claims: Claims,
    deadline: Instant,
    started: Instant,
    active: Arc<Signal>,
}
#[derive(Clone)]
pub struct Cancellation(Arc<Signal>);
impl Cancellation {
    pub fn revoke(&self) {
        self.0.revoke();
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
    pub fn cancellation(&self) -> Cancellation {
        Cancellation(self.active.clone())
    }

    /// Wakes native supervision on revocation, channel loss or the original local deadline.
    /// Execution still requires fresh observation and the per-operation checks below.
    pub async fn ended(&self) {
        if let Some(channel) = self.channel.upgrade() {
            tokio::select! {
                _ = self.active.ended() => {},
                _ = channel.live.ended() => {},
                _ = tokio::time::sleep_until(self.deadline.into()) => {},
            }
        }
        self.active.revoke();
    }
    fn validate(&self, observed: Observation, now: Instant) -> Result<(), Error> {
        let result = self.validate_inner(observed, now);
        if result.is_err() {
            self.active.revoke();
        }
        result
    }
    fn validate_inner(&self, observed: Observation, now: Instant) -> Result<(), Error> {
        live(&self.channel)?;
        if !self.active.active() {
            return Err(Error::Revoked);
        }
        if now < self.started || now >= self.deadline {
            return Err(Error::Expired);
        }
        if observed.target != self.claims.target || observed.desktop_epoch.is_nil() {
            return Err(Error::Target);
        }
        match observed.presence {
            Presence::Occupied => Err(Error::ConsentRequired),
            Presence::Unknown => Err(Error::Target),
            Presence::Vacant => Ok(()),
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
