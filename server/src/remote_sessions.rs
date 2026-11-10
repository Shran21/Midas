use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use midas_protocol::sessions::{Presence, SessionReport, SessionRequest, valid_sessions};
use serde::Serialize;
use tokio::sync::{Semaphore, mpsc, oneshot};
use uuid::Uuid;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::sessions::{SessionToken, Sessions},
    authorization::{
        AccessError, Permission, Target,
        unattended::{
            Presence as ObservedPresence, SessionObservation, SessionSelection, UnattendedDecision,
            UnattendedRequest,
        },
    },
    database::driver as sqlx,
    enrollment::{Admission, AgentIdentity, Enrollment},
};

const DEADLINE: Duration = Duration::from_secs(8);
type Result<T> = std::result::Result<T, AccessError>;

#[derive(Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<HashMap<Uuid, Entry>>>);

struct Entry {
    identity: AgentIdentity,
    sender: mpsc::Sender<Request>,
    slot: Arc<Semaphore>,
}

pub(crate) struct Registration {
    registry: Registry,
    connection: Uuid,
    requests: mpsc::Receiver<Request>,
}

pub(crate) struct Request {
    pub id: Uuid,
    reply: oneshot::Sender<Result<Snapshot>>,
}

pub(crate) struct Pending {
    request: Request,
    started: Instant,
    pub deadline: tokio::time::Instant,
}

struct Snapshot {
    connection: Uuid,
    credential: Vec<u8>,
    view: SessionsView,
    observations: Vec<SessionObservation>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum UnattendedCheck {
    LocalConsentRequired,
    Eligible {
        session: SessionSelection,
        expires_at: i64,
    },
}

#[derive(Debug, Serialize)]
pub struct SessionsView {
    pub connection_id: Uuid,
    pub sessions: Vec<SessionView>,
}

#[derive(Debug, Serialize)]
pub struct SessionView {
    pub selection: SessionSelection,
    pub presence: &'static str,
    pub account: String,
    pub station: String,
    pub console: bool,
    pub locked: Option<bool>,
}

impl Registry {
    pub fn register(&self, admission: &Admission) -> Registration {
        let (sender, requests) = mpsc::channel(1);
        let connection = admission.connection_id();
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(
            connection,
            Entry {
                identity: admission.identity,
                sender,
                slot: Arc::new(Semaphore::new(1)),
            },
        );
        Registration {
            registry: self.clone(),
            connection,
            requests,
        }
    }

    async fn query(&self, identity: AgentIdentity) -> Result<Snapshot> {
        let (sender, slot) = {
            let entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
            let mut matching = entries.values().filter(|entry| entry.identity == identity);
            let entry = matching.next().ok_or(AccessError::Offline)?;
            if matching.next().is_some() {
                return Err(AccessError::Conflict);
            }
            (entry.sender.clone(), entry.slot.clone())
        };
        let _permit = slot
            .try_acquire_owned()
            .map_err(|_| AccessError::Conflict)?;
        let (reply, result) = oneshot::channel();
        sender
            .try_send(Request {
                id: Uuid::new_v4(),
                reply,
            })
            .map_err(|_| AccessError::Offline)?;
        tokio::time::timeout(DEADLINE + Duration::from_secs(2), result)
            .await
            .map_err(|_| AccessError::Offline)?
            .map_err(|_| AccessError::Offline)?
    }

    fn active(&self, identity: AgentIdentity, connection: Uuid) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&connection)
            .is_some_and(|entry| entry.identity == identity && !entry.sender.is_closed())
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

impl Pending {
    pub fn start(request: Request, admission: &Admission) -> Option<(Self, SessionRequest)> {
        if request.reply.is_closed() {
            return None;
        }
        let message = SessionRequest {
            request_id: request.id.as_bytes().to_vec(),
            connection_id: admission.connection_id().as_bytes().to_vec(),
        };
        Some((
            Self {
                request,
                started: Instant::now(),
                deadline: tokio::time::Instant::now() + DEADLINE,
            },
            message,
        ))
    }

    pub fn matches(&self, report: &SessionReport, admission: &Admission) -> bool {
        report.request_id == self.request.id.as_bytes()
            && report.connection_id == admission.connection_id().as_bytes()
    }

    pub fn finish(self, report: SessionReport, admission: &Admission) {
        let valid = self.matches(&report, admission)
            && self.started.elapsed() < DEADLINE
            && !admission.signal.borrow().revoked
            && report.available
            && valid_sessions(&report.sessions);
        let result = if valid {
            let connection = admission.connection_id();
            let sessions: Vec<_> = report
                .sessions
                .into_iter()
                .map(|session| SessionView {
                    selection: SessionSelection {
                        connection_id: connection,
                        os_session_id: session.id.to_string(),
                        generation: Uuid::from_slice(&session.generation)
                            .expect("validated generation"),
                    },
                    presence: match Presence::try_from(session.presence) {
                        Ok(Presence::Vacant) => "vacant",
                        Ok(Presence::Occupied) => "occupied",
                        _ => "unknown",
                    },
                    account: session.account,
                    station: session.station,
                    console: session.console,
                    locked: session.locked,
                })
                .collect();
            let observations = sessions
                .iter()
                .map(|session| {
                    SessionObservation::from_request(
                        admission,
                        session.selection.clone(),
                        match session.presence {
                            "vacant" => ObservedPresence::Vacant,
                            "occupied" => ObservedPresence::Occupied,
                            _ => ObservedPresence::Unknown,
                        },
                        self.started,
                    )
                })
                .collect::<Result<Vec<_>>>();
            observations.map(|observations| Snapshot {
                connection,
                credential: admission.credential_fingerprint.clone(),
                view: SessionsView {
                    connection_id: connection,
                    sessions,
                },
                observations,
            })
        } else {
            Err(AccessError::Offline)
        };
        let _ = self.request.reply.send(result);
    }

    pub fn expire(self) {
        let _ = self.request.reply.send(Err(AccessError::Offline));
    }
}

pub(crate) async fn next(registration: &mut Option<Registration>) -> Option<Request> {
    match registration {
        Some(registration) => registration.requests.recv().await,
        None => std::future::pending().await,
    }
}

pub(crate) async fn expired(pending: &Option<Pending>) {
    match pending {
        Some(pending) => tokio::time::sleep_until(pending.deadline).await,
        None => std::future::pending().await,
    }
}

impl Enrollment {
    pub async fn check_unattended(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
        request: &UnattendedRequest,
    ) -> Result<UnattendedCheck> {
        let target = Target {
            tenant,
            device: Some(device),
        };
        let operator = self
            .authorizer
            .prepare_unattended_request(sessions, token, target, request)
            .await?;
        let identity = AgentIdentity { tenant, device };
        if !self
            .remote_sessions
            .active(identity, request.session.connection_id)
        {
            self.authorizer
                .record_unattended_refusal(&operator, target, request, &AccessError::Offline)
                .await?;
            return Err(AccessError::Offline);
        }
        let snapshot = match self.remote_sessions.query(identity).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.authorizer
                    .record_unattended_refusal(&operator, target, request, &error)
                    .await?;
                return Err(error);
            }
        };
        let observation = snapshot
            .view
            .sessions
            .iter()
            .position(|session| session.selection == request.session)
            .and_then(|index| snapshot.observations.get(index));
        let decision = self
            .authorizer
            .check_unattended_request(sessions, token, target, request, observation)
            .await?;
        if !self.remote_sessions.active(identity, snapshot.connection) {
            return Err(AccessError::Offline);
        }
        Ok(match decision {
            UnattendedDecision::LocalConsentRequired => UnattendedCheck::LocalConsentRequired,
            UnattendedDecision::Eligible {
                session,
                expires_at,
                ..
            } => UnattendedCheck::Eligible {
                session,
                expires_at,
            },
        })
    }

    pub async fn remote_sessions(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<SessionsView> {
        let target = Target {
            tenant,
            device: Some(device),
        };
        self.authorizer
            .authorize(sessions, token, target, Permission::DeviceRead)
            .await?;
        self.authorizer
            .authorize(sessions, token, target, Permission::RemoteView)
            .await?;
        let identity = AgentIdentity { tenant, device };
        let snapshot = self.remote_sessions.query(identity).await?;
        let operator = sessions.validate(token, false).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT set_config('midas.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        crate::authorization::Authorizer::evaluate_in(
            &mut tx,
            &operator,
            target,
            &[Permission::DeviceRead, Permission::RemoteView],
            true,
        )
        .await?;
        let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.devices d JOIN midas.agent_credentials c ON c.tenant_id=d.tenant_id AND c.device_id=d.id WHERE d.id=$1 AND d.tenant_id=$2 AND d.enrollment='active' AND c.fingerprint=$3 AND c.revoked_at IS NULL AND c.expires_at>clock_timestamp())")
            .bind(device).bind(tenant).bind(&snapshot.credential).fetch_one(&mut *tx).await?;
        if !active || !self.remote_sessions.active(identity, snapshot.connection) {
            return Err(AccessError::Offline);
        }
        let mut event = Event::new(
            Action::RemoteSessionsRead,
            Outcome::Success,
            Actor::session(&operator),
        )
        .detail("connection_id", snapshot.connection.to_string())
        .detail("count", snapshot.view.sessions.len());
        event.tenant = Some(tenant);
        event.device = Some(device);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        if !self.remote_sessions.active(identity, snapshot.connection) {
            return Err(AccessError::Offline);
        }
        Ok(snapshot.view)
    }
}
