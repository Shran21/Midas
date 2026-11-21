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
    /// Present only for an attended request: the agent must obtain local consent first.
    #[prost(message, optional, tag = "7")]
    pub consent: Option<ConsentRequest>,
    #[prost(bool, tag = "8")]
    pub clipboard: bool,
    /// Files from the operator to the device.
    #[prost(bool, tag = "9")]
    pub upload: bool,
    /// Files from the device to the operator.
    #[prost(bool, tag = "10")]
    pub download: bool,
}

/// What the local consent window names, as verified by the server.
#[derive(Clone, PartialEq, Message)]
pub struct ConsentRequest {
    #[prost(string, tag = "1")]
    pub operator_name: String,
    #[prost(string, tag = "2")]
    pub customer_name: String,
    #[prost(uint32, tag = "3")]
    pub timeout_seconds: u32,
}

/// The local answer to an attended request; `Unspecified` for unattended requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Consent {
    Unspecified = 0,
    Allowed = 1,
    Declined = 2,
    Expired = 3,
    Unavailable = 4,
    Cancelled = 5,
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
    #[prost(enumeration = "Consent", tag = "4")]
    pub consent: i32,
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

/// Why the agent ended a session without the server asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum EndReason {
    Unspecified = 0,
    /// The local user pressed End.
    Local = 1,
    /// The target session or desktop changed.
    Desktop = 2,
    /// No renewal arrived within the grant's lifetime.
    Expired = 3,
    /// The workers stopped.
    Stopped = 4,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ended {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(enumeration = "EndReason", tag = "2")]
    pub reason: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum TransferKind {
    Unspecified = 0,
    ClipboardToDevice = 1,
    ClipboardFromDevice = 2,
    /// A file from the operator to the device.
    Upload = 3,
    /// A file from the device to the operator.
    Download = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum TransferOutcome {
    Unspecified = 0,
    Started = 1,
    Completed = 2,
    Failed = 3,
    Cancelled = 4,
    /// The session ended first; the file may resume in another one.
    Interrupted = 5,
}

/// The most UTF-8 bytes of a reported file name.
pub const MAX_TRANSFER_NAME: usize = 1024;

/// What the audit records of one transfer: never the clipboard's content or length, nor a
/// file's bytes.
#[derive(Clone, PartialEq, Message)]
pub struct TransferReport {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(enumeration = "TransferKind", tag = "2")]
    pub kind: i32,
    #[prost(enumeration = "TransferOutcome", tag = "3")]
    pub outcome: i32,
    /// The file's name; empty for the clipboard.
    #[prost(string, tag = "4")]
    pub name: String,
    #[prost(uint64, optional, tag = "5")]
    pub size: Option<u64>,
    /// SHA-256 of a completed file; empty otherwise.
    #[prost(bytes = "vec", tag = "6")]
    pub sha256: Vec<u8>,
    /// Why it failed, as the agent names it, such as `no_space`.
    #[prost(string, tag = "7")]
    pub problem: String,
}

/// Characters of one chat message.
pub const MAX_CHAT_CHARS: usize = 1000;

#[derive(Clone, PartialEq, Message)]
pub struct Chat {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(string, tag = "2")]
    pub text: String,
}

/// A chat message both sides accept: not blank, within the limit, and no control character
/// other than a line break.
pub fn chat_valid(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= MAX_CHAT_CHARS
        && !text.chars().any(|c| c.is_control() && c != '\n')
}

/// The most SDP bytes either side accepts.
pub const MAX_SDP: usize = 16 * 1024;

/// The browser's WebRTC offer for the active, acknowledged grant of `request`, with the TURN
/// relays and short-lived credentials the agent may allocate on.
#[derive(Clone, PartialEq, Message)]
pub struct MediaOffer {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(string, tag = "2")]
    pub sdp: String,
    #[prost(message, repeated, tag = "3")]
    pub relays: Vec<Relay>,
}

#[derive(Clone, PartialEq, Message)]
#[prost(skip_debug)]
pub struct Relay {
    #[prost(string, repeated, tag = "1")]
    pub urls: Vec<String>,
    #[prost(string, tag = "2")]
    pub username: String,
    #[prost(string, tag = "3")]
    pub credential: String,
}

impl std::fmt::Debug for Relay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relay")
            .field("urls", &self.urls)
            .finish_non_exhaustive()
    }
}

/// The agent's answer; `sdp` is empty when the agent refused the offer.
#[derive(Clone, PartialEq, Message)]
pub struct MediaAnswer {
    #[prost(bytes = "vec", tag = "1")]
    pub request: Vec<u8>,
    #[prost(string, tag = "2")]
    pub sdp: String,
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
                consent: Some(ConsentRequest {
                    operator_name: "Operator".into(),
                    customer_name: "Customer".into(),
                    timeout_seconds: 60,
                }),
                clipboard: true,
                upload: false,
                download: true,
            }),
            Body::RemoteChallenge(Challenge {
                request: vec![1; 16],
                challenge: vec![4; 16],
                presence: crate::sessions::Presence::Occupied as i32,
                consent: Consent::Allowed as i32,
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
            Body::MediaOffer(MediaOffer {
                request: vec![1; 16],
                sdp: "v=0".into(),
                relays: vec![Relay {
                    urls: vec!["turn:relay:3478".into()],
                    username: "1:request".into(),
                    credential: "secret".into(),
                }],
            }),
            Body::MediaAnswer(MediaAnswer {
                request: vec![1; 16],
                sdp: String::new(),
            }),
            Body::RemoteRenew(Deliver {
                request: vec![1; 16],
                grant: b"private renewal".to_vec(),
            }),
            Body::RemoteRenewed(Ack {
                request: vec![1; 16],
                accepted: false,
            }),
            Body::RemoteEnded(Ended {
                request: vec![1; 16],
                reason: EndReason::Local as i32,
            }),
            Body::RemoteChat(Chat {
                request: vec![1; 16],
                text: "Szia!\nMindjárt nézem.".into(),
            }),
            Body::RemoteTransfer(TransferReport {
                request: vec![1; 16],
                kind: TransferKind::Download as i32,
                outcome: TransferOutcome::Completed as i32,
                name: "jelentés.pdf".into(),
                size: Some(5),
                sha256: vec![0xab; 32],
                problem: String::new(),
            }),
        ];
        assert!(chat_valid("Szia!\nMindjárt nézem."));
        assert!(!chat_valid("  \n "));
        assert!(!chat_valid("a\u{7}b"));
        assert!(!chat_valid(&"x".repeat(MAX_CHAT_CHARS + 1)));
        for message in messages {
            assert_eq!(
                decode(&encode(message.clone())).unwrap(),
                Some(message.clone())
            );
            if matches!(message, Body::RemoteDeliver(_)) {
                assert_eq!(format!("{message:?}"), "RemoteDeliver(Deliver { .. })");
            }
            if matches!(message, Body::RemoteRenew(_)) {
                assert_eq!(format!("{message:?}"), "RemoteRenew(Deliver { .. })");
            }
        }
    }
}
