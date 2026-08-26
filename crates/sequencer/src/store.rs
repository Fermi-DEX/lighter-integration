//! Chain store: the canonical tape, sealed tick history, and the DA layer.
//!
//! The tape (§9) is the deliverable: entries (t, j, h, outcome) with
//! outcome ∈ {opened intent, gap(reason)}, in lexicographic (t, j) order,
//! within-tick order fixed by the receipt chain. Gaps keep their positions
//! forever; later entries never slide forward.
//!
//! DA discipline (§4.5): "no data, no receipt-finality" — envelope bodies
//! are persisted *before* a receipt is returned, and every tick record
//! carries a DA reference. The provider behind `DaStore` is a versioned
//! edge (§15.1); this implementation is a durable local content-addressed
//! directory standing in for an external DA layer.

use crate::envelope::Bucket;
use crate::records::{BatchEntry, Receipt, SegmentSeal, TickRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapReason {
    /// AEAD failed: mauled or garbage ciphertext (paid, §2.5).
    Undecryptable,
    /// Opened, but the inner intent failed signature/namespace/expiry checks.
    InvalidIntent,
    /// No valid opening by the execution cutoff u_mat + G.
    Unopened,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    /// Awaiting maturity/solve; u_mat = admission tick + D_k.
    Pending { mature_at: u64 },
    Opened { intent: crate::envelope::Intent },
    Gap { reason: GapReason },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TapePosition {
    pub tick: u64,
    pub pos: u32,
    pub h: [u8; 32],
    pub bucket: Bucket,
    pub delay_class: u8,
    pub outcome: Outcome,
}

/// Everything sealed for one tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealedTick {
    pub record: TickRecord,
    pub entries: Vec<BatchEntry>,
    pub receipts: Vec<Receipt>,
}

pub struct ChainStore {
    pub ticks: BTreeMap<u64, SealedTick>,
    pub segments: BTreeMap<u64, SegmentSeal>,
    pub tape: BTreeMap<(u64, u32), TapePosition>,
    da: DaStore,
}

impl ChainStore {
    pub fn new(da: DaStore) -> Self {
        Self { ticks: BTreeMap::new(), segments: BTreeMap::new(), tape: BTreeMap::new(), da }
    }

    pub fn da(&self) -> &DaStore {
        &self.da
    }

    pub fn insert_tick(&mut self, sealed: SealedTick) -> std::io::Result<()> {
        self.da.put_tick(&sealed)?;
        self.ticks.insert(sealed.record.tick, sealed);
        Ok(())
    }

    pub fn insert_segment(&mut self, seal: SegmentSeal) -> std::io::Result<()> {
        self.da.put_segment(&seal)?;
        self.segments.insert(seal.segment, seal);
        Ok(())
    }

    pub fn add_position(&mut self, p: TapePosition) {
        self.tape.insert((p.tick, p.pos), p);
    }

    pub fn set_outcome(&mut self, tick: u64, pos: u32, outcome: Outcome) -> Option<&TapePosition> {
        let entry = self.tape.get_mut(&(tick, pos))?;
        // A resolved outcome is terminal: G1/G3 — never overwrite an opened
        // intent or a gap.
        if matches!(entry.outcome, Outcome::Pending { .. }) {
            entry.outcome = outcome;
        }
        Some(entry)
    }

    /// Positions still pending whose cutoff (u_mat + grace) has passed.
    pub fn overdue_positions(&self, current_tick: u64, grace_ticks: u64) -> Vec<(u64, u32)> {
        self.tape
            .iter()
            .filter_map(|(&key, p)| match p.outcome {
                Outcome::Pending { mature_at } if mature_at + grace_ticks <= current_tick => Some(key),
                _ => None,
            })
            .collect()
    }

    pub fn latest_tick(&self) -> Option<u64> {
        self.ticks.keys().next_back().copied()
    }
}

/// Durable content-addressed storage: envelopes by commitment hash, tick
/// records and segment seals by number.
pub struct DaStore {
    root: PathBuf,
}

impl DaStore {
    pub fn open(root: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(root.join("envelopes"))?;
        std::fs::create_dir_all(root.join("ticks"))?;
        std::fs::create_dir_all(root.join("segments"))?;
        Ok(Self { root })
    }

    pub fn temp() -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "posq-da-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Self::open(dir)
    }

    fn write_atomic(&self, path: PathBuf, bytes: &[u8]) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            // No per-file fsync on the hot path; durability semantics belong
            // to the DA provider behind this interface.
        }
        std::fs::rename(tmp, path)
    }

    /// Persist an envelope body before its receipt is issued.
    pub fn put_envelope(&self, h: &[u8; 32], bytes: &[u8]) -> std::io::Result<()> {
        self.write_atomic(self.root.join("envelopes").join(hex::encode(h)), bytes)
    }

    pub fn get_envelope(&self, h: &[u8; 32]) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.root.join("envelopes").join(hex::encode(h)))
    }

    pub fn has_envelope(&self, h: &[u8; 32]) -> bool {
        self.root.join("envelopes").join(hex::encode(h)).exists()
    }

    pub fn put_tick(&self, sealed: &SealedTick) -> std::io::Result<()> {
        let bytes = bincode::serialize(sealed).expect("serialize sealed tick");
        self.write_atomic(
            self.root.join("ticks").join(format!("{:020}.bin", sealed.record.tick)),
            &bytes,
        )
    }

    pub fn get_tick(&self, tick: u64) -> std::io::Result<SealedTick> {
        let bytes = std::fs::read(self.root.join("ticks").join(format!("{tick:020}.bin")))?;
        bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn put_segment(&self, seal: &SegmentSeal) -> std::io::Result<()> {
        let bytes = bincode::serialize(seal).expect("serialize segment seal");
        self.write_atomic(
            self.root.join("segments").join(format!("{:020}.bin", seal.segment)),
            &bytes,
        )
    }

    /// DA attestation over a span: hash of the stored tick payloads
    /// (a real provider returns its own attestation; interface-compatible).
    pub fn attest_span(&self, ticks: impl Iterator<Item = u64>) -> [u8; 32] {
        let mut acc = Vec::new();
        for t in ticks {
            if let Ok(sealed) = self.get_tick(t) {
                acc.extend_from_slice(&sealed.record.batch_root);
            }
        }
        crate::identity::keccak256(&acc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_is_terminal() {
        let mut store = ChainStore::new(DaStore::temp().unwrap());
        store.add_position(TapePosition {
            tick: 1,
            pos: 0,
            h: [1; 32],
            bucket: Bucket::Majors,
            delay_class: 0,
            outcome: Outcome::Pending { mature_at: 10 },
        });
        store.set_outcome(1, 0, Outcome::Gap { reason: GapReason::Unopened });
        // A later "opening" must not revive the gap.
        store.set_outcome(
            1,
            0,
            Outcome::Opened { intent: crate::envelope::Intent {
                namespace: 0, account: [0; 20], nonce: 0, expiry_tick: 0,
                payload: vec![], signature: crate::identity::Sig65([0; 65]),
            }},
        );
        assert!(matches!(
            store.tape[&(1, 0)].outcome,
            Outcome::Gap { reason: GapReason::Unopened }
        ));
    }

    #[test]
    fn overdue_detection() {
        let mut store = ChainStore::new(DaStore::temp().unwrap());
        store.add_position(TapePosition {
            tick: 1, pos: 0, h: [1; 32], bucket: Bucket::Majors, delay_class: 0,
            outcome: Outcome::Pending { mature_at: 10 },
        });
        assert!(store.overdue_positions(11, 5).is_empty());
        assert_eq!(store.overdue_positions(15, 5), vec![(1, 0)]);
    }

    #[test]
    fn envelope_da_roundtrip() {
        let da = DaStore::temp().unwrap();
        let h = [7u8; 32];
        da.put_envelope(&h, b"envelope bytes").unwrap();
        assert!(da.has_envelope(&h));
        assert_eq!(da.get_envelope(&h).unwrap(), b"envelope bytes");
    }
}
