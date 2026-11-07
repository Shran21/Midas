use axum_server::{Handle, tls_rustls::RustlsConfig};
use clap::Parser;
use midas_server::{
    agent_router,
    config::Arguments,
    transport::{TransportError, pem_resolver, server_config},
    web_router,
};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair,
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{PrivatePkcs8KeyDer, ServerName},
};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn identity() -> (Certificate, Issuer<'static, KeyPair>) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec![]).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let cert = params.self_signed(&key).unwrap();
    (cert, Issuer::new(params, key))
}

fn leaf(
    issuer: &Issuer<'static, KeyPair>,
    usage: ExtendedKeyUsagePurpose,
) -> (Certificate, KeyPair) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
    params.extended_key_usages = vec![usage];
    (params.signed_by(&key, issuer).unwrap(), key)
}

async fn request(
    address: std::net::SocketAddr,
    root: &Certificate,
    client: Option<&(Certificate, KeyPair)>,
    path: &str,
) -> Result<String, TransportError> {
    let mut roots = RootCertStore::empty();
    roots.add(root.der().clone())?;
    let builder =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(roots);
    let mut config = if let Some((cert, key)) = client {
        builder.with_client_auth_cert(
            vec![cert.der().clone()],
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )?
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let stream = tokio::net::TcpStream::connect(address).await?;
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("localhost")?, stream)
        .await?;
    assert_eq!(
        tls.get_ref().1.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_3)
    );
    tls.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await?;
    let mut response = String::new();
    tls.read_to_string(&mut response).await?;
    Ok(response)
}

#[tokio::test]
async fn https_and_mtls_enforce_distinct_boundaries() {
    let (root, issuer) = identity();
    let (server, key) = leaf(&issuer, ExtendedKeyUsagePurpose::ServerAuth);
    let valid = leaf(&issuer, ExtendedKeyUsagePurpose::ClientAuth);
    let (_, other_issuer) = identity();
    let foreign = leaf(&other_issuer, ExtendedKeyUsagePurpose::ClientAuth);
    for operational in [false, true] {
        let resolver =
            pem_resolver(server.pem().as_bytes(), key.serialize_pem().as_bytes()).unwrap();
        let ca = root.pem();
        let config = server_config(resolver, operational.then_some(ca.as_bytes()), None).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = Handle::new();
        let app = if operational {
            agent_router()
        } else {
            web_router()
        };
        let task = tokio::spawn(
            axum_server::from_tcp_rustls(listener, RustlsConfig::from_config(config))
                .unwrap()
                .handle(handle.clone())
                .serve(app.into_make_service()),
        );
        let checks = async {
            if operational {
                assert!(
                    request(address, &root, None, "/agent/connect")
                        .await
                        .is_err()
                );
                assert!(
                    request(address, &root, Some(&foreign), "/agent/connect")
                        .await
                        .is_err()
                );
                assert!(
                    request(address, &root, Some(&valid), "/agent/connect")
                        .await
                        .unwrap()
                        .starts_with("HTTP/1.1 426")
                );
                assert!(
                    request(address, &root, Some(&valid), "/agent/enroll")
                        .await
                        .unwrap()
                        .starts_with("HTTP/1.1 404")
                );
                assert!(
                    request(address, &root, Some(&valid), "/health/live")
                        .await
                        .unwrap()
                        .starts_with("HTTP/1.1 404")
                );
            } else {
                let response = request(address, &root, None, "/health/live").await.unwrap();
                assert!(
                    response.starts_with("HTTP/1.1 200") && response.contains(r#"{"status":"ok"}"#)
                );
                assert!(
                    request(address, &root, None, "/agent/connect")
                        .await
                        .unwrap()
                        .starts_with("HTTP/1.1 404")
                );
                let (untrusted_root, _) = identity();
                assert!(
                    request(address, &untrusted_root, None, "/health/live")
                        .await
                        .is_err()
                );
            }
        };
        let outcome = tokio::time::timeout(Duration::from_secs(15), checks).await;
        handle.shutdown();
        task.await.unwrap().unwrap();
        outcome.unwrap();
    }
}

#[test]
fn invalid_tls_material_is_rejected() {
    let (root, issuer) = identity();
    let (server, key) = leaf(&issuer, ExtendedKeyUsagePurpose::ServerAuth);
    let foreign_key = KeyPair::generate().unwrap();
    assert!(
        pem_resolver(
            server.pem().as_bytes(),
            foreign_key.serialize_pem().as_bytes()
        )
        .is_err()
    );
    assert!(pem_resolver(b"invalid", key.serialize_pem().as_bytes()).is_err());
    let resolver = pem_resolver(server.pem().as_bytes(), key.serialize_pem().as_bytes()).unwrap();
    assert!(server_config(resolver.clone(), Some(b""), None).is_err());
    assert!(
        server_config(
            resolver.clone(),
            Some(root.pem().as_bytes()),
            Some(b"invalid")
        )
        .is_err()
    );
    assert!(server_config(resolver, None, Some(b"invalid")).is_err());
}

#[test]
fn configuration_fails_closed() {
    for args in [
        vec!["server", "--tls-cert", "cert.pem"],
        vec!["server", "--agent-bind", "127.0.0.1:7444"],
        vec!["server", "--acme-domain", "example.com"],
        vec!["server", "--acme-production"],
    ] {
        assert!(Arguments::try_parse_from(args).is_err());
    }
    for args in [
        vec!["server", "--bind", "0.0.0.0:7443"],
        vec![
            "server",
            "--agent-bind",
            "127.0.0.1:7444",
            "--agent-ca",
            "ca.pem",
        ],
        vec![
            "server",
            "--tls-cert",
            "cert.pem",
            "--tls-key",
            "key.pem",
            "--agent-bind",
            "127.0.0.1:7443",
            "--agent-ca",
            "ca.pem",
        ],
    ] {
        assert!(Arguments::try_parse_from(args).unwrap().validate().is_err());
    }
    assert!(
        Arguments::try_parse_from(["server"])
            .unwrap()
            .validate()
            .is_ok()
    );
    for domain in [
        "https://example.com",
        "*.example.com",
        "127.0.0.1",
        "bad..example.com",
    ] {
        let args = Arguments::try_parse_from([
            "server",
            "--acme-domain",
            domain,
            "--acme-cache",
            "cache",
            "--acme-contact",
            "admin@example.com",
            "--acme-accept-terms",
        ])
        .unwrap();
        assert!(args.validate().is_err());
    }
    let args = Arguments::try_parse_from([
        "server",
        "--acme-domain",
        "midas.example.com",
        "--acme-cache",
        "cache",
        "--acme-contact",
        "admin@example.com",
        "--acme-accept-terms",
    ])
    .unwrap();
    assert!(!args.acme_production);
    assert!(args.validate().is_ok());
}

#[tokio::test]
async fn revoked_client_is_rejected() {
    let (root, issuer) = identity();
    let (server, server_key) = leaf(&issuer, ExtendedKeyUsagePurpose::ServerAuth);
    let valid = leaf(&issuer, ExtendedKeyUsagePurpose::ClientAuth);
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec![]).unwrap();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    params.serial_number = Some(42u64.into());
    let revoked = (params.signed_by(&key, &issuer).unwrap(), key);
    let crl = rcgen::CertificateRevocationListParams {
        this_update: rcgen::date_time_ymd(2020, 1, 1),
        next_update: rcgen::date_time_ymd(4096, 1, 1),
        crl_number: 1u64.into(),
        issuing_distribution_point: None,
        revoked_certs: vec![rcgen::RevokedCertParams {
            serial_number: 42u64.into(),
            revocation_time: rcgen::date_time_ymd(2020, 1, 1),
            reason_code: None,
            invalidity_date: None,
        }],
        key_identifier_method: rcgen::KeyIdMethod::Sha256,
    }
    .signed_by(&issuer)
    .unwrap()
    .pem()
    .unwrap();
    let resolver = pem_resolver(
        server.pem().as_bytes(),
        server_key.serialize_pem().as_bytes(),
    )
    .unwrap();
    let config =
        server_config(resolver, Some(root.pem().as_bytes()), Some(crl.as_bytes())).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let handle = Handle::new();
    let task = tokio::spawn(
        axum_server::from_tcp_rustls(listener, RustlsConfig::from_config(config))
            .unwrap()
            .handle(handle.clone())
            .serve(agent_router().into_make_service()),
    );
    let checks = async {
        assert!(
            request(address, &root, Some(&valid), "/agent/connect")
                .await
                .unwrap()
                .starts_with("HTTP/1.1 426")
        );
        assert!(
            request(address, &root, Some(&revoked), "/agent/connect")
                .await
                .is_err()
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(10), checks).await;
    handle.shutdown();
    task.await.unwrap().unwrap();
    result.unwrap();
}
