use std::fmt;

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    auth::{
        AuthError,
        sessions::{SessionToken, Sessions},
    },
    database::Database,
};

macro_rules! permissions {
    ($($variant:ident => ($name:literal, $fresh:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub enum Permission { $(#[serde(rename = $name)] $variant),+ }
        impl Permission {
            pub const ALL: &[Self] = &[$(Self::$variant),+];
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $name),+ } }
            pub const fn requires_fresh_passkey(self) -> bool { match self { $(Self::$variant => $fresh),+ } }
        }
    };
}
permissions! {
    DeviceRead => ("device.read", false),
    MonitorRead => ("monitor.read", false),
    RemoteView => ("remote.view", false),
    RemoteControl => ("remote.control", false),
    RemoteAdmin => ("remote.admin", true),
    RemoteUacInteract => ("remote.uac.interact", true),
    RemoteUnattended => ("remote.unattended", true),
    TerminalExecute => ("terminal.execute", true),
    FilesRead => ("files.read", false),
    FilesWrite => ("files.write", true),
    ScriptExecute => ("script.execute", true),
    UpdatesManage => ("updates.manage", true),
    DomainJoin => ("domain.join", true),
    AccessManage => ("access.manage", true),
    AuditRead => ("audit.read", false),
}

impl Permission {
    pub const fn requires_device(self) -> bool {
        matches!(
            self,
            Self::RemoteView
                | Self::RemoteControl
                | Self::RemoteAdmin
                | Self::RemoteUacInteract
                | Self::RemoteUnattended
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BuiltinRole {
    Administrator,
    Technician,
    Observer,
}
impl BuiltinRole {
    pub const fn id(self) -> Uuid {
        let number = match self {
            Self::Administrator => 1,
            Self::Technician => 2,
            Self::Observer => 3,
        };
        Uuid::from_u128(0x00000000000040008000000000000000 + number)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Scope {
    Tenant,
    Site(Uuid),
    Group(Uuid),
    Device(Uuid),
}
impl Scope {
    fn columns(self) -> (Option<Uuid>, Option<Uuid>, Option<Uuid>) {
        match self {
            Self::Tenant => (None, None, None),
            Self::Site(id) => (Some(id), None, None),
            Self::Group(id) => (None, Some(id), None),
            Self::Device(id) => (None, None, Some(id)),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Allow,
    Deny,
}
impl Effect {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub permission: Permission,
    pub effect: Effect,
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub tenant: Uuid,
    pub device: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteMode {
    Normal,
    Administrative,
}

#[derive(Clone, Copy, Debug)]
pub struct RemoteRequest {
    pub mode: RemoteMode,
    pub uac: bool,
    pub unattended: bool,
}

/// A current policy decision, not a signed grant or a substitute for consent and execution-time checks.
#[derive(Debug, Serialize)]
pub struct AccessDecision {
    pub session_id: Uuid,
    pub operator_id: Uuid,
    pub tenant_id: Uuid,
    pub device_id: Option<Uuid>,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessError {
    Unauthenticated,
    Denied,
    FreshPasskeyRequired,
    InvalidInput,
    Conflict,
    Unavailable,
}
impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unauthenticated => "authentication required",
            Self::Denied => "access denied",
            Self::FreshPasskeyRequired => "fresh passkey verification required",
            Self::InvalidInput => "invalid access policy",
            Self::Conflict => "access policy conflict",
            Self::Unavailable => "authorization unavailable",
        })
    }
}
impl std::error::Error for AccessError {}
impl From<AuthError> for AccessError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::InvalidCredentials => Self::Unauthenticated,
            _ => Self::Unavailable,
        }
    }
}
impl From<sqlx::Error> for AccessError {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23503" | "23514" | "23502" | "22P02") => Self::InvalidInput,
            Some("23505") => Self::Conflict,
            Some("42501") => Self::Denied,
            _ => Self::Unavailable,
        }
    }
}
type Result<T> = std::result::Result<T, AccessError>;
type DeviceRow = (Option<Uuid>, Vec<Uuid>, String, String, bool, bool);

pub struct Authorizer {
    pool: PgPool,
}

impl Authorizer {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
        }
    }

    async fn tenant_transaction(
        &self,
        tenant: Uuid,
        read_only: bool,
    ) -> Result<Transaction<'_, Postgres>> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.pool.begin().await?;
        if read_only {
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    pub async fn authorize(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        permission: Permission,
    ) -> Result<AccessDecision> {
        self.evaluate(sessions, token, target, &[permission]).await
    }

    pub async fn check_remote_request(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: RemoteRequest,
    ) -> Result<AccessDecision> {
        let mut permissions = vec![
            Permission::DeviceRead,
            Permission::RemoteView,
            Permission::RemoteControl,
        ];
        if matches!(request.mode, RemoteMode::Administrative) {
            permissions.push(Permission::RemoteAdmin);
        }
        if request.uac {
            permissions.push(Permission::RemoteUacInteract);
        }
        if request.unattended {
            permissions.push(Permission::RemoteUnattended);
        }
        self.evaluate(sessions, token, target, &permissions).await
    }

    async fn evaluate(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        permissions: &[Permission],
    ) -> Result<AccessDecision> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.tenant_transaction(target.tenant, true).await?;
        let (member, fresh): (bool, bool) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM midas.tenant_memberships m WHERE m.tenant_id = $3 AND m.operator_id = s.operator_id AND m.enabled), s.authentication_method = 'passkey' AND s.authenticated_at >= clock_timestamp() - interval '5 minutes' AND s.authenticated_at <= clock_timestamp() + interval '1 second' FROM midas.browser_sessions s JOIN midas.operators o ON o.id = s.operator_id WHERE s.id = $1 AND s.operator_id = $2 AND o.enabled AND s.credential_version = o.credential_version AND s.expires_at > clock_timestamp() AND s.idle_expires_at > clock_timestamp()")
            .bind(identity.id).bind(identity.operator_id).bind(target.tenant).fetch_optional(&mut *tx).await?
            .ok_or(AccessError::Unauthenticated)?;
        if !member {
            return Err(AccessError::Denied);
        }
        let mut site = None;
        let mut groups: Vec<Uuid> = Vec::new();
        if let Some(device) = target.device {
            let row: Option<DeviceRow> = sqlx::query_as("SELECT d.site_id, ARRAY(SELECT g.group_id FROM midas.device_group_members g WHERE g.tenant_id = d.tenant_id AND g.device_id = d.id), d.kind, d.platform, d.unattended_enabled, t.unattended_enabled FROM midas.devices d JOIN midas.tenants t ON t.id = d.tenant_id WHERE d.tenant_id = $1 AND d.id = $2")
                .bind(target.tenant).bind(device).fetch_optional(&mut *tx).await?;
            let (device_site, device_groups, kind, platform, device_policy, tenant_policy) =
                row.ok_or(AccessError::Denied)?;
            site = device_site;
            groups = device_groups;
            if permissions.contains(&Permission::RemoteUnattended)
                && (kind != "server" || !tenant_policy || !device_policy)
            {
                return Err(AccessError::Denied);
            }
            if permissions.contains(&Permission::RemoteUacInteract) && platform != "windows" {
                return Err(AccessError::Denied);
            }
        } else if permissions.iter().any(|p| p.requires_device()) {
            return Err(AccessError::Denied);
        }
        let names: Vec<_> = permissions.iter().map(|p| p.as_str()).collect();
        // Without a device the decision covers the whole tenant, so denials at any narrower scope also apply.
        let applicable: Vec<(String, bool, bool)> = sqlx::query_as(
            "WITH applicable AS (SELECT p.permission, p.effect, a.site_id, a.group_id, a.device_id FROM midas.role_assignments a JOIN midas.roles r ON r.id = a.role_id JOIN midas.role_permissions p ON p.role_id = r.id AND (r.enabled OR p.effect = 'deny') WHERE a.tenant_id = $1 AND a.operator_id = $2 AND p.permission = ANY($3) UNION ALL SELECT permission, effect, site_id, group_id, device_id FROM midas.permission_overrides WHERE tenant_id = $1 AND operator_id = $2 AND permission = ANY($3)) SELECT permission, bool_or(effect = 'allow'), bool_or(effect = 'deny') FROM applicable WHERE (site_id IS NULL AND group_id IS NULL AND device_id IS NULL) OR site_id = $4 OR group_id = ANY($5) OR device_id = $6 OR ($6::uuid IS NULL AND effect = 'deny') GROUP BY permission")
            .bind(target.tenant).bind(identity.operator_id).bind(&names).bind(site).bind(&groups).bind(target.device)
            .fetch_all(&mut *tx).await?;
        if names.iter().any(|name| {
            !applicable
                .iter()
                .any(|(permission, allow, deny)| permission == name && *allow && !*deny)
        }) {
            return Err(AccessError::Denied);
        }
        if permissions.iter().any(|p| p.requires_fresh_passkey()) && !fresh {
            return Err(AccessError::FreshPasskeyRequired);
        }
        tx.commit().await?;
        Ok(AccessDecision {
            session_id: identity.id,
            operator_id: identity.operator_id,
            tenant_id: target.tenant,
            device_id: target.device,
            permissions: permissions.to_vec(),
        })
    }

    /// Internal trusted provisioning; expose only behind setup or administrative authorization and audit.
    pub async fn set_membership(&self, tenant: Uuid, operator: Uuid, enabled: bool) -> Result<()> {
        let mut tx = self.tenant_transaction(tenant, false).await?;
        sqlx::query("INSERT INTO midas.tenant_memberships (tenant_id, operator_id, enabled) VALUES ($1,$2,$3) ON CONFLICT (tenant_id,operator_id) DO UPDATE SET enabled=excluded.enabled")
            .bind(tenant).bind(operator).bind(enabled).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Internal trusted provisioning; this does not authorize its caller to manage access.
    pub async fn create_role(&self, name: &str, rules: &[Rule]) -> Result<Uuid> {
        if name.trim().is_empty()
            || name != name.trim()
            || name.chars().count() > 100
            || rules.len() > Permission::ALL.len()
        {
            return Err(AccessError::InvalidInput);
        }
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO midas.roles (id,name) VALUES ($1,$2)")
            .bind(id)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        for rule in rules {
            sqlx::query(
                "INSERT INTO midas.role_permissions (role_id,permission,effect) VALUES ($1,$2,$3)",
            )
            .bind(id)
            .bind(rule.permission.as_str())
            .bind(rule.effect.as_str())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Internal trusted provisioning; the caller must already be authorized for this tenant and scope.
    pub async fn assign_role(
        &self,
        tenant: Uuid,
        operator: Uuid,
        role: Uuid,
        scope: Scope,
    ) -> Result<Uuid> {
        let (site, group, device) = scope.columns();
        let id = Uuid::new_v4();
        let mut tx = self.tenant_transaction(tenant, false).await?;
        sqlx::query("INSERT INTO midas.role_assignments (tenant_id,id,operator_id,role_id,site_id,group_id,device_id) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(tenant).bind(id).bind(operator).bind(role).bind(site).bind(group).bind(device).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Internal trusted provisioning; both allows and denials need administrative authorization and audit.
    pub async fn override_permission(
        &self,
        tenant: Uuid,
        operator: Uuid,
        scope: Scope,
        rule: Rule,
    ) -> Result<Uuid> {
        let (site, group, device) = scope.columns();
        let id = Uuid::new_v4();
        let mut tx = self.tenant_transaction(tenant, false).await?;
        sqlx::query("INSERT INTO midas.permission_overrides (tenant_id,id,operator_id,permission,effect,site_id,group_id,device_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(tenant).bind(id).bind(operator).bind(rule.permission.as_str()).bind(rule.effect.as_str()).bind(site).bind(group).bind(device).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Internal trusted revocation; tenant authorization and audit belong to the calling administration layer.
    pub async fn remove_assignment(&self, tenant: Uuid, id: Uuid) -> Result<bool> {
        let mut tx = self.tenant_transaction(tenant, false).await?;
        let removed =
            sqlx::query("DELETE FROM midas.role_assignments WHERE tenant_id=$1 AND id=$2")
                .bind(tenant)
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                == 1;
        tx.commit().await?;
        Ok(removed)
    }

    /// Internal trusted revocation; removing a denial may increase access and also requires authorization.
    pub async fn remove_override(&self, tenant: Uuid, id: Uuid) -> Result<bool> {
        let mut tx = self.tenant_transaction(tenant, false).await?;
        let removed =
            sqlx::query("DELETE FROM midas.permission_overrides WHERE tenant_id=$1 AND id=$2")
                .bind(tenant)
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                == 1;
        tx.commit().await?;
        Ok(removed)
    }
}
