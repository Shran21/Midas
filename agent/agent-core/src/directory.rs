use std::{sync::Arc, time::Duration};

use ldap3::{
    LdapConnAsync, LdapConnSettings, Scope, SearchEntry,
    adapters::{Adapter, EntriesOnly, PagedResults},
};
use midas_protocol::{
    DirectoryAssignment, DirectoryReport,
    directory::{valid_bases, within},
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use zeroize::Zeroizing;

const PAGE_SIZE: i32 = 500;
const TIMEOUT: Duration = Duration::from_secs(30);
const COMPUTERS: &str = "(objectClass=computer)";

/// Read-only directory account, held only in the local agent configuration.
#[derive(Clone)]
pub struct Credentials {
    pub user: String,
    pub password: Zeroizing<String>,
}

/// Connects to the assigned directory and counts the computer objects inside its boundary.
pub async fn check(assignment: &DirectoryAssignment, credentials: &Credentials) -> DirectoryReport {
    let (ok, objects, error) = match scan(assignment, credentials).await {
        Ok(objects) => (true, objects, String::new()),
        Err(code) => (false, 0, code.to_owned()),
    };
    if !ok {
        tracing::warn!(error, "directory check failed");
    }
    DirectoryReport {
        revision: assignment.revision,
        ok,
        objects,
        error,
    }
}

fn trust(ca_pem: &str) -> Option<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(ca_pem.as_bytes()) {
        roots.add(certificate.ok()?).ok()?;
    }
    if roots.is_empty() {
        return None;
    }
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .ok()?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Some(Arc::new(config))
}

async fn scan(
    assignment: &DirectoryAssignment,
    credentials: &Credentials,
) -> Result<u32, &'static str> {
    if !assignment.url.to_ascii_lowercase().starts_with("ldaps://") {
        return Err("insecure");
    }
    if !valid_bases(&assignment.domain, &assignment.base_dns) {
        return Err("boundary");
    }
    // An empty password would be an unauthenticated bind, which many directories accept.
    if credentials.user.is_empty() || credentials.password.is_empty() {
        return Err("bind");
    }
    let tls = trust(&assignment.ca_pem).ok_or("tls")?;
    let settings = LdapConnSettings::new()
        .set_config(tls)
        .set_conn_timeout(TIMEOUT);
    let (connection, mut ldap) = LdapConnAsync::with_settings(settings, &assignment.url)
        .await
        .map_err(|_| "connect")?;
    ldap3::drive!(connection);
    ldap.with_timeout(TIMEOUT)
        .simple_bind(&credentials.user, &credentials.password)
        .await
        .map_err(|_| "connect")?
        .success()
        .map_err(|_| "bind")?;
    let mut objects = 0u32;
    for base in &assignment.base_dns {
        let adapters: Vec<Box<dyn Adapter<_, _>>> = vec![
            Box::new(EntriesOnly::new()),
            Box::new(PagedResults::new(PAGE_SIZE)),
        ];
        let mut search = ldap
            .with_timeout(TIMEOUT)
            .streaming_search_with(
                adapters,
                base,
                Scope::Subtree,
                COMPUTERS,
                vec!["objectGUID", "dNSHostName", "cn"],
            )
            .await
            .map_err(|_| "search")?;
        while let Some(entry) = search.next().await.map_err(|_| "search")? {
            if within(&SearchEntry::construct(entry).dn, base) {
                objects = objects.saturating_add(1);
            }
        }
        search.finish().await.success().map_err(|_| "search")?;
    }
    let _ = ldap.unbind().await;
    Ok(objects)
}
