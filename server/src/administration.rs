use crate::database::driver as sqlx;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::{
        AuthService,
        sessions::{SessionInfo, SessionToken, Sessions},
    },
    authorization::{
        AccessError, Authorizer, BuiltinRole, Effect, Permission, Rule, Scope, Target,
        TenantSummary,
    },
    database::{Database, Site},
};

type Result<T> = std::result::Result<T, AccessError>;

#[derive(Debug, Serialize)]
pub struct Assignment {
    pub id: Uuid,
    pub role_id: Uuid,
    pub role_name: String,
    pub scope: Scope,
}

#[derive(Debug, Serialize)]
pub struct Override {
    pub id: Uuid,
    pub permission: String,
    pub effect: String,
    pub scope: Scope,
}

#[derive(Debug, Serialize)]
pub struct Member {
    pub operator_id: Uuid,
    pub login: String,
    pub display_name: String,
    pub enabled: bool,
    pub assignments: Vec<Assignment>,
    pub overrides: Vec<Override>,
}

#[derive(Debug, Serialize)]
pub struct RoleView {
    pub id: Uuid,
    pub name: String,
    pub builtin: bool,
    pub allows: Vec<String>,
    pub denies: Vec<String>,
}

/// Returned once; the recovery codes are never stored in plaintext or shown again.
#[derive(Debug, Serialize)]
pub struct CreatedOperator {
    pub id: Uuid,
    pub login: String,
    pub display_name: String,
    pub recovery_codes: Vec<String>,
}

type MemberRow = (Uuid, String, String, bool);
type RoleRow = (Uuid, String, bool, Vec<String>, Vec<String>);
type AssignmentRow = (
    Uuid,
    Uuid,
    Uuid,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
);
type OverrideRow = (
    Uuid,
    Uuid,
    String,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
);

pub struct Administration {
    pool: PgPool,
    authorizer: Authorizer,
}

impl Administration {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
            authorizer: Authorizer::new(database),
        }
    }

    async fn tenant_tx(&self, tenant: Uuid) -> Result<Transaction<'_, Postgres>> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    /// Platform administrators need a passkey sign-in within five minutes for every change.
    async fn platform(&self, sessions: &Sessions, token: &SessionToken) -> Result<SessionInfo> {
        let identity = sessions.validate(token, false).await?;
        let (admin, fresh): (bool, bool) = sqlx::query_as("SELECT o.platform_admin, s.authentication_method = 'passkey' AND s.authenticated_at >= clock_timestamp() - interval '5 minutes' AND s.authenticated_at <= clock_timestamp() + interval '1 second' FROM midas.browser_sessions s JOIN midas.operators o ON o.id = s.operator_id WHERE s.id = $1 AND o.enabled AND s.credential_version = o.credential_version AND s.expires_at > clock_timestamp() AND s.idle_expires_at > clock_timestamp()")
            .bind(identity.id).fetch_optional(&self.pool).await?
            .ok_or(AccessError::Unauthenticated)?;
        if !admin {
            return Err(AccessError::Denied);
        }
        if !fresh {
            return Err(AccessError::FreshPasskeyRequired);
        }
        Ok(identity)
    }

    async fn manager(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        change: bool,
    ) -> Result<SessionInfo> {
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: None,
        };
        if change {
            self.authorizer
                .authorize_identity(&identity, target, Permission::AccessManage)
                .await?;
        } else {
            self.authorizer
                .authorize_view(&identity, target, Permission::AccessManage)
                .await?;
        }
        Ok(identity)
    }

    async fn ensure_held(
        &self,
        identity: &SessionInfo,
        tenant: Uuid,
        permissions: &[String],
    ) -> Result<()> {
        let held = self.authorizer.held(identity, tenant).await?;
        if permissions
            .iter()
            .any(|p| !held.iter().any(|h| h.as_str() == p))
        {
            return Err(AccessError::Escalation);
        }
        Ok(())
    }

    /// Trusted provisioning only, for setup and local development; not exposed over HTTP.
    pub async fn set_platform_admin(
        &self,
        operator: Uuid,
        enabled: bool,
        actor: Actor,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE midas.operators SET platform_admin = $2 WHERE id = $1")
            .bind(operator)
            .bind(enabled)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed != 1 {
            return Err(AccessError::InvalidInput);
        }
        let mut event = Event::new(Action::PlatformAdminSet, Outcome::Success, actor)
            .detail("enabled", enabled);
        event.subject = Some(operator);
        audit::append_platform(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn is_platform_admin(&self, operator: Uuid) -> Result<bool> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT platform_admin FROM midas.operators WHERE id = $1 AND enabled",
        )
        .bind(operator)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false))
    }

    /// The creator becomes an enabled member and tenant-wide Administrator of the new tenant.
    pub async fn create_tenant(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        name: &str,
    ) -> Result<TenantSummary> {
        let identity = self.platform(sessions, token).await?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 200 {
            return Err(AccessError::InvalidInput);
        }
        let tenant = Uuid::new_v4();
        let assignment = Uuid::new_v4();
        let actor = Actor::session(&identity);
        let mut tx = self.tenant_tx(tenant).await?;
        sqlx::query("INSERT INTO midas.tenants (id, name) VALUES ($1, $2)")
            .bind(tenant)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO midas.tenant_memberships (tenant_id, operator_id) VALUES ($1, $2)",
        )
        .bind(tenant)
        .bind(identity.operator_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO midas.role_assignments (tenant_id, id, operator_id, role_id) VALUES ($1, $2, $3, $4)")
            .bind(tenant).bind(assignment).bind(identity.operator_id).bind(BuiltinRole::Administrator.id())
            .execute(&mut *tx).await?;
        let mut created =
            Event::new(Action::TenantCreate, Outcome::Success, actor).detail("name", name);
        created.tenant = Some(tenant);
        let mut membership =
            Event::new(Action::MembershipSet, Outcome::Success, actor).detail("enabled", true);
        membership.tenant = Some(tenant);
        membership.subject = Some(identity.operator_id);
        let mut role = Event::new(Action::RoleAssign, Outcome::Success, actor)
            .detail("assignment_id", assignment.to_string())
            .detail("role_id", BuiltinRole::Administrator.id().to_string())
            .detail("scope", serde_json::json!({ "kind": "tenant" }));
        role.tenant = Some(tenant);
        role.subject = Some(identity.operator_id);
        for event in [&created, &membership, &role] {
            audit::append_tenant(&mut tx, event).await?;
        }
        let mut platform =
            Event::new(Action::TenantCreate, Outcome::Success, actor).detail("name", name);
        platform.tenant = Some(tenant);
        audit::append_platform(&mut tx, &platform).await?;
        tx.commit().await?;
        Ok(TenantSummary {
            id: tenant,
            name: name.to_owned(),
        })
    }

    pub async fn create_operator(
        &self,
        auth: &AuthService,
        sessions: &Sessions,
        token: &SessionToken,
        login: &str,
        display_name: &str,
    ) -> Result<CreatedOperator> {
        let identity = self.platform(sessions, token).await?;
        let display_name = display_name.trim();
        if display_name.is_empty() || display_name.chars().count() > 200 {
            return Err(AccessError::InvalidInput);
        }
        let id = auth
            .create_operator(login, display_name)
            .await
            .map_err(|e| match e {
                crate::auth::AuthError::InvalidInput => AccessError::InvalidInput,
                crate::auth::AuthError::Conflict => AccessError::Conflict,
                _ => AccessError::Unavailable,
            })?;
        let codes: Vec<Zeroizing<String>> = auth
            .replace_recovery_codes(id)
            .await
            .map_err(|_| AccessError::Unavailable)?;
        let login: String = sqlx::query_scalar("SELECT login FROM midas.operators WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let mut event = Event::new(
            Action::OperatorCreate,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("login", login.as_str());
        event.subject = Some(id);
        let mut connection = self.pool.acquire().await?;
        audit::append_platform(&mut connection, &event).await?;
        Ok(CreatedOperator {
            id,
            login,
            display_name: display_name.to_owned(),
            recovery_codes: codes.iter().map(|c| c.to_string()).collect(),
        })
    }

    pub async fn roles(&self, sessions: &Sessions, token: &SessionToken) -> Result<Vec<RoleView>> {
        sessions.validate(token, false).await?;
        let rows: Vec<RoleRow> = sqlx::query_as("SELECT r.id, r.name, r.builtin IS NOT NULL, coalesce(array_agg(p.permission ORDER BY p.permission) FILTER (WHERE p.effect = 'allow'), '{}'), coalesce(array_agg(p.permission ORDER BY p.permission) FILTER (WHERE p.effect = 'deny'), '{}') FROM midas.roles r LEFT JOIN midas.role_permissions p ON p.role_id = r.id WHERE r.enabled GROUP BY r.id ORDER BY r.builtin IS NULL, lower(r.name) LIMIT 500")
            .fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, builtin, allows, denies)| RoleView {
                id,
                name,
                builtin,
                allows,
                denies,
            })
            .collect())
    }

    pub async fn members(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Vec<Member>> {
        self.manager(sessions, token, tenant, false).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let members: Vec<MemberRow> = sqlx::query_as("SELECT m.operator_id, o.login, o.display_name, m.enabled FROM midas.tenant_memberships m JOIN midas.operators o ON o.id = m.operator_id WHERE m.tenant_id = $1 ORDER BY lower(o.display_name), o.login LIMIT 1000")
            .bind(tenant).fetch_all(&mut *tx).await?;
        let assignments: Vec<AssignmentRow> = sqlx::query_as("SELECT a.id, a.operator_id, a.role_id, r.name, a.site_id, a.group_id, a.device_id FROM midas.role_assignments a JOIN midas.roles r ON r.id = a.role_id WHERE a.tenant_id = $1 ORDER BY lower(r.name), a.id")
            .bind(tenant).fetch_all(&mut *tx).await?;
        let overrides: Vec<OverrideRow> = sqlx::query_as("SELECT id, operator_id, permission, effect, site_id, group_id, device_id FROM midas.permission_overrides WHERE tenant_id = $1 ORDER BY permission, id")
            .bind(tenant).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(members
            .into_iter()
            .map(|(operator_id, login, display_name, enabled)| Member {
                operator_id,
                login,
                display_name,
                enabled,
                assignments: assignments
                    .iter()
                    .filter(|a| a.1 == operator_id)
                    .map(|a| Assignment {
                        id: a.0,
                        role_id: a.2,
                        role_name: a.3.clone(),
                        scope: Scope::from_columns(a.4, a.5, a.6),
                    })
                    .collect(),
                overrides: overrides
                    .iter()
                    .filter(|o| o.1 == operator_id)
                    .map(|o| Override {
                        id: o.0,
                        permission: o.2.clone(),
                        effect: o.3.clone(),
                        scope: Scope::from_columns(o.4, o.5, o.6),
                    })
                    .collect(),
            })
            .collect())
    }

    pub async fn sites(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Vec<Site>> {
        self.manager(sessions, token, tenant, false).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let sites = sqlx::query_as("SELECT id, name FROM midas.sites WHERE tenant_id = $1 ORDER BY lower(name), id LIMIT 1000")
            .bind(tenant).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(sites)
    }

    pub async fn create_site(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        name: &str,
    ) -> Result<Site> {
        let identity = self.manager(sessions, token, tenant, true).await?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 200 {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        let site: Site = sqlx::query_as(
            "INSERT INTO midas.sites (tenant_id, id, name) VALUES ($1, $2, $3) RETURNING id, name",
        )
        .bind(tenant)
        .bind(Uuid::new_v4())
        .bind(name)
        .fetch_one(&mut *tx)
        .await?;
        let mut event = Event::new(
            Action::SiteCreate,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("site_id", site.id.to_string())
        .detail("name", name);
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(site)
    }

    /// Adds an existing operator by exact login; unknown logins are indistinguishable from conflicts elsewhere.
    pub async fn add_member(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        login: &str,
    ) -> Result<Uuid> {
        let identity = self.manager(sessions, token, tenant, true).await?;
        let login = login.trim().to_ascii_lowercase();
        let operator: Uuid =
            sqlx::query_scalar("SELECT id FROM midas.operators WHERE login = $1 AND enabled")
                .bind(&login)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(AccessError::Denied)?;
        self.set_enabled(&identity, tenant, operator, true).await?;
        Ok(operator)
    }

    pub async fn set_member(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        operator: Uuid,
        enabled: bool,
    ) -> Result<()> {
        let identity = self.manager(sessions, token, tenant, true).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.tenant_memberships WHERE tenant_id = $1 AND operator_id = $2)")
            .bind(tenant).bind(operator).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        if !exists {
            return Err(AccessError::Denied);
        }
        self.set_enabled(&identity, tenant, operator, enabled).await
    }

    async fn set_enabled(
        &self,
        identity: &SessionInfo,
        tenant: Uuid,
        operator: Uuid,
        enabled: bool,
    ) -> Result<()> {
        if enabled {
            let mut tx = self.tenant_tx(tenant).await?;
            let granted: Vec<String> = sqlx::query_scalar("SELECT p.permission FROM midas.role_assignments a JOIN midas.roles r ON r.id = a.role_id AND r.enabled JOIN midas.role_permissions p ON p.role_id = r.id AND p.effect = 'allow' WHERE a.tenant_id = $1 AND a.operator_id = $2 UNION SELECT permission FROM midas.permission_overrides WHERE tenant_id = $1 AND operator_id = $2 AND effect = 'allow'")
                .bind(tenant).bind(operator).fetch_all(&mut *tx).await?;
            tx.commit().await?;
            self.ensure_held(identity, tenant, &granted).await?;
        }
        self.authorizer
            .set_membership(tenant, operator, enabled, Actor::session(identity))
            .await
    }

    pub async fn assign_role(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        operator: Uuid,
        role: Uuid,
        scope: Scope,
    ) -> Result<Uuid> {
        let identity = self.manager(sessions, token, tenant, true).await?;
        let allows: Vec<String> = sqlx::query_scalar("SELECT p.permission FROM midas.roles r JOIN midas.role_permissions p ON p.role_id = r.id WHERE r.id = $1 AND r.enabled AND p.effect = 'allow'")
            .bind(role).fetch_all(&self.pool).await?;
        let known: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM midas.roles WHERE id = $1 AND enabled)",
        )
        .bind(role)
        .fetch_one(&self.pool)
        .await?;
        if !known {
            return Err(AccessError::InvalidInput);
        }
        self.ensure_held(&identity, tenant, &allows).await?;
        self.authorizer
            .assign_role(tenant, operator, role, scope, Actor::session(&identity))
            .await
    }

    pub async fn remove_assignment(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        assignment: Uuid,
    ) -> Result<bool> {
        let identity = self.manager(sessions, token, tenant, true).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let denies: Option<Vec<String>> = sqlx::query_scalar("SELECT coalesce(array_agg(p.permission) FILTER (WHERE p.effect = 'deny'), '{}') FROM midas.role_assignments a LEFT JOIN midas.role_permissions p ON p.role_id = a.role_id WHERE a.tenant_id = $1 AND a.id = $2 GROUP BY a.id")
            .bind(tenant).bind(assignment).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        let Some(denies) = denies else {
            return Ok(false);
        };
        self.ensure_held(&identity, tenant, &denies).await?;
        self.authorizer
            .remove_assignment(tenant, assignment, Actor::session(&identity))
            .await
    }

    pub async fn set_override(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        operator: Uuid,
        scope: Scope,
        rule: Rule,
    ) -> Result<Uuid> {
        let identity = self.manager(sessions, token, tenant, true).await?;
        if matches!(rule.effect, Effect::Allow) {
            self.ensure_held(&identity, tenant, &[rule.permission.as_str().to_owned()])
                .await?;
        }
        self.authorizer
            .override_permission(tenant, operator, scope, rule, Actor::session(&identity))
            .await
    }

    pub async fn remove_override(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        id: Uuid,
    ) -> Result<bool> {
        let identity = self.manager(sessions, token, tenant, true).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let rule: Option<(String, String)> = sqlx::query_as("SELECT permission, effect FROM midas.permission_overrides WHERE tenant_id = $1 AND id = $2")
            .bind(tenant).bind(id).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        let Some((permission, effect)) = rule else {
            return Ok(false);
        };
        if effect == "deny" {
            self.ensure_held(&identity, tenant, &[permission]).await?;
        }
        self.authorizer
            .remove_override(tenant, id, Actor::session(&identity))
            .await
    }
}
