//! Fraud proofs (§10.2): compact, checkable from signatures, Merkle paths,
//! hash chains, and modexp alone. These Rust verifiers mirror the PoSqHost
//! contract logic one-to-one (same encodings, same keccak) so watchers can
//! pre-check evidence before spending gas.

use crate::records::{
    log_step, merkle_verify, BatchEntry, Receipt, TickRecord,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FraudKind {
    Equivocation,
    Reorder,
    Omission,
    InvalidLogTransition,
}

/// Proof 1 — Equivocation: two signed tick records for the same
/// (epoch, t) with different content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquivocationProof {
    pub a: TickRecord,
    pub b: TickRecord,
}

impl EquivocationProof {
    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        self.a.epoch == self.b.epoch
            && self.a.tick == self.b.tick
            && self.a.content_hash() != self.b.content_hash()
            && self.a.verify(sequencer)
            && self.b.verify(sequencer)
    }
}

/// Proof 2 — Reorder: a receipt for h at (t, j) together with the signed
/// tick record whose batch places a *different* entry at (t, j).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderProof {
    pub receipt: Receipt,
    pub record: TickRecord,
    /// The entry actually at position j in the published batch.
    pub conflicting_entry: BatchEntry,
    pub merkle_path: Vec<[u8; 32]>,
}

impl ReorderProof {
    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        // Both objects signed by the sequencer, same (epoch, tick).
        if self.receipt.epoch != self.record.epoch || self.receipt.tick != self.record.tick {
            return false;
        }
        if !self.receipt.verify(sequencer) || !self.record.verify(sequencer) {
            return false;
        }
        // The conflicting entry sits at the receipt's position in the
        // signed batch root, but carries a different commitment.
        if self.conflicting_entry.tick != self.receipt.tick
            || self.conflicting_entry.pos != self.receipt.pos
            || self.conflicting_entry.h == self.receipt.h
        {
            return false;
        }
        merkle_verify(
            &self.record.batch_root,
            &self.conflicting_entry.leaf(),
            self.conflicting_entry.pos as usize,
            &self.merkle_path,
        )
    }
}

/// Proof 3 — Omission: a valid receipt for (t, j, h) plus the *full*
/// published batch for tick t (data-available by construction) that does
/// not contain h at j. Full-batch revelation keeps the proof objective
/// without a non-membership accumulator; batches are per-tick and small.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmissionProof {
    pub receipt: Receipt,
    pub record: TickRecord,
    pub full_batch: Vec<BatchEntry>,
}

impl OmissionProof {
    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        if self.receipt.epoch != self.record.epoch || self.receipt.tick != self.record.tick {
            return false;
        }
        if !self.receipt.verify(sequencer) || !self.record.verify(sequencer) {
            return false;
        }
        // The batch must be the signed one...
        let leaves: Vec<[u8; 32]> = self.full_batch.iter().map(|e| e.leaf()).collect();
        if crate::records::merkle_root(&leaves) != self.record.batch_root {
            return false;
        }
        // ...and must omit or displace the receipted commitment.
        !self
            .full_batch
            .iter()
            .any(|e| e.pos == self.receipt.pos && e.h == self.receipt.h)
    }
}

/// Proof 5 — Invalid log transition: consecutive signed records where
/// C_t ≠ H(C_{t-1}, t, root(B_t), x_t).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidLogProof {
    pub prev: TickRecord,
    pub curr: TickRecord,
}

impl InvalidLogProof {
    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        if self.prev.epoch != self.curr.epoch || self.curr.tick != self.prev.tick + 1 {
            return false;
        }
        if !self.prev.verify(sequencer) || !self.curr.verify(sequencer) {
            return false;
        }
        let expected =
            log_step(&self.curr.c_prev, self.curr.tick, &self.curr.batch_root, &self.curr.x_hash());
        // Fraud if the chain link is broken in either direction.
        self.curr.c_prev != self.prev.c_t || self.curr.c_t != expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{Bucket, Window};
    use crate::identity::Identity;
    use crate::records::*;
    use num_bigint::BigUint;

    fn seq() -> Identity {
        Identity::from_seed(b"sequencer")
    }

    fn record(id: &Identity, tick: u64, batch_root: [u8; 32], c_prev: [u8; 32]) -> TickRecord {
        let x = BigUint::from(1000u32 + tick as u32);
        let x_hash = hash_group_element(&x);
        let c_t = log_step(&c_prev, tick, &batch_root, &x_hash);
        let msg = TickRecord::signing_message(0, 0, tick, &x_hash, &c_prev, &c_t, &batch_root, &batch_root);
        TickRecord {
            epoch: 0, segment: 0, tick, x, c_prev, c_t,
            batch_root, da_ref: batch_root, sig: id.sign(&msg),
        }
    }

    fn entry(tick: u64, pos: u32, h: [u8; 32]) -> BatchEntry {
        BatchEntry {
            tick, pos, h, bucket: Bucket::Majors,
            ticket_id: [1; 16], receipt_sig: crate::identity::Sig65([0; 65]),
        }
    }

    fn receipt_at(id: &Identity, tick: u64, pos: u32, h: [u8; 32]) -> Receipt {
        let d_prev = digest_genesis(0, tick);
        let d = digest_step(&d_prev, tick, pos, &h, Bucket::Majors, &[1; 16]);
        let w = Window { start: tick, len: 2 };
        let msg = Receipt::signing_message(0, tick, pos, &h, Bucket::Majors, w, &[1; 16], &[0; 32], &log_genesis(0), &d_prev, &d);
        Receipt {
            epoch: 0, tick, pos, h, bucket: Bucket::Majors, window: w,
            ticket_id: [1; 16], x_prev_hash: [0; 32], c_prev: log_genesis(0),
            d_prev, d, sig: id.sign(&msg),
        }
    }

    #[test]
    fn equivocation_detected() {
        let id = seq();
        let a = record(&id, 5, [1; 32], log_genesis(0));
        let b = record(&id, 5, [2; 32], log_genesis(0));
        assert!(EquivocationProof { a: a.clone(), b }.verify(&id.address()));
        // Same record twice is not equivocation.
        assert!(!EquivocationProof { a: a.clone(), b: a }.verify(&id.address()));
    }

    #[test]
    fn reorder_detected() {
        let id = seq();
        let h_receipted = crate::identity::keccak256(b"mine");
        let h_other = crate::identity::keccak256(b"displaced");
        let receipt = receipt_at(&id, 3, 0, h_receipted);
        // Published batch has a different entry at position 0.
        let entries = vec![entry(3, 0, h_other), entry(3, 1, h_receipted)];
        let leaves: Vec<[u8; 32]> = entries.iter().map(|e| e.leaf()).collect();
        let root = merkle_root(&leaves);
        let rec = record(&id, 3, root, log_genesis(0));
        let proof = ReorderProof {
            receipt,
            record: rec,
            conflicting_entry: entries[0].clone(),
            merkle_path: merkle_path(&leaves, 0).unwrap(),
        };
        assert!(proof.verify(&id.address()));
    }

    #[test]
    fn omission_detected_and_honest_batch_passes() {
        let id = seq();
        let h = crate::identity::keccak256(b"omitted");
        let receipt = receipt_at(&id, 4, 0, h);
        // Batch without the receipted entry.
        let entries = vec![entry(4, 0, crate::identity::keccak256(b"other"))];
        let leaves: Vec<[u8; 32]> = entries.iter().map(|e| e.leaf()).collect();
        let rec = record(&id, 4, merkle_root(&leaves), log_genesis(0));
        assert!(OmissionProof { receipt: receipt.clone(), record: rec, full_batch: entries }.verify(&id.address()));

        // Honest batch (contains the entry): no fraud.
        let good = vec![entry(4, 0, h)];
        let good_leaves: Vec<[u8; 32]> = good.iter().map(|e| e.leaf()).collect();
        let good_rec = record(&id, 4, merkle_root(&good_leaves), log_genesis(0));
        assert!(!OmissionProof { receipt, record: good_rec, full_batch: good }.verify(&id.address()));
    }

    #[test]
    fn invalid_log_transition_detected() {
        let id = seq();
        let r1 = record(&id, 1, [1; 32], log_genesis(0));
        // Forge tick 2 chaining from a wrong C (not r1.c_t).
        let r2_bad = record(&id, 2, [2; 32], crate::identity::keccak256(b"wrong prev"));
        assert!(InvalidLogProof { prev: r1.clone(), curr: r2_bad }.verify(&id.address()));

        // Honest continuation: no fraud.
        let r2_good = record(&id, 2, [2; 32], r1.c_t);
        assert!(!InvalidLogProof { prev: r1, curr: r2_good }.verify(&id.address()));
    }
}
