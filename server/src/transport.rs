use axum_server::{
    accept::Accept,
    tls_rustls::{RustlsAcceptor, RustlsConfig},
};
use rustls::pki_types::{
    CertificateDer, CertificateRevocationListDer, PrivateKeyDer, pem::PemObject,
};
use rustls::{
    RootCertStore, ServerConfig,
    server::{ResolvesServerCert, WebPkiClientVerifier},
};
use std::{error::Error, future::Future, io, pin::Pin, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::server::TlsStream;
use tower::Layer;
pub type TransportError = Box<dyn Error + Send + Sync>;

pub fn pem_resolver(
    cert: &[u8],
    key: &[u8],
) -> Result<Arc<dyn ResolvesServerCert>, TransportError> {
    let chain = CertificateDer::pem_slice_iter(cert).collect::<Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(key)?;
    let certified = rustls::sign::CertifiedKey::from_der(
        chain,
        key,
        &rustls::crypto::ring::default_provider(),
    )?;
    Ok(Arc::new(rustls::sign::SingleCertAndKey::from(certified)))
}

pub fn server_config(
    resolver: Arc<dyn ResolvesServerCert>,
    client_ca: Option<&[u8]>,
    client_crl: Option<&[u8]>,
) -> Result<Arc<ServerConfig>, TransportError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?;
    let mut config = if let Some(pem) = client_ca {
        let mut roots = RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(pem) {
            roots.add(cert?)?;
        }
        let mut verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider);
        if let Some(crl) = client_crl {
            let crls =
                CertificateRevocationListDer::pem_slice_iter(crl).collect::<Result<Vec<_>, _>>()?;
            if crls.is_empty() {
                return Err("empty client revocation list".into());
            }
            verifier = verifier.with_crls(crls).enforce_revocation_expiration();
        }
        builder
            .with_client_cert_verifier(verifier.build()?)
            .with_cert_resolver(resolver)
    } else {
        if client_crl.is_some() {
            return Err("client CRL requires a client CA".into());
        }
        builder.with_no_client_auth().with_cert_resolver(resolver)
    };
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// End-entity certificate the client presented on the mTLS listener.
#[derive(Clone)]
pub struct PeerCertificate(pub Arc<[u8]>);

/// TLS acceptor that exposes the verified client certificate to request handlers.
#[derive(Clone)]
pub struct PeerCertificates {
    inner: RustlsAcceptor,
}

impl PeerCertificates {
    pub fn new(config: Arc<ServerConfig>) -> Self {
        Self {
            inner: RustlsAcceptor::new(RustlsConfig::from_config(config)),
        }
    }
}

type Accepted<S, I> = io::Result<(
    TlsStream<I>,
    axum::middleware::AddExtension<S, PeerCertificate>,
)>;

impl<I, S> Accept<I, S> for PeerCertificates
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S: Send + 'static,
{
    type Stream = TlsStream<I>;
    type Service = axum::middleware::AddExtension<S, PeerCertificate>;
    type Future = Pin<Box<dyn Future<Output = Accepted<S, I>> + Send>>;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        let accepted = self.inner.accept(stream, service);
        Box::pin(async move {
            let (stream, service) = accepted.await?;
            let certificate: Arc<[u8]> = stream
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|chain| chain.first())
                .map_or_else(|| Arc::from(&[][..]), |leaf| Arc::from(leaf.as_ref()));
            Ok((
                stream,
                axum::Extension(PeerCertificate(certificate)).layer(service),
            ))
        })
    }
}
