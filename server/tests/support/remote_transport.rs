use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};

use midas_agent_core::remote::{
    DesktopKind, Lease, Observation, Presence, Worker, transport::Native,
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

use super::remote_sessions::Context;

pub struct Broker {
    generation: Uuid,
    presence: AtomicU8,
    fail: AtomicBool,
    calls: AtomicUsize,
    last: Mutex<Option<Observation>>,
    lease: Mutex<Option<(Observation, Lease)>>,
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    entered: tokio::sync::Notify,
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
        }
    }
    pub fn allowed(&self) -> bool {
        self.lease
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(observed, lease)| lease.capture(*observed, Worker::Privileged).is_ok())
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
        let observed = Observation {
            target,
            presence: match self.presence.load(Ordering::SeqCst) {
                0 => Presence::Vacant,
                1 => Presence::Occupied,
                _ => Presence::Unknown,
            },
            desktop: DesktopKind::Secure,
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
        lease.capture(observed, Worker::Privileged)?;
        *self.lease.lock().unwrap() = Some((observed, lease));
        Ok(())
    }
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
