use crate::database::driver as sqlx;
use std::net::IpAddr;

use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    auth::sessions::{SessionInfo, SessionToken, Sessions},
    authorization::{AccessError, Authorizer, Permission, Target},
    database::Database,
};

macro_rules! names {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        pub enum $name { $(#[serde(rename = $text)] $variant),+ }
        impl $name {
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $text),+ } }
        }
    };
}
names!(Action {
    SignIn => "auth.sign_in",
    SignOut => "auth.sign_out",
    SessionRevoke => "session.revoke",
    MembershipSet => "access.membership_set",
    RoleCreate => "access.role_create",
    RoleAssign => "access.role_assign",
    RoleUnassign => "access.role_unassign",
    OverrideSet => "access.override_set",
    OverrideRemove => "access.override_remove",
    RemoteRequest => "remote.request",
    UnattendedPolicySet => "remote.unattended_policy_set",
    TenantCreate => "tenant.create",
    SiteCreate => "inventory.site_create",
    OperatorCreate => "access.operator_create",
    PlatformAdminSet => "access.platform_admin_set",
    SetupComplete => "setup.complete",
    PasskeyRegister => "auth.passkey_register",
    DirectoryConfigure => "access.directory_configure",
    DirectoryGroupMap => "access.directory_group_map",
    EnrollmentTokenCreate => "device.enroll_token_create",
    EnrollmentRequest => "device.enroll_request",
    EnrollmentApprove => "device.enroll_approve",
    EnrollmentReject => "device.enroll_reject",
    DeviceRevoke => "device.revoke",
    DirectoryConnectorSet => "directory.connector_set",
    PolicySet => "policy.set",
    PolicyClear => "policy.clear",
    GroupCreate => "inventory.group_create",
    GroupMemberAdd => "inventory.group_member_add",
    GroupMemberRemove => "inventory.group_member_remove",
    AlertAcknowledge => "alert.acknowledge",
    AlertRecipientsSet => "alert.recipients_set",
    DeviceRefresh => "device.refresh",
    AlertChannelAdd => "alert.channel_add",
    AlertChannelRemove => "alert.channel_remove",
    AlertChannelTest => "alert.channel_test",
});
names!(Outcome {
    Success => "success",
    Denied => "denied",
    Failure => "failure",
});

/// Who caused an event; `SYSTEM` marks trusted internal provisioning without an operator session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actor {
    pub operator: Option<Uuid>,
    pub session: Option<Uuid>,
}
impl Actor {
    pub const SYSTEM: Self = Self {
        operator: None,
        session: None,
    };
    pub fn session(info: &SessionInfo) -> Self {
        Self {
            operator: Some(info.operator_id),
            session: Some(info.id),
        }
    }
}

pub(crate) struct Event {
    pub action: Action,
    pub outcome: Outcome,
    pub actor: Actor,
    pub subject: Option<Uuid>,
    pub tenant: Option<Uuid>,
    pub device: Option<Uuid>,
    pub source: Option<IpAddr>,
    pub details: Map<String, Value>,
}
impl Event {
    pub fn new(action: Action, outcome: Outcome, actor: Actor) -> Self {
        Self {
            action,
            outcome,
            actor,
            subject: None,
            tenant: None,
            device: None,
            source: None,
            details: Map::new(),
        }
    }
    pub fn detail(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.details.insert(key.to_owned(), value.into());
        self
    }
}

macro_rules! insert_sql {
    ($table:literal) => {
        concat!(
            "INSERT INTO midas.",
            $table,
            " (tenant_id, action, outcome, actor_operator_id, actor_session_id, subject_operator_id, device_id, source, details) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::inet, $9::jsonb)"
        )
    };
}

async fn append(connection: &mut PgConnection, tenant: bool, event: &Event) -> sqlx::Result<()> {
    let sql = if tenant {
        insert_sql!("tenant_audit")
    } else {
        insert_sql!("platform_audit")
    };
    sqlx::query(sql)
        .bind(event.tenant)
        .bind(event.action.as_str())
        .bind(event.outcome.as_str())
        .bind(event.actor.operator)
        .bind(event.actor.session)
        .bind(event.subject)
        .bind(event.device)
        .bind(event.source.map(|ip| ip.to_canonical().to_string()))
        .bind(Value::Object(event.details.clone()).to_string())
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn append_platform(
    connection: &mut PgConnection,
    event: &Event,
) -> sqlx::Result<()> {
    append(connection, false, event).await
}

/// The connection must already carry the transaction-local context of `event.tenant`.
pub(crate) async fn append_tenant(
    connection: &mut PgConnection,
    event: &Event,
) -> sqlx::Result<()> {
    append(connection, true, event).await
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Filter {
    pub action: Option<Action>,
    pub outcome: Option<Outcome>,
    pub actor: Option<Uuid>,
    pub subject: Option<Uuid>,
    pub device: Option<Uuid>,
    /// Unix seconds, inclusive.
    pub since: Option<i64>,
    /// Unix seconds, exclusive.
    pub until: Option<i64>,
    pub before_sequence: Option<i64>,
    pub limit: u16,
}

#[derive(Debug, Serialize)]
pub struct Record {
    pub sequence: i64,
    pub occurred_at: i64,
    pub tenant_id: Option<Uuid>,
    pub action: String,
    pub outcome: String,
    pub actor_operator_id: Option<Uuid>,
    pub actor_session_id: Option<Uuid>,
    pub subject_operator_id: Option<Uuid>,
    pub device_id: Option<Uuid>,
    pub source: Option<String>,
    pub details: Value,
    pub hash: String,
}

struct Row {
    sequence: i64,
    occurred_at: i64,
    tenant_id: Option<Uuid>,
    action: String,
    outcome: String,
    actor_operator_id: Option<Uuid>,
    actor_session_id: Option<Uuid>,
    subject_operator_id: Option<Uuid>,
    device_id: Option<Uuid>,
    source: Option<String>,
    details: String,
    hash: String,
}
crate::database::postgres_row!(Row {
    sequence,
    occurred_at,
    tenant_id,
    action,
    outcome,
    actor_operator_id,
    actor_session_id,
    subject_operator_id,
    device_id,
    source,
    details,
    hash
});
impl TryFrom<Row> for Record {
    type Error = AccessError;
    fn try_from(row: Row) -> Result<Self, AccessError> {
        Ok(Self {
            sequence: row.sequence,
            occurred_at: row.occurred_at,
            tenant_id: row.tenant_id,
            action: row.action,
            outcome: row.outcome,
            actor_operator_id: row.actor_operator_id,
            actor_session_id: row.actor_session_id,
            subject_operator_id: row.subject_operator_id,
            device_id: row.device_id,
            source: row.source,
            details: serde_json::from_str(&row.details).map_err(|_| AccessError::Unavailable)?,
            hash: row.hash,
        })
    }
}

/// Result of recomputing a chain. `first_invalid` names the first record whose hash or link is wrong.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Verification {
    pub length: i64,
    pub head_hash: Option<String>,
    pub first_invalid: Option<i64>,
}

macro_rules! search_sql {
    ($table:literal, $scope:literal) => {
        concat!(
            "SELECT sequence, extract(epoch from occurred_at)::bigint AS occurred_at, tenant_id, action, outcome, actor_operator_id, actor_session_id, subject_operator_id, device_id, host(source) AS source, details::text AS details, encode(hash, 'hex') AS hash FROM midas.",
            $table,
            " WHERE ($1::text IS NULL OR action = $1) AND ($2::text IS NULL OR outcome = $2) AND ($3::uuid IS NULL OR actor_operator_id = $3) AND ($4::uuid IS NULL OR subject_operator_id = $4) AND ($5::uuid IS NULL OR device_id = $5) AND ($6::bigint IS NULL OR occurred_at >= to_timestamp($6)) AND ($7::bigint IS NULL OR occurred_at < to_timestamp($7)) AND ($8::bigint IS NULL OR sequence < $8) ",
            $scope,
            " ORDER BY sequence DESC LIMIT $9"
        )
    };
}
macro_rules! verify_sql {
    ($table:literal, $scope:literal) => {
        concat!(
            "WITH chain AS (SELECT sequence, hash, hash = midas.audit_hash(previous_hash, sequence, occurred_at, tenant_id, action, outcome, actor_operator_id, actor_session_id, subject_operator_id, device_id, source, details) AND previous_hash = coalesce(lag(hash) OVER w, '\\x0000000000000000000000000000000000000000000000000000000000000000'::bytea) AND sequence = row_number() OVER w AS valid FROM midas.",
            $table,
            " ",
            $scope,
            " WINDOW w AS (ORDER BY sequence)) SELECT count(*), (SELECT encode(hash, 'hex') FROM chain ORDER BY sequence DESC LIMIT 1), min(sequence) FILTER (WHERE NOT valid) FROM chain"
        )
    };
}

async fn search(
    connection: &mut PgConnection,
    tenant: Option<Uuid>,
    filter: &Filter,
) -> Result<Vec<Record>, AccessError> {
    let sql = if tenant.is_some() {
        search_sql!("tenant_audit", "AND tenant_id = $10")
    } else {
        search_sql!("platform_audit", "")
    };
    let limit = i64::from(if filter.limit == 0 {
        100
    } else {
        filter.limit.min(500)
    });
    let mut query = sqlx::query_as::<_, Row>(sql)
        .bind(filter.action.map(Action::as_str))
        .bind(filter.outcome.map(Outcome::as_str))
        .bind(filter.actor)
        .bind(filter.subject)
        .bind(filter.device)
        .bind(filter.since)
        .bind(filter.until)
        .bind(filter.before_sequence)
        .bind(limit);
    if let Some(tenant) = tenant {
        query = query.bind(tenant);
    }
    query
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
}

async fn verify(
    connection: &mut PgConnection,
    tenant: Option<Uuid>,
) -> Result<Verification, AccessError> {
    let sql = if tenant.is_some() {
        verify_sql!("tenant_audit", "WHERE tenant_id = $1")
    } else {
        verify_sql!("platform_audit", "")
    };
    let mut query = sqlx::query_as::<_, (i64, Option<String>, Option<i64>)>(sql);
    if let Some(tenant) = tenant {
        query = query.bind(tenant);
    }
    let (length, head_hash, first_invalid) = query.fetch_one(connection).await?;
    Ok(Verification {
        length,
        head_hash,
        first_invalid,
    })
}

pub struct Audit {
    pool: PgPool,
}

impl Audit {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
        }
    }

    async fn tenant_snapshot(
        &self,
        tenant: Uuid,
    ) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, AccessError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    /// Requires `audit.read` for the whole tenant in the current session.
    pub async fn search_tenant(
        &self,
        authorizer: &Authorizer,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        filter: &Filter,
    ) -> Result<Vec<Record>, AccessError> {
        authorizer
            .authorize(
                sessions,
                token,
                Target {
                    tenant,
                    device: None,
                },
                Permission::AuditRead,
            )
            .await?;
        let mut tx = self.tenant_snapshot(tenant).await?;
        let records = search(&mut tx, Some(tenant), filter).await?;
        tx.commit().await?;
        Ok(records)
    }

    /// Requires `audit.read` for the whole tenant in the current session.
    pub async fn verify_tenant(
        &self,
        authorizer: &Authorizer,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Verification, AccessError> {
        authorizer
            .authorize(
                sessions,
                token,
                Target {
                    tenant,
                    device: None,
                },
                Permission::AuditRead,
            )
            .await?;
        let mut tx = self.tenant_snapshot(tenant).await?;
        let result = verify(&mut tx, Some(tenant)).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Internal trusted call; expose only behind future platform-level authority.
    pub async fn search_platform(&self, filter: &Filter) -> Result<Vec<Record>, AccessError> {
        let mut connection = self.pool.acquire().await?;
        search(&mut connection, None, filter).await
    }

    /// Internal trusted call; expose only behind future platform-level authority.
    pub async fn verify_platform(&self) -> Result<Verification, AccessError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let result = verify(&mut tx, None).await?;
        tx.commit().await?;
        Ok(result)
    }
}
