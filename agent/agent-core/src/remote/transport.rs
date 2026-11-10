use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use midas_protocol::{Body, Welcome, remote as wire, sessions::Presence as WirePresence};
use midas_remote_auth::{Connection, Error, Mode, Rights, SignedGrant, Target};
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

use super::{Cancellation, Channel, Lease, Observation, Pending, Presence};

/// Implemented only by the protected native broker. It must check the lease at each
/// actual operation and serialize native observation, execution and stopping.
pub trait Native: Send + Sync {
    fn observe(&self, target: Target) -> Result<Observation, Error>;
    fn activate(&self, lease: Lease) -> Result<(), Error>;
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

enum Work {
    Prepare {
        request: Uuid,
        target: Target,
        pending: Pending,
        stop: Arc<Revocation>,
    },
    Deliver {
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
}

impl Engine {
    fn process(&mut self, work: Work, available: bool) -> Body {
        match work {
            Work::Prepare {
                request,
                target,
                pending,
                stop,
            } => {
                self.pending = None;
                let observed = available
                    .then(|| {
                        stop.check()?;
                        let observed = self.native.observe(target)?;
                        stop.check()?;
                        if observed.target != target || observed.desktop_epoch.is_nil() {
                            return Err(Error::Target);
                        }
                        Ok(observed)
                    })
                    .transpose()
                    .ok()
                    .flatten();
                let presence = observed.map_or(WirePresence::Unknown, |o| match o.presence {
                    Presence::Vacant => WirePresence::Vacant,
                    Presence::Occupied => WirePresence::Occupied,
                    Presence::Unknown => WirePresence::Unknown,
                });
                let challenge = if presence == WirePresence::Vacant {
                    let challenge = pending.challenge().as_bytes().to_vec();
                    self.pending = Some(Prepared {
                        request,
                        target,
                        pending,
                        stop,
                    });
                    challenge
                } else {
                    stop.revoke();
                    Vec::new()
                };
                Body::RemoteChallenge(wire::Challenge {
                    request: request.as_bytes().to_vec(),
                    challenge,
                    presence: presence as i32,
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
                    self.native.activate(lease)?;
                    stop.check()
                })()
                .is_ok();
                if !accepted {
                    stop.revoke();
                }
                Body::RemoteAck(wire::Ack {
                    request: request.as_bytes().to_vec(),
                    accepted,
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
            };
            while let Some(work) = inbox.recv().await {
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
        let pending = self
            .channel
            .prepare(target, id(&request.operator)?, rights)?;
        let stop = Arc::new(Revocation::default());
        self.commands
            .try_send(Work::Prepare {
                request: request_id,
                target,
                pending,
                stop: stop.clone(),
            })
            .map_err(|_| Error::Revoked)?;
        self.operation = Some((request_id, stop));
        Ok(())
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

fn id(bytes: &[u8]) -> Result<Uuid, Error> {
    Uuid::from_slice(bytes)
        .ok()
        .filter(|id| !id.is_nil())
        .ok_or(Error::Invalid)
}

pub(crate) async fn next(transport: &mut Option<Transport>) -> Option<Body> {
    match transport {
        Some(transport) => transport.results.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
