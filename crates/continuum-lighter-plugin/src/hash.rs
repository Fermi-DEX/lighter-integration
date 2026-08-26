use sha2::{Digest, Sha256};

use crate::types::Hash32;

/// Adapter implemented by the exact hash configuration pinned to Lighter's
/// proving system. Domain and payload encoding are part of the protocol.
pub trait LighterNativeHash {
    fn hash(&self, domain: &'static [u8], payload: &[u8]) -> Hash32;

    /// Hash native Goldilocks field elements. Production adapters override
    /// this to avoid byte decomposition in the Lighter circuit. The default
    /// encoding keeps source-independent tests deterministic.
    fn hash_fields(&self, domain: &'static [u8], fields: &[u64]) -> Hash32 {
        let mut payload = Vec::with_capacity(8 + fields.len() * 8);
        payload.extend_from_slice(&(fields.len() as u64).to_be_bytes());
        for field in fields {
            payload.extend_from_slice(&field.to_be_bytes());
        }
        self.hash(domain, &payload)
    }
}

/// Deterministic host-test hash. This is not the production H_L.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sha256ReferenceHasher;

impl LighterNativeHash for Sha256ReferenceHasher {
    fn hash(&self, domain: &'static [u8], payload: &[u8]) -> Hash32 {
        let mut hasher = Sha256::new();
        hasher.update((domain.len() as u32).to_be_bytes());
        hasher.update(domain);
        hasher.update((payload.len() as u64).to_be_bytes());
        hasher.update(payload);
        hasher.finalize().into()
    }
}
