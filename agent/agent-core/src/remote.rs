use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use midas_remote_auth::{Claims, Connection, Error, Mode, Rights, SignedGrant, Target, verify};
use uuid::Uuid;

struct ChannelState {
    identity: Connection,
    key: [u8; 32],
    live: AtomicBool,
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
            live: AtomicBool::new(true),
        })))
    }
    pub fn prepare(
        &self,
        target: Target,
        operator: Uuid,
        rights: Rights,
    ) -> Result<Pending, Error> {
        if !self.0.live.load(Ordering::Acquire) {
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
        self.0.live.store(false, Ordering::Release);
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
            active: Arc::new(AtomicBool::new(true)),
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
    active: Arc<AtomicBool>,
}
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn revoke(&self) {
        self.0.store(false, Ordering::Release);
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
        .filter(|c| c.live.load(Ordering::Acquire))
        .ok_or(Error::Revoked)
}

impl Lease {
    pub fn id(&self) -> Uuid {
        self.claims.id
    }
    pub fn cancellation(&self) -> Cancellation {
        Cancellation(self.active.clone())
    }
    fn validate(&self, observed: Observation, now: Instant) -> Result<(), Error> {
        let result = self.validate_inner(observed, now);
        if result.is_err() {
            self.active.store(false, Ordering::Release);
        }
        result
    }
    fn validate_inner(&self, observed: Observation, now: Instant) -> Result<(), Error> {
        live(&self.channel)?;
        if !self.active.load(Ordering::Acquire) {
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
        self.active.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
