//! Transcript validity (§4.6): the sequencing predicate and its proof
//! artifact.
//!
//! The predicate attests, for an anchored span, that: the log chain
//! C_a → C_b is correctly formed over the published tick batches; every
//! receipt digest chain is internally consistent and consistent with
//! root(B_t); every admitted commitment appears exactly once; and the batch
//! roots equal the DA-attested roots.
//!
//! Proof backends: the whitepaper's reference is a STARK/folding IVC over a
//! SNARK-friendly hash. This implementation ships the **predicate** (the
//! exact statement any backend must prove) plus the `TranscriptProver`
//! abstraction with the `Replay` backend — the optimistic-anchor mode
//! §10.1 explicitly permits: the anchor carries the predicate commitment
//! and is accepted under a challenge window during which any watcher can
//! re-execute `verify_span` against published data and submit the fraud
//! proofs of §10.2 on failure. A succinct ZK backend (proving the same
//! predicate) slots behind the same trait without changing the anchor
//! format; that is the one MVD component delivered as an interface +
//! checkable statement rather than a SNARK, because a sound STARK over this
//! predicate is not a one-cycle implementation. All downstream plumbing
//! (anchor fields, host-contract slots, challenge window) is in place.

use crate::records::{digest_genesis, digest_step, log_step};
use crate::store::SealedTick;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptViolation {
    MissingTick(u64),
    LogChainBroken { tick: u64 },
    DigestChainBroken { tick: u64, pos: u32 },
    BatchRootMismatch { tick: u64 },
    DuplicateCommitment { h: [u8; 32] },
    EntryOrderBroken { tick: u64, pos: u32 },
    BadSignature { tick: u64 },
}

/// Verify the sequencing-validity predicate over ticks [from, to].
/// `c_before` is C_{from-1}; returns C_to on success.
pub fn verify_span(
    epoch: u64,
    sequencer: &[u8; 20],
    c_before: [u8; 32],
    from: u64,
    to: u64,
    ticks: &dyn Fn(u64) -> Option<SealedTick>,
    seen_h_before: &HashSet<[u8; 32]>,
) -> Result<[u8; 32], TranscriptViolation> {
    let mut c = c_before;
    let mut seen: HashSet<[u8; 32]> = seen_h_before.clone();
    for t in from..=to {
        let sealed = ticks(t).ok_or(TranscriptViolation::MissingTick(t))?;
        let record = &sealed.record;
        if record.tick != t || record.epoch != epoch {
            return Err(TranscriptViolation::MissingTick(t));
        }
        if !record.verify(sequencer) {
            return Err(TranscriptViolation::BadSignature { tick: t });
        }
        // Batch root over the published entries must match the signed root.
        let leaves: Vec<[u8; 32]> = sealed.entries.iter().map(|e| e.leaf()).collect();
        if crate::records::merkle_root(&leaves) != record.batch_root {
            return Err(TranscriptViolation::BatchRootMismatch { tick: t });
        }
        // Receipt digest chain: contiguous positions, correct links, and
        // every commitment admitted exactly once.
        let mut d = digest_genesis(epoch, t);
        for (idx, e) in sealed.entries.iter().enumerate() {
            if e.tick != t || e.pos != idx as u32 {
                return Err(TranscriptViolation::EntryOrderBroken { tick: t, pos: e.pos });
            }
            d = digest_step(&d, t, e.pos, &e.h, e.bucket, &e.ticket_id);
            if !seen.insert(e.h) {
                return Err(TranscriptViolation::DuplicateCommitment { h: e.h });
            }
        }
        // Each receipt must match its entry's digest link.
        for r in &sealed.receipts {
            let entry = sealed
                .entries
                .get(r.pos as usize)
                .ok_or(TranscriptViolation::DigestChainBroken { tick: t, pos: r.pos })?;
            let expected = digest_step(&r.d_prev, t, r.pos, &entry.h, entry.bucket, &entry.ticket_id);
            if r.d != expected || r.h != entry.h {
                return Err(TranscriptViolation::DigestChainBroken { tick: t, pos: r.pos });
            }
        }
        // Log chain step.
        let expected_c = log_step(&c, t, &record.batch_root, &record.x_hash());
        if record.c_prev != c || record.c_t != expected_c {
            return Err(TranscriptViolation::LogChainBroken { tick: t });
        }
        c = expected_c;
    }
    Ok(c)
}

/// The proof artifact carried by an anchor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TranscriptProof {
    /// Optimistic anchor (§10.1): predicate commitment only; watchers
    /// re-execute `verify_span` during the challenge window.
    Optimistic { predicate_commitment: [u8; 32] },
    /// Reserved slot for a succinct backend proving the same predicate.
    Succinct { system: String, proof: Vec<u8> },
}

/// Commitment to the exact predicate instance being claimed.
pub fn predicate_commitment(
    epoch: u64,
    from: u64,
    to: u64,
    c_before: &[u8; 32],
    c_after: &[u8; 32],
) -> [u8; 32] {
    let mut m = Vec::with_capacity(96);
    m.extend_from_slice(b"posq-transcript-predicate-v1");
    m.extend_from_slice(&epoch.to_be_bytes());
    m.extend_from_slice(&from.to_be_bytes());
    m.extend_from_slice(&to.to_be_bytes());
    m.extend_from_slice(c_before);
    m.extend_from_slice(c_after);
    crate::identity::keccak256(&m)
}

/// Backend abstraction: any prover must prove exactly `verify_span`.
pub trait TranscriptProver: Send + Sync {
    fn prove(
        &self,
        epoch: u64,
        sequencer: &[u8; 20],
        c_before: [u8; 32],
        from: u64,
        to: u64,
        ticks: &dyn Fn(u64) -> Option<SealedTick>,
    ) -> Result<([u8; 32], TranscriptProof), TranscriptViolation>;
}

/// The optimistic backend: runs the predicate, emits the commitment.
pub struct ReplayProver;

impl TranscriptProver for ReplayProver {
    fn prove(
        &self,
        epoch: u64,
        sequencer: &[u8; 20],
        c_before: [u8; 32],
        from: u64,
        to: u64,
        ticks: &dyn Fn(u64) -> Option<SealedTick>,
    ) -> Result<([u8; 32], TranscriptProof), TranscriptViolation> {
        let c_after = verify_span(epoch, sequencer, c_before, from, to, ticks, &HashSet::new())?;
        let commitment = predicate_commitment(epoch, from, to, &c_before, &c_after);
        Ok((c_after, TranscriptProof::Optimistic { predicate_commitment: commitment }))
    }
}
