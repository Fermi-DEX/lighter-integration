use serde::{Deserialize, Serialize};

use crate::canonical::{put_fixed, put_lighter_hash, put_u16, put_u32, put_u64, put_u8};

pub type Hash32 = [u8; 32];
pub type LighterHash = [u64; 4];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ResolutionV3 {
    Clear = 0,
    BadAead = 1,
    BadEncoding = 2,
    L1Cancelled = 3,
}

/// One logical Continuum position projected into the protected Lighter stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DerivedItemV3 {
    pub domain_hash: Hash32,
    pub frame_id: u64,
    pub chunk_id: u32,
    pub tick: u64,
    pub position: u32,
    pub envelope_hash: Hash32,
    pub receipt_digest: Hash32,
    pub resolution: ResolutionV3,
    pub cleartext_length: u32,
    pub cleartext_hash: LighterHash,
    pub terminal_reason: u16,
}

impl DerivedItemV3 {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(159);
        put_fixed(&mut out, &self.domain_hash);
        put_u64(&mut out, self.frame_id);
        put_u32(&mut out, self.chunk_id);
        put_u64(&mut out, self.tick);
        put_u32(&mut out, self.position);
        put_fixed(&mut out, &self.envelope_hash);
        put_fixed(&mut out, &self.receipt_digest);
        put_u8(&mut out, self.resolution as u8);
        put_u32(&mut out, self.cleartext_length);
        put_lighter_hash(&mut out, &self.cleartext_hash);
        put_u16(&mut out, self.terminal_reason);
        out
    }
}

/// The compact leaf that Lighter can derive from values already present in
/// its transaction circuit. The sequence proof proves its one-to-one mapping
/// from the richer DerivedItemV3.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionItemV3 {
    pub logical_index: u64,
    pub tx_type: u16,
    pub tx_hash: [u64; 5],
    pub outcome_class: u16,
    pub terminal_noop: bool,
}

impl ExecutionItemV3 {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(53);
        put_u64(&mut out, self.logical_index);
        put_u16(&mut out, self.tx_type);
        for limb in self.tx_hash {
            put_u64(&mut out, limb);
        }
        put_u16(&mut out, self.outcome_class);
        put_u8(&mut out, self.terminal_noop as u8);
        out
    }
}
