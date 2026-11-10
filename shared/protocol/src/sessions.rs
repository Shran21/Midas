use std::collections::HashSet;

use prost::Message;
use uuid::Uuid;

pub const CAPABILITY: &str = "remote.sessions";
pub const MAX_SESSIONS: usize = 128;

#[derive(Clone, PartialEq, Message)]
pub struct SessionRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub request_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub connection_id: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SessionReport {
    #[prost(bytes = "vec", tag = "1")]
    pub request_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub connection_id: Vec<u8>,
    #[prost(bool, tag = "3")]
    pub available: bool,
    #[prost(message, repeated, tag = "4")]
    pub sessions: Vec<OsSession>,
}

#[derive(Clone, PartialEq, Message)]
pub struct OsSession {
    #[prost(uint32, tag = "1")]
    pub id: u32,
    #[prost(bytes = "vec", tag = "2")]
    pub generation: Vec<u8>,
    #[prost(enumeration = "Presence", tag = "3")]
    pub presence: i32,
    #[prost(string, tag = "4")]
    pub station: String,
    #[prost(string, tag = "5")]
    pub account: String,
    #[prost(bool, tag = "6")]
    pub console: bool,
    #[prost(bool, optional, tag = "7")]
    pub locked: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Presence {
    Unknown = 0,
    Vacant = 1,
    Occupied = 2,
}

pub fn identifier(value: &[u8]) -> bool {
    Uuid::from_slice(value).is_ok_and(|id| !id.is_nil())
}

pub fn valid_sessions(sessions: &[OsSession]) -> bool {
    let mut ids = HashSet::new();
    sessions.len() <= MAX_SESSIONS
        && sessions.iter().all(|session| {
            session.id != 0
                && session.id != u32::MAX
                && ids.insert(session.id)
                && identifier(&session.generation)
                && Presence::try_from(session.presence).is_ok()
                && text(&session.station, 128)
                && text(&session.account, 256)
                && (session.presence != Presence::Vacant as i32 || session.account.is_empty())
        })
}

fn text(value: &str, limit: usize) -> bool {
    value.len() <= limit && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> OsSession {
        OsSession {
            id: 1,
            generation: Uuid::new_v4().as_bytes().to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn largest_valid_report_fits_the_protocol_envelope() {
        let report = SessionReport {
            request_id: Uuid::new_v4().as_bytes().to_vec(),
            connection_id: Uuid::new_v4().as_bytes().to_vec(),
            available: true,
            sessions: (1..=MAX_SESSIONS as u32)
                .map(|id| OsSession {
                    id,
                    station: "s".repeat(128),
                    account: "a".repeat(256),
                    presence: Presence::Occupied as i32,
                    locked: Some(true),
                    console: true,
                    ..session()
                })
                .collect(),
        };
        assert!(valid_sessions(&report.sessions));
        let body = crate::Body::SessionReport(report);
        let encoded = crate::encode(body.clone());
        assert!(encoded.len() <= crate::MAX_MESSAGE_BYTES);
        assert_eq!(crate::decode(&encoded).unwrap(), Some(body));
    }

    #[test]
    fn malformed_or_partial_session_lists_are_not_accepted() {
        let mut entry = session();
        assert!(valid_sessions(&[entry.clone()]));
        assert!(!valid_sessions(&[entry.clone(), entry.clone()]));
        entry.presence = 99;
        assert!(!valid_sessions(&[entry.clone()]));
        entry.presence = Presence::Vacant as i32;
        entry.account = "user".into();
        assert!(!valid_sessions(&[entry.clone()]));
        entry.account.clear();
        entry.generation = vec![0; 16];
        assert!(!valid_sessions(&[entry]));
        assert!(!valid_sessions(
            &(1..=129)
                .map(|id| OsSession { id, ..session() })
                .collect::<Vec<_>>()
        ));
    }
}
