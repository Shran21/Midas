use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use futures_util::{SinkExt, StreamExt};
use ldap3_proto::{
    LdapCodec,
    proto::{LdapPartialAttribute, LdapSearchResultEntry},
    simple::ServerOps,
};
use midas_server::{
    audit::Actor,
    auth::{
        AuthConfig,
        directory::DirectoryConfig,
        http::{WebAuth, router},
        sessions::SessionConfig,
    },
    authorization::{BuiltinRole, Scope},
    database::Database,
};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer},
};
use serde_json::{Value, json};
use sqlx::PgConnection;
use tokio_util::codec::{FramedRead, FramedWrite};
use tower::ServiceExt;
use uuid::Uuid;
use webauthn_rs::prelude::Url;
use zeroize::Zeroizing;

const ORIGIN: &str = "https://midas.example";
const TECHNICIANS: &str = "CN=MIDAS Technicians,OU=Groups,DC=corp,DC=example";
const PASSWORD: &str = "Directory test password 1!";

#[derive(Clone)]
struct Account {
    upn: &'static str,
    guid: [u8; 16],
    disabled: bool,
    groups: Vec<String>,
}

type Accounts = Arc<Mutex<Vec<Account>>>;

fn authority() -> (String, Issuer<'static, KeyPair>) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec![]).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let cert = params.self_signed(&key).unwrap();
    (cert.pem(), Issuer::new(params, key))
}

async fn directory_server(issuer: &Issuer<'static, KeyPair>, accounts: Accounts) -> u16 {
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .signed_by(&key, issuer)
        .unwrap();
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert.der().to_vec())],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let accounts = accounts.clone();
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(socket).await else {
                    return;
                };
                let (read, write) = tokio::io::split(stream);
                let mut requests = FramedRead::new(read, LdapCodec::default());
                let mut responses = FramedWrite::new(write, LdapCodec::default());
                let mut bound: Option<Account> = None;
                while let Some(Ok(message)) = requests.next().await {
                    let replies = match ServerOps::try_from(message) {
                        Ok(ServerOps::SimpleBind(request)) => {
                            let found = accounts
                                .lock()
                                .unwrap()
                                .iter()
                                .find(|a| {
                                    a.upn.eq_ignore_ascii_case(&request.dn)
                                        && !request.pw.is_empty()
                                        && request.pw == PASSWORD
                                })
                                .cloned();
                            let reply = if found.is_some() {
                                request.gen_success()
                            } else {
                                request.gen_invalid_cred()
                            };
                            bound = found;
                            vec![reply]
                        }
                        Ok(ServerOps::Search(request)) => {
                            let mut replies = Vec::new();
                            if let Some(account) = &bound {
                                let current = accounts
                                    .lock()
                                    .unwrap()
                                    .iter()
                                    .find(|a| a.upn == account.upn)
                                    .cloned()
                                    .unwrap();
                                let attribute =
                                    |atype: &str, vals: Vec<Vec<u8>>| LdapPartialAttribute {
                                        atype: atype.into(),
                                        vals,
                                    };
                                replies.push(request.gen_result_entry(LdapSearchResultEntry {
                                    dn: format!("CN={},DC=corp,DC=example", current.upn),
                                    attributes: vec![
                                            attribute("objectGUID", vec![current.guid.to_vec()]),
                                            attribute(
                                                "userAccountControl",
                                                vec![if current.disabled {
                                                    b"514".to_vec()
                                                } else {
                                                    b"512".to_vec()
                                                }],
                                            ),
                                            attribute(
                                                "memberOf",
                                                current
                                                    .groups
                                                    .iter()
                                                    .map(|g| g.as_bytes().to_vec())
                                                    .collect(),
                                            ),
                                            attribute(
                                                "displayName",
                                                vec![
                                                    format!("Directory {}", current.upn)
                                                        .into_bytes(),
                                                ],
                                            ),
                                        ],
                                }));
                            }
                            replies.push(request.gen_success());
                            replies
                        }
                        _ => return,
                    };
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

async fn sign_in(app: &Router, login: &str, password: &str) -> (StatusCode, Value, Option<String>) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/directory")
        .header("origin", ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo(
            "198.51.100.95:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(
            json!({ "login": login, "password": password }).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .find(|v| v.starts_with("__Host-midas-session=") && !v.ends_with('='));
    let bytes = to_bytes(response.into_body(), 100_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

pub async fn verify(database: &Database, admin: &mut PgConnection) {
    let mut key = [0; 32];
    getrandom::fill(&mut key).unwrap();
    let mut config = AuthConfig::new(Url::parse(ORIGIN).unwrap(), Zeroizing::new(key));
    config.account_attempts = 300;
    config.source_attempts = 1000;
    let web = Arc::new(
        WebAuth::new(database, config, SessionConfig::default(), 4)
            .await
            .unwrap(),
    );
    let app = router(web.clone());
    let auth = web.authentication();

    let tenant = Uuid::new_v4();
    sqlx::query("INSERT INTO midas.tenants (id,name) VALUES ($1,'Directory Kft.')")
        .bind(tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    let (ca, issuer) = authority();
    let (foreign_ca, _) = authority();
    let accounts: Accounts = Arc::new(Mutex::new(vec![
        Account {
            upn: "alice@corp.example",
            guid: [1; 16],
            disabled: false,
            groups: vec![TECHNICIANS.into()],
        },
        Account {
            upn: "bob@corp.example",
            guid: [2; 16],
            disabled: true,
            groups: vec![TECHNICIANS.into()],
        },
        Account {
            upn: "carol@corp.example",
            guid: [3; 16],
            disabled: false,
            groups: vec!["CN=Other,DC=corp,DC=example".into()],
        },
        Account {
            upn: "dave@corp.example",
            guid: [4; 16],
            disabled: false,
            groups: vec![TECHNICIANS.into()],
        },
        Account {
            upn: "erin@other.example",
            guid: [5; 16],
            disabled: false,
            groups: vec![TECHNICIANS.into()],
        },
    ]));
    let port = directory_server(&issuer, accounts.clone()).await;
    let url = format!("ldaps://localhost:{port}");
    for bad in [
        DirectoryConfig {
            domain: "plain.example",
            url: &format!("ldap://localhost:{port}"),
            base_dn: "DC=corp,DC=example",
            ca_pem: &ca,
        },
        DirectoryConfig {
            domain: "nocert.example",
            url: &url,
            base_dn: "DC=corp,DC=example",
            ca_pem: "-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----\n",
        },
    ] {
        assert!(
            auth.configure_directory(tenant, bad, Actor::SYSTEM)
                .await
                .is_err()
        );
    }
    let directory = auth
        .configure_directory(
            tenant,
            DirectoryConfig {
                domain: "Corp.Example",
                url: &url,
                base_dn: "DC=corp,DC=example",
                ca_pem: &ca,
            },
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    let mapping = auth
        .map_directory_group(
            tenant,
            directory,
            TECHNICIANS,
            BuiltinRole::Technician.id(),
            Scope::Tenant,
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    let other = auth
        .configure_directory(
            tenant,
            DirectoryConfig {
                domain: "other.example",
                url: &url,
                base_dn: "DC=corp,DC=example",
                ca_pem: &foreign_ca,
            },
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    auth.map_directory_group(
        tenant,
        other,
        TECHNICIANS,
        BuiltinRole::Technician.id(),
        Scope::Tenant,
        Actor::SYSTEM,
    )
    .await
    .unwrap();
    auth.create_operator("dave@corp.example", "Local Dave")
        .await
        .unwrap();

    let (status, body, cookie) = sign_in(&app, "Alice@corp.example", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["session"]["authentication_method"], "directory");
    assert_eq!(body["operator"]["login"], "alice@corp.example");
    let cookie = cookie.unwrap();
    let request = Request::builder()
        .uri(format!("/api/tenants/{tenant}"))
        .header("cookie", &cookie)
        .extension(ConnectInfo(
            "198.51.100.95:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let context: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert!(
        context["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "remote.control")
    );
    let (status, again, _) = sign_in(&app, "alice@corp.example", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["operator"]["login"], "alice@corp.example");

    for (login, password) in [
        ("alice@corp.example", "wrong"),
        ("alice@corp.example", ""),
        ("bob@corp.example", PASSWORD),
        ("carol@corp.example", PASSWORD),
        ("dave@corp.example", PASSWORD),
        ("mallory@unknown.example", PASSWORD),
        ("alice", PASSWORD),
    ] {
        let (status, body, cookie) = sign_in(&app, login, password).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{login}: {body}");
        assert!(cookie.is_none());
    }
    let (status, _, cookie) = sign_in(&app, "erin@other.example", PASSWORD).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(cookie.is_none());
    let linked: Vec<String> = sqlx::query_scalar("SELECT o.login FROM midas.directory_identities i JOIN midas.operators o ON o.id = i.operator_id ORDER BY o.login")
        .fetch_all(&mut *admin).await.unwrap();
    assert_eq!(linked, ["alice@corp.example"]);
    let local: String = sqlx::query_scalar(
        "SELECT display_name FROM midas.operators WHERE login = 'dave@corp.example'",
    )
    .fetch_one(&mut *admin)
    .await
    .unwrap();
    assert_eq!(local, "Local Dave");

    let derived = "SELECT count(*) FROM midas.role_assignments a JOIN midas.operators o ON o.id = a.operator_id WHERE o.login = 'alice@corp.example' AND a.directory_group_id = $1";
    let count: i64 = sqlx::query_scalar(derived)
        .bind(mapping)
        .fetch_one(&mut *admin)
        .await
        .unwrap();
    assert_eq!(count, 1);
    accounts.lock().unwrap()[0].groups.clear();
    assert_eq!(
        sign_in(&app, "alice@corp.example", PASSWORD).await.0,
        StatusCode::UNAUTHORIZED
    );
    let count: i64 = sqlx::query_scalar(derived)
        .bind(mapping)
        .fetch_one(&mut *admin)
        .await
        .unwrap();
    assert_eq!(count, 0);

    let rows: Vec<String> = sqlx::query_scalar("SELECT row_to_json(a)::text FROM midas.platform_audit a UNION ALL SELECT row_to_json(a)::text FROM midas.tenant_audit a")
        .fetch_all(&mut *admin).await.unwrap();
    assert!(rows.iter().all(|row| !row.contains(PASSWORD)));
    assert!(
        rows.iter()
            .any(|row| row.contains("\"directory\"") && row.contains("auth.sign_in"))
    );
    let stored: Vec<String> =
        sqlx::query_scalar("SELECT row_to_json(d)::text FROM midas.directories d")
            .fetch_all(&mut *admin)
            .await
            .unwrap();
    assert!(stored.iter().all(|row| !row.contains(PASSWORD)));
}
