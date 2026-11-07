use std::{sync::Arc, time::Duration};

use midas_agent_core::{
    connection::{Reason, Settings, Status, run},
    directory::Credentials,
    enrollment::{EnrollError, Identity, Server, enroll, poll},
};
use midas_protocol::enrollment::{EnrollAnswer, EnrollRequest, Platform, PollAnswer};
use rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer, pki_types::pem::PemObject};
use serde::Serialize;
use tokio::sync::watch;
use zeroize::Zeroizing;

use crate::config::{Config, DirectoryAccount, Paths, Pending, read_json, write_atomic};

const RETRY: Duration = Duration::from_secs(60);

/// What the status IPC reports; it never contains keys, tokens or secrets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    #[default]
    Starting,
    Unconfigured,
    Unenrolled,
    Pending,
    Rejected,
    Connecting {
        attempt: u32,
    },
    Connected {
        protocol: u32,
    },
    Waiting {
        reason: &'static str,
        retry_in_ms: u64,
    },
    Stopped,
}

impl From<&Status> for State {
    fn from(status: &Status) -> Self {
        match status {
            Status::Connecting { attempt } => Self::Connecting { attempt: *attempt },
            Status::Connected { version, .. } => Self::Connected { protocol: *version },
            Status::Waiting { retry_in, reason } => Self::Waiting {
                reason: match reason {
                    Reason::Unreachable => "unreachable",
                    Reason::Handshake => "handshake",
                    Reason::Incompatible { .. } => "incompatible",
                    Reason::Lost => "lost",
                },
                retry_in_ms: u64::try_from(retry_in.as_millis()).unwrap_or(u64::MAX),
            },
            Status::Stopped => Self::Stopped,
        }
    }
}

fn roots(config: &Config) -> Result<RootCertStore, &'static str> {
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(config.server_ca.as_bytes()) {
        roots
            .add(certificate.map_err(|_| "invalid server_ca")?)
            .map_err(|_| "invalid server_ca")?;
    }
    if roots.is_empty() {
        return Err("server_ca has no certificate");
    }
    Ok(roots)
}

fn enrollment_server(config: &Config) -> Result<Server, &'static str> {
    let tls =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| "TLS setup failed")?
            .with_root_certificates(roots(config)?)
            .with_no_client_auth();
    Ok(Server {
        host: config.server.clone(),
        port: config.enrollment_port,
        tls: Arc::new(tls),
        timeout: Duration::from_secs(30),
    })
}

pub fn load_config(paths: &Paths) -> Result<Config, String> {
    let config: Config = read_json(&paths.config())
        .map_err(|error| format!("cannot read config: {error}"))?
        .ok_or("config.json is missing")?;
    config.validate()?;
    Ok(config)
}

/// One-time enrollment; the token comes from stdin and is never stored.
pub async fn enroll_with(paths: &Paths, token: &str) -> Result<&'static str, String> {
    let config = load_config(paths)?;
    if paths.certificate().exists() {
        return Err("this agent is already enrolled".into());
    }
    let identity = Identity::generate().map_err(|e| e.to_string())?;
    let request = EnrollRequest {
        token: token.trim().into(),
        csr: identity.signing_request().map_err(|e| e.to_string())?,
        platform: Platform::Windows,
        kind: config.kind,
        name: config.name.clone(),
    };
    let answer = enroll(&enrollment_server(&config)?, &request)
        .await
        .map_err(|error| format!("enrollment refused: {error:?}"))?;
    write_atomic(&paths.key(), identity.key_pem().as_bytes()).map_err(|e| e.to_string())?;
    match answer {
        EnrollAnswer::Issued(issued) => {
            write_atomic(&paths.certificate(), issued.certificate.as_bytes())
                .map_err(|e| e.to_string())?;
            Ok("enrolled")
        }
        EnrollAnswer::Pending {
            request_id,
            request_secret,
        } => {
            let pending = Pending {
                request_id,
                secret: request_secret,
            };
            let bytes = serde_json::to_vec(&pending).map_err(|e| e.to_string())?;
            write_atomic(&paths.pending(), &bytes).map_err(|e| e.to_string())?;
            Ok("waiting for operator approval")
        }
    }
}

async fn pause(shutdown: &mut watch::Receiver<bool>, period: Duration) -> bool {
    tokio::select! {
        () = tokio::time::sleep(period) => false,
        _ = shutdown.wait_for(|stop| *stop) => true,
    }
}

/// Waits until the agent holds a certificate, polling a pending request.
async fn credentials(
    paths: &Paths,
    state: &watch::Sender<State>,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<(Config, Identity, String)> {
    loop {
        if let Some(found) = attempt(paths, state).await {
            return Some(found);
        }
        if pause(shutdown, RETRY).await {
            return None;
        }
    }
}

async fn attempt(
    paths: &Paths,
    state: &watch::Sender<State>,
) -> Option<(Config, Identity, String)> {
    let config = match load_config(paths) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(error, "agent not configured");
            state.send_replace(State::Unconfigured);
            return None;
        }
    };
    let Ok(key) = std::fs::read_to_string(paths.key()) else {
        state.send_replace(State::Unenrolled);
        return None;
    };
    let Ok(identity) = Identity::from_pem(&key) else {
        tracing::error!("agent key is unreadable");
        state.send_replace(State::Unenrolled);
        return None;
    };
    if !paths.certificate().exists() {
        collect(paths, &config, state).await;
    }
    let certificate = std::fs::read_to_string(paths.certificate()).ok()?;
    Some((config, identity, certificate))
}

async fn collect(paths: &Paths, config: &Config, state: &watch::Sender<State>) {
    let Ok(Some(pending)) = read_json::<Pending>(&paths.pending()) else {
        state.send_replace(State::Unenrolled);
        return;
    };
    let Ok(server) = enrollment_server(config) else {
        state.send_replace(State::Unconfigured);
        return;
    };
    match poll(&server, pending.request_id, &pending.secret).await {
        Ok(PollAnswer::Issued(issued)) => {
            if write_atomic(&paths.certificate(), issued.certificate.as_bytes()).is_ok() {
                let _ = std::fs::remove_file(paths.pending());
                tracing::info!(device = %issued.device_id, "agent certificate issued");
            }
        }
        Ok(PollAnswer::Pending) => {
            state.send_replace(State::Pending);
        }
        Err(EnrollError::Rejected) => {
            tracing::warn!("enrollment request was rejected or revoked");
            state.send_replace(State::Rejected);
        }
        Err(error) => {
            tracing::warn!(?error, "enrollment status unavailable");
            state.send_replace(State::Pending);
        }
    }
}

/// Keeps the agent enrolled and connected until `shutdown` becomes true.
pub async fn serve(paths: Paths, state: watch::Sender<State>, mut shutdown: watch::Receiver<bool>) {
    let Some((config, identity, certificate)) = credentials(&paths, &state, &mut shutdown).await
    else {
        state.send_replace(State::Stopped);
        return;
    };
    let tls = match roots(&config).map(|roots| identity.client_config(roots, &certificate)) {
        Ok(Ok(tls)) => tls,
        _ => {
            tracing::error!("agent certificate or server_ca is unusable");
            state.send_replace(State::Unconfigured);
            return;
        }
    };
    let directory = read_json::<DirectoryAccount>(&paths.directory())
        .ok()
        .flatten()
        .map(|account| Credentials {
            user: account.user,
            password: Zeroizing::new(account.password),
        });
    let settings = Settings {
        directory,
        ..Settings::new(config.server.clone(), config.port, tls, "windows")
    };
    let (status_tx, mut status) = watch::channel(Status::Connecting { attempt: 0 });
    let connection = tokio::spawn(run(settings, status_tx, shutdown.clone()));
    while status.changed().await.is_ok() {
        state.send_replace(State::from(&*status.borrow_and_update()));
    }
    let _ = connection.await;
    state.send_replace(State::Stopped);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_reported_without_secrets() {
        let waiting = State::from(&Status::Waiting {
            retry_in: Duration::from_millis(1500),
            reason: Reason::Lost,
        });
        assert_eq!(
            serde_json::to_string(&waiting).unwrap(),
            r#"{"state":"waiting","reason":"lost","retry_in_ms":1500}"#
        );
        let connected = State::from(&Status::Connected {
            version: 1,
            keepalive: Duration::from_secs(30),
        });
        assert_eq!(
            serde_json::to_string(&connected).unwrap(),
            r#"{"state":"connected","protocol":1}"#
        );
    }
}
