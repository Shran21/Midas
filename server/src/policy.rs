use std::collections::HashMap;

use midas_protocol::{PolicyAssignment, PolicySetting, policy};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::sessions::{SessionInfo, SessionToken, Sessions},
    authorization::{AccessError, Permission, Target},
    enrollment::{AgentIdentity, Enrollment},
};

type Result<T> = std::result::Result<T, AccessError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Tenant,
    Site,
    Group,
}

impl Level {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Tenant => "tenant",
            Self::Site => "site",
            Self::Group => "group",
        }
    }
}

/// Where an effective value came from; `default` has no scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Source {
    pub level: &'static str,
    pub id: Option<Uuid>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectiveSetting {
    pub key: &'static str,
    pub value: i64,
    pub default: i64,
    pub source: Source,
}

#[derive(Debug, Serialize)]
pub struct CatalogueEntry {
    pub key: &'static str,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StoredValue {
    pub level: String,
    pub scope_id: Uuid,
    pub key: String,
    pub value: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct PolicyView {
    pub settings: Vec<CatalogueEntry>,
    pub values: Vec<StoredValue>,
}

#[derive(Debug, Deserialize)]
pub struct ValueSpec {
    pub value: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub priority: i32,
}

#[derive(Debug, Deserialize)]
pub struct GroupSpec {
    pub name: String,
    #[serde(default)]
    pub priority: i32,
}

/// One layer of overrides in the order they apply.
struct Layer {
    source: Source,
    values: HashMap<String, i64>,
}

/// Defaults, then the tenant, the site and the groups in rising priority; later layers win.
fn resolve(layers: &[Layer]) -> Vec<EffectiveSetting> {
    policy::SETTINGS
        .iter()
        .map(|setting| {
            let mut effective = EffectiveSetting {
                key: setting.key,
                value: setting.default,
                default: setting.default,
                source: Source {
                    level: "default",
                    id: None,
                    name: None,
                },
            };
            for layer in layers {
                if let Some(value) = layer.values.get(setting.key) {
                    effective.value = setting.clamp(*value);
                    effective.source = layer.source.clone();
                }
            }
            effective
        })
        .collect()
}

type GroupRow = (Uuid, String);
type ValueRow = (String, Uuid, String, i64);

/// The device's effective settings; `None` for a device outside the transaction's tenant.
pub(crate) async fn effective(
    tx: &mut PgConnection,
    tenant: Uuid,
    device: Uuid,
) -> std::result::Result<Option<Vec<EffectiveSetting>>, sqlx::Error> {
    let row: Option<(String, Option<Uuid>, Option<String>)> = sqlx::query_as("SELECT t.name, d.site_id, s.name FROM midas.devices d JOIN midas.tenants t ON t.id = d.tenant_id LEFT JOIN midas.sites s ON s.tenant_id = d.tenant_id AND s.id = d.site_id WHERE d.id = $1")
        .bind(device).fetch_optional(&mut *tx).await?;
    let Some((tenant_name, site, site_name)) = row else {
        return Ok(None);
    };
    let groups: Vec<GroupRow> = sqlx::query_as("SELECT g.id, g.name FROM midas.device_group_members m JOIN midas.device_groups g ON g.tenant_id = m.tenant_id AND g.id = m.group_id WHERE m.device_id = $1 ORDER BY g.policy_priority, g.name, g.id")
        .bind(device).fetch_all(&mut *tx).await?;
    let group_ids: Vec<Uuid> = groups.iter().map(|(id, _)| *id).collect();
    let rows: Vec<ValueRow> = sqlx::query_as("SELECT level, scope_id, key, value FROM midas.policy_values WHERE level = 'tenant' OR (level = 'site' AND scope_id = $1) OR (level = 'group' AND scope_id = ANY($2))")
        .bind(site).bind(&group_ids).fetch_all(&mut *tx).await?;
    let values = |level: &str, scope: Uuid| -> HashMap<String, i64> {
        rows.iter()
            .filter(|(l, s, _, _)| l == level && *s == scope)
            .map(|(_, _, key, value)| (key.clone(), *value))
            .collect()
    };
    let mut layers = vec![Layer {
        source: Source {
            level: "tenant",
            id: Some(tenant),
            name: Some(tenant_name),
        },
        values: values("tenant", tenant),
    }];
    if let Some(site) = site {
        layers.push(Layer {
            source: Source {
                level: "site",
                id: Some(site),
                name: site_name,
            },
            values: values("site", site),
        });
    }
    for (id, name) in groups {
        layers.push(Layer {
            source: Source {
                level: "group",
                id: Some(id),
                name: Some(name),
            },
            values: values("group", id),
        });
    }
    Ok(Some(resolve(&layers)))
}

impl Enrollment {
    pub(crate) async fn policy_operator(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        permission: Permission,
        change: bool,
    ) -> Result<SessionInfo> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: None,
        };
        if change {
            self.authorizer
                .authorize_identity(&identity, target, permission)
                .await?;
        } else {
            self.authorizer
                .authorize_view(&identity, target, permission)
                .await?;
        }
        Ok(identity)
    }

    /// Wakes every live session of the tenant so each resends its policy if it changed.
    async fn refresh_policies(&self, tenant: Uuid) {
        let Ok(mut tx) = self.tenant_tx(tenant).await else {
            return;
        };
        let devices: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM midas.devices")
            .fetch_all(&mut *tx)
            .await
            .unwrap_or_default();
        let _ = tx.commit().await;
        for device in devices {
            self.signal(device, |signal| signal.policy += 1);
        }
    }

    /// The assignment for an agent session; defaults when nothing overrides them.
    pub async fn policy_assignment(&self, identity: AgentIdentity) -> Option<PolicyAssignment> {
        let mut tx = self.tenant_tx(identity.tenant).await.ok()?;
        let settings = effective(&mut tx, identity.tenant, identity.device)
            .await
            .ok()??;
        tx.commit().await.ok()?;
        Some(PolicyAssignment {
            settings: settings
                .into_iter()
                .map(|setting| PolicySetting {
                    key: setting.key.into(),
                    value: setting.value,
                })
                .collect(),
        })
    }

    pub async fn device_policy(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<Vec<EffectiveSetting>> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: Some(device),
        };
        self.authorizer
            .authorize_view(&identity, target, Permission::DeviceRead)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let settings = effective(&mut tx, tenant, device)
            .await?
            .ok_or(AccessError::Denied)?;
        tx.commit().await?;
        Ok(settings)
    }

    pub async fn policies(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<PolicyView> {
        self.policy_operator(sessions, token, tenant, Permission::PolicyManage, false)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let values = sqlx::query_as("SELECT level, scope_id, key, value, extract(epoch FROM updated_at)::bigint AS updated_at FROM midas.policy_values ORDER BY level, scope_id, key")
            .fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(PolicyView {
            settings: policy::SETTINGS
                .iter()
                .map(|setting| CatalogueEntry {
                    key: setting.key,
                    minimum: setting.minimum,
                    maximum: setting.maximum,
                    default: setting.default,
                })
                .collect(),
            values,
        })
    }

    /// The scope must belong to the tenant; otherwise it is reported as not found.
    async fn scope_exists(
        tx: &mut PgConnection,
        tenant: Uuid,
        level: Level,
        scope: Uuid,
    ) -> Result<()> {
        let exists: bool = match level {
            Level::Tenant => scope == tenant,
            Level::Site => {
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.sites WHERE id = $1)")
                    .bind(scope)
                    .fetch_one(&mut *tx)
                    .await?
            }
            Level::Group => {
                sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM midas.device_groups WHERE id = $1)",
                )
                .bind(scope)
                .fetch_one(&mut *tx)
                .await?
            }
        };
        if exists {
            Ok(())
        } else {
            Err(AccessError::Denied)
        }
    }

    pub async fn set_policy(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        (level, scope, key): (Level, Uuid, &str),
        value: i64,
    ) -> Result<()> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::PolicyManage, true)
            .await?;
        let setting = policy::find(key).ok_or(AccessError::InvalidInput)?;
        if setting.clamp(value) != value {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        Self::scope_exists(&mut tx, tenant, level, scope).await?;
        sqlx::query("INSERT INTO midas.policy_values (tenant_id, level, scope_id, key, value, updated_by) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (tenant_id, level, scope_id, key) DO UPDATE SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = clock_timestamp()")
            .bind(tenant).bind(level.as_str()).bind(scope).bind(setting.key).bind(value).bind(identity.operator_id)
            .execute(&mut *tx).await?;
        let mut event = Event::new(
            Action::PolicySet,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("level", level.as_str())
        .detail("scope_id", scope.to_string())
        .detail("key", setting.key)
        .detail("value", value);
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        self.refresh_policies(tenant).await;
        Ok(())
    }

    /// Removes an override so the scope inherits again; `false` when there was none.
    pub async fn clear_policy(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        (level, scope, key): (Level, Uuid, &str),
    ) -> Result<bool> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::PolicyManage, true)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let removed = sqlx::query(
            "DELETE FROM midas.policy_values WHERE level = $1 AND scope_id = $2 AND key = $3",
        )
        .bind(level.as_str())
        .bind(scope)
        .bind(key)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if removed {
            let mut event = Event::new(
                Action::PolicyClear,
                Outcome::Success,
                Actor::session(&identity),
            )
            .detail("level", level.as_str())
            .detail("scope_id", scope.to_string())
            .detail("key", key);
            event.tenant = Some(tenant);
            audit::append_tenant(&mut tx, &event).await?;
        }
        tx.commit().await?;
        if removed {
            self.refresh_policies(tenant).await;
        }
        Ok(removed)
    }

    pub async fn groups(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Vec<Group>> {
        self.policy_operator(sessions, token, tenant, Permission::AccessManage, false)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let groups = sqlx::query_as("SELECT id, name, policy_priority AS priority FROM midas.device_groups ORDER BY name, id LIMIT 500")
            .fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(groups)
    }

    /// Group membership also scopes role assignments, so it needs `access.manage`.
    pub async fn create_group(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        spec: &GroupSpec,
    ) -> Result<Group> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::AccessManage, true)
            .await?;
        let name = spec.name.trim();
        if name.is_empty() || name.chars().count() > 200 || !(-1000..=1000).contains(&spec.priority)
        {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        let group: Group = sqlx::query_as("INSERT INTO midas.device_groups (tenant_id, id, name, policy_priority) VALUES ($1, $2, $3, $4) RETURNING id, name, policy_priority AS priority")
            .bind(tenant).bind(Uuid::new_v4()).bind(name).bind(spec.priority)
            .fetch_one(&mut *tx).await?;
        let mut event = Event::new(
            Action::GroupCreate,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("group_id", group.id.to_string())
        .detail("name", name)
        .detail("priority", spec.priority);
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(group)
    }

    /// Adds or removes a device; `false` when nothing changed.
    pub async fn set_membership(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        (group, device): (Uuid, Uuid),
        member: bool,
    ) -> Result<bool> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::AccessManage, true)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.device_groups WHERE id = $1) AND EXISTS (SELECT 1 FROM midas.devices WHERE id = $2)")
            .bind(group).bind(device).fetch_one(&mut *tx).await?;
        if !known {
            return Err(AccessError::Denied);
        }
        let changed = if member {
            sqlx::query("INSERT INTO midas.device_group_members (tenant_id, group_id, device_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
                .bind(tenant).bind(group).bind(device).execute(&mut *tx).await?
        } else {
            sqlx::query("DELETE FROM midas.device_group_members WHERE group_id = $1 AND device_id = $2")
                .bind(group).bind(device).execute(&mut *tx).await?
        }
        .rows_affected()
            > 0;
        if changed {
            let action = if member {
                Action::GroupMemberAdd
            } else {
                Action::GroupMemberRemove
            };
            let mut event = Event::new(action, Outcome::Success, Actor::session(&identity))
                .detail("group_id", group.to_string());
            event.tenant = Some(tenant);
            event.device = Some(device);
            audit::append_tenant(&mut tx, &event).await?;
        }
        tx.commit().await?;
        if changed {
            self.signal(device, |signal| signal.policy += 1);
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(level: &'static str, values: &[(&str, i64)]) -> Layer {
        Layer {
            source: Source {
                level,
                id: Some(Uuid::nil()),
                name: Some(level.into()),
            },
            values: values
                .iter()
                .map(|(key, value)| ((*key).to_owned(), *value))
                .collect(),
        }
    }

    #[test]
    fn later_layers_win_and_the_source_follows_the_value() {
        let low = policy::LOW_SPACE_PERCENT.key;
        let days = policy::UPDATE_WARNING_DAYS.key;
        let resolved = resolve(&[
            layer("tenant", &[(low, 15), (days, 20)]),
            layer("site", &[(low, 12)]),
            layer("group", &[(low, 999)]),
        ]);
        let find = |key| resolved.iter().find(|setting| setting.key == key).unwrap();
        assert_eq!((find(low).value, find(low).source.level), (50, "group"));
        assert_eq!((find(days).value, find(days).source.level), (20, "tenant"));
        let critical = find(policy::CRITICAL_SPACE_PERCENT.key);
        assert_eq!((critical.value, critical.source.level), (5, "default"));
        assert_eq!(resolved.len(), policy::SETTINGS.len());
    }
}
