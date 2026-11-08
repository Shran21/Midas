//! Wire contract between MIDAS agents and the server.
//!
//! Compatible changes (new optional fields or new message types) keep the
//! protocol version, because receivers skip unknown fields and ignore unknown
//! message types. Changing or removing the meaning of an existing field needs a
//! new version, negotiated through [`VersionRange::negotiate`].

use prost::Message;

pub mod directory;
pub mod enrollment;
pub mod policy;

/// Versions this build can speak.
pub const SUPPORTED: VersionRange = VersionRange { min: 1, max: 1 };
/// Upper bound for one encoded message, enforced on both ends of the socket.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const CONNECT_PATH: &str = "/agent/connect";
pub const MAX_AGENT_VERSION_BYTES: usize = 64;
pub const MAX_PLATFORM_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionRange {
    pub min: u32,
    pub max: u32,
}

impl VersionRange {
    pub fn new(min: u32, max: u32) -> Option<Self> {
        (min >= 1 && min <= max).then_some(Self { min, max })
    }

    /// Highest version both sides support.
    pub fn negotiate(self, peer: Self) -> Option<u32> {
        let low = self.min.max(peer.min);
        let high = self.max.min(peer.max);
        (low <= high).then_some(high)
    }

    pub fn contains(self, version: u32) -> bool {
        (self.min..=self.max).contains(&version)
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct Envelope {
    #[prost(
        oneof = "Body",
        tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15"
    )]
    pub body: Option<Body>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
pub enum Body {
    #[prost(message, tag = "1")]
    Hello(Hello),
    #[prost(message, tag = "2")]
    Welcome(Welcome),
    #[prost(message, tag = "3")]
    Reject(Reject),
    #[prost(message, tag = "4")]
    Ping(Ping),
    #[prost(message, tag = "5")]
    Pong(Pong),
    #[prost(message, tag = "6")]
    DirectoryAssignment(DirectoryAssignment),
    #[prost(message, tag = "7")]
    DirectoryReport(DirectoryReport),
    #[prost(message, tag = "8")]
    MetricsBatch(MetricsBatch),
    #[prost(message, tag = "9")]
    MetricsAck(MetricsAck),
    #[prost(message, tag = "10")]
    InventoryUpdate(InventoryUpdate),
    #[prost(message, tag = "11")]
    InventoryAck(InventoryAck),
    #[prost(message, tag = "12")]
    HealthReport(HealthReport),
    #[prost(message, tag = "13")]
    HealthAck(HealthAck),
    #[prost(message, tag = "14")]
    PolicyAssignment(PolicyAssignment),
    #[prost(message, tag = "15")]
    RefreshRequest(RefreshRequest),
}

/// First message from the agent.
#[derive(Clone, PartialEq, Message)]
pub struct Hello {
    #[prost(uint32, tag = "1")]
    pub min_version: u32,
    #[prost(uint32, tag = "2")]
    pub max_version: u32,
    #[prost(string, tag = "3")]
    pub agent_version: String,
    #[prost(string, tag = "4")]
    pub platform: String,
    /// Features this agent build offers on its platform, such as `inventory` or `health.encryption`.
    #[prost(string, repeated, tag = "5")]
    pub capabilities: Vec<String>,
}

pub const MAX_CAPABILITIES: usize = 32;
pub const MAX_CAPABILITY_BYTES: usize = 48;

/// Accepted handshake; `version` applies for the rest of the session.
#[derive(Clone, PartialEq, Message)]
pub struct Welcome {
    #[prost(uint32, tag = "1")]
    pub version: u32,
    #[prost(uint32, tag = "2")]
    pub keepalive_seconds: u32,
    #[prost(string, tag = "3")]
    pub server_version: String,
}

/// Refused handshake; the server closes the connection after sending it.
#[derive(Clone, PartialEq, Message)]
pub struct Reject {
    #[prost(enumeration = "RejectReason", tag = "1")]
    pub reason: i32,
    #[prost(uint32, tag = "2")]
    pub min_version: u32,
    #[prost(uint32, tag = "3")]
    pub max_version: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum RejectReason {
    Unspecified = 0,
    UnsupportedVersion = 1,
    InvalidHello = 2,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ping {
    #[prost(uint64, tag = "1")]
    pub nonce: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Pong {
    #[prost(uint64, tag = "1")]
    pub nonce: u64,
}

/// Server to the tenant's designated directory connector: what it may read.
#[derive(Clone, PartialEq, Message)]
pub struct DirectoryAssignment {
    #[prost(uint64, tag = "1")]
    pub revision: u64,
    #[prost(string, tag = "2")]
    pub domain: String,
    /// Always `ldaps://`; plain LDAP is refused by the connector.
    #[prost(string, tag = "3")]
    pub url: String,
    /// Trust anchors for the directory; system roots are never used.
    #[prost(string, tag = "4")]
    pub ca_pem: String,
    /// Searches stay inside these organizational units.
    #[prost(string, repeated, tag = "5")]
    pub base_dns: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct DirectoryReport {
    #[prost(uint64, tag = "1")]
    pub revision: u64,
    #[prost(bool, tag = "2")]
    pub ok: bool,
    #[prost(uint32, tag = "3")]
    pub objects: u32,
    /// Stable error code such as `tls`, `bind` or `boundary`; never directory content.
    #[prost(string, tag = "4")]
    pub error: String,
}

/// Most samples in one batch; the next batch waits for the acknowledgement.
pub const MAX_BATCH_SAMPLES: usize = 60;

/// One minute of device metrics; counters are deltas since the previous sample.
#[derive(Clone, PartialEq, Message)]
pub struct Sample {
    /// Unix time in seconds, aligned to the minute.
    #[prost(int64, tag = "1")]
    pub at: i64,
    #[prost(uint32, tag = "2")]
    pub cpu_permille: u32,
    #[prost(uint64, tag = "3")]
    pub memory_used: u64,
    #[prost(uint64, tag = "4")]
    pub memory_total: u64,
    #[prost(uint64, tag = "5")]
    pub disk_used: u64,
    #[prost(uint64, tag = "6")]
    pub disk_total: u64,
    #[prost(uint64, tag = "7")]
    pub net_rx: u64,
    #[prost(uint64, tag = "8")]
    pub net_tx: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct MetricsBatch {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
    #[prost(message, repeated, tag = "2")]
    pub samples: Vec<Sample>,
    /// Samples the agent discarded because its buffer was full.
    #[prost(uint64, tag = "3")]
    pub dropped: u64,
}

/// Sent after the batch is stored; only then may the agent forget it.
#[derive(Clone, PartialEq, Message)]
pub struct MetricsAck {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
}

/// Items per inventory chunk; keeps every chunk well below the message limit.
pub const MAX_INVENTORY_ITEMS: usize = 200;
pub const MAX_INVENTORY_TEXT: usize = 256;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Message)]
pub struct InventoryKey {
    /// `hardware`, `os`, `disk`, `software` or `admin`.
    #[prost(string, tag = "1")]
    pub kind: String,
    #[prost(string, tag = "2")]
    pub key: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct InventoryItem {
    #[prost(message, optional, tag = "1")]
    pub key: Option<InventoryKey>,
    #[prost(string, tag = "2")]
    pub value: String,
}

/// One chunk of a full reconciliation or of a change set.
#[derive(Clone, PartialEq, Message)]
pub struct InventoryUpdate {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
    /// A full reconciliation starts a new generation; changes carry the current one.
    #[prost(uint64, tag = "2")]
    pub generation: u64,
    #[prost(bool, tag = "3")]
    pub full: bool,
    /// Set on the final chunk; a full reconciliation then removes everything not resent.
    #[prost(bool, tag = "4")]
    pub last: bool,
    #[prost(message, repeated, tag = "5")]
    pub upserts: Vec<InventoryItem>,
    #[prost(message, repeated, tag = "6")]
    pub removals: Vec<InventoryKey>,
}

#[derive(Clone, PartialEq, Message)]
pub struct InventoryAck {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
    /// False when a change set did not match the stored generation; the agent then reconciles fully.
    #[prost(bool, tag = "2")]
    pub applied: bool,
}

pub const MAX_HEALTH_CHECKS: usize = 16;
pub const MAX_HEALTH_SUBJECTS: usize = 16;
/// Characters in a summary or subject.
pub const MAX_HEALTH_TEXT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum HealthState {
    Unspecified = 0,
    Ok = 1,
    Warning = 2,
    Critical = 3,
    /// The check cannot run here: not on this platform, edition or with these rights.
    Unavailable = 4,
}

#[derive(Clone, PartialEq, Message)]
pub struct HealthCheck {
    /// `services`, `updates`, `antivirus`, `encryption` or `storage`.
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(enumeration = "HealthState", tag = "2")]
    pub state: i32,
    /// Stable reason such as `stopped`, `reboot_pending` or `access_denied`; the panel translates it.
    #[prost(string, tag = "3")]
    pub code: String,
    /// English fallback text for logs and older panels.
    #[prost(string, tag = "4")]
    pub summary: String,
    /// What the reason refers to: service names, volumes, disks or a day count.
    #[prost(string, repeated, tag = "5")]
    pub subjects: Vec<String>,
}

/// The latest result of every check; each report replaces the previous one.
#[derive(Clone, PartialEq, Message)]
pub struct HealthReport {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
    #[prost(message, repeated, tag = "2")]
    pub checks: Vec<HealthCheck>,
}

#[derive(Clone, PartialEq, Message)]
pub struct HealthAck {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
}

pub const MAX_POLICY_SETTINGS: usize = 64;

#[derive(Clone, PartialEq, Message)]
pub struct PolicySetting {
    /// Such as `health.low_space_percent`; agents ignore keys they do not know.
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(int64, tag = "2")]
    pub value: i64,
}

/// The device's effective settings after inheritance; it replaces any earlier assignment.
#[derive(Clone, PartialEq, Message)]
pub struct PolicyAssignment {
    #[prost(message, repeated, tag = "1")]
    pub settings: Vec<PolicySetting>,
}

/// An operator asked for current data: the agent runs its health checks and collects its inventory at once.
#[derive(Clone, PartialEq, Message)]
pub struct RefreshRequest {}

#[derive(Debug)]
pub enum DecodeError {
    TooLarge,
    Malformed(prost::DecodeError),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("protocol message exceeds the size limit"),
            Self::Malformed(error) => write!(f, "malformed protocol message: {error}"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn encode(body: Body) -> Vec<u8> {
    Envelope { body: Some(body) }.encode_to_vec()
}

/// `Ok(None)` is a message type from a newer peer that this build does not know.
pub fn decode(bytes: &[u8]) -> Result<Option<Body>, DecodeError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(DecodeError::TooLarge);
    }
    Envelope::decode(bytes)
        .map(|envelope| envelope.body)
        .map_err(DecodeError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min: u32, max: u32) -> VersionRange {
        VersionRange::new(min, max).unwrap()
    }

    #[test]
    fn negotiation_picks_the_highest_common_version() {
        assert_eq!(range(1, 2).negotiate(range(1, 1)), Some(1));
        assert_eq!(range(1, 2).negotiate(range(2, 3)), Some(2));
        assert_eq!(range(2, 4).negotiate(range(1, 3)), Some(3));
        assert_eq!(range(2, 2).negotiate(range(1, 1)), None);
        assert_eq!(range(1, 1).negotiate(range(3, 5)), None);
    }

    #[test]
    fn invalid_ranges_are_refused() {
        assert_eq!(VersionRange::new(0, 1), None);
        assert_eq!(VersionRange::new(3, 2), None);
        assert!(range(1, 3).contains(2) && !range(1, 3).contains(4));
    }

    #[test]
    fn messages_round_trip() {
        let hello = Body::Hello(Hello {
            min_version: 1,
            max_version: 2,
            agent_version: "0.1.0".into(),
            platform: "windows".into(),
            capabilities: vec!["inventory".into()],
        });
        assert_eq!(decode(&encode(hello.clone())).unwrap(), Some(hello));
        let reject = Body::Reject(Reject {
            reason: RejectReason::UnsupportedVersion as i32,
            min_version: 2,
            max_version: 3,
        });
        assert_eq!(decode(&encode(reject.clone())).unwrap(), Some(reject));
    }

    #[derive(Clone, PartialEq, Message)]
    struct NewerHello {
        #[prost(uint32, tag = "1")]
        min_version: u32,
        #[prost(uint32, tag = "2")]
        max_version: u32,
        #[prost(string, tag = "3")]
        agent_version: String,
        #[prost(string, tag = "4")]
        platform: String,
        #[prost(string, repeated, tag = "9")]
        capabilities: Vec<String>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct NewerEnvelope {
        #[prost(message, optional, tag = "1")]
        hello: Option<NewerHello>,
        #[prost(message, optional, tag = "40")]
        notice: Option<Ping>,
    }

    #[test]
    fn newer_fields_and_message_types_are_tolerated() {
        let hello = NewerEnvelope {
            hello: Some(NewerHello {
                min_version: 1,
                max_version: 3,
                agent_version: "0.2.0".into(),
                platform: "linux".into(),
                capabilities: vec!["inventory".into()],
            }),
            notice: None,
        };
        let Some(Body::Hello(decoded)) = decode(&hello.encode_to_vec()).unwrap() else {
            panic!("hello was not decoded");
        };
        assert_eq!(
            (decoded.max_version, decoded.platform.as_str()),
            (3, "linux")
        );
        let unknown = NewerEnvelope {
            hello: None,
            notice: Some(Ping { nonce: 7 }),
        };
        assert_eq!(decode(&unknown.encode_to_vec()).unwrap(), None);
    }

    #[test]
    fn oversized_and_malformed_input_is_rejected() {
        assert!(matches!(
            decode(&vec![0; MAX_MESSAGE_BYTES + 1]),
            Err(DecodeError::TooLarge)
        ));
        assert!(matches!(
            decode(&[0x0a, 0xff]),
            Err(DecodeError::Malformed(_))
        ));
    }
}
