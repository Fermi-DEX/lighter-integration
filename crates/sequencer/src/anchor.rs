//! Anchors (§10.1): the periodic host-chain checkpoint.
//!
//! At anchor cadence the sequencer posts: the checkpoint (x_b, C_b) closing
//! the span, the batched segment proofs (verified on Ethereum through the
//! modexp precompile), the transcript proof (or an optimistic commitment
//! under a challenge window), the receipt-tree root, and the DA
//! attestation. Anchors are kilobytes; bulk data lives with the DA layer.

use crate::identity::{keccak256, Identity, Sig65};
use crate::records::{hash_group_element, SegmentSeal};
use crate::transcript::TranscriptProof;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub epoch: u64,
    /// Segment span [first, last] and the tick range it covers.
    pub first_segment: u64,
    pub last_segment: u64,
    pub first_tick: u64,
    pub last_tick: u64,
    /// Checkpoint closing the span.
    pub x_b_hash: [u8; 32],
    pub c_b: [u8; 32],
    /// One Wesolowski proof per segment in the span.
    pub segment_seals: Vec<SegmentSeal>,
    /// Merkle root over the span's per-tick final receipt digests.
    pub receipts_root: [u8; 32],
    pub da_attestation: [u8; 32],
    pub transcript: TranscriptProof,
    pub sig: Sig65,
}

impl Anchor {
    pub fn signing_message(&self) -> Vec<u8> {
        let mut m = Vec::with_capacity(256);
        m.extend_from_slice(b"\x06posq-anchor-v1");
        m.extend_from_slice(&self.epoch.to_be_bytes());
        m.extend_from_slice(&self.first_segment.to_be_bytes());
        m.extend_from_slice(&self.last_segment.to_be_bytes());
        m.extend_from_slice(&self.first_tick.to_be_bytes());
        m.extend_from_slice(&self.last_tick.to_be_bytes());
        m.extend_from_slice(&self.x_b_hash);
        m.extend_from_slice(&self.c_b);
        for seal in &self.segment_seals {
            m.extend_from_slice(&hash_group_element(&seal.x_end));
        }
        m.extend_from_slice(&self.receipts_root);
        m.extend_from_slice(&self.da_attestation);
        m.extend_from_slice(&self.transcript_commitment());
        m
    }

    pub fn transcript_commitment(&self) -> [u8; 32] {
        match &self.transcript {
            TranscriptProof::Optimistic { predicate_commitment } => *predicate_commitment,
            TranscriptProof::Succinct { proof, .. } => keccak256(proof),
        }
    }

    pub fn sign(mut self, identity: &Identity) -> Self {
        self.sig = identity.sign(&self.signing_message());
        self
    }

    pub fn verify_sig(&self, sequencer: &[u8; 20]) -> bool {
        crate::identity::verify_signed(&self.signing_message(), &self.sig, sequencer)
    }

    /// ABI-style calldata for `PoSqHost.submitAnchor`: the fixed head fields
    /// packed, with segment proof elements appended as 256-byte words for
    /// modexp verification on-chain.
    pub fn calldata(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.epoch.to_be_bytes());
        out.extend_from_slice(&self.first_segment.to_be_bytes());
        out.extend_from_slice(&self.last_segment.to_be_bytes());
        out.extend_from_slice(&self.first_tick.to_be_bytes());
        out.extend_from_slice(&self.last_tick.to_be_bytes());
        out.extend_from_slice(&self.x_b_hash);
        out.extend_from_slice(&self.c_b);
        out.extend_from_slice(&self.receipts_root);
        out.extend_from_slice(&self.da_attestation);
        out.extend_from_slice(&self.transcript_commitment());
        out.extend_from_slice(self.sig.as_bytes());
        out.extend_from_slice(&(self.segment_seals.len() as u32).to_be_bytes());
        for seal in &self.segment_seals {
            out.extend_from_slice(&seal.segment.to_be_bytes());
            out.extend_from_slice(&pad256(&seal.y));
            out.extend_from_slice(&pad256(&seal.x_end));
            out.extend_from_slice(&pad256(&seal.proof.pi));
            out.extend_from_slice(&seal.proof.iterations.to_be_bytes());
        }
        out
    }
}

fn pad256(x: &num_bigint::BigUint) -> [u8; 256] {
    let bytes = x.to_bytes_be();
    let mut out = [0u8; 256];
    out[256 - bytes.len()..].copy_from_slice(&bytes);
    out
}

/// Host-chain submission edge. The real deployment posts `calldata()` to
/// the PoSqHost contract via JSON-RPC; tests and local runs use the mock.
pub trait HostClient: Send + Sync {
    fn post_anchor(&self, anchor: &Anchor) -> anyhow::Result<()>;
}

#[derive(Default)]
pub struct MockHost {
    pub posted: std::sync::Mutex<Vec<Anchor>>,
}

impl HostClient for MockHost {
    fn post_anchor(&self, anchor: &Anchor) -> anyhow::Result<()> {
        self.posted.lock().unwrap().push(anchor.clone());
        Ok(())
    }
}
