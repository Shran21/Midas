use midas_protocol::{InventoryItem, InventoryUpdate, MAX_INVENTORY_ITEMS, MAX_INVENTORY_TEXT};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    auth::sessions::{SessionToken, Sessions},
    authorization::{AccessError, Permission, Target},
    enrollment::{AgentIdentity, Enrollment},
};

#[derive(Debug, Serialize)]
pub struct InventoryView {
    pub reconciled_at: Option<i64>,
    pub items: Vec<InventoryEntry>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct InventoryEntry {
    pub kind: String,
    pub key: String,
    pub value: String,
    pub updated_at: i64,
}

fn valid_kind(kind: &str) -> bool {
    (1..=16).contains(&kind.len()) && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

fn valid_text(text: &str, allow_empty: bool) -> bool {
    (allow_empty || !text.is_empty())
        && !text.contains('\0')
        && text.chars().count() <= MAX_INVENTORY_TEXT
}

fn columns(items: &[InventoryItem]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut columns = (Vec::new(), Vec::new(), Vec::new());
    for item in items {
        let Some(key) = &item.key else {
            continue;
        };
        if valid_kind(&key.kind) && valid_text(&key.key, false) && valid_text(&item.value, true) {
            columns.0.push(key.kind.clone());
            columns.1.push(key.key.clone());
            columns.2.push(item.value.clone());
        }
    }
    columns
}

impl Enrollment {
    /// Applies one chunk. A change set for another generation is refused, so the agent
    /// reconciles fully; a full reconciliation's last chunk removes everything not resent.
    pub async fn apply_inventory(
        &self,
        identity: AgentIdentity,
        update: &InventoryUpdate,
    ) -> Result<bool, sqlx::Error> {
        if update.upserts.len() + update.removals.len() > MAX_INVENTORY_ITEMS {
            return Err(sqlx::Error::Protocol("inventory chunk too large".into()));
        }
        let Ok(generation) = i64::try_from(update.generation) else {
            return Ok(false);
        };
        let mut tx = self.tenant_tx(identity.tenant).await?;
        let stored: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT inventory_generation FROM midas.devices WHERE id = $1 FOR UPDATE",
        )
        .bind(identity.device)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        if !update.full && stored != Some(generation) {
            return Ok(false);
        }
        let (kinds, keys, values) = columns(&update.upserts);
        sqlx::query("INSERT INTO midas.device_inventory (tenant_id, device_id, kind, key, value, generation) SELECT $1, $2, k, n, v, $6 FROM unnest($3::text[], $4::text[], $5::text[]) AS u(k, n, v) ON CONFLICT (tenant_id, device_id, kind, key) DO UPDATE SET generation = EXCLUDED.generation, updated_at = CASE WHEN midas.device_inventory.value IS DISTINCT FROM EXCLUDED.value THEN clock_timestamp() ELSE midas.device_inventory.updated_at END, value = EXCLUDED.value")
            .bind(identity.tenant).bind(identity.device).bind(&kinds).bind(&keys).bind(&values).bind(generation)
            .execute(&mut *tx).await?;
        if !update.full && !update.removals.is_empty() {
            let (kinds, keys): (Vec<_>, Vec<_>) = update
                .removals
                .iter()
                .map(|key| (key.kind.clone(), key.key.clone()))
                .unzip();
            sqlx::query("DELETE FROM midas.device_inventory d USING unnest($2::text[], $3::text[]) AS u(k, n) WHERE d.device_id = $1 AND d.kind = u.k AND d.key = u.n")
                .bind(identity.device).bind(&kinds).bind(&keys).execute(&mut *tx).await?;
        }
        if update.full && update.last {
            sqlx::query(
                "DELETE FROM midas.device_inventory WHERE device_id = $1 AND generation <> $2",
            )
            .bind(identity.device)
            .bind(generation)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE midas.devices SET inventory_generation = $2, inventory_reconciled_at = clock_timestamp() WHERE id = $1")
                .bind(identity.device).bind(generation).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn inventory(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<InventoryView, AccessError> {
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
        let reconciled: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT extract(epoch FROM inventory_reconciled_at)::bigint FROM midas.devices WHERE id = $1",
        )
        .bind(device)
        .fetch_optional(&mut *tx)
        .await?;
        let reconciled_at = reconciled.ok_or(AccessError::Denied)?;
        let items = sqlx::query_as("SELECT kind, key, value, extract(epoch FROM updated_at)::bigint AS updated_at FROM midas.device_inventory WHERE device_id = $1 ORDER BY kind, key")
            .bind(device).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(InventoryView {
            reconciled_at,
            items,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use midas_protocol::InventoryKey;

    #[test]
    fn invalid_items_are_skipped_instead_of_failing_the_chunk() {
        let item = |kind: &str, key: &str, value: &str| InventoryItem {
            key: Some(InventoryKey {
                kind: kind.into(),
                key: key.into(),
            }),
            value: value.into(),
        };
        let items = [
            item("software", "ok", "1.0"),
            item("software", "nul\0", "1.0"),
            item("software", "value", "1\0"),
            item("Software", "case", ""),
            item("software", "", "empty key"),
            item("software", &"x".repeat(257), ""),
        ];
        let (kinds, keys, values) = columns(&items);
        assert_eq!(
            (kinds.len(), keys[0].as_str(), values[0].as_str()),
            (1, "ok", "1.0")
        );
    }
}
