use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, body::Bytes, header};
use hyper_util::rt::TokioIo;
use midas_protocol::enrollment::{
    ENROLL_PATH, EnrollAnswer, EnrollRequest, MAX_BODY_BYTES, PollAnswer, PollRequest,
};
use rcgen::{CertificateParams, KeyPair};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
};
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Enrollment endpoint on the server-authenticated HTTPS listener.
#[derive(Clone)]
pub struct Server {
    pub host: String,
    pub port: u16,
    /// Verifies the server; no client certificate is needed for enrollment.
    pub tls: Arc<ClientConfig>,
    pub timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnrollError {
    Rejected,
    Invalid,
    Unavailable,
    Unreachable,
    Protocol,
}

#[derive(Debug)]
pub struct IdentityError;

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid agent key or certificate")
    }
}

impl std::error::Error for IdentityError {}

/// The agent key pair; it is created on the device and never sent anywhere.
pub struct Identity {
    key: KeyPair,
}

impl Identity {
    pub fn generate() -> Result<Self, IdentityError> {
        KeyPair::generate()
            .map(|key| Self { key })
            .map_err(|_| IdentityError)
    }

    pub fn from_pem(pem: &str) -> Result<Self, IdentityError> {
        KeyPair::from_pem(pem)
            .map(|key| Self { key })
            .map_err(|_| IdentityError)
    }

    /// For the platform's protected key store.
    pub fn key_pem(&self) -> Zeroizing<String> {
        Zeroizing::new(self.key.serialize_pem())
    }

    /// PKCS#10 request; its signature proves possession of the key.
    pub fn signing_request(&self) -> Result<String, IdentityError> {
        CertificateParams::default()
            .serialize_request(&self.key)
            .and_then(|request| request.pem())
            .map_err(|_| IdentityError)
    }

    /// Client configuration for the operational mTLS channel.
    pub fn client_config(
        &self,
        roots: RootCertStore,
        certificate_pem: &str,
    ) -> Result<Arc<ClientConfig>, IdentityError> {
        let chain = CertificateDer::pem_slice_iter(certificate_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| IdentityError)?;
        let key =
            PrivateKeyDer::from_pem_slice(self.key_pem().as_bytes()).map_err(|_| IdentityError)?;
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(|_| IdentityError)?
                .with_root_certificates(roots)
                .with_client_auth_cert(chain, key)
                .map_err(|_| IdentityError)?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

pub async fn enroll(server: &Server, request: &EnrollRequest) -> Result<EnrollAnswer, EnrollError> {
    let body = serde_json::to_vec(request).map_err(|_| EnrollError::Invalid)?;
    let (status, body) = post(server, ENROLL_PATH, body).await?;
    match status {
        201 | 202 => serde_json::from_slice(&body).map_err(|_| EnrollError::Protocol),
        status => Err(refusal(status)),
    }
}

/// Asks whether an operator approved a pending request.
pub async fn poll(
    server: &Server,
    request_id: Uuid,
    secret: &str,
) -> Result<PollAnswer, EnrollError> {
    let body = serde_json::to_vec(&PollRequest {
        secret: secret.into(),
    })
    .map_err(|_| EnrollError::Invalid)?;
    let (status, body) = post(server, &format!("{ENROLL_PATH}/{request_id}"), body).await?;
    match status {
        200 | 202 => serde_json::from_slice(&body).map_err(|_| EnrollError::Protocol),
        status => Err(refusal(status)),
    }
}

fn refusal(status: u16) -> EnrollError {
    match status {
        403 => EnrollError::Rejected,
        400 | 413 | 422 => EnrollError::Invalid,
        503 => EnrollError::Unavailable,
        _ => EnrollError::Protocol,
    }
}

async fn post(server: &Server, path: &str, body: Vec<u8>) -> Result<(u16, Bytes), EnrollError> {
    let limit = server.timeout;
    let tcp = crate::connection::reach(&server.host, server.port, limit)
        .await
        .ok_or(EnrollError::Unreachable)?;
    let name = ServerName::try_from(server.host.clone()).map_err(|_| EnrollError::Invalid)?;
    let mut config = (*server.tls).clone();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tls = timeout(
        limit,
        TlsConnector::from(Arc::new(config)).connect(name, tcp),
    )
    .await
    .map_err(|_| EnrollError::Unreachable)?
    .map_err(|_| EnrollError::Unreachable)?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
        .await
        .map_err(|_| EnrollError::Protocol)?;
    let driver = tokio::spawn(connection);
    let request = Request::post(path)
        .header(header::HOST, format!("{}:{}", server.host, server.port))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(|_| EnrollError::Invalid)?;
    let exchange = async {
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| EnrollError::Unreachable)?;
        let status = response.status().as_u16();
        let body = Limited::new(response.into_body(), MAX_BODY_BYTES)
            .collect()
            .await
            .map_err(|_| EnrollError::Protocol)?
            .to_bytes();
        Ok((status, body))
    };
    let result = timeout(limit, exchange)
        .await
        .map_err(|_| EnrollError::Unreachable)?;
    driver.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_requests_are_pem() {
        let identity = Identity::generate().unwrap();
        let again = Identity::from_pem(&identity.key_pem()).unwrap();
        assert_eq!(*identity.key_pem(), *again.key_pem());
        let request = identity.signing_request().unwrap();
        assert!(request.starts_with("-----BEGIN CERTIFICATE REQUEST-----"));
        assert!(Identity::from_pem("not a key").is_err());
    }

    #[test]
    fn statuses_map_to_refusals() {
        assert_eq!(refusal(403), EnrollError::Rejected);
        assert_eq!(refusal(400), EnrollError::Invalid);
        assert_eq!(refusal(503), EnrollError::Unavailable);
        assert_eq!(refusal(500), EnrollError::Protocol);
    }
}
