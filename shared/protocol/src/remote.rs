use prost::Message;

pub const CAPABILITY: &str = "remote.grants";

#[derive(Clone, PartialEq, Message)]
pub struct Context {
    #[prost(bytes = "vec", tag = "1")]
    pub tenant: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub device: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub public_key: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Prepare {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub session: u32,
    #[prost(bytes = "vec", tag = "3")]
    pub generation: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    pub operator: Vec<u8>,
    #[prost(enumeration = "Mode", tag = "5")]
    pub mode: i32,
    #[prost(bool, tag = "6")]
    pub uac: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Mode {
    Unspecified = 0,
    Normal = 1,
    Administrative = 2,
}

#[derive(Clone, PartialEq, Message)]
pub struct Challenge {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub challenge: Vec<u8>,
    #[prost(enumeration = "crate::sessions::Presence", tag = "3")]
    pub presence: i32,
}

#[derive(Clone, PartialEq, Message)]
#[prost(skip_debug)]
pub struct Deliver {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub grant: Vec<u8>,
}

impl std::fmt::Debug for Deliver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Deliver { .. }")
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct Revoke {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ack {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(bool, tag = "2")]
    pub accepted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Body, Welcome, decode, encode};

    #[test]
    fn grant_messages_round_trip_without_logging_the_payload() {
        let messages = [
            Body::Welcome(Welcome {
                remote: Some(Context {
                    tenant: vec![1; 16],
                    device: vec![2; 16],
                    public_key: vec![3; 32],
                }),
                ..Default::default()
            }),
            Body::RemotePrepare(Prepare {
                request: vec![1; 16],
                session: 2,
                generation: vec![2; 16],
                operator: vec![3; 16],
                mode: Mode::Normal as i32,
                uac: true,
            }),
            Body::RemoteChallenge(Challenge {
                request: vec![1; 16],
                challenge: vec![4; 16],
                presence: crate::sessions::Presence::Vacant as i32,
            }),
            Body::RemoteDeliver(Deliver {
                request: vec![1; 16],
                grant: b"private signed payload".to_vec(),
            }),
            Body::RemoteRevoke(Revoke {
                request: vec![1; 16],
            }),
            Body::RemoteAck(Ack {
                request: vec![1; 16],
                accepted: true,
            }),
        ];
        for message in messages {
            assert_eq!(
                decode(&encode(message.clone())).unwrap(),
                Some(message.clone())
            );
            if matches!(message, Body::RemoteDeliver(_)) {
                assert_eq!(format!("{message:?}"), "RemoteDeliver(Deliver { .. })");
            }
        }
    }
}
