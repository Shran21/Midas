use crate::database::driver as sqlx;
pub mod unattended;
use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::{
        AuthError,
        sessions::{SessionInfo, SessionToken, Sessions},
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
    DeviceEnroll => ("device.enroll", true),
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
    DirectoryDiscover => ("directory.discover", true),
    AccessManage => ("access.manage", true),
    PolicyManage => ("policy.manage", true),
    AlertAcknowledge => ("alert.acknowledge", false),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Scope {
    Tenant,
    Site(Uuid),
    Group(Uuid),
    Device(Uuid),
}
impl Scope {
    pub(crate) fn from_columns(
        site: Option<Uuid>,
        group: Option<Uuid>,
        device: Option<Uuid>,
    ) -> Self {
        match (site, group, device) {
            (Some(id), _, _) => Self::Site(id),
            (_, Some(id), _) => Self::Group(id),
            (_, _, Some(id)) => Self::Device(id),
            _ => Self::Tenant,
        }
    }
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

#[derive(Debug, Serialize)]
pub struct TenantSummary {
    pub id: Uuid,
    pub name: String,
}
crate::database::postgres_row!(TenantSummary { id, name });

/// Navigation hint for the signed-in operator; every operation still needs its own authorization.
#[derive(Debug, Serialize)]
pub struct TenantContext {
    pub id: Uuid,
    pub name: String,
    /// Granted anywhere in the tenant, minus tenant-wide denials.
    pub capabilities: Vec<Permission>,
    /// Held for the whole tenant, as checked for tenant-wide operations.
    pub tenant_wide: Vec<Permission>,
    /// Operations with these permissions need a passkey sign-in from the last five minutes.
    pub fresh_passkey: Vec<Permission>,
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
    /// The actor would grant or restore a permission it does not hold tenant-wide.
    Escalation,
    /// The operation needs a live agent session, and the device has none.
    Offline,
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
            Self::Escalation => "permission exceeds the actor's own access",
            Self::Offline => "the device is offline",
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
/// Device properties some permissions depend on, beyond the operator's rules.
#[derive(Clone, Copy, Debug)]
struct Traits {
    server: bool,
    windows: bool,
    unattended: bool,
}

impl Traits {
    fn eligible(self, permission: Permission) -> bool {
        match permission {
            Permission::RemoteUnattended => self.server && self.unattended,
            Permission::RemoteUacInteract => self.windows,
            _ => true,
        }
    }
}
type AssignmentRow = (Uuid, Uuid, Option<Uuid>, Option<Uuid>, Option<Uuid>);
type OverrideRow = (
    Uuid,
    String,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
);

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

    /// Tenants with an enabled membership for the session's operator.
    pub async fn tenants(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
    ) -> Result<Vec<TenantSummary>> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT set_config('midas.operator_id', $1, true)")
            .bind(identity.operator_id.to_string())
            .execute(&mut *tx)
            .await?;
        let tenants = sqlx::query_as("SELECT t.id, t.name FROM midas.tenant_memberships m JOIN midas.tenants t ON t.id = m.tenant_id WHERE m.operator_id = $1 AND m.enabled ORDER BY lower(t.name), t.id LIMIT 500")
            .bind(identity.operator_id).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(tenants)
    }

    /// Tenant name and permissions granted anywhere in it, minus tenant-wide denials.
    pub async fn tenant_context(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<TenantContext> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.tenant_transaction(tenant, true).await?;
        let name: Option<String> = sqlx::query_scalar("SELECT t.name FROM midas.tenants t JOIN midas.tenant_memberships m ON m.tenant_id = t.id AND m.operator_id = $2 AND m.enabled WHERE t.id = $1")
            .bind(tenant).bind(identity.operator_id).fetch_optional(&mut *tx).await?;
        let name = name.ok_or(AccessError::Denied)?;
        let granted: Vec<String> = sqlx::query_scalar("WITH rules AS (SELECT p.permission, p.effect, r.enabled, a.site_id, a.group_id, a.device_id FROM midas.role_assignments a JOIN midas.roles r ON r.id = a.role_id JOIN midas.role_permissions p ON p.role_id = r.id WHERE a.tenant_id = $1 AND a.operator_id = $2 UNION ALL SELECT permission, effect, true, site_id, group_id, device_id FROM midas.permission_overrides WHERE tenant_id = $1 AND operator_id = $2) SELECT DISTINCT permission FROM rules WHERE effect = 'allow' AND enabled EXCEPT SELECT permission FROM rules WHERE effect = 'deny' AND site_id IS NULL AND group_id IS NULL AND device_id IS NULL")
            .bind(tenant).bind(identity.operator_id).fetch_all(&mut *tx).await?;
        let names: Vec<_> = Permission::ALL.iter().map(|p| p.as_str()).collect();
        let whole = tenant_rules(&mut tx, tenant, identity.operator_id, &names).await?;
        tx.commit().await?;
        Ok(TenantContext {
            id: tenant,
            name,
            capabilities: Permission::ALL
                .iter()
                .copied()
                .filter(|p| granted.iter().any(|g| g == p.as_str()))
                .collect(),
            tenant_wide: allowed(&whole),
            fresh_passkey: Permission::ALL
                .iter()
                .copied()
                .filter(|p| p.requires_fresh_passkey())
                .collect(),
        })
    }

    /// Effective permissions per device, with the same rules `authorize` applies to a device target.
    pub(crate) async fn device_permissions(
        &self,
        identity: &SessionInfo,
        tenant: Uuid,
        device: Option<Uuid>,
    ) -> Result<HashMap<Uuid, Vec<Permission>>> {
        let mut tx = self.tenant_transaction(tenant, true).await?;
        session_state(&mut tx, identity, tenant).await?;
        let names: Vec<_> = Permission::ALL.iter().map(|p| p.as_str()).collect();
        let traits = device_traits(&mut tx, tenant, device).await?;
        let granted: HashSet<(Uuid, String)> =
            device_grants(&mut tx, tenant, identity.operator_id, &names, device)
                .await?
                .into_iter()
                .collect();
        tx.commit().await?;
        Ok(traits
            .into_iter()
            .map(|(id, traits)| {
                let permissions = Permission::ALL
                    .iter()
                    .copied()
                    .filter(|p| {
                        traits.eligible(*p) && granted.contains(&(id, p.as_str().to_owned()))
                    })
                    .collect();
                (id, permissions)
            })
            .collect())
    }

    pub async fn authorize(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        permission: Permission,
    ) -> Result<AccessDecision> {
        let identity = sessions.validate(token, false).await?;
        self.evaluate(&identity, target, &[permission], true).await
    }

    /// Read-only views of sensitive configuration; never use this for a change or an operation.
    pub(crate) async fn authorize_view(
        &self,
        identity: &SessionInfo,
        target: Target,
        permission: Permission,
    ) -> Result<AccessDecision> {
        self.evaluate(identity, target, &[permission], false).await
    }

    pub(crate) async fn authorize_identity(
        &self,
        identity: &SessionInfo,
        target: Target,
        permission: Permission,
    ) -> Result<AccessDecision> {
        self.evaluate(identity, target, &[permission], true).await
    }

    /// Permissions the operator holds for the whole tenant, ignoring passkey freshness.
    pub(crate) async fn held(
        &self,
        identity: &SessionInfo,
        tenant: Uuid,
    ) -> Result<Vec<Permission>> {
        let mut tx = self.tenant_transaction(tenant, true).await?;
        let member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.tenant_memberships WHERE tenant_id = $1 AND operator_id = $2 AND enabled)")
            .bind(tenant).bind(identity.operator_id).fetch_one(&mut *tx).await?;
        if !member {
            return Ok(Vec::new());
        }
        let names: Vec<_> = Permission::ALL.iter().map(|p| p.as_str()).collect();
        let rules = tenant_rules(&mut tx, tenant, identity.operator_id, &names).await?;
        tx.commit().await?;
        Ok(allowed(&rules))
    }

    /// Every eligibility decision for an authenticated session is audited; an unaudited allow is never returned.
    pub async fn check_remote_request(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: RemoteRequest,
    ) -> Result<AccessDecision> {
        let identity = sessions.validate(token, false).await?;
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
        let result = self.evaluate(&identity, target, &permissions, true).await;
        let (outcome, label) = match &result {
            Ok(_) => (Outcome::Success, "allowed"),
            Err(AccessError::Denied) => (Outcome::Denied, "denied"),
            Err(AccessError::FreshPasskeyRequired) => (Outcome::Denied, "fresh_passkey_required"),
            Err(_) => return result,
        };
        let mut event = Event::new(Action::RemoteRequest, outcome, Actor::session(&identity))
            .detail(
                "mode",
                match request.mode {
                    RemoteMode::Normal => "normal",
                    RemoteMode::Administrative => "administrative",
                },
            )
            .detail("uac", request.uac)
            .detail("unattended", request.unattended)
            .detail("result", label);
        event.tenant = Some(target.tenant);
        event.device = target.device;
        let recorded = self.record_decision(&event).await;
        match (result, recorded) {
            (Ok(_), Err(_)) => Err(AccessError::Unavailable),
            (result, _) => result,
        }
    }

    pub(crate) async fn record_decision(&self, event: &Event) -> Result<()> {
        let tenant = event.tenant.ok_or(AccessError::InvalidInput)?;
        let mut tx = self.tenant_transaction(tenant, false).await?;
        match audit::append_tenant(&mut tx, event).await {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                if AccessError::from(error) != AccessError::InvalidInput {
                    return Err(AccessError::Unavailable);
                }
                drop(tx);
                let mut connection = self.pool.acquire().await?;
                Ok(audit::append_platform(&mut connection, event).await?)
            }
        }
    }

    async fn evaluate(
        &self,
        identity: &SessionInfo,
        target: Target,
        permissions: &[Permission],
        fresh_required: bool,
    ) -> Result<AccessDecision> {
        let mut tx = self.tenant_transaction(target.tenant, true).await?;
        let decision =
            Self::evaluate_in(&mut tx, identity, target, permissions, fresh_required).await?;
        tx.commit().await?;
        Ok(decision)
    }

    pub(crate) async fn evaluate_in(
        tx: &mut Transaction<'_, Postgres>,
        identity: &SessionInfo,
        target: Target,
        permissions: &[Permission],
        fresh_required: bool,
    ) -> Result<AccessDecision> {
        let fresh = session_state(tx, identity, target.tenant).await?;
        let names: Vec<_> = permissions.iter().map(|p| p.as_str()).collect();
        if let Some(device) = target.device {
            let traits = device_traits(tx, target.tenant, Some(device)).await?;
            let (_, traits) = traits.first().ok_or(AccessError::Denied)?;
            if permissions.iter().any(|p| !traits.eligible(*p)) {
                return Err(AccessError::Denied);
            }
            let granted = device_grants(
                tx,
                target.tenant,
                identity.operator_id,
                &names,
                Some(device),
            )
            .await?;
            if names
                .iter()
                .any(|name| !granted.iter().any(|(_, permission)| permission == name))
            {
                return Err(AccessError::Denied);
            }
        } else {
            if permissions.iter().any(|p| p.requires_device()) {
                return Err(AccessError::Denied);
            }
            let rules = tenant_rules(tx, target.tenant, identity.operator_id, &names).await?;
            let held = allowed(&rules);
            if permissions.iter().any(|p| !held.contains(p)) {
                return Err(AccessError::Denied);
            }
        }
        if fresh_required && permissions.iter().any(|p| p.requires_fresh_passkey()) && !fresh {
            return Err(AccessError::FreshPasskeyRequired);
        }
        Ok(AccessDecision {
            session_id: identity.id,
            operator_id: identity.operator_id,
            tenant_id: target.tenant,
            device_id: target.device,
            permissions: permissions.to_vec(),
        })
    }

    /// Internal trusted provisioning; expose only behind setup or administrative authorization.
    pub async fn set_membership(
        &self,
        tenant: Uuid,
        operator: Uuid,
        enabled: bool,
        actor: Actor,
    ) -> Result<()> {
        let mut tx = self.tenant_transaction(tenant, false).await?;
        sqlx::query("INSERT INTO midas.tenant_memberships (tenant_id, operator_id, enabled) VALUES ($1,$2,$3) ON CONFLICT (tenant_id,operator_id) DO UPDATE SET enabled=excluded.enabled")
            .bind(tenant).bind(operator).bind(enabled).execute(&mut *tx).await?;
        let mut event =
            Event::new(Action::MembershipSet, Outcome::Success, actor).detail("enabled", enabled);
        event.tenant = Some(tenant);
        event.subject = Some(operator);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Internal trusted provisioning; this does not authorize its caller to manage access.
    pub async fn create_role(&self, name: &str, rules: &[Rule], actor: Actor) -> Result<Uuid> {
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
        let rules: Vec<Value> = rules
            .iter()
            .map(|r| json!({ "permission": r.permission.as_str(), "effect": r.effect.as_str() }))
            .collect();
        let event = Event::new(Action::RoleCreate, Outcome::Success, actor)
            .detail("role_id", id.to_string())
            .detail("name", name)
            .detail("rules", rules);
        audit::append_platform(&mut tx, &event).await?;
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
        actor: Actor,
    ) -> Result<Uuid> {
        let (site, group, device) = scope.columns();
        let id = Uuid::new_v4();
        let mut tx = self.tenant_transaction(tenant, false).await?;
        sqlx::query("INSERT INTO midas.role_assignments (tenant_id,id,operator_id,role_id,site_id,group_id,device_id) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(tenant).bind(id).bind(operator).bind(role).bind(site).bind(group).bind(device).execute(&mut *tx).await?;
        let event = Event::new(Action::RoleAssign, Outcome::Success, actor)
            .detail("assignment_id", id.to_string())
            .detail("role_id", role.to_string());
        audit::append_tenant(
            &mut tx,
            &scoped(event, tenant, operator, site, group, device),
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Internal trusted provisioning; both allows and denials need administrative authorization.
    pub async fn override_permission(
        &self,
        tenant: Uuid,
        operator: Uuid,
        scope: Scope,
        rule: Rule,
        actor: Actor,
    ) -> Result<Uuid> {
        let (site, group, device) = scope.columns();
        let id = Uuid::new_v4();
        let mut tx = self.tenant_transaction(tenant, false).await?;
        sqlx::query("INSERT INTO midas.permission_overrides (tenant_id,id,operator_id,permission,effect,site_id,group_id,device_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(tenant).bind(id).bind(operator).bind(rule.permission.as_str()).bind(rule.effect.as_str()).bind(site).bind(group).bind(device).execute(&mut *tx).await?;
        let event = Event::new(Action::OverrideSet, Outcome::Success, actor)
            .detail("override_id", id.to_string())
            .detail("permission", rule.permission.as_str())
            .detail("effect", rule.effect.as_str());
        audit::append_tenant(
            &mut tx,
            &scoped(event, tenant, operator, site, group, device),
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Internal trusted revocation; the calling administration layer must authorize it.
    pub async fn remove_assignment(&self, tenant: Uuid, id: Uuid, actor: Actor) -> Result<bool> {
        let mut tx = self.tenant_transaction(tenant, false).await?;
        let removed: Option<AssignmentRow> = sqlx::query_as("DELETE FROM midas.role_assignments WHERE tenant_id=$1 AND id=$2 RETURNING operator_id, role_id, site_id, group_id, device_id")
            .bind(tenant).bind(id).fetch_optional(&mut *tx).await?;
        let Some((operator, role, site, group, device)) = removed else {
            return Ok(false);
        };
        let event = Event::new(Action::RoleUnassign, Outcome::Success, actor)
            .detail("assignment_id", id.to_string())
            .detail("role_id", role.to_string());
        audit::append_tenant(
            &mut tx,
            &scoped(event, tenant, operator, site, group, device),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Internal trusted revocation; removing a denial may increase access and also requires authorization.
    pub async fn remove_override(&self, tenant: Uuid, id: Uuid, actor: Actor) -> Result<bool> {
        let mut tx = self.tenant_transaction(tenant, false).await?;
        let removed: Option<OverrideRow> = sqlx::query_as("DELETE FROM midas.permission_overrides WHERE tenant_id=$1 AND id=$2 RETURNING operator_id, permission, effect, site_id, group_id, device_id")
            .bind(tenant).bind(id).fetch_optional(&mut *tx).await?;
        let Some((operator, permission, effect, site, group, device)) = removed else {
            return Ok(false);
        };
        let event = Event::new(Action::OverrideRemove, Outcome::Success, actor)
            .detail("override_id", id.to_string())
            .detail("permission", permission)
            .detail("effect", effect);
        audit::append_tenant(
            &mut tx,
            &scoped(event, tenant, operator, site, group, device),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

/// Session validity and membership; the result tells whether the passkey sign-in is fresh.
async fn session_state(
    tx: &mut Transaction<'_, Postgres>,
    identity: &SessionInfo,
    tenant: Uuid,
) -> Result<bool> {
    let (member, fresh): (bool, bool) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM midas.tenant_memberships m WHERE m.tenant_id = $3 AND m.operator_id = s.operator_id AND m.enabled), s.authentication_method = 'passkey' AND s.authenticated_at >= clock_timestamp() - interval '5 minutes' AND s.authenticated_at <= clock_timestamp() + interval '1 second' FROM midas.browser_sessions s JOIN midas.operators o ON o.id = s.operator_id WHERE s.id = $1 AND s.operator_id = $2 AND o.enabled AND s.credential_version = o.credential_version AND s.expires_at > clock_timestamp() AND s.idle_expires_at > clock_timestamp()")
        .bind(identity.id).bind(identity.operator_id).bind(tenant).fetch_optional(&mut **tx).await?
        .ok_or(AccessError::Unauthenticated)?;
    if !member {
        return Err(AccessError::Denied);
    }
    Ok(fresh)
}

fn allowed(rules: &[(String, bool, bool)]) -> Vec<Permission> {
    Permission::ALL
        .iter()
        .copied()
        .filter(|p| {
            rules
                .iter()
                .any(|(name, allow, deny)| name == p.as_str() && *allow && !*deny)
        })
        .collect()
}

macro_rules! rules {
    () => {
        "SELECT p.permission, p.effect, a.site_id, a.group_id, a.device_id FROM midas.role_assignments a JOIN midas.roles r ON r.id = a.role_id JOIN midas.role_permissions p ON p.role_id = r.id AND (r.enabled OR p.effect = 'deny') WHERE a.tenant_id = $1 AND a.operator_id = $2 AND p.permission = ANY($3) UNION ALL SELECT permission, effect, site_id, group_id, device_id FROM midas.permission_overrides WHERE tenant_id = $1 AND operator_id = $2 AND permission = ANY($3)"
    };
}

// A tenant-wide decision needs a tenant-wide allow, and a denial at any narrower scope also applies.
async fn tenant_rules(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    operator: Uuid,
    names: &[&str],
) -> Result<Vec<(String, bool, bool)>> {
    Ok(sqlx::query_as(concat!(
        "WITH applicable AS (",
        rules!(),
        ") SELECT permission, bool_or(effect = 'allow' AND site_id IS NULL AND group_id IS NULL AND device_id IS NULL), bool_or(effect = 'deny') FROM applicable GROUP BY permission"
    ))
    .bind(tenant)
    .bind(operator)
    .bind(names)
    .fetch_all(&mut **tx)
    .await?)
}

// A rule applies to a device tenant-wide or through its site, one of its groups or the device itself; any applicable denial wins.
async fn device_grants(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    operator: Uuid,
    names: &[&str],
    device: Option<Uuid>,
) -> Result<Vec<(Uuid, String)>> {
    Ok(sqlx::query_as(concat!(
        "WITH applicable AS (",
        rules!(),
        "), targets AS (SELECT d.id, d.site_id, ARRAY(SELECT g.group_id FROM midas.device_group_members g WHERE g.tenant_id = d.tenant_id AND g.device_id = d.id) AS groups FROM midas.devices d WHERE d.tenant_id = $1 AND ($4::uuid IS NULL OR d.id = $4)) SELECT t.id, a.permission FROM targets t JOIN applicable a ON (a.site_id IS NULL AND a.group_id IS NULL AND a.device_id IS NULL) OR a.site_id = t.site_id OR a.group_id = ANY(t.groups) OR a.device_id = t.id GROUP BY t.id, a.permission HAVING bool_or(a.effect = 'allow') AND NOT bool_or(a.effect = 'deny')"
    ))
    .bind(tenant)
    .bind(operator)
    .bind(names)
    .bind(device)
    .fetch_all(&mut **tx)
    .await?)
}

async fn device_traits(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    device: Option<Uuid>,
) -> Result<Vec<(Uuid, Traits)>> {
    let rows: Vec<(Uuid, String, String, bool, bool)> = sqlx::query_as("SELECT d.id, d.kind, d.platform, d.unattended_enabled, t.unattended_enabled FROM midas.devices d JOIN midas.tenants t ON t.id = d.tenant_id WHERE d.tenant_id = $1 AND ($2::uuid IS NULL OR d.id = $2)")
        .bind(tenant).bind(device).fetch_all(&mut **tx).await?;
    Ok(rows
        .into_iter()
        .map(|(id, kind, platform, device_policy, tenant_policy)| {
            (
                id,
                Traits {
                    server: kind == "server",
                    windows: platform == "windows",
                    unattended: device_policy && tenant_policy,
                },
            )
        })
        .collect())
}

fn scoped(
    mut event: Event,
    tenant: Uuid,
    operator: Uuid,
    site: Option<Uuid>,
    group: Option<Uuid>,
    device: Option<Uuid>,
) -> Event {
    let scope = match (site, group, device) {
        (Some(id), _, _) => json!({ "kind": "site", "id": id.to_string() }),
        (_, Some(id), _) => json!({ "kind": "group", "id": id.to_string() }),
        (_, _, Some(id)) => json!({ "kind": "device", "id": id.to_string() }),
        _ => json!({ "kind": "tenant" }),
    };
    event.tenant = Some(tenant);
    event.subject = Some(operator);
    event.device = device;
    event.detail("scope", scope)
}
