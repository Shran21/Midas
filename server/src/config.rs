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
}

impl Arguments {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.tls_cert.is_none() && self.acme_domain.is_empty() {
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
