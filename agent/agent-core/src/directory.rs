use std::{sync::Arc, time::Duration};

use ldap3::{
    LdapConnAsync, LdapConnSettings, Scope, SearchEntry,
    adapters::{Adapter, EntriesOnly, PagedResults},
};
use midas_protocol::{
    DIRECTORY_CHUNK_BYTES, DirectoryAssignment, DirectoryChunk, DirectoryComputer, DirectoryReport,
    MAX_DIRECTORY_CHUNK, MAX_DIRECTORY_DN, MAX_DIRECTORY_TEXT,
    directory::{valid_bases, within},
};
use prost::Message;
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use zeroize::Zeroizing;

const PAGE_SIZE: i32 = 500;
const TIMEOUT: Duration = Duration::from_secs(30);
const COMPUTERS: &str = "(objectClass=computer)";
const ATTRIBUTES: [&str; 7] = [
    "objectGUID",
    "cn",
    "dNSHostName",
    "operatingSystem",
    "operatingSystemVersion",
    "lastLogonTimestamp",
    "userAccountControl",
];
/// Seconds between 1601-01-01 (Windows file time) and the Unix epoch.
const FILETIME_EPOCH: i64 = 11_644_473_600;
const ACCOUNT_DISABLED: u32 = 0x2;

/// Read-only directory account, held only in the local agent configuration.
#[derive(Clone)]
pub struct Credentials {
    pub user: String,
    pub password: Zeroizing<String>,
}

/// A finished scan: the computer objects inside the boundary and the closing report.
pub struct Scan {
    pub computers: Vec<DirectoryComputer>,
    pub report: DirectoryReport,
}

impl Scan {
    /// Chunks bounded by count and encoded size; the report follows the last one.
    pub fn chunks(&self) -> Vec<DirectoryChunk> {
        let mut chunks = Vec::new();
        let mut current = DirectoryChunk {
            revision: self.report.revision,
            scan: self.report.scan,
            computers: Vec::new(),
        };
        for computer in &self.computers {
            let size =
                1 + prost::length_delimiter_len(computer.encoded_len()) + computer.encoded_len();
            if !current.computers.is_empty()
                && (current.computers.len() == MAX_DIRECTORY_CHUNK
                    || current.encoded_len() + size > DIRECTORY_CHUNK_BYTES)
            {
                let next = DirectoryChunk {
                    computers: Vec::new(),
                    ..current.clone()
                };
                chunks.push(std::mem::replace(&mut current, next));
            }
            current.computers.push(computer.clone());
        }
        if !current.computers.is_empty() {
            chunks.push(current);
        }
        chunks
    }
}

/// Connects to the assigned directory and reads the computer objects inside its boundary.
pub async fn check(assignment: &DirectoryAssignment, credentials: &Credentials) -> Scan {
    let (computers, error) = match scan(assignment, credentials).await {
        Ok(computers) => (computers, String::new()),
        Err(code) => (Vec::new(), code.to_owned()),
    };
    if !error.is_empty() {
        tracing::warn!(error, "directory check failed");
    }
    let mut id = [0u8; 8];
    let _ = getrandom::fill(&mut id);
    let report = DirectoryReport {
        revision: assignment.revision,
        ok: error.is_empty(),
        objects: u32::try_from(computers.len()).unwrap_or(u32::MAX),
        error,
        scan: if computers.is_empty() {
            0
        } else {
            u64::from_le_bytes(id).max(1)
        },
    };
    Scan { computers, report }
}

fn text(value: Option<&String>, limit: usize) -> String {
    value
        .map(|value| {
            value
                .chars()
                .filter(|c| *c != '\u{0}')
                .take(limit)
                .collect()
        })
        .unwrap_or_default()
}

/// Only what identifies and describes the computer; everything else stays in the directory.
fn computer(entry: SearchEntry) -> Option<DirectoryComputer> {
    let first = |name: &str| entry.attrs.get(name).and_then(|values| values.first());
    let object_guid = entry
        .bin_attrs
        .get("objectGUID")
        .and_then(|values| values.first().cloned())
        .or_else(|| first("objectGUID").map(|value| value.as_bytes().to_vec()))
        .filter(|guid| guid.len() == 16)?;
    let last_logon = first("lastLogonTimestamp")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|ticks| *ticks > 0)
        .map_or(0, |ticks| ticks / 10_000_000 - FILETIME_EPOCH);
    let disabled = first("userAccountControl")
        .and_then(|value| value.parse::<u32>().ok())
        .is_some_and(|flags| flags & ACCOUNT_DISABLED != 0);
    Some(DirectoryComputer {
        object_guid,
        name: text(first("cn"), MAX_DIRECTORY_TEXT),
        dns_host_name: text(first("dNSHostName"), MAX_DIRECTORY_TEXT),
        distinguished_name: text(Some(&entry.dn), MAX_DIRECTORY_DN),
        operating_system: text(first("operatingSystem"), MAX_DIRECTORY_TEXT),
        operating_system_version: text(first("operatingSystemVersion"), MAX_DIRECTORY_TEXT),
        last_logon: last_logon.max(0),
        disabled,
    })
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
) -> Result<Vec<DirectoryComputer>, &'static str> {
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
    let mut computers = Vec::new();
    // Overlapping boundaries must not list a computer twice.
    let mut seen = std::collections::HashSet::new();
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
                ATTRIBUTES.to_vec(),
            )
            .await
            .map_err(|_| "search")?;
        while let Some(entry) = search.next().await.map_err(|_| "search")? {
            let entry = SearchEntry::construct(entry);
            // A distinguished name longer than the limit could not be checked against the boundary.
            if entry.dn.chars().count() <= MAX_DIRECTORY_DN
                && within(&entry.dn, base)
                && let Some(computer) = computer(entry)
                && seen.insert(computer.object_guid.clone())
            {
                computers.push(computer);
            }
        }
        search.finish().await.success().map_err(|_| "search")?;
    }
    let _ = ldap.unbind().await;
    Ok(computers)
}
