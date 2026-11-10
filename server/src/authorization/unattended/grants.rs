use midas_remote_auth::{Claims, Connection, SignedGrant, SigningKey};
use uuid::Uuid;

use crate::{
    authorization::{AccessError, Result},
    enrollment::{Admission, AgentEvidence},
};

pub struct GrantAuthority {
    signer: SigningKey,
    source: AgentEvidence,
    connection: Connection,
}
pub struct GrantRequest<'a> {
    pub authority: &'a GrantAuthority,
    pub challenge: Uuid,
}

pub enum GrantDecision {
    LocalConsentRequired,
    Issued(SignedGrant),
}

impl GrantAuthority {
    pub fn for_agent(admission: &Admission) -> Result<Self> {
        let source = admission.evidence();
        if !source.is_live() {
            return Err(AccessError::Offline);
        }
        Ok(Self {
            signer: SigningKey::generate().map_err(|_| AccessError::Unavailable)?,
            source,
            connection: Connection {
                tenant: admission.identity.tenant,
                device: admission.identity.device,
                id: admission.connection_id(),
            },
        })
    }
    pub fn public_key(&self) -> [u8; 32] {
        self.signer.public_key()
    }
    pub(super) fn sign(&self, claims: &Claims, observed: &AgentEvidence) -> Result<SignedGrant> {
        if !self.source.is_live() || !observed.is_live() {
            return Err(AccessError::Offline);
        }
        if claims.target.connection != self.connection
            || observed.identity != self.source.identity
            || observed.credential_fingerprint != self.source.credential_fingerprint
        {
            return Err(AccessError::Denied);
        }
        self.signer
            .sign(claims)
            .map_err(|_| AccessError::InvalidInput)
    }
}
