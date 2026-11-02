use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(version, about = "MIDAS server")]
pub struct Arguments {
    #[arg(long, env = "MIDAS_BIND", default_value = "127.0.0.1:7443")]
    pub bind: SocketAddr,
    #[arg(long, requires = "tls_key", conflicts_with = "acme_domain")]
    pub tls_cert: Option<PathBuf>,
    #[arg(long, requires = "tls_cert")]
    pub tls_key: Option<PathBuf>,
    #[arg(long, requires_all = ["acme_cache", "acme_contact", "acme_accept_terms"])]
    pub acme_domain: Vec<String>,
    #[arg(long, requires = "acme_domain")]
    pub acme_cache: Option<PathBuf>,
    #[arg(long, requires = "acme_domain")]
    pub acme_contact: Option<String>,
    #[arg(long, requires = "acme_domain")]
    pub acme_accept_terms: bool,
    #[arg(long, requires = "acme_domain")]
    pub acme_production: bool,
    #[arg(long, requires = "agent_ca")]
    pub agent_bind: Option<SocketAddr>,
    #[arg(long, requires = "agent_bind")]
    pub agent_ca: Option<PathBuf>,
    #[arg(long, requires = "agent_ca")]
    pub agent_crl: Option<PathBuf>,
    #[arg(long, env = "MIDAS_AUTH_ORIGIN", requires = "auth_key_file")]
    pub auth_origin: Option<webauthn_rs::prelude::Url>,
    #[arg(long, env = "MIDAS_AUTH_KEY_FILE", requires = "auth_origin")]
    pub auth_key_file: Option<PathBuf>,
    #[arg(long, env = "MIDAS_SESSION_SECONDS", default_value_t = 43_200)]
    pub session_seconds: u64,
    #[arg(long, env = "MIDAS_SESSION_IDLE_SECONDS", default_value_t = 1_800)]
    pub session_idle_seconds: u64,
    #[arg(long, env = "MIDAS_SESSION_SOCKET_CAPACITY", default_value_t = 128)]
    pub session_socket_capacity: usize,
}

impl Arguments {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.tls_cert.is_none() && self.acme_domain.is_empty() {
            if self.auth_origin.is_some() {
                return Err("browser authentication requires the HTTPS listener");
            }
            crate::validate_development_address(self.bind)?;
            if self.agent_bind.is_some() {
                return Err("the agent listener requires TLS");
            }
        }
        if self.agent_bind == Some(self.bind) {
            return Err("web and agent listeners must differ");
        }
        for domain in &self.acme_domain {
            if domain.len() > 253
                || !domain.contains('.')
                || domain.parse::<std::net::IpAddr>().is_ok()
                || domain.split('.').any(|part| {
                    part.is_empty()
                        || part.len() > 63
                        || part.starts_with('-')
                        || part.ends_with('-')
                        || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err("ACME requires DNS hostnames without wildcards or URLs");
            }
        }
        if let Some(contact) = &self.acme_contact
            && (!contact.contains('@')
                || contact.chars().any(char::is_whitespace)
                || contact.contains(':'))
        {
            return Err("ACME contact must be an email address");
        }
        Ok(())
    }
}

pub fn authentication_key(
    path: &std::path::Path,
) -> Result<zeroize::Zeroizing<[u8; 32]>, crate::transport::TransportError> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("authentication key must be a private regular file".into());
    }
    let mut file = std::fs::File::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file.metadata()?.permissions().mode() & 0o077 != 0 {
            return Err("authentication key permissions must exclude group and other users".into());
        }
    }
    let mut bytes = zeroize::Zeroizing::new([0_u8; 33]);
    let mut count = 0;
    while count < bytes.len() {
        let read = file.read(&mut bytes[count..])?;
        if read == 0 {
            break;
        }
        count += read;
    }
    if count != 32 {
        return Err("authentication key must contain exactly 32 raw bytes".into());
    }
    let mut key = zeroize::Zeroizing::new([0; 32]);
    key.copy_from_slice(&bytes[..32]);
    if *key == [0; 32] {
        return Err("authentication key must not be zero".into());
    }
    Ok(key)
}
