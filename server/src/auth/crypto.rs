use std::sync::Arc;

use argon2::{
    Argon2, Params,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{AuthError, Result};

pub(super) fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Unavailable)?;
    Ok(bytes)
}

pub(super) fn digest(domain: &[u8], value: &[u8]) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update([0]);
    hash.update(value);
    hash.finalize().to_vec()
}

pub(super) struct Passwords {
    workers: Arc<Semaphore>,
}

impl Passwords {
    pub fn new(workers: usize) -> Result<Self> {
        if workers == 0 || workers > Semaphore::MAX_PERMITS {
            return Err(AuthError::InvalidInput);
        }
        Ok(Self {
            workers: Arc::new(Semaphore::new(workers)),
        })
    }

    pub async fn hash(&self, password: Zeroizing<String>) -> Result<String> {
        if password.len() > 1024 || password.chars().count() < 12 {
            return Err(AuthError::InvalidInput);
        }
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError::Busy)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let salt =
                SaltString::encode_b64(&random::<16>()?).map_err(|_| AuthError::Unavailable)?;
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map(|hash| hash.to_string())
                .map_err(|_| AuthError::Unavailable)
        })
        .await
        .map_err(|_| AuthError::Unavailable)?
    }

    pub async fn verify(&self, password: Zeroizing<String>, stored: String) -> Result<bool> {
        if password.len() > 1024 || stored.len() > 512 {
            return Ok(false);
        }
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError::Busy)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let parsed = PasswordHash::new(&stored).map_err(|_| AuthError::Unavailable)?;
            let params = Params::try_from(&parsed).map_err(|_| AuthError::Unavailable)?;
            if parsed.algorithm.as_str() != "argon2id"
                || parsed.version != Some(19)
                || params.m_cost() != 19 * 1024
                || params.t_cost() != 2
                || params.p_cost() != 1
            {
                return Err(AuthError::Unavailable);
            }
            Ok(Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok())
        })
        .await
        .map_err(|_| AuthError::Unavailable)?
    }
}

pub(super) struct Vault {
    key: Zeroizing<[u8; 32]>,
}

impl Vault {
    pub fn new(key: Zeroizing<[u8; 32]>) -> Result<Self> {
        if *key == [0; 32] {
            return Err(AuthError::InvalidInput);
        }
        Ok(Self { key })
    }

    pub fn seal(&self, user: Uuid, secret: &[u8]) -> Result<Vec<u8>> {
        let nonce = random::<24>()?;
        let cipher = XChaCha20Poly1305::new((&*self.key).into());
        let aad = context(user);
        let encrypted = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: secret,
                    aad: &aad,
                },
            )
            .map_err(|_| AuthError::Unavailable)?;
        let mut envelope = vec![1];
        envelope.extend(nonce);
        envelope.extend(encrypted);
        Ok(envelope)
    }

    pub fn open(&self, user: Uuid, envelope: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if envelope.len() != 61 || envelope[0] != 1 {
            return Err(AuthError::Unavailable);
        }
        let nonce: [u8; 24] = envelope[1..25]
            .try_into()
            .map_err(|_| AuthError::Unavailable)?;
        let cipher = XChaCha20Poly1305::new((&*self.key).into());
        let aad = context(user);
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &envelope[25..],
                    aad: &aad,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| AuthError::Unavailable)
    }
}

fn context(user: Uuid) -> Vec<u8> {
    [b"midas/totp/v1/".as_slice(), user.as_bytes()].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn passwords_use_independent_salts_and_strong_parameters() {
        let passwords = Passwords::new(1).unwrap();
        let input = || Zeroizing::new("hosszú jelszó mondat 🔑".to_owned());
        let a = passwords.hash(input()).await.unwrap();
        let b = passwords.hash(input()).await.unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(passwords.verify(input(), a.clone()).await.unwrap());
        assert!(
            !passwords
                .verify(Zeroizing::new("wrong".into()), a)
                .await
                .unwrap()
        );
        assert!(
            passwords
                .hash(Zeroizing::new("short".into()))
                .await
                .is_err()
        );
        let permit = passwords.workers.clone().acquire_owned().await.unwrap();
        assert!(matches!(
            passwords.hash(input()).await,
            Err(AuthError::Busy)
        ));
        drop(permit);
    }

    #[test]
    fn encrypted_seeds_bind_identity_key_and_integrity() {
        let vault = Vault::new(Zeroizing::new(random().unwrap())).unwrap();
        let user = Uuid::new_v4();
        let secret = random::<20>().unwrap();
        let a = vault.seal(user, &secret).unwrap();
        assert_ne!(a, vault.seal(user, &secret).unwrap());
        assert_eq!(vault.open(user, &a).unwrap().as_slice(), secret);
        assert!(vault.open(Uuid::new_v4(), &a).is_err());
        assert!(
            Vault::new(Zeroizing::new(random().unwrap()))
                .unwrap()
                .open(user, &a)
                .is_err()
        );
        let mut changed = a;
        changed[40] ^= 1;
        assert!(vault.open(user, &changed).is_err());
        assert!(vault.open(user, &[0]).is_err());
    }
}
