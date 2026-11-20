//! Short-lived credentials for the TURN relay, in eturnal's shared-secret scheme: the user name is
//! the expiry and the request, the password a Base64 HMAC-SHA1 of the user name. They only open a
//! relay allocation for a short while; how long anything runs is still decided by the session's
//! own grant, which the agent enforces.

use std::time::Duration;

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha1::Sha1;
use uuid::Uuid;
use zeroize::Zeroizing;

/// A relay and its credential for one browser or agent, in the WebRTC `RTCIceServer` shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

pub struct Relay {
    urls: Vec<String>,
    secret: Zeroizing<Vec<u8>>,
    lifetime: Duration,
}

impl Relay {
    pub fn new(
        urls: Vec<String>,
        secret: &str,
        lifetime_seconds: u64,
    ) -> Result<Self, &'static str> {
        if urls.is_empty() || !urls.iter().all(|url| valid(url)) {
            return Err("TURN URLs must start with turn: or turns: and name a host");
        }
        let secret = secret.trim();
        if secret.len() < 16 {
            return Err("the TURN secret must be at least 16 characters");
        }
        if !(60..=3600).contains(&lifetime_seconds) {
            return Err("TURN credentials must live between 60 and 3600 seconds");
        }
        Ok(Self {
            urls,
            secret: Zeroizing::new(secret.as_bytes().to_vec()),
            lifetime: Duration::from_secs(lifetime_seconds),
        })
    }

    /// A credential for `request`, usable to open an allocation until it expires.
    pub fn credentials(&self, request: Uuid, now: i64) -> IceServer {
        let username = format!("{}:{request}", now + self.lifetime.as_secs() as i64);
        let mut mac =
            <Hmac<Sha1> as KeyInit>::new_from_slice(&self.secret).expect("any key length");
        mac.update(username.as_bytes());
        IceServer {
            urls: self.urls.clone(),
            username,
            credential: base64::engine::general_purpose::STANDARD
                .encode(mac.finalize().into_bytes()),
        }
    }
}

fn valid(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("turn:")
        .or_else(|| url.strip_prefix("turns:"))
    else {
        return false;
    };
    let host = rest.split(['?', ':']).next().unwrap_or_default();
    url.len() <= 256
        && !host.is_empty()
        && url
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '<' | '>' | '\\' | '`'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_follow_the_shared_secret_scheme() {
        let relay = Relay::new(
            vec!["turn:relay.example.com:3478?transport=udp".into()],
            "relay-secret-of-the-test",
            300,
        )
        .unwrap();
        let request = Uuid::from_u128(1);
        let server = relay.credentials(request, 1_700_000_000);
        assert_eq!(server.username, format!("1700000300:{request}"));
        assert_eq!(server.urls, ["turn:relay.example.com:3478?transport=udp"]);
        // Base64(HMAC-SHA1(secret, user name)) for the made-up secret above, as computed
        // independently with Python's hmac.
        assert_eq!(server.credential, "GozoELqXKWdUASMZYDSSF3gCU8k="); // gitleaks:allow
    }

    #[test]
    fn only_turn_urls_a_strong_secret_and_a_short_lifetime_are_accepted() {
        let secret = "relay-secret-of-the-test";
        assert!(Relay::new(vec![], secret, 300).is_err());
        for url in [
            "stun:relay:3478",
            "https://relay",
            "turn:",
            "turn:?transport=udp",
            "turn:relay 3478",
        ] {
            assert!(Relay::new(vec![url.into()], secret, 300).is_err(), "{url}");
        }
        assert!(
            Relay::new(
                vec!["turns:relay.example.com:443?transport=tcp".into()],
                secret,
                300
            )
            .is_ok()
        );
        assert!(Relay::new(vec!["turn:relay:3478".into()], "short", 300).is_err());
        assert!(Relay::new(vec!["turn:relay:3478".into()], secret, 30).is_err());
        assert!(Relay::new(vec!["turn:relay:3478".into()], secret, 86_400).is_err());
    }
}
