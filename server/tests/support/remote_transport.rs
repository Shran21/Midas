use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::http::StatusCode;
use midas_agent_core::remote::{
    Consent, ConsentRequest, DesktopKind, Lease, Observation, Presence, Worker, transport::Native,
};
use midas_remote_auth::{Error, Target};
use midas_server::{
    auth::sessions::SessionToken,
    authorization::{
        AccessError, RemoteMode,
        unattended::{SessionSelection, UnattendedRequest},
    },
    database::driver as sqlx,
    remote_grants::Decision,
};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use super::{administration::call, remote_sessions::Context};

pub struct Broker {
    generation: Uuid,
    presence: AtomicU8,
    fail: AtomicBool,
    calls: AtomicUsize,
    last: Mutex<Option<Observation>>,
    lease: Mutex<Option<(Observation, Lease)>>,
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    entered: tokio::sync::Notify,
    /// The local answer; with `None` the window stays open until revoked, then allows late.
    answer: Mutex<Option<Consent>>,
    asked: Mutex<Vec<ConsentRequest>>,
    asking: tokio::sync::Notify,
    consenting: AtomicBool,
}

impl Broker {
    pub fn new(generation: Uuid) -> Self {
        Self {
            generation,
            presence: AtomicU8::new(0),
            fail: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            last: Mutex::new(None),
            lease: Mutex::new(None),
            hold: Mutex::new(None),
            entered: tokio::sync::Notify::new(),
            answer: Mutex::new(Some(Consent::Allowed)),
            asked: Mutex::new(Vec::new()),
            asking: tokio::sync::Notify::new(),
            consenting: AtomicBool::new(false),
        }
    }
    pub fn allowed(&self) -> bool {
        self.lease
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(observed, lease)| lease.capture(*observed, worker(lease)).is_ok())
    }
    /// Waits until the window of a withdrawn request has closed, so a later answer is not its.
    async fn closed(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.consenting.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the withdrawn window closes");
    }
    async fn stopped(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.allowed() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}

fn worker(lease: &Lease) -> Worker {
    if lease.attended() {
        Worker::User
    } else {
        Worker::Privileged
    }
}

impl Native for Broker {
    fn observe(&self, target: Target) -> Result<Observation, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let hold = self.hold.lock().unwrap().take();
        if let Some(wait) = hold {
            self.entered.notify_one();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        if target.session != 2 || target.generation != self.generation {
            return Err(Error::Target);
        }
        let presence = match self.presence.load(Ordering::SeqCst) {
            0 => Presence::Vacant,
            1 => Presence::Occupied,
            _ => Presence::Unknown,
        };
        let observed = Observation {
            target,
            presence,
            desktop: if presence == Presence::Occupied {
                DesktopKind::User
            } else {
                DesktopKind::Secure
            },
            desktop_epoch: self.generation,
        };
        *self.last.lock().unwrap() = Some(observed);
        Ok(observed)
    }
    fn activate(&self, lease: Lease) -> Result<(), Error> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(Error::Desktop);
        }
        let observed = self.last.lock().unwrap().unwrap();
        lease.capture(observed, worker(&lease))?;
        *self.lease.lock().unwrap() = Some((observed, lease));
        Ok(())
    }
    fn consent(&self, request: &ConsentRequest, stopped: &dyn Fn() -> bool) -> Consent {
        self.consenting.store(true, Ordering::SeqCst);
        self.asked.lock().unwrap().push(request.clone());
        self.asking.notify_one();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let answer = loop {
            if let Some(answer) = *self.answer.lock().unwrap() {
                break answer;
            }
            if stopped() {
                break Consent::Allowed;
            }
            assert!(std::time::Instant::now() < deadline, "unanswered consent");
            std::thread::sleep(Duration::from_millis(5));
        };
        self.consenting.store(false, Ordering::SeqCst);
        answer
    }
    fn media(&self, offer: &str) -> Result<String, Error> {
        if !self.allowed() {
            return Err(Error::Revoked);
        }
        Ok(format!("answer to {} bytes", offer.len()))
    }
}

async fn settled(ctx: &Context<'_>, path: &str, from: &str) -> Value {
    for _ in 0..200 {
        let (status, body, _) = call(ctx.app, "GET", path, Some(ctx.manager), None, &[]).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["state"] != from {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("request stayed {from}");
}

/// Attended requests through the panel API, the agent transport and the local answer.
pub async fn verify_attended(
    ctx: &Context<'_>,
    admin: &mut PgConnection,
    view: &Value,
    broker: &Arc<Broker>,
) {
    let base = format!(
        "/api/tenants/{}/devices/{}/remote-requests",
        ctx.tenant, ctx.device
    );
    let request = |mode: &str, uac: bool| {
        serde_json::json!({
            "mode": mode,
            "uac": uac,
            "session": view["sessions"][0]["selection"].clone(),
        })
    };
    sqlx::query("UPDATE midas.devices SET platform='windows' WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant)
        .bind(ctx.device)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &base,
            None,
            Some(request("normal", false)),
            &[]
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &base,
            Some(ctx.outsider),
            Some(request("normal", false)),
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let start = async |body: Value| {
        let (status, view, _) =
            call(ctx.app, "POST", &base, Some(ctx.manager), Some(body), &[]).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{view}");
        assert_eq!(view["state"], "waiting");
        assert!(view["answer_by"].as_i64().unwrap() > 0);
        format!("{base}/{}", view["id"].as_str().unwrap())
    };
    let token = SessionToken::parse(ctx.manager.cookie().split_once('=').unwrap().1).unwrap();
    let operator = ctx
        .web
        .sessions()
        .validate(&token, false)
        .await
        .unwrap()
        .operator_id;
    let (operator_name, customer_name): (String, String) = sqlx::query_as("SELECT o.display_name, t.name FROM midas.operators o JOIN midas.tenants t ON t.id=$2 WHERE o.id=$1")
        .bind(operator).bind(ctx.tenant).fetch_one(&mut *admin).await.unwrap();

    broker.presence.store(1, Ordering::SeqCst);
    let path = start(request("normal", false)).await;
    let active = settled(ctx, &path, "waiting").await;
    assert_eq!(active["state"], "active", "{active}");
    assert!(broker.allowed());
    let asked = broker.asked.lock().unwrap().last().cloned().unwrap();
    assert_eq!(asked.timeout, Duration::from_secs(60));
    assert_eq!(asked.rights.mode, midas_remote_auth::Mode::Normal);
    assert!(!asked.rights.uac);
    assert_eq!(asked.operator_name, operator_name);
    assert_eq!(asked.customer_name, customer_name);
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &base,
            Some(ctx.manager),
            Some(request("normal", false)),
            &[]
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(ctx.app, "GET", &path, Some(ctx.outsider), None, &[])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let media = format!("{path}/media");
    let offer = serde_json::json!({ "sdp": "v=0 offer" });
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &media,
            Some(ctx.outsider),
            Some(offer.clone()),
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &media,
            Some(ctx.manager),
            Some(serde_json::json!({ "sdp": "" })),
            &[]
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, answer, _) = call(
        ctx.app,
        "POST",
        &media,
        Some(ctx.manager),
        Some(offer.clone()),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["sdp"], "answer to 9 bytes");
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &media,
            Some(ctx.manager),
            Some(offer.clone()),
            &[]
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "one media setup per request"
    );
    assert_eq!(
        call(ctx.app, "DELETE", &path, Some(ctx.manager), None, &[])
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(settled(ctx, &path, "active").await["state"], "ended");
    broker.stopped().await;
    assert_eq!(
        call(ctx.app, "DELETE", &path, Some(ctx.manager), None, &[])
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    *broker.answer.lock().unwrap() = None;
    let waiting = start(request("normal", false)).await;
    tokio::time::timeout(Duration::from_secs(5), broker.asking.notified())
        .await
        .unwrap();
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &format!("{waiting}/media"),
            Some(ctx.manager),
            Some(serde_json::json!({ "sdp": "v=0 early" })),
            &[]
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "no media before the user allowed"
    );
    assert_eq!(
        call(ctx.app, "DELETE", &waiting, Some(ctx.manager), None, &[])
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        settled(ctx, &waiting, "waiting").await["state"],
        "cancelled"
    );
    broker.closed().await;
    for (answer, state) in [
        (Consent::Declined, "declined"),
        (Consent::Expired, "expired"),
        (Consent::Unavailable, "unavailable"),
    ] {
        *broker.answer.lock().unwrap() = Some(answer);
        let path = start(request("normal", false)).await;
        assert_eq!(settled(ctx, &path, "waiting").await["state"], state);
        assert!(!broker.allowed(), "{state}");
    }
    *broker.answer.lock().unwrap() = Some(Consent::Allowed);
    broker.presence.store(0, Ordering::SeqCst);
    let asked = broker.asked.lock().unwrap().len();
    let path = start(request("normal", false)).await;
    assert_eq!(settled(ctx, &path, "waiting").await["state"], "unavailable");
    assert_eq!(broker.asked.lock().unwrap().len(), asked);
    broker.presence.store(1, Ordering::SeqCst);

    // A late "allow" after the operator cancelled, and one after the passkey sign-in went stale.
    for late in ["cancel", "passkey"] {
        *broker.answer.lock().unwrap() = None;
        let path = start(request("administrative", true)).await;
        tokio::time::timeout(Duration::from_secs(5), broker.asking.notified())
            .await
            .unwrap();
        if late == "cancel" {
            assert_eq!(
                call(ctx.app, "DELETE", &path, Some(ctx.manager), None, &[])
                    .await
                    .0,
                StatusCode::NO_CONTENT
            );
            assert_eq!(settled(ctx, &path, "waiting").await["state"], "cancelled");
        } else {
            sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp()-interval '6 minutes' WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
            *broker.answer.lock().unwrap() = Some(Consent::Allowed);
            let failed = settled(ctx, &path, "waiting").await;
            assert_eq!(failed["state"], "failed", "{failed}");
            assert_eq!(failed["reason"], "fresh_passkey_required");
            sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp() WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
        }
        broker.closed().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!broker.allowed(), "{late}");
    }
    *broker.answer.lock().unwrap() = Some(Consent::Allowed);

    let consents: Vec<(String, String)> = sqlx::query_as("SELECT outcome, details->>'result' FROM midas.tenant_audit WHERE tenant_id=$1 AND device_id=$2 AND action='remote.consent' ORDER BY sequence")
        .bind(ctx.tenant).bind(ctx.device).fetch_all(&mut *admin).await.unwrap();
    let results: Vec<_> = consents.iter().map(|(_, result)| result.as_str()).collect();
    assert_eq!(
        results,
        [
            "allowed",
            "cancelled",
            "declined",
            "expired",
            "unavailable",
            "unavailable",
            "cancelled",
            "allowed"
        ]
    );
    let media: Vec<(String, String)> = sqlx::query_as("SELECT outcome, details->>'result' FROM midas.tenant_audit WHERE tenant_id=$1 AND device_id=$2 AND action='remote.media' ORDER BY sequence")
        .bind(ctx.tenant).bind(ctx.device).fetch_all(&mut *admin).await.unwrap();
    assert_eq!(media, [("success".to_owned(), "answered".to_owned())]);
    let issued: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_audit WHERE tenant_id=$1 AND device_id=$2 AND action='remote.grant_issued' AND details->>'unattended'='false'")
        .bind(ctx.tenant).bind(ctx.device).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(issued, 1);
    broker.presence.store(0, Ordering::SeqCst);
    sqlx::query("UPDATE midas.devices SET platform='linux' WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant)
        .bind(ctx.device)
        .execute(&mut *admin)
        .await
        .unwrap();
}

pub async fn verify(
    ctx: &Context<'_>,
    admin: &mut PgConnection,
    view: &Value,
    broker: &Arc<Broker>,
) -> midas_server::remote_grants::Session {
    let token = SessionToken::parse(ctx.manager.cookie().split_once('=').unwrap().1).unwrap();
    let selection: SessionSelection =
        serde_json::from_value(view["sessions"][0]["selection"].clone()).unwrap();
    let make_request = || UnattendedRequest {
        mode: RemoteMode::Administrative,
        uac: true,
        reason: "Protected transport trial".into(),
        session: selection.clone(),
    };
    let operator = ctx
        .web
        .sessions()
        .validate(&token, false)
        .await
        .unwrap()
        .operator_id;
    sqlx::query("UPDATE midas.devices SET platform='windows' WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant)
        .bind(ctx.device)
        .execute(&mut *admin)
        .await
        .unwrap();
    let original_tenant: bool =
        sqlx::query_scalar("SELECT unattended_enabled FROM midas.tenants WHERE id=$1")
            .bind(ctx.tenant)
            .fetch_one(&mut *admin)
            .await
            .unwrap();
    let original_device: (bool, Option<i64>, Option<i64>) = sqlx::query_as("SELECT unattended_enabled,unattended_window_start,unattended_window_end FROM midas.devices WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant).bind(ctx.device).fetch_one(&mut *admin).await.unwrap();
    sqlx::query("UPDATE midas.tenants SET unattended_enabled=true WHERE id=$1")
        .bind(ctx.tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    sqlx::query("UPDATE midas.devices SET unattended_enabled=true,unattended_window_start=floor(extract(epoch FROM clock_timestamp()))::bigint-60,unattended_window_end=floor(extract(epoch FROM clock_timestamp()))::bigint+600 WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant).bind(ctx.device).execute(&mut *admin).await.unwrap();
    let start = |request: UnattendedRequest| {
        let (enrollment, web, token, tenant, device) = (
            ctx.enrollment.clone(),
            ctx.web.clone(),
            token.clone(),
            ctx.tenant,
            ctx.device,
        );
        tokio::spawn(async move {
            enrollment
                .authorize_remote(web.sessions(), &token, tenant, device, &request)
                .await
        })
    };

    for stop in ["explicit", "drop"] {
        let Decision::Granted(session) = start(make_request()).await.unwrap().unwrap() else {
            panic!("grant");
        };
        assert!(session.is_active() && broker.allowed());
        assert!(matches!(
            start(make_request()).await.unwrap(),
            Err(AccessError::Conflict)
        ));
        if stop == "explicit" {
            session.revoke();
            assert!(!session.is_active());
        }
        drop(session);
        broker.stopped().await;
    }
    for presence in [1, 2] {
        broker.presence.store(presence, Ordering::SeqCst);
        let result = start(make_request()).await.unwrap();
        if presence == 1 {
            assert!(matches!(result, Ok(Decision::LocalConsentRequired)));
        } else {
            assert!(matches!(result, Err(AccessError::Offline)));
        }
        assert!(!broker.allowed());
    }
    broker.presence.store(0, Ordering::SeqCst);
    let mut stale = make_request();
    stale.session.generation = Uuid::new_v4();
    assert!(matches!(
        start(stale).await.unwrap(),
        Err(AccessError::Offline)
    ));
    let mut stale = make_request();
    stale.session.connection_id = Uuid::new_v4();
    assert!(matches!(
        start(stale).await.unwrap(),
        Err(AccessError::Offline)
    ));
    broker.fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        start(make_request()).await.unwrap(),
        Err(AccessError::Offline)
    ));
    broker.fail.store(false, Ordering::SeqCst);

    for condition in [
        "policy",
        "passkey",
        "credential",
        "membership",
        "audit",
        "delivery_audit",
        "cancel",
    ] {
        let (release, hold) = std::sync::mpsc::channel();
        *broker.hold.lock().unwrap() = Some(hold);
        let task = start(make_request());
        tokio::time::timeout(Duration::from_secs(5), broker.entered.notified())
            .await
            .unwrap();
        match condition {
            "policy" => {
                sqlx::query("UPDATE midas.tenants SET unattended_enabled=false WHERE id=$1")
                    .bind(ctx.tenant)
                    .execute(&mut *admin)
                    .await
                    .unwrap();
            }
            "passkey" => {
                sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp()-interval '6 minutes' WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
            }
            "credential" => {
                sqlx::query("UPDATE midas.agent_credentials SET revoked_at=clock_timestamp() WHERE tenant_id=$1 AND device_id=$2").bind(ctx.tenant).bind(ctx.device).execute(&mut *admin).await.unwrap();
            }
            "membership" => {
                sqlx::query("UPDATE midas.tenant_memberships SET enabled=false WHERE tenant_id=$1 AND operator_id=$2").bind(ctx.tenant).bind(operator).execute(&mut *admin).await.unwrap();
            }
            "audit" => {
                sqlx::raw_sql("CREATE FUNCTION midas.reject_transport_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='remote.grant_issued' THEN RAISE EXCEPTION 'test'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_transport_audit BEFORE INSERT ON midas.tenant_audit FOR EACH ROW EXECUTE FUNCTION midas.reject_transport_audit();").execute(&mut *admin).await.unwrap();
            }
            "delivery_audit" => {
                sqlx::raw_sql("CREATE FUNCTION midas.reject_transport_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='remote.grant_delivery' THEN RAISE EXCEPTION 'test'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_transport_audit BEFORE INSERT ON midas.tenant_audit FOR EACH ROW EXECUTE FUNCTION midas.reject_transport_audit();").execute(&mut *admin).await.unwrap();
            }
            _ => task.abort(),
        }
        release.send(()).unwrap();
        if condition == "cancel" {
            assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        } else {
            assert!(task.await.unwrap().is_err(), "{condition}");
        }
        broker.stopped().await;
        assert!(!broker.allowed(), "{condition}");
        match condition {
            "policy" => {
                sqlx::query("UPDATE midas.tenants SET unattended_enabled=true WHERE id=$1")
                    .bind(ctx.tenant)
                    .execute(&mut *admin)
                    .await
                    .unwrap();
            }
            "passkey" => {
                sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp() WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
            }
            "credential" => {
                sqlx::query("UPDATE midas.agent_credentials SET revoked_at=NULL WHERE tenant_id=$1 AND device_id=$2").bind(ctx.tenant).bind(ctx.device).execute(&mut *admin).await.unwrap();
            }
            "membership" => {
                sqlx::query("UPDATE midas.tenant_memberships SET enabled=true WHERE tenant_id=$1 AND operator_id=$2").bind(ctx.tenant).bind(operator).execute(&mut *admin).await.unwrap();
            }
            "audit" | "delivery_audit" => {
                sqlx::raw_sql("DROP TRIGGER reject_transport_audit ON midas.tenant_audit; DROP FUNCTION midas.reject_transport_audit();").execute(&mut *admin).await.unwrap();
            }
            _ => {}
        }
    }
    let calls = broker.calls.load(Ordering::SeqCst);
    sqlx::raw_sql("CREATE FUNCTION midas.reject_transport_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='remote.grant_prepare' THEN RAISE EXCEPTION 'test'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_transport_audit BEFORE INSERT ON midas.tenant_audit FOR EACH ROW EXECUTE FUNCTION midas.reject_transport_audit();").execute(&mut *admin).await.unwrap();
    assert!(matches!(
        start(make_request()).await.unwrap(),
        Err(AccessError::Unavailable)
    ));
    assert_eq!(broker.calls.load(Ordering::SeqCst), calls);
    sqlx::raw_sql("DROP TRIGGER reject_transport_audit ON midas.tenant_audit; DROP FUNCTION midas.reject_transport_audit();").execute(&mut *admin).await.unwrap();
    let Decision::Granted(session) = start(make_request()).await.unwrap().unwrap() else {
        panic!("fresh grant after cancellation");
    };
    assert!(session.is_active() && broker.allowed());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_audit WHERE tenant_id=$1 AND device_id=$2 AND action='remote.grant_issued' AND details->>'reason'='Protected transport trial'")
        .bind(ctx.tenant).bind(ctx.device).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(count, 5);
    let accepted: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_audit WHERE tenant_id=$1 AND device_id=$2 AND action='remote.grant_delivery' AND outcome='success' AND details->>'reason'='Protected transport trial'")
        .bind(ctx.tenant).bind(ctx.device).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(accepted, 3);

    sqlx::query("UPDATE midas.tenants SET unattended_enabled=$2 WHERE id=$1")
        .bind(ctx.tenant)
        .bind(original_tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    sqlx::query("UPDATE midas.devices SET platform='linux',unattended_enabled=$3,unattended_window_start=$4,unattended_window_end=$5 WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant).bind(ctx.device).bind(original_device.0).bind(original_device.1).bind(original_device.2).execute(&mut *admin).await.unwrap();
    session
}
