//! The service's native backend for remote grants. It observes the selected session from fresh
//! WTS data and, for an accepted lease, runs the matching workers until the lease ends.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use midas_agent_core::remote::{
    Consent, ConsentRequest, DesktopKind, Lease, Observation, Presence,
    transport::{Native, Relay},
};
use midas_protocol::sessions::{OsSession, Presence as WirePresence};
use midas_remote_auth::{Error, Mode, Rights, Target, Transfer};

use super::{
    IO_TIMEOUT, Peer, STARTUP_TIMEOUT,
    bootstrap::{
        attach_privileged, delivery::Control, start_consent, start_pair, start_privileged,
    },
    broker::SuspendedBroker,
    denied,
    launch::{RunningWorker, SuspendedWorker, supervise},
    tokens::PreparedToken,
};
use crate::{
    consent::{Answer, END, INDICATE, Question, decode_answer},
    win::sessions::Observer,
};

/// How long after its deadline the window's own answer may still arrive.
const ANSWER_GRACE: Duration = Duration::from_secs(2);
/// How long an allowed window waits for its session to start before it is closed.
const START_WAIT: Duration = Duration::from_secs(30);

/// The window of an allowed request, kept to become the session's indicator with its End button.
struct Window {
    allowed: std::time::Instant,
    session: u32,
    running: RunningWorker,
    control: Control,
}

type Channel = super::Connection<tokio::net::windows::named_pipe::NamedPipeServer>;

pub struct Coordinator {
    image: PathBuf,
    parent: Peer,
    observer: Arc<Observer>,
    runtime: tokio::runtime::Handle,
    active: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The running ordinary session's companion, which carries its video.
    /// The companion's control channel and the transfer the grant allows.
    media: Arc<tokio::sync::Mutex<Option<(Control, Transfer)>>>,
    window: Mutex<Option<Window>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Ordinary,
    Privileged,
}

impl Coordinator {
    /// `parent` is the service's pinned SYSTEM identity; `runtime` runs the supervision tasks.
    pub fn new(
        image: PathBuf,
        parent: &Peer,
        observer: Arc<Observer>,
        runtime: tokio::runtime::Handle,
    ) -> std::io::Result<Self> {
        Ok(Self {
            image,
            parent: parent.duplicate()?,
            observer,
            runtime,
            active: Mutex::new(None),
            media: Arc::new(tokio::sync::Mutex::new(None)),
            window: Mutex::new(None),
        })
    }

    /// Started workers, plus the service's control channel to a privileged worker or to the
    /// ordinary pair's companion; closing it stops them, so it lives as long as the supervision.
    /// With UAC rights an occupied session also gets the privileged worker, attached to the
    /// companion, for the secure desktop and elevated windows in the same connection.
    fn start(
        &self,
        session: OsSession,
        kind: Kind,
        uac: bool,
    ) -> std::io::Result<(Vec<RunningWorker>, Option<Channel>, Option<Control>)> {
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        match kind {
            Kind::Privileged => {
                let token =
                    PreparedToken::privileged(&self.parent, self.observer.clone(), session)?;
                let worker = SuspendedWorker::spawn(&self.parent, token, &self.image)?;
                let (running, connection) =
                    self.runtime
                        .block_on(start_privileged(&self.parent, worker, deadline))?;
                Ok((vec![running], Some(connection), None))
            }
            Kind::Ordinary => {
                let token =
                    PreparedToken::user(&self.parent, self.observer.clone(), session.clone())?;
                let worker = SuspendedWorker::spawn(&self.parent, token, &self.image)?;
                let companion = SuspendedBroker::spawn(&self.parent, &worker, &self.image)?;
                let identity = companion.peer()?;
                let (pair, mut control) =
                    self.runtime
                        .block_on(start_pair(&self.parent, companion, worker, deadline))?;
                let mut workers: Vec<RunningWorker> = pair.into();
                if uac {
                    let token =
                        PreparedToken::privileged(&self.parent, self.observer.clone(), session)?;
                    let privileged = SuspendedWorker::spawn(&self.parent, token, &self.image)?;
                    let running = self.runtime.block_on(attach_privileged(
                        &self.parent,
                        &identity,
                        &workers[0],
                        &mut control,
                        privileged,
                        deadline,
                    ))?;
                    workers.push(running);
                }
                Ok((workers, None, Some(control)))
            }
        }
    }

    fn busy(&self) -> bool {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    /// Opens the window as the signed-in user of the target session and waits for its answer.
    fn ask(
        &self,
        request: &ConsentRequest,
        stopped: &dyn Fn() -> bool,
    ) -> std::io::Result<Consent> {
        let answer_by = tokio::time::Instant::now() + request.timeout;
        // While a session runs, its technician could answer a second window with remote input.
        if self.busy() {
            return Ok(Consent::Unavailable);
        }
        let Ok(session) = select(request.target, self.observer.collect()) else {
            return Ok(Consent::Unavailable);
        };
        if presence(&session) != Presence::Occupied {
            return Ok(Consent::Unavailable);
        }
        // A window kept for a session that never started closes now.
        drop(self.window.lock().unwrap_or_else(|e| e.into_inner()).take());
        let id = session.id;
        let token = PreparedToken::user(&self.parent, self.observer.clone(), session)?;
        let window = SuspendedWorker::spawn_consent(&self.parent, token, &self.image)?;
        self.runtime.block_on(async {
            let startup = (tokio::time::Instant::now() + STARTUP_TIMEOUT).min(answer_by);
            let (running, mut control) = start_consent(&self.parent, window, startup).await?;
            let seconds = answer_by
                .saturating_duration_since(tokio::time::Instant::now())
                .as_secs();
            if seconds == 0 {
                return Ok(Consent::Expired);
            }
            let question = Question {
                request: request.request,
                operator: request.operator_name.clone(),
                customer: request.customer_name.clone(),
                administrative: request.rights.mode == Mode::Administrative,
                uac: request.rights.uac,
                transfer: request.rights.transfer,
                seconds: seconds as u32,
            }
            .encode()
            .ok_or_else(denied)?;
            let revoked = async {
                while !stopped() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            };
            // The person has until the deadline; the I/O limit applies only to the message itself.
            let frame = tokio::select! {
                frame = control.ask(&question, answer_by + ANSWER_GRACE) => frame,
                () = revoked => return Ok(Consent::Cancelled),
            };
            let answer = match frame {
                Ok(frame) => match decode_answer(&frame) {
                    Some((asked, answer)) if asked == request.request => match answer {
                        Answer::Allow => Consent::Allowed,
                        Answer::Decline => Consent::Declined,
                        Answer::Expired => Consent::Expired,
                    },
                    _ => return Err(denied()),
                },
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Consent::Expired,
                Err(error) => return Err(error),
            };
            if answer == Consent::Allowed {
                *self.window.lock().unwrap_or_else(|e| e.into_inner()) = Some(Window {
                    allowed: std::time::Instant::now(),
                    session: id,
                    running,
                    control,
                });
            }
            Ok(answer)
        })
    }
}

/// Waits for the lease to end. A consented session keeps its indicator: the user's End, or the
/// indicator going away, ends the session at once, since it must not run without that control.
async fn watched(lease: &Lease, window: Option<Window>) {
    let Some(mut window) = window else {
        return lease.ended().await;
    };
    let shown = window
        .control
        .send(INDICATE, tokio::time::Instant::now() + IO_TIMEOUT)
        .await
        .is_ok();
    if shown {
        tokio::select! {
            () = lease.ended() => return,
            ended = window.control.receive() => {
                tracing::info!(pressed = matches!(ended, Ok(Some(ref m)) if m == END), "session indicator ended the session");
            }
        }
    }
    lease.cancellation().end_locally();
    lease.ended().await;
    drop(window.running);
}

/// The session the target names, only while the observer still reports the same generation.
fn select(target: Target, sessions: Option<Vec<OsSession>>) -> Result<OsSession, Error> {
    sessions
        .ok_or(Error::Target)?
        .into_iter()
        .find(|session| {
            session.id == target.session && session.generation == target.generation.as_bytes()
        })
        .ok_or(Error::Target)
}

fn presence(session: &OsSession) -> Presence {
    match WirePresence::try_from(session.presence) {
        Ok(WirePresence::Vacant) => Presence::Vacant,
        Ok(WirePresence::Occupied) => Presence::Occupied,
        _ => Presence::Unknown,
    }
}

/// A vacant session has no user token, so only the privileged worker can serve it, and the grant
/// must allow the privileged route. An occupied desktop keeps the user's own worker.
fn worker_for(presence: Presence, rights: Rights) -> Result<Kind, Error> {
    match presence {
        Presence::Vacant if rights.mode == Mode::Administrative || rights.uac => {
            Ok(Kind::Privileged)
        }
        Presence::Vacant => Err(Error::Desktop),
        Presence::Occupied => Ok(Kind::Ordinary),
        Presence::Unknown => Err(Error::Target),
    }
}

impl Native for Coordinator {
    fn observe(&self, target: Target) -> Result<Observation, Error> {
        let session = select(target, self.observer.collect())?;
        Ok(Observation {
            target,
            presence: presence(&session),
            desktop: DesktopKind::User,
            desktop_epoch: target.generation,
        })
    }

    fn consent(&self, request: &ConsentRequest, stopped: &dyn Fn() -> bool) -> Consent {
        let answer = self.ask(request, stopped).unwrap_or_else(|error| {
            tracing::warn!(request = %request.request, %error, "local consent could not be asked");
            Consent::Unavailable
        });
        tracing::info!(request = %request.request, ?answer, "local consent answered");
        answer
    }

    fn activate(&self, lease: Lease) -> Result<(), Error> {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if active.as_ref().is_some_and(|task| !task.is_finished()) {
            return Err(Error::Target);
        }
        let session = select(lease.target(), self.observer.collect())?;
        let kind = worker_for(presence(&session), lease.rights())?;
        let id = lease.id();
        let uac = lease.rights().uac;
        let (workers, channel, control) = self.start(session, kind, uac).map_err(|error| {
            tracing::warn!(lease = %id, ?kind, %error, "remote support workers could not start");
            Error::Target
        })?;
        tracing::info!(lease = %id, ?kind, uac, "remote support workers started");
        let window = self
            .window
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .filter(|window| {
                window.allowed.elapsed() < START_WAIT && window.session == lease.target().session
            });
        let media = self.media.clone();
        let transfer = lease.rights().transfer;
        *media.blocking_lock() = control.map(|control| (control, transfer));
        *active = Some(self.runtime.spawn(async move {
            match supervise(workers, watched(&lease, window)).await {
                Ok(stopped) => tracing::info!(lease = %id, ?stopped, "remote support workers stopped"),
                Err(error) => {
                    tracing::warn!(lease = %id, %error, "remote support workers did not stop cleanly");
                }
            }
            media.lock().await.take();
            drop(channel);
            drop(lease);
        }));
        Ok(())
    }

    fn media(&self, offer: &str, relays: &[Relay]) -> Result<String, Error> {
        let hosts = crate::win::network::local_addresses().unwrap_or_default();
        // The agent allocates over UDP; the browser may use any relay the server named.
        let relay = midas_agent_core::media::turn::udp_relay(relays);
        let reply = self.runtime.block_on(async {
            let mut media = self.media.lock().await;
            let (control, transfer) = media.as_mut().ok_or(Error::Target)?;
            let request = super::media::offer_message(&hosts, relay.as_ref(), offer, *transfer);
            control
                .exchange(&request, tokio::time::Instant::now() + IO_TIMEOUT)
                .await
                .map_err(|error| {
                    tracing::warn!(%error, "media offer not delivered");
                    Error::Target
                })
        })?;
        super::media::read_answer(&reply).ok_or(Error::Target)
    }
}

#[cfg(test)]
mod tests {
    use midas_remote_auth::Connection;
    use uuid::Uuid;

    use super::*;

    fn target(generation: Uuid) -> Target {
        Target {
            connection: Connection {
                tenant: Uuid::new_v4(),
                device: Uuid::new_v4(),
                id: Uuid::new_v4(),
            },
            session: 2,
            generation,
        }
    }

    fn session(id: u32, generation: Uuid, presence: WirePresence) -> OsSession {
        OsSession {
            id,
            generation: generation.as_bytes().to_vec(),
            presence: presence as i32,
            ..Default::default()
        }
    }

    #[test]
    fn only_the_same_session_generation_is_selected() {
        let generation = Uuid::new_v4();
        let current = session(2, generation, WirePresence::Vacant);
        assert_eq!(
            select(target(generation), Some(vec![current.clone()])),
            Ok(current)
        );
        let replaced = session(2, Uuid::new_v4(), WirePresence::Vacant);
        assert_eq!(
            select(target(generation), Some(vec![replaced])),
            Err(Error::Target)
        );
        let other = session(3, generation, WirePresence::Vacant);
        assert_eq!(
            select(target(generation), Some(vec![other])),
            Err(Error::Target)
        );
        assert_eq!(select(target(generation), None), Err(Error::Target));
    }

    #[test]
    fn presence_maps_unknown_values_to_unknown() {
        let generation = Uuid::new_v4();
        assert_eq!(
            presence(&session(2, generation, WirePresence::Occupied)),
            Presence::Occupied
        );
        let mut invalid = session(2, generation, WirePresence::Vacant);
        invalid.presence = 99;
        assert_eq!(presence(&invalid), Presence::Unknown);
    }

    #[test]
    fn a_vacant_session_needs_the_privileged_route() {
        let normal = Rights {
            mode: Mode::Normal,
            uac: false,
            transfer: midas_remote_auth::Transfer::NONE,
        };
        let administrative = Rights {
            mode: Mode::Administrative,
            uac: false,
            transfer: midas_remote_auth::Transfer::NONE,
        };
        let uac = Rights {
            mode: Mode::Normal,
            uac: true,
            transfer: midas_remote_auth::Transfer::NONE,
        };
        assert_eq!(worker_for(Presence::Vacant, normal), Err(Error::Desktop));
        assert_eq!(
            worker_for(Presence::Vacant, administrative),
            Ok(Kind::Privileged)
        );
        assert_eq!(worker_for(Presence::Vacant, uac), Ok(Kind::Privileged));
        assert_eq!(worker_for(Presence::Occupied, normal), Ok(Kind::Ordinary));
        assert_eq!(worker_for(Presence::Unknown, uac), Err(Error::Target));
    }

    #[test]
    fn administrative_mode_keeps_the_users_own_worker_on_an_occupied_desktop() {
        // Running applications keep their rights: no mode puts the user's desktop under SYSTEM
        // input; only UAC rights add the privileged worker, and only beside the user's own.
        for rights in [
            Rights {
                mode: Mode::Administrative,
                uac: false,
                transfer: midas_remote_auth::Transfer::NONE,
            },
            Rights {
                mode: Mode::Administrative,
                uac: true,
                transfer: midas_remote_auth::Transfer::NONE,
            },
            Rights {
                mode: Mode::Normal,
                uac: true,
                transfer: midas_remote_auth::Transfer::NONE,
            },
        ] {
            assert_eq!(
                worker_for(Presence::Occupied, rights),
                Ok(Kind::Ordinary),
                "{rights:?}"
            );
        }
    }
}
