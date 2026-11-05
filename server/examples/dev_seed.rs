//! Local development only: creates demo tenants, operators and a self-signed localhost certificate.
use std::{fs, path::PathBuf};

use midas_server::{
    audit::Actor,
    auth::{AuthConfig, AuthService},
    authorization::{Authorizer, BuiltinRole, Scope},
    database::Database,
};
use uuid::Uuid;
use webauthn_rs::prelude::Url;
use zeroize::Zeroizing;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(std::env::var("MIDAS_DEV_DIR")?);
    let url = std::env::var("MIDAS_DATABASE_URL")?;
    let origin = Url::parse(&std::env::var("MIDAS_AUTH_ORIGIN")?)?;
    let certificate =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])?;
    fs::write(directory.join("tls.pem"), certificate.cert.pem())?;
    fs::write(
        directory.join("tls.key"),
        certificate.signing_key.serialize_pem(),
    )?;
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key)?;
    fs::write(directory.join("auth.key"), key)?;

    let database = Database::connect(&url, true, 2)
        .await
        .map_err(|e| e.to_string())?;
    let auth = AuthService::new(&database, AuthConfig::new(origin, Zeroizing::new(key)))
        .await
        .map_err(|e| e.to_string())?;
    let policy = Authorizer::new(&database);
    let mut lines =
        vec!["MIDAS local development credentials. Never reuse outside this machine.".to_owned()];
    let mut operators = Vec::new();
    for (login, name) in [
        ("admin", "Demo Adminisztrátor"),
        ("observer", "Demo Megfigyelő"),
    ] {
        let id = auth
            .create_operator(login, name)
            .await
            .map_err(|e| e.to_string())?;
        let codes = auth
            .replace_recovery_codes(id)
            .await
            .map_err(|e| e.to_string())?;
        lines.push(String::new());
        lines.push(format!("{login} ({name}) recovery codes:"));
        lines.extend(codes.iter().map(|code| code.to_string()));
        operators.push(id);
    }
    let mut tenants = Vec::new();
    for (name, site) in [("Alfa Kft.", "Budapest"), ("Béta Zrt.", "Debrecen")] {
        let tenant = Uuid::new_v4();
        let mut tx = database.begin(tenant).await?;
        tx.create_tenant(name).await?;
        tx.create_site(site).await?;
        tx.commit().await?;
        tenants.push(tenant);
    }
    let (admin, observer) = (operators[0], operators[1]);
    for (tenant, operator, role) in [
        (tenants[0], admin, BuiltinRole::Administrator),
        (tenants[1], admin, BuiltinRole::Technician),
        (tenants[0], observer, BuiltinRole::Observer),
    ] {
        policy
            .set_membership(tenant, operator, true, Actor::SYSTEM)
            .await
            .map_err(|e| e.to_string())?;
        policy
            .assign_role(tenant, operator, role.id(), Scope::Tenant, Actor::SYSTEM)
            .await
            .map_err(|e| e.to_string())?;
    }
    fs::write(directory.join("credentials.txt"), lines.join("\n") + "\n")?;
    database.close().await;
    println!("Seeded local development data");
    Ok(())
}
