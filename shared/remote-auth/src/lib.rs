use prost::Message;
use ring::{
    rand::SystemRandom,
    signature::{self, Ed25519KeyPair, KeyPair},
};
use uuid::Uuid;

pub const MAX_LIFETIME_SECONDS: u64 = 30;
pub const MAX_PAYLOAD_BYTES: usize = 512;
pub const MAX_GRANT_BYTES: usize = MAX_PAYLOAD_BYTES + 80;
const DOMAIN: &[u8] = b"MIDAS/remote-unattended-grant/v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Signature,
    Target,
    Expired,
    Revoked,
    ConsentRequired,
    Desktop,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "remote authorization refused: {self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connection {
    pub tenant: Uuid,
    pub device: Uuid,
    pub id: Uuid,
}
impl Connection {
    pub fn valid(self) -> bool {
        !self.tenant.is_nil() && !self.device.is_nil() && !self.id.is_nil()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub connection: Connection,
    pub session: u32,
    pub generation: Uuid,
}
impl Target {
    pub fn valid(self) -> bool {
        self.connection.valid()
            && self.session != 0
            && self.session != u32::MAX
            && !self.generation.is_nil()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Administrative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rights {
    pub mode: Mode,
    pub uac: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claims {
    pub id: Uuid,
    pub challenge: Uuid,
    pub operator: Uuid,
    pub target: Target,
    pub rights: Rights,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl Claims {
    pub fn lifetime_seconds(&self) -> Result<u64, Error> {
        let lifetime = self
            .expires_at
            .checked_sub(self.issued_at)
            .ok_or(Error::Invalid)?;
        if self.id.is_nil()
            || self.challenge.is_nil()
            || self.operator.is_nil()
            || !self.target.valid()
            || self.issued_at < 0
            || self.expires_at > 253_402_300_799
            || !(1..=MAX_LIFETIME_SECONDS as i64).contains(&lifetime)
        {
            return Err(Error::Invalid);
        }
        Ok(lifetime as u64)
    }
}

#[derive(Clone, PartialEq, Message)]
#[prost(skip_debug)]
pub struct SignedGrant {
    #[prost(bytes = "vec", tag = "1")]
    pub payload: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub signature: Vec<u8>,
}

impl std::fmt::Debug for SignedGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignedGrant").finish_non_exhaustive()
    }
}

impl SignedGrant {
    pub fn decode_checked(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_GRANT_BYTES {
            return Err(Error::Invalid);
        }
        let grant = Self::decode(bytes).map_err(|_| Error::Invalid)?;
        if grant.encode_to_vec() != bytes
            || grant.payload.is_empty()
            || grant.payload.len() > MAX_PAYLOAD_BYTES
            || grant.signature.len() != 64
        {
            return Err(Error::Invalid);
        }
        Ok(grant)
    }
}

#[derive(Clone, PartialEq, Message)]
struct Wire {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(bytes = "vec", tag = "2")]
    id: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    challenge: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    operator: Vec<u8>,
    #[prost(bytes = "vec", tag = "5")]
    tenant: Vec<u8>,
    #[prost(bytes = "vec", tag = "6")]
    device: Vec<u8>,
    #[prost(bytes = "vec", tag = "7")]
    connection: Vec<u8>,
    #[prost(uint32, tag = "8")]
    session: u32,
    #[prost(bytes = "vec", tag = "9")]
    generation: Vec<u8>,
    #[prost(uint32, tag = "10")]
    mode: u32,
    #[prost(bool, tag = "11")]
    uac: bool,
    #[prost(int64, tag = "12")]
    issued_at: i64,
    #[prost(int64, tag = "13")]
    expires_at: i64,
}

impl From<&Claims> for Wire {
    fn from(c: &Claims) -> Self {
        Self {
            version: 1,
            id: c.id.as_bytes().to_vec(),
            challenge: c.challenge.as_bytes().to_vec(),
            operator: c.operator.as_bytes().to_vec(),
            tenant: c.target.connection.tenant.as_bytes().to_vec(),
            device: c.target.connection.device.as_bytes().to_vec(),
            connection: c.target.connection.id.as_bytes().to_vec(),
            session: c.target.session,
            generation: c.target.generation.as_bytes().to_vec(),
            mode: match c.rights.mode {
                Mode::Normal => 1,
                Mode::Administrative => 2,
            },
            uac: c.rights.uac,
            issued_at: c.issued_at,
            expires_at: c.expires_at,
        }
    }
}

fn uuid(bytes: &[u8]) -> Result<Uuid, Error> {
    Uuid::from_slice(bytes).map_err(|_| Error::Invalid)
}
fn message(payload: &[u8]) -> Vec<u8> {
    [DOMAIN, payload].concat()
}

pub struct SigningKey(Ed25519KeyPair);
impl SigningKey {
    pub fn generate() -> Result<Self, Error> {
        let document =
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| Error::Invalid)?;
        Ok(Self(
            Ed25519KeyPair::from_pkcs8(document.as_ref()).map_err(|_| Error::Invalid)?,
        ))
    }
    pub fn public_key(&self) -> [u8; 32] {
        self.0
            .public_key()
            .as_ref()
            .try_into()
            .expect("Ed25519 public key")
    }
    pub fn sign(&self, claims: &Claims) -> Result<SignedGrant, Error> {
        claims.lifetime_seconds()?;
        let payload = Wire::from(claims).encode_to_vec();
        Ok(SignedGrant {
            signature: self.0.sign(&message(&payload)).as_ref().to_vec(),
            payload,
        })
    }
}

pub struct VerifiedGrant(Claims);
impl VerifiedGrant {
    pub fn claims(&self) -> &Claims {
        &self.0
    }
    pub fn into_claims(self) -> Claims {
        self.0
    }
}

pub fn verify(key: &[u8; 32], grant: &SignedGrant) -> Result<VerifiedGrant, Error> {
    if grant.payload.is_empty()
        || grant.payload.len() > MAX_PAYLOAD_BYTES
        || grant.signature.len() != 64
    {
        return Err(Error::Invalid);
    }
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&message(&grant.payload), &grant.signature)
        .map_err(|_| Error::Signature)?;
    let wire = Wire::decode(grant.payload.as_slice()).map_err(|_| Error::Invalid)?;
    if wire.version != 1 || wire.encode_to_vec() != grant.payload {
        return Err(Error::Invalid);
    }
    let claims = Claims {
        id: uuid(&wire.id)?,
        challenge: uuid(&wire.challenge)?,
        operator: uuid(&wire.operator)?,
        target: Target {
            connection: Connection {
                tenant: uuid(&wire.tenant)?,
                device: uuid(&wire.device)?,
                id: uuid(&wire.connection)?,
            },
            session: wire.session,
            generation: uuid(&wire.generation)?,
        },
        rights: Rights {
            mode: match wire.mode {
                1 => Mode::Normal,
                2 => Mode::Administrative,
                _ => return Err(Error::Invalid),
            },
            uac: wire.uac,
        },
        issued_at: wire.issued_at,
        expires_at: wire.expires_at,
    };
    claims.lifetime_seconds()?;
    Ok(VerifiedGrant(claims))
}

#[cfg(test)]
mod tests;
