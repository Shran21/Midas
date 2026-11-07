//! JSON contract of the enrollment endpoint on the server-authenticated HTTPS listener.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ENROLL_PATH: &str = "/agent/enroll";
pub const TOKEN_PREFIX: &str = "midas-enroll-";
pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const MAX_NAME_CHARS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
}

impl Platform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Workstation,
    Server,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Workstation => "workstation",
            Self::Server => "server",
        }
    }
}

/// First contact: a single-use token plus a PKCS#10 request proving possession of the agent key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollRequest {
    pub token: String,
    pub csr: String,
    pub platform: Platform,
    pub kind: Kind,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum EnrollAnswer {
    Issued(Issued),
    /// Awaiting operator approval; the secret is shown only once and authorizes polling.
    Pending {
        request_id: Uuid,
        request_secret: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollRequest {
    pub secret: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum PollAnswer {
    Issued(Issued),
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issued {
    pub device_id: Uuid,
    /// PEM client certificate for the operational mTLS channel.
    pub certificate: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_use_a_status_tag() {
        let pending = EnrollAnswer::Pending {
            request_id: Uuid::nil(),
            request_secret: "s".into(),
        };
        let text = serde_json::to_string(&pending).unwrap();
        assert!(text.contains(r#""status":"pending""#), "{text}");
        assert_eq!(
            serde_json::from_str::<EnrollAnswer>(&text).unwrap(),
            pending
        );
        let polled: PollAnswer = serde_json::from_str(r#"{"status":"pending"}"#).unwrap();
        assert_eq!(polled, PollAnswer::Pending);
    }

    #[test]
    fn requests_reject_unknown_fields_and_values() {
        let valid = r#"{"token":"t","csr":"c","platform":"windows","kind":"server","name":"n"}"#;
        assert!(serde_json::from_str::<EnrollRequest>(valid).is_ok());
        let extra = r#"{"token":"t","csr":"c","platform":"windows","kind":"server","name":"n","tenant":"x"}"#;
        assert!(serde_json::from_str::<EnrollRequest>(extra).is_err());
        let platform = r#"{"token":"t","csr":"c","platform":"macos","kind":"server","name":"n"}"#;
        assert!(serde_json::from_str::<EnrollRequest>(platform).is_err());
    }
}
