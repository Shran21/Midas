use std::{
    net::IpAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use midas_server::{
    auth::{AuthConfig, AuthError, AuthService, AuthenticationMethod, BrowserBinding},
    database::Database,
};
use sqlx::PgConnection;
use totp_rs::{Builder, Secret};
use uuid::Uuid;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::*;
use webauthn_rs_proto::UserVerificationPolicy;
use zeroize::Zeroizing;

fn origin() -> Url {
    Url::parse("https://midas.example").unwrap()
}
fn ip() -> IpAddr {
    "192.0.2.1".parse().unwrap()
}
fn password() -> Zeroizing<String> {
    Zeroizing::new("Az én hosszú jelszavam! 🔑".into())
}
fn config(key: [u8; 32]) -> AuthConfig {
    let mut config = AuthConfig::new(origin(), Zeroizing::new(key));
    config.account_attempts = 200;
    config.source_attempts = 1000;
    config
}

async fn enroll(
    service: &AuthService,
    user: Uuid,
    binding: &BrowserBinding,
    token: &mut WebauthnAuthenticator<SoftPasskey>,
) {
    let challenge = service.begin_registration(user, binding).await.unwrap();
    assert_eq!(
        challenge
            .options
            .public_key
            .authenticator_selection
            .as_ref()
            .unwrap()
            .user_verification,
        UserVerificationPolicy::Required
    );
    let response = token.do_registration(origin(), challenge.options).unwrap();
    service
        .finish_registration(&challenge.id, binding, &response)
        .await
        .unwrap();
    assert!(matches!(
        service
            .finish_registration(&challenge.id, binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));
}

pub async fn verify(database: &Database, admin: &mut PgConnection) {
    let mut key = [0; 32];
    getrandom::fill(&mut key).unwrap();
    let service = AuthService::new(database, config(key)).await.unwrap();
    assert!(matches!(
        AuthService::new(database, config([0; 32])).await,
        Err(AuthError::InvalidInput)
    ));
    for bad in [
        "http://midas.example",
        "https://midas.example/path",
        "https://midas.example/?query=1",
        "https://other@midas.example",
    ] {
        let mut config = config(key);
        config.origin = Url::parse(bad).unwrap();
        assert!(matches!(
            AuthService::new(database, config).await,
            Err(AuthError::InvalidInput)
        ));
    }
    let user = service
        .create_operator(" Technikus@Example ", "Technikus")
        .await
        .unwrap();
    let other = service
        .create_operator("other", "Másik technikus")
        .await
        .unwrap();
    assert!(matches!(
        service
            .create_operator("TECHNIKUS@example", "Duplicate")
            .await,
        Err(AuthError::Conflict)
    ));
    assert!(
        service
            .create_operator("bad name", "Invalid")
            .await
            .is_err()
    );
    let binding = BrowserBinding::random().unwrap();
    let mut token = WebauthnAuthenticator::new(SoftPasskey::new(true));
    enroll(&service, user, &binding, &mut token).await;

    let issued = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    assert_eq!(
        issued.options.public_key.user_verification,
        UserVerificationPolicy::Required
    );
    let response = token.do_authentication(origin(), issued.options).unwrap();
    let result = service
        .finish_passkey_login(&issued.id, ip(), &binding, &response)
        .await
        .unwrap();
    assert_eq!(result.operator_id(), user);
    assert_eq!(result.method(), AuthenticationMethod::Passkey);
    assert!(result.verified_at() <= SystemTime::now());
    assert!(matches!(
        service
            .finish_passkey_login(&issued.id, ip(), &binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    let issued = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let mut no_uv = issued.options;
    no_uv.public_key.user_verification = UserVerificationPolicy::Discouraged_DO_NOT_USE;
    let response = token.do_authentication(origin(), no_uv).unwrap();
    assert_eq!(response.response.authenticator_data[32] & 4, 0);
    assert!(matches!(
        service
            .finish_passkey_login(&issued.id, ip(), &binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    let issued = service.begin_registration(other, &binding).await.unwrap();
    let mut no_uv = issued.options;
    no_uv
        .public_key
        .authenticator_selection
        .as_mut()
        .unwrap()
        .user_verification = UserVerificationPolicy::Discouraged_DO_NOT_USE;
    let response = WebauthnAuthenticator::new(SoftPasskey::new(false))
        .do_registration(origin(), no_uv)
        .unwrap();
    assert!(matches!(
        service
            .finish_registration(&issued.id, &binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    let issued = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let response = token
        .do_authentication(
            Url::parse("https://midas.example:8443").unwrap(),
            issued.options,
        )
        .unwrap();
    assert!(matches!(
        service
            .finish_passkey_login(&issued.id, ip(), &binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    let issued = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let response = token.do_authentication(origin(), issued.options).unwrap();
    assert!(matches!(
        service
            .finish_passkey_login(
                &issued.id,
                ip(),
                &BrowserBinding::random().unwrap(),
                &response
            )
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    assert!(
        service
            .finish_passkey_login(&issued.id, ip(), &binding, &response)
            .await
            .is_err()
    );

    let first = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let second = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let first_response = token.do_authentication(origin(), first.options).unwrap();
    let second_response = token.do_authentication(origin(), second.options).unwrap();
    service
        .finish_passkey_login(&second.id, ip(), &binding, &second_response)
        .await
        .unwrap();
    assert!(matches!(
        service
            .finish_passkey_login(&first.id, ip(), &binding, &first_response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    let issued = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let response = token.do_authentication(origin(), issued.options).unwrap();
    let (a, b) = tokio::join!(
        service.finish_passkey_login(&issued.id, ip(), &binding, &response),
        service.finish_passkey_login(&issued.id, ip(), &binding, &response)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);

    let issued = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let mut response = token.do_authentication(origin(), issued.options).unwrap();
    response.response.signature[0] ^= 1;
    assert!(matches!(
        service
            .finish_passkey_login(&issued.id, ip(), &binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    let issued = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let response = token.do_authentication(origin(), issued.options).unwrap();
    sqlx::query("UPDATE midas.operators SET enabled = false WHERE id = $1")
        .bind(user)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert!(matches!(
        service
            .finish_passkey_login(&issued.id, ip(), &binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    sqlx::query("UPDATE midas.operators SET enabled = true WHERE id = $1")
        .bind(user)
        .execute(&mut *admin)
        .await
        .unwrap();

    let registered: (Vec<u8>, String) = sqlx::query_as(
        "SELECT credential_id, credential FROM midas.passkeys WHERE operator_id = $1",
    )
    .bind(user)
    .fetch_one(&mut *admin)
    .await
    .unwrap();
    let duplicate = sqlx::query(
        "INSERT INTO midas.passkeys (credential_id, operator_id, credential) VALUES ($1, $2, $3)",
    )
    .bind(registered.0)
    .bind(other)
    .bind(registered.1)
    .execute(&mut *admin)
    .await
    .unwrap_err();
    assert!(duplicate.as_database_error().unwrap().is_unique_violation());

    let enrollment = service.begin_fallback_enrollment(user).await.unwrap();
    let uri = enrollment.provisioning_uri().unwrap();
    let secret = Url::parse(&uri)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "secret")
        .unwrap()
        .1
        .into_owned();
    let seed = Secret::try_from_base32(secret.as_str()).unwrap();
    let generator = Builder::new().with_secret(seed).build().unwrap();
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let code = generator.generate(epoch).to_string();
    service
        .finish_fallback_enrollment(enrollment, password(), &code)
        .await
        .unwrap();
    assert!(matches!(
        service
            .login_password_totp("technikus@example", password(), &code, ip())
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    let code = generator.generate(epoch + 30).to_string();
    assert!(matches!(
        service
            .login_password_totp(
                "technikus@example",
                Zeroizing::new("wrong password".into()),
                &code,
                ip()
            )
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    assert!(matches!(
        service
            .login_password_totp("missing", password(), &code, ip())
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    assert!(matches!(
        service
            .login_password_totp("technikus@example", password(), "not-a-code", ip())
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    let (a, b) = tokio::join!(
        service.login_password_totp("technikus@example", password(), &code, ip()),
        service.login_password_totp("technikus@example", password(), &code, ip())
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let result = a.or(b).unwrap();
    assert_eq!(result.method(), AuthenticationMethod::PasswordTotp);

    let mut wrong_key = key;
    wrong_key[0] ^= 1;
    let cannot_decrypt = AuthService::new(database, config(wrong_key)).await.unwrap();
    assert!(matches!(
        cannot_decrypt
            .login_password_totp("technikus@example", password(), &code, ip())
            .await,
        Err(AuthError::Unavailable)
    ));

    let stored: (String, Vec<u8>) =
        sqlx::query_as("SELECT password_hash, totp_secret FROM midas.operators WHERE id = $1")
            .bind(user)
            .fetch_one(&mut *admin)
            .await
            .unwrap();
    assert!(stored.0.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    assert_eq!(stored.1.len(), 61);
    assert!(
        !stored
            .1
            .windows(secret.len())
            .any(|window| window == secret.as_bytes())
    );
    let bad_enrollment = service.begin_fallback_enrollment(other).await.unwrap();
    assert!(matches!(
        service
            .finish_fallback_enrollment(bad_enrollment, password(), "invalid")
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    let unset: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM midas.operators WHERE id = $1")
            .bind(other)
            .fetch_one(&mut *admin)
            .await
            .unwrap();
    assert!(unset.is_none());

    let codes = service.replace_recovery_codes(user).await.unwrap();
    assert_eq!(codes.len(), 10);
    let distinct = codes
        .iter()
        .map(|c| c.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(distinct.len(), 10);
    let digests: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT digest FROM midas.recovery_codes WHERE operator_id = $1")
            .bind(user)
            .fetch_all(&mut *admin)
            .await
            .unwrap();
    assert!(digests.iter().all(|digest| digest.len() == 32 && !codes.iter().any(|code| code.as_bytes() == digest)));
    assert!(matches!(
        service
            .login_recovery("other", codes[0].clone(), ip())
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    let (a, b) = tokio::join!(
        service.login_recovery("technikus@example", codes[0].clone(), ip()),
        service.login_recovery("technikus@example", codes[0].clone(), ip())
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(
        a.or(b).unwrap().method(),
        AuthenticationMethod::RecoveryCode
    );

    let restarted = AuthService::new(database, config(key)).await.unwrap();
    assert!(matches!(
        restarted
            .login_recovery("technikus@example", codes[0].clone(), ip())
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    restarted
        .login_recovery("technikus@example", codes[1].clone(), ip())
        .await
        .unwrap();
    let issued = restarted
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let response = token.do_authentication(origin(), issued.options).unwrap();
    restarted
        .finish_passkey_login(&issued.id, ip(), &binding, &response)
        .await
        .unwrap();
    let pending = service
        .begin_passkey_login("technikus@example", ip(), &binding)
        .await
        .unwrap();
    let response = token.do_authentication(origin(), pending.options).unwrap();
    assert!(
        restarted
            .finish_passkey_login(&pending.id, ip(), &binding, &response)
            .await
            .is_err()
    );

    service.replace_recovery_codes(user).await.unwrap();
    assert!(matches!(
        service
            .login_recovery("technikus@example", codes[2].clone(), ip())
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    assert!(matches!(
        service
            .finish_passkey_login(&pending.id, ip(), &binding, &response)
            .await,
        Err(AuthError::InvalidCredentials)
    ));
    let codes = service.replace_recovery_codes(other).await.unwrap();
    sqlx::query("UPDATE midas.operators SET enabled = false WHERE id = $1")
        .bind(other)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert!(matches!(
        service
            .login_recovery("other", codes[0].clone(), ip())
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    rate_limits(database, admin, key).await;
}

async fn rate_limits(database: &Database, admin: &mut PgConnection, key: [u8; 32]) {
    sqlx::query("DELETE FROM midas.authentication_attempts")
        .execute(&mut *admin)
        .await
        .unwrap();
    let mut settings = config(key);
    settings.account_attempts = 2;
    settings.source_attempts = 3;
    let limited = AuthService::new(database, settings).await.unwrap();
    let binding = BrowserBinding::random().unwrap();
    let (a, b, c) = tokio::join!(
        limited.begin_passkey_login("rate-user", "192.0.2.10".parse().unwrap(), &binding),
        limited.begin_passkey_login("rate-user", "192.0.2.11".parse().unwrap(), &binding),
        limited.begin_passkey_login("rate-user", "192.0.2.12".parse().unwrap(), &binding)
    );
    assert_eq!([a, b, c].into_iter().filter(Result::is_ok).count(), 2);
    let mut settings = config(key);
    settings.account_attempts = 2;
    settings.source_attempts = 3;
    let restarted = AuthService::new(database, settings).await.unwrap();
    assert!(matches!(
        restarted
            .begin_passkey_login("RATE-USER", "192.0.2.13".parse().unwrap(), &binding)
            .await,
        Err(AuthError::RateLimited)
    ));
    let source = "192.0.2.20".parse().unwrap();
    for user in ["unknown-one", "unknown-two", "unknown-three"] {
        limited
            .begin_passkey_login(user, source, &binding)
            .await
            .unwrap();
    }
    assert!(matches!(
        limited
            .begin_passkey_login("unknown-four", source, &binding)
            .await,
        Err(AuthError::RateLimited)
    ));
    assert!(matches!(
        limited
            .login_recovery("unknown-five", Zeroizing::new("0".repeat(64)), source)
            .await,
        Err(AuthError::RateLimited)
    ));
    sqlx::query("UPDATE midas.authentication_attempts SET expires_at = clock_timestamp() - interval '1 second'").execute(&mut *admin).await.unwrap();
    limited
        .begin_passkey_login("rate-user", source, &binding)
        .await
        .unwrap();
}
