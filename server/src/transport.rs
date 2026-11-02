use rustls::pki_types::{
    CertificateDer, CertificateRevocationListDer, PrivateKeyDer, pem::PemObject,
};
use rustls::{
    RootCertStore, ServerConfig,
    server::{ResolvesServerCert, WebPkiClientVerifier},
};
use std::{error::Error, sync::Arc};
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
