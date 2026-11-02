use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;
use webauthn_rs::prelude::{PasskeyAuthentication, PasskeyRegistration};

use super::{AuthError, BrowserBinding, Result, crypto};

pub(super) enum Ceremony {
    Register(PasskeyRegistration),
    Authenticate(PasskeyAuthentication),
}

pub(super) struct Pending {
    pub user: Option<Uuid>,
    pub version: i64,
    pub login: String,
    pub ceremony: Ceremony,
}

struct Bound {
    pending: Pending,
    binding: Vec<u8>,
    expires: Instant,
}

pub(super) struct Challenges {
    items: Mutex<HashMap<String, Bound>>,
    lifetime: Duration,
    capacity: usize,
}

impl Challenges {
    pub fn new(lifetime: Duration, capacity: usize) -> Result<Self> {
        if lifetime.is_zero() || lifetime > Duration::from_secs(600) || capacity == 0 {
            return Err(AuthError::InvalidInput);
        }
        Ok(Self {
            items: Mutex::new(HashMap::new()),
            lifetime,
            capacity,
        })
    }

    pub fn insert(&self, binding: &BrowserBinding, pending: Pending) -> Result<String> {
        let mut items = self.items.lock().map_err(|_| AuthError::Unavailable)?;
        let now = Instant::now();
        items.retain(|_, item| item.expires > now);
        if items.len() >= self.capacity {
            return Err(AuthError::Busy);
        }
        let id = hex::encode(crypto::random::<32>()?);
        items.insert(
            id.clone(),
            Bound {
                pending,
                binding: binding.digest(),
                expires: now + self.lifetime,
            },
        );
        Ok(id)
    }

    pub fn take(&self, id: &str, binding: &BrowserBinding) -> Result<Pending> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AuthError::InvalidCredentials);
        }
        let item = self
            .items
            .lock()
            .map_err(|_| AuthError::Unavailable)?
            .remove(id)
            .ok_or(AuthError::InvalidCredentials)?;
        if item.expires <= Instant::now() || item.binding != binding.digest() {
            return Err(AuthError::InvalidCredentials);
        }
        Ok(item.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use webauthn_rs::prelude::*;

    #[test]
    fn challenges_are_bound_single_use_expiring_and_bounded() {
        let origin = Url::parse("https://midas.example").unwrap();
        let webauthn = WebauthnBuilder::new("midas.example", &origin)
            .unwrap()
            .build()
            .unwrap();
        let pending = || Pending {
            user: None,
            version: 0,
            login: "unknown".into(),
            ceremony: Ceremony::Authenticate(webauthn.start_passkey_authentication(&[]).unwrap().1),
        };
        let cache = Challenges::new(Duration::from_secs(300), 1).unwrap();
        let binding = BrowserBinding::random().unwrap();
        let id = cache.insert(&binding, pending()).unwrap();
        assert!(matches!(
            cache.insert(&binding, pending()),
            Err(AuthError::Busy)
        ));
        assert!(cache.take(&id, &BrowserBinding::random().unwrap()).is_err());
        assert!(cache.take(&id, &binding).is_err());
        let id = cache.insert(&binding, pending()).unwrap();
        assert!(cache.take(&id, &binding).is_ok());
        assert!(cache.take(&id, &binding).is_err());
        let id = cache.insert(&binding, pending()).unwrap();
        cache.items.lock().unwrap().get_mut(&id).unwrap().expires = Instant::now();
        assert!(cache.take(&id, &binding).is_err());
    }
}
