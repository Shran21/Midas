use std::{net::IpAddr, sync::Arc, time::Duration};

use ldap3::{LdapConnAsync, LdapConnSettings, Scope as LdapScope, SearchEntry, ldap_escape};
use rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer, pki_types::pem::PemObject};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{
    AuthError, AuthService, Authenticated, AuthenticationMethod, Result, authenticated,
    normalize_login,
};
use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    authorization::Scope,
};

/// Active Directory `userAccountControl` flag for a disabled account.
const ACCOUNT_DISABLE: i64 = 0x2;

pub struct DirectoryConfig<'a> {
    pub domain: &'a str,
    pub url: &'a str,
    pub base_dn: &'a str,
    pub ca_pem: &'a str,
}

struct Directory {
    tenant: Uuid,
    id: Uuid,
    url: String,
    base_dn: String,
    ca_pem: String,
}

struct DirectoryUser {
    external_id: Vec<u8>,
    display_name: String,
    groups: Vec<String>,
}

fn client_config(ca_pem: &str) -> Result<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(ca_pem.as_bytes()) {
        roots
            .add(cert.map_err(|_| AuthError::InvalidInput)?)
            .map_err(|_| AuthError::InvalidInput)?;
    }
    if roots.is_empty() {
        return Err(AuthError::InvalidInput);
    }
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| AuthError::Unavailable)?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Ok(Arc::new(config))
}

async fn lookup(directory: &Directory, upn: &str, password: &str) -> Result<DirectoryUser> {
    // An empty password would perform an unauthenticated bind that many directories accept.
    if password.is_empty() {
        return Err(AuthError::InvalidCredentials);
    }
    let settings = LdapConnSettings::new()
        .set_config(client_config(&directory.ca_pem)?)
        .set_conn_timeout(Duration::from_secs(10));
    let (connection, mut ldap) = LdapConnAsync::with_settings(settings, &directory.url)
        .await
        .map_err(|_| AuthError::Unavailable)?;
    ldap3::drive!(connection);
    ldap.with_timeout(Duration::from_secs(10))
        .simple_bind(upn, password)
        .await
        .map_err(|_| AuthError::Unavailable)?
        .success()
        .map_err(|_| AuthError::InvalidCredentials)?;
    let filter = format!("(userPrincipalName={})", ldap_escape(upn));
    let (entries, _) = ldap
        .with_timeout(Duration::from_secs(10))
        .search(
            &directory.base_dn,
            LdapScope::Subtree,
            &filter,
            vec![
                "objectGUID",
                "userAccountControl",
                "memberOf",
                "displayName",
            ],
        )
        .await
        .map_err(|_| AuthError::Unavailable)?
        .success()
        .map_err(|_| AuthError::InvalidCredentials)?;
    let _ = ldap.unbind().await;
    let [entry] = <[_; 1]>::try_from(entries).map_err(|_| AuthError::InvalidCredentials)?;
    let entry = SearchEntry::construct(entry);
    let control: i64 = entry
        .attrs
        .get("userAccountControl")
        .and_then(|v| v.first())
        .and_then(|v| v.parse().ok())
        .ok_or(AuthError::InvalidCredentials)?;
    if control & ACCOUNT_DISABLE != 0 {
        return Err(AuthError::InvalidCredentials);
    }
    let external_id = entry
        .bin_attrs
        .get("objectGUID")
        .and_then(|v| v.first())
        .cloned()
        .or_else(|| {
            entry
                .attrs
                .get("objectGUID")
                .and_then(|v| v.first())
                .map(|v| v.as_bytes().to_vec())
        })
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .ok_or(AuthError::InvalidCredentials)?;
    let display_name = entry
        .attrs
        .get("displayName")
        .and_then(|v| v.first())
        .map(|v| v.trim().chars().take(200).collect::<String>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| upn.to_owned());
    let groups = entry
        .attrs
        .get("memberOf")
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|g| g.to_lowercase())
        .collect();
    Ok(DirectoryUser {
        external_id,
        display_name,
        groups,
    })
}

impl AuthService {
    /// Trusted provisioning only; exposed later behind tenant access management.
    pub async fn configure_directory(
        &self,
        tenant: Uuid,
        config: DirectoryConfig<'_>,
        actor: Actor,
    ) -> Result<Uuid> {
        client_config(config.ca_pem)?;
        let id = Uuid::new_v4();
        let domain = config.domain.trim().to_ascii_lowercase();
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO midas.directories (tenant_id, id, domain, url, base_dn, ca_pem) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(tenant).bind(id).bind(&domain).bind(config.url).bind(config.base_dn).bind(config.ca_pem)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO midas.directory_domains (domain, tenant_id, directory_id) VALUES ($1, $2, $3)")
            .bind(&domain).bind(tenant).bind(id).execute(&mut *tx).await?;
        let mut event = Event::new(Action::DirectoryConfigure, Outcome::Success, actor)
            .detail("directory_id", id.to_string())
            .detail("domain", domain.as_str());
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Trusted provisioning only: members of `group_dn` receive `role` in `scope` at their next sign-in.
    pub async fn map_directory_group(
        &self,
        tenant: Uuid,
        directory: Uuid,
        group_dn: &str,
        role: Uuid,
        scope: Scope,
        actor: Actor,
    ) -> Result<Uuid> {
        let (site, group, device) = match scope {
            Scope::Tenant => (None, None, None),
            Scope::Site(id) => (Some(id), None, None),
            Scope::Group(id) => (None, Some(id), None),
            Scope::Device(id) => (None, None, Some(id)),
        };
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO midas.directory_groups (tenant_id, id, directory_id, group_dn, role_id, site_id, group_id, device_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
            .bind(tenant).bind(id).bind(directory).bind(group_dn.trim().to_lowercase()).bind(role).bind(site).bind(group).bind(device)
            .execute(&mut *tx).await?;
        let mut event = Event::new(Action::DirectoryGroupMap, Outcome::Success, actor)
            .detail("mapping_id", id.to_string())
            .detail("role_id", role.to_string());
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Binds as the user over verified LDAPS; the password is used only for that bind.
    pub async fn login_directory(
        &self,
        login: &str,
        password: Zeroizing<String>,
        source: IpAddr,
    ) -> Result<Authenticated> {
        let upn = normalize_login(login).map_err(|_| AuthError::InvalidCredentials)?;
        self.limit(&upn, source).await?;
        let domain = upn
            .rsplit_once('@')
            .map(|(_, d)| d.to_owned())
            .ok_or(AuthError::InvalidCredentials)?;
        let route: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT tenant_id, directory_id FROM midas.directory_domains WHERE domain = $1",
        )
        .bind(&domain)
        .fetch_optional(&self.pool)
        .await?;
        let (tenant, id) = route.ok_or(AuthError::InvalidCredentials)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let row: Option<(String, String, String)> = sqlx::query_as("SELECT url, base_dn, ca_pem FROM midas.directories WHERE tenant_id = $1 AND id = $2 AND enabled")
            .bind(tenant).bind(id).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        let (url, base_dn, ca_pem) = row.ok_or(AuthError::InvalidCredentials)?;
        let directory = Directory {
            tenant,
            id,
            url,
            base_dn,
            ca_pem,
        };
        let user = lookup(&directory, &upn, &password).await?;
        self.synchronise(&directory, &upn, user).await
    }

    async fn synchronise(
        &self,
        directory: &Directory,
        upn: &str,
        user: DirectoryUser,
    ) -> Result<Authenticated> {
        let tenant = directory.tenant;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let mappings: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, group_dn FROM midas.directory_groups WHERE tenant_id = $1 AND directory_id = $2")
            .bind(tenant).bind(directory.id).fetch_all(&mut *tx).await?;
        let granted: Vec<Uuid> = mappings
            .iter()
            .filter(|(_, dn)| user.groups.iter().any(|g| g == dn))
            .map(|(id, _)| *id)
            .collect();
        let linked: Option<Uuid> = sqlx::query_scalar("SELECT operator_id FROM midas.directory_identities WHERE tenant_id = $1 AND directory_id = $2 AND external_id = $3")
            .bind(tenant).bind(directory.id).bind(&user.external_id).fetch_optional(&mut *tx).await?;
        let operator = match linked {
            Some(operator) => operator,
            None if granted.is_empty() => return Err(AuthError::InvalidCredentials),
            None => {
                // A local account with the same login is never taken over by a directory identity.
                let operator = self
                    .create_operator(upn, &user.display_name)
                    .await
                    .map_err(|e| match e {
                        AuthError::Conflict => AuthError::InvalidCredentials,
                        other => other,
                    })?;
                sqlx::query("INSERT INTO midas.directory_identities (tenant_id, directory_id, external_id, operator_id) VALUES ($1, $2, $3, $4)")
                    .bind(tenant).bind(directory.id).bind(&user.external_id).bind(operator).execute(&mut *tx).await?;
                operator
            }
        };
        let removed: Vec<Uuid> = sqlx::query_scalar("DELETE FROM midas.role_assignments WHERE tenant_id = $1 AND operator_id = $2 AND directory_group_id IS NOT NULL AND NOT (directory_group_id = ANY($3)) RETURNING id")
            .bind(tenant).bind(operator).bind(&granted).fetch_all(&mut *tx).await?;
        let actor = Actor::SYSTEM;
        for id in &removed {
            let mut event = Event::new(Action::RoleUnassign, Outcome::Success, actor)
                .detail("assignment_id", id.to_string())
                .detail("source", "directory");
            event.tenant = Some(tenant);
            event.subject = Some(operator);
            audit::append_tenant(&mut tx, &event).await?;
        }
        if granted.is_empty() {
            tx.commit().await?;
            return Err(AuthError::InvalidCredentials);
        }
        sqlx::query("INSERT INTO midas.tenant_memberships (tenant_id, operator_id, enabled) VALUES ($1, $2, true) ON CONFLICT (tenant_id, operator_id) DO NOTHING")
            .bind(tenant).bind(operator).execute(&mut *tx).await?;
        let added: Vec<Uuid> = sqlx::query_scalar("INSERT INTO midas.role_assignments (tenant_id, id, operator_id, role_id, site_id, group_id, device_id, directory_group_id) SELECT g.tenant_id, gen_random_uuid(), $2, g.role_id, g.site_id, g.group_id, g.device_id, g.id FROM midas.directory_groups g WHERE g.tenant_id = $1 AND g.id = ANY($3) AND NOT EXISTS (SELECT 1 FROM midas.role_assignments a WHERE a.tenant_id = $1 AND a.operator_id = $2 AND a.directory_group_id = g.id) RETURNING id")
            .bind(tenant).bind(operator).bind(&granted).fetch_all(&mut *tx).await?;
        for id in &added {
            let mut event = Event::new(Action::RoleAssign, Outcome::Success, actor)
                .detail("assignment_id", id.to_string())
                .detail("source", "directory");
            event.tenant = Some(tenant);
            event.subject = Some(operator);
            audit::append_tenant(&mut tx, &event).await?;
        }
        let version: Option<i64> = sqlx::query_scalar(
            "SELECT credential_version FROM midas.operators WHERE id = $1 AND enabled",
        )
        .bind(operator)
        .fetch_optional(&mut *tx)
        .await?;
        let version = version.ok_or(AuthError::InvalidCredentials)?;
        tx.commit().await?;
        Ok(authenticated(
            operator,
            version,
            AuthenticationMethod::Directory,
        ))
    }
}
