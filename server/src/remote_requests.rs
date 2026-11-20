//! Attended remote-support requests started from the admin panel. Each request runs in its own
//! task, which owns the grant session; the panel only reads the status and may cancel it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

use midas_protocol::remote as wire;
use serde::Serialize;
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    audit::{Action, Actor, Event, Outcome},
    auth::sessions::{SessionToken, Sessions},
    authorization::{
        AccessError, Permission, RemoteMode, Target,
        attended::{Asked, AttendedRequest, CONSENT_TIMEOUT, refusal},
    },
    enrollment::Enrollment,
    remote_grants::{Attended, Session},
};

type Result<T> = std::result::Result<T, AccessError>;
/// Unfinished requests one operator may have at a time.
const OPERATOR_LIMIT: usize = 4;
/// Finished requests stay readable this long, so the panel sees the final state.
const RETAIN: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// The local window is open until `answer_by` (Unix seconds).
    Waiting {
        answer_by: i64,
    },
    /// The grant is acknowledged until `until` (Unix seconds).
    Active {
        until: i64,
    },
    Declined,
    Expired,
    /// Nobody could be asked: no signed-in user, no consent component or a session in progress.
    Unavailable,
    Cancelled,
    Ended,
    Failed {
        reason: &'static str,
    },
}

impl Status {
    fn finished(self) -> bool {
        !matches!(self, Self::Waiting { .. } | Self::Active { .. })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestView {
    pub id: Uuid,
    pub mode: RemoteMode,
    pub uac: bool,
    pub os_session_id: String,
    #[serde(flatten)]
    pub status: Status,
}

struct Tracked {
    tenant: Uuid,
    device: Uuid,
    operator: Uuid,
    mode: RemoteMode,
    uac: bool,
    os_session_id: String,
    status: watch::Receiver<Status>,
    cancel: watch::Sender<bool>,
    created: Instant,
    /// The granted session while it is active, for its single media setup.
    session: Option<Weak<Session>>,
    offered: bool,
}

impl Tracked {
    fn view(&self, id: Uuid) -> RequestView {
        RequestView {
            id,
            mode: self.mode,
            uac: self.uac,
            os_session_id: self.os_session_id.clone(),
            status: *self.status.borrow(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Requests(Arc<Mutex<HashMap<Uuid, Tracked>>>);

impl Requests {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Tracked>> {
        let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|_, entry| {
            !entry.status.borrow().finished() || entry.created.elapsed() < RETAIN
        });
        entries
    }

    fn find(&self, id: Uuid, tenant: Uuid, device: Uuid, operator: Uuid) -> Option<RequestView> {
        self.entries()
            .get(&id)
            .filter(|e| e.tenant == tenant && e.device == device && e.operator == operator)
            .map(|entry| entry.view(id))
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    if cancel.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await;
    }
}

impl Enrollment {
    /// Starts an attended request; the device's single remote-support slot is reserved before
    /// this returns, so a busy or offline device is refused at once.
    pub async fn request_attended<S>(
        self: &Arc<Self>,
        sessions: Arc<S>,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
        request: AttendedRequest,
    ) -> Result<RequestView>
    where
        S: AsRef<Sessions> + Send + Sync + 'static,
    {
        let target = Target {
            tenant,
            device: Some(device),
        };
        let id = Uuid::new_v4();
        let requester = self
            .authorizer
            .prepare_attended_request((*sessions).as_ref(), token, target, &request, id)
            .await?;
        let operator = requester.identity.operator_id;
        let refuse = async |error: AccessError| {
            self.authorizer
                .record_attended(
                    &requester.identity,
                    target,
                    &request,
                    id,
                    (Action::RemoteRequest, Outcome::Denied, refusal(&error)),
                )
                .await?;
            Err(error)
        };
        let unfinished = self
            .remote_requests
            .entries()
            .values()
            .filter(|entry| entry.operator == operator && !entry.status.borrow().finished())
            .count();
        if unfinished >= OPERATOR_LIMIT {
            return refuse(AccessError::Conflict).await;
        }
        let reserved = match self.reserve(target, &request.session) {
            Ok(reserved) => reserved,
            Err(error) => return refuse(error).await,
        };
        self.authorizer
            .record_attended(
                &requester.identity,
                target,
                &request,
                id,
                (Action::RemoteRequest, Outcome::Success, "requested"),
            )
            .await?;
        let (status, watched) = watch::channel(Status::Waiting {
            answer_by: unix_now() + CONSENT_TIMEOUT.as_secs() as i64,
        });
        let (cancel, mut stop) = watch::channel(false);
        let tracked = Tracked {
            tenant,
            device,
            operator,
            mode: request.mode,
            uac: request.uac,
            os_session_id: request.session.os_session_id.clone(),
            status: watched,
            cancel,
            created: Instant::now(),
            session: None,
            offered: false,
        };
        let view = tracked.view(id);
        self.remote_requests.entries().insert(id, tracked);
        let enrollment = self.clone();
        let requests = self.remote_requests.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let sessions = (*sessions).as_ref();
            let started = Instant::now();
            let asked = Asked {
                sessions,
                token: &token,
                target,
                request: &request,
                id,
            };
            let outcome = tokio::select! {
                biased;
                () = cancelled(&mut stop) => None,
                result = enrollment.deliver_attended(&asked, &requester, reserved) => Some(result),
            };
            let session = match outcome {
                None => {
                    let _ = enrollment
                        .authorizer
                        .record_attended(
                            &requester.identity,
                            target,
                            &request,
                            id,
                            (Action::RemoteConsent, Outcome::Denied, "cancelled"),
                        )
                        .await;
                    status.send_replace(Status::Cancelled);
                    return;
                }
                Some(Ok(Attended::Granted(session))) => session,
                Some(Ok(Attended::Answered(answer))) => {
                    status.send_replace(match answer {
                        wire::Consent::Declined => Status::Declined,
                        wire::Consent::Expired => Status::Expired,
                        wire::Consent::Cancelled => Status::Cancelled,
                        _ => Status::Unavailable,
                    });
                    return;
                }
                // No answer reached the server before the window closed.
                Some(Err(AccessError::Offline)) if started.elapsed() >= CONSENT_TIMEOUT => {
                    status.send_replace(Status::Expired);
                    return;
                }
                Some(Err(error)) => {
                    status.send_replace(Status::Failed {
                        reason: refusal(&error),
                    });
                    return;
                }
            };
            let session = Arc::new(session);
            if let Some(entry) = requests.entries().get_mut(&id) {
                entry.session = Some(Arc::downgrade(&session));
            }
            status.send_replace(Status::Active {
                until: unix_now() + session.remaining().as_secs() as i64,
            });
            while session.is_active() {
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(250)) => {}
                    () = cancelled(&mut stop) => session.revoke(),
                }
            }
            drop(session);
            status.send_replace(Status::Ended);
        });
        Ok(view)
    }

    /// The status of the caller's own request; other operators' requests are not visible.
    pub async fn remote_request(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
        id: Uuid,
    ) -> Result<Option<RequestView>> {
        let target = Target {
            tenant,
            device: Some(device),
        };
        let decision = self
            .authorizer
            .authorize(sessions, token, target, Permission::RemoteView)
            .await?;
        Ok(self
            .remote_requests
            .find(id, tenant, device, decision.operator_id))
    }

    /// Passes the browser's WebRTC offer to the agent for the caller's own active request, once.
    pub async fn remote_media(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
        id: Uuid,
        sdp: String,
    ) -> Result<Option<String>> {
        let target = Target {
            tenant,
            device: Some(device),
        };
        let decision = self
            .authorizer
            .authorize(sessions, token, target, Permission::RemoteControl)
            .await?;
        let own = |e: &Tracked| {
            e.tenant == tenant && e.device == device && e.operator == decision.operator_id
        };
        let Some((mode, uac)) = self
            .remote_requests
            .entries()
            .get(&id)
            .filter(|e| own(e))
            .map(|e| (e.mode, e.uac))
        else {
            return Ok(None);
        };
        // The picture starts control: the session's whole mode must still be granted.
        self.authorizer
            .authorize_attended_operation(sessions, token, target, mode, uac)
            .await?;
        let session = {
            let mut entries = self.remote_requests.entries();
            let Some(entry) = entries.get_mut(&id).filter(|e| own(e)) else {
                return Ok(None);
            };
            if entry.offered {
                return Err(AccessError::Conflict);
            }
            let session = entry
                .session
                .as_ref()
                .and_then(Weak::upgrade)
                .ok_or(AccessError::Offline)?;
            entry.offered = true;
            session
        };
        let answer = session.media(sdp).await;
        drop(session);
        let identity = sessions.validate(token, false).await?;
        let mut event = Event::new(
            Action::RemoteMedia,
            if answer.is_ok() {
                Outcome::Success
            } else {
                Outcome::Failure
            },
            Actor::session(&identity),
        )
        .detail("request_id", id.to_string())
        .detail(
            "result",
            if answer.is_ok() {
                "answered"
            } else {
                "refused"
            },
        );
        event.tenant = Some(tenant);
        event.device = Some(device);
        self.authorizer.record_decision(&event).await?;
        answer.map(Some)
    }

    /// Cancels a waiting request or ends an active one; the request task records the outcome.
    pub async fn cancel_remote_request(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
        id: Uuid,
    ) -> Result<bool> {
        let operator = sessions.validate(token, false).await?.operator_id;
        let entries = self.remote_requests.entries();
        let Some(entry) = entries
            .get(&id)
            .filter(|e| e.tenant == tenant && e.device == device && e.operator == operator)
        else {
            return Ok(false);
        };
        if entry.status.borrow().finished() {
            return Ok(false);
        }
        entry.cancel.send_replace(true);
        Ok(true)
    }
}
