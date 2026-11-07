use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use ldap3_proto::{
    LdapCodec,
    control::LdapControl,
    proto::{
        LdapBindCred, LdapBindResponse, LdapMsg, LdapOp, LdapPartialAttribute, LdapResult,
        LdapResultCode, LdapSearchResultEntry,
    },
};
use midas_agent_core::directory::{Credentials, check};
use midas_protocol::{DirectoryAssignment, directory::within};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer},
};
use tokio_util::codec::{FramedRead, FramedWrite};
use zeroize::Zeroizing;

const USER: &str = "svc-midas-read@corp.example";
const PASSWORD: &str = "Read-only directory password 1!";
const WORKSTATIONS: &str = "OU=Workstations,DC=corp,DC=example";
const SERVERS: &str = "OU=Servers,DC=corp,DC=example";

fn authority() -> (String, Issuer<'static, KeyPair>) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec![]).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let certificate = params.self_signed(&key).unwrap();
    (certificate.pem(), Issuer::new(params, key))
}

fn result(code: LdapResultCode) -> LdapResult {
    LdapResult {
        code,
        matcheddn: String::new(),
        message: String::new(),
        referral: vec![],
    }
}

/// LDAPS directory that pages like Active Directory and logs every operation it receives.
async fn directory(issuer: &Issuer<'static, KeyPair>, ops: Arc<Mutex<Vec<String>>>) -> u16 {
    let key = KeyPair::generate().unwrap();
    let certificate = CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .signed_by(&key, issuer)
        .unwrap();
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(certificate.der().to_vec())],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
    let mut entries: Vec<String> = (0..1203)
        .map(|n| format!("CN=PC-{n:04},{WORKSTATIONS}"))
        .collect();
    entries.extend((0..3).map(|n| format!("CN=SRV-{n},{SERVERS}")));
    entries.push("CN=DC-01,OU=Domain Controllers,DC=corp,DC=example".into());
    let entries = Arc::new(entries);
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let (acceptor, entries, ops) = (acceptor.clone(), entries.clone(), ops.clone());
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(socket).await else {
                    return;
                };
                let (read, write) = tokio::io::split(stream);
                let mut requests = FramedRead::new(read, LdapCodec::default());
                let mut responses = FramedWrite::new(write, LdapCodec::default());
                let mut bound = false;
                while let Some(Ok(message)) = requests.next().await {
                    let msgid = message.msgid;
                    let reply = |op| LdapMsg {
                        msgid,
                        op,
                        ctrl: vec![],
                    };
                    let mut replies = Vec::new();
                    match message.op {
                        LdapOp::BindRequest(request) => {
                            ops.lock().unwrap().push("bind".into());
                            bound = request.dn == USER
                                && matches!(&request.cred, LdapBindCred::Simple(pw) if pw == PASSWORD);
                            let code = if bound {
                                LdapResultCode::Success
                            } else {
                                LdapResultCode::InvalidCredentials
                            };
                            replies.push(reply(LdapOp::BindResponse(LdapBindResponse {
                                res: result(code),
                                saslcreds: None,
                            })));
                        }
                        LdapOp::SearchRequest(request) => {
                            ops.lock().unwrap().push(format!("search {}", request.base));
                            let (size, offset) = message
                                .ctrl
                                .iter()
                                .find_map(|control| match control {
                                    LdapControl::SimplePagedResults { size, cookie } => Some((
                                        usize::try_from(*size).unwrap(),
                                        String::from_utf8(cookie.clone())
                                            .ok()
                                            .and_then(|c| c.parse().ok())
                                            .unwrap_or(0usize),
                                    )),
                                    _ => None,
                                })
                                .unwrap_or((usize::MAX, 0));
                            let matching: Vec<_> = if bound {
                                entries
                                    .iter()
                                    .filter(|dn| within(dn, &request.base))
                                    .collect()
                            } else {
                                vec![]
                            };
                            let end = offset.saturating_add(size).min(matching.len());
                            let mut page: Vec<String> = matching[offset..end]
                                .iter()
                                .map(|dn| (*dn).clone())
                                .collect();
                            // A misbehaving server must not widen the boundary.
                            if bound && offset == 0 {
                                page.push("CN=ROGUE,OU=Elsewhere,DC=corp,DC=example".into());
                            }
                            for dn in page {
                                replies.push(reply(LdapOp::SearchResultEntry(
                                    LdapSearchResultEntry {
                                        dn,
                                        attributes: vec![LdapPartialAttribute {
                                            atype: "objectGUID".into(),
                                            vals: vec![vec![7; 16]],
                                        }],
                                    },
                                )));
                            }
                            let cookie = if end < matching.len() {
                                end.to_string().into_bytes()
                            } else {
                                vec![]
                            };
                            replies.push(LdapMsg {
                                msgid,
                                op: LdapOp::SearchResultDone(result(LdapResultCode::Success)),
                                ctrl: vec![LdapControl::SimplePagedResults { size: 0, cookie }],
                            });
                        }
                        LdapOp::UnbindRequest => {
                            ops.lock().unwrap().push("unbind".into());
                            return;
                        }
                        other => {
                            ops.lock().unwrap().push(format!("unexpected {other:?}"));
                            return;
                        }
                    }
                    for reply in replies {
                        if responses.send(reply).await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    port
}

fn assignment(port: u16, ca_pem: &str) -> DirectoryAssignment {
    DirectoryAssignment {
        revision: 4,
        domain: "corp.example".into(),
        url: format!("ldaps://localhost:{port}"),
        ca_pem: ca_pem.into(),
        base_dns: vec![WORKSTATIONS.into(), SERVERS.into()],
    }
}

fn credentials(password: &str) -> Credentials {
    Credentials {
        user: USER.into(),
        password: Zeroizing::new(password.into()),
    }
}

#[tokio::test]
async fn reads_only_inside_the_boundary_with_paging() {
    let (ca, issuer) = authority();
    let ops = Arc::new(Mutex::new(Vec::new()));
    let port = directory(&issuer, ops.clone()).await;
    let report = check(&assignment(port, &ca), &credentials(PASSWORD)).await;
    assert!(report.ok, "{}", report.error);
    assert_eq!((report.revision, report.objects), (4, 1206));
    let ops = ops.lock().unwrap().clone();
    let pages = ops.iter().filter(|op| op.starts_with("search ")).count();
    assert_eq!(pages, 4, "{ops:?}");
    assert_eq!(ops.first().map(String::as_str), Some("bind"));
    assert!(
        ops.iter()
            .all(|op| op == "bind" || op == "unbind" || op.starts_with("search ")),
        "the connector must only bind and search: {ops:?}"
    );
}

#[tokio::test]
async fn refuses_untrusted_or_insecure_directories_and_bad_credentials() {
    let (ca, issuer) = authority();
    let (other_ca, _) = authority();
    let ops = Arc::new(Mutex::new(Vec::new()));
    let port = directory(&issuer, ops).await;
    let cases = [
        (assignment(port, &other_ca), PASSWORD, "connect"),
        (assignment(port, "not a certificate"), PASSWORD, "tls"),
        (
            DirectoryAssignment {
                url: format!("ldap://localhost:{port}"),
                ..assignment(port, &ca)
            },
            PASSWORD,
            "insecure",
        ),
        (
            DirectoryAssignment {
                base_dns: vec!["OU=Workstations,DC=other,DC=example".into()],
                ..assignment(port, &ca)
            },
            PASSWORD,
            "boundary",
        ),
        (assignment(port, &ca), "wrong password", "bind"),
        (assignment(port, &ca), "", "bind"),
    ];
    for (assignment, password, error) in cases {
        let report = check(&assignment, &credentials(password)).await;
        assert!(!report.ok);
        assert_eq!((report.objects, report.error.as_str()), (0, error));
    }
}
