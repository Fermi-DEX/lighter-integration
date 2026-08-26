//! Signed protocol objects and hash chains (§4.5, §5.1, §8, Appendix A).
//!
//! All hashing on the accountability path is keccak256 so every object is
//! recomputable by the Ethereum host contract. (The whitepaper reference
//! profile uses Poseidon2 for the log and receipt-digest chains to make ZK
//! transcript proofs cheap; this MVD deliberately uses keccak256 for native
//! EVM fraud-proof verification and notes Poseidon2 as the ZK-backend
//! migration. Changing the hash re-geneses the chain — the epoch field
//! exists for exactly that.)
//!
//! Causality note on receipt fields: eq. (2) lists x_t and C_t in the
//! receipt, but a receipt is issued *during* tick t, before the tick's
//! squarings and batch root exist. We therefore bind the receipt to the
//! latest sealed state — x_{t-1} (hashed) and C_{t-1} — which is the only
//! causally available choice; the tick record then binds x_t and C_t over
//! the batch containing the receipt.

use crate::envelope::{Bucket, Window};
use crate::identity::{keccak256, recover_address, Identity, Sig65};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};

/// Pad a group element to 256 bytes big-endian and hash it.
pub fn hash_group_element(x: &BigUint) -> [u8; 32] {
    let bytes = x.to_bytes_be();
    let mut padded = vec![0u8; 256usize.saturating_sub(bytes.len())];
    padded.extend_from_slice(&bytes);
    keccak256(&padded)
}

// ---------------------------------------------------------------------------
// Chains
// ---------------------------------------------------------------------------

/// Log-commitment genesis for an epoch.
pub fn log_genesis(epoch: u64) -> [u8; 32] {
    keccak256(&[b"posq-log-genesis-v1".as_slice(), &epoch.to_be_bytes()].concat())
}

/// C_t = H(C_{t-1}, t, root(B_t), x_t)   (§4.4)
pub fn log_step(c_prev: &[u8; 32], tick: u64, batch_root: &[u8; 32], x_hash: &[u8; 32]) -> [u8; 32] {
    let mut m = Vec::with_capacity(128);
    m.extend_from_slice(b"posq-log-v1");
    m.extend_from_slice(c_prev);
    m.extend_from_slice(&tick.to_be_bytes());
    m.extend_from_slice(batch_root);
    m.extend_from_slice(x_hash);
    keccak256(&m)
}

/// Receipt-digest chain genesis for tick t.
pub fn digest_genesis(epoch: u64, tick: u64) -> [u8; 32] {
    keccak256(&[b"posq-digest-genesis-v1".as_slice(), &epoch.to_be_bytes(), &tick.to_be_bytes()].concat())
}

/// d_{t,j} = H(d_{t,j-1}, t, j, h, bucket, τ)   (eq. 3)
pub fn digest_step(
    d_prev: &[u8; 32],
    tick: u64,
    pos: u32,
    h: &[u8; 32],
    bucket: Bucket,
    ticket_id: &[u8; 16],
) -> [u8; 32] {
    let mut m = Vec::with_capacity(112);
    m.extend_from_slice(b"posq-digest-v1");
    m.extend_from_slice(d_prev);
    m.extend_from_slice(&tick.to_be_bytes());
    m.extend_from_slice(&pos.to_be_bytes());
    m.extend_from_slice(h);
    m.push(bucket as u8);
    m.extend_from_slice(ticket_id);
    keccak256(&m)
}

// ---------------------------------------------------------------------------
// Batch entries and the keccak merkle tree
// ---------------------------------------------------------------------------

/// Batch entry e_{t,j} = (h, bucket, τ, ρ_{t,j})   (§8.1)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchEntry {
    pub tick: u64,
    pub pos: u32,
    pub h: [u8; 32],
    pub bucket: Bucket,
    pub ticket_id: [u8; 16],
    pub receipt_sig: Sig65,
}

impl BatchEntry {
    pub fn leaf(&self) -> [u8; 32] {
        let mut m = Vec::with_capacity(128);
        m.extend_from_slice(b"posq-leaf-v1");
        m.extend_from_slice(&self.tick.to_be_bytes());
        m.extend_from_slice(&self.pos.to_be_bytes());
        m.extend_from_slice(&self.h);
        m.push(self.bucket as u8);
        m.extend_from_slice(&self.ticket_id);
        m.extend_from_slice(&keccak256(self.receipt_sig.as_bytes()));
        keccak256(&m)
    }
}

pub fn empty_batch_root() -> [u8; 32] {
    keccak256(b"posq-empty-batch-v1")
}

fn merkle_parent(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut m = Vec::with_capacity(65);
    m.push(0x01);
    m.extend_from_slice(left);
    m.extend_from_slice(right);
    keccak256(&m)
}

/// Keccak merkle root with duplicate-last for odd levels.
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return empty_batch_root();
    }
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let right = if pair.len() == 2 { &pair[1] } else { &pair[0] };
            next.push(merkle_parent(&pair[0], right));
        }
        level = next;
    }
    level[0]
}

/// Membership path for leaf `index`; verified by the host contract for
/// reorder/omission fraud proofs.
pub fn merkle_path(leaves: &[[u8; 32]], index: usize) -> Option<Vec<[u8; 32]>> {
    if index >= leaves.len() {
        return None;
    }
    let mut path = Vec::new();
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    let mut idx = index;
    while level.len() > 1 {
        let sibling = if idx % 2 == 0 {
            *level.get(idx + 1).unwrap_or(&level[idx])
        } else {
            level[idx - 1]
        };
        path.push(sibling);
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let right = if pair.len() == 2 { &pair[1] } else { &pair[0] };
            next.push(merkle_parent(&pair[0], right));
        }
        level = next;
        idx /= 2;
    }
    Some(path)
}

pub fn merkle_verify(root: &[u8; 32], leaf: &[u8; 32], index: usize, path: &[[u8; 32]]) -> bool {
    let mut acc = *leaf;
    let mut idx = index;
    for sibling in path {
        acc = if idx % 2 == 0 {
            merkle_parent(&acc, sibling)
        } else {
            merkle_parent(sibling, &acc)
        };
        idx /= 2;
    }
    acc == *root
}

// ---------------------------------------------------------------------------
// Signed objects
// ---------------------------------------------------------------------------

/// R_t = (epoch, s, t, x_t, C_{t-1}, C_t, root(B_t), DA_t, sig_t)   (§5.1)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickRecord {
    pub epoch: u64,
    pub segment: u64,
    pub tick: u64,
    /// Full end-of-tick state (watchers verify squaring against it).
    pub x: BigUint,
    pub c_prev: [u8; 32],
    pub c_t: [u8; 32],
    pub batch_root: [u8; 32],
    pub da_ref: [u8; 32],
    pub sig: Sig65,
}

impl TickRecord {
    pub fn signing_message(
        epoch: u64,
        segment: u64,
        tick: u64,
        x_hash: &[u8; 32],
        c_prev: &[u8; 32],
        c_t: &[u8; 32],
        batch_root: &[u8; 32],
        da_ref: &[u8; 32],
    ) -> Vec<u8> {
        let mut m = Vec::with_capacity(200);
        m.extend_from_slice(b"\x01posq-tick-record-v1");
        m.extend_from_slice(&epoch.to_be_bytes());
        m.extend_from_slice(&segment.to_be_bytes());
        m.extend_from_slice(&tick.to_be_bytes());
        m.extend_from_slice(x_hash);
        m.extend_from_slice(c_prev);
        m.extend_from_slice(c_t);
        m.extend_from_slice(batch_root);
        m.extend_from_slice(da_ref);
        m
    }

    pub fn x_hash(&self) -> [u8; 32] {
        hash_group_element(&self.x)
    }

    pub fn message(&self) -> Vec<u8> {
        Self::signing_message(
            self.epoch,
            self.segment,
            self.tick,
            &self.x_hash(),
            &self.c_prev,
            &self.c_t,
            &self.batch_root,
            &self.da_ref,
        )
    }

    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        crate::identity::verify_signed(&self.message(), &self.sig, sequencer)
    }

    /// Content hash used by equivocation checks (two records, same
    /// (epoch, tick), different content hash → fraud).
    pub fn content_hash(&self) -> [u8; 32] {
        keccak256(&self.message())
    }
}

/// ρ_{t,j}: the admission receipt (eq. 2, with the causality note above).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    pub epoch: u64,
    pub tick: u64,
    pub pos: u32,
    pub h: [u8; 32],
    pub bucket: Bucket,
    pub window: Window,
    pub ticket_id: [u8; 16],
    pub x_prev_hash: [u8; 32],
    pub c_prev: [u8; 32],
    pub d_prev: [u8; 32],
    pub d: [u8; 32],
    pub sig: Sig65,
}

impl Receipt {
    #[allow(clippy::too_many_arguments)]
    pub fn signing_message(
        epoch: u64,
        tick: u64,
        pos: u32,
        h: &[u8; 32],
        bucket: Bucket,
        window: Window,
        ticket_id: &[u8; 16],
        x_prev_hash: &[u8; 32],
        c_prev: &[u8; 32],
        d_prev: &[u8; 32],
        d: &[u8; 32],
    ) -> Vec<u8> {
        let mut m = Vec::with_capacity(256);
        m.extend_from_slice(b"\x02posq-receipt-v1");
        m.extend_from_slice(&epoch.to_be_bytes());
        m.extend_from_slice(&tick.to_be_bytes());
        m.extend_from_slice(&pos.to_be_bytes());
        m.extend_from_slice(h);
        m.push(bucket as u8);
        m.extend_from_slice(&window.start.to_be_bytes());
        m.extend_from_slice(&window.len.to_be_bytes());
        m.extend_from_slice(ticket_id);
        m.extend_from_slice(x_prev_hash);
        m.extend_from_slice(c_prev);
        m.extend_from_slice(d_prev);
        m.extend_from_slice(d);
        m
    }

    pub fn message(&self) -> Vec<u8> {
        Self::signing_message(
            self.epoch,
            self.tick,
            self.pos,
            &self.h,
            self.bucket,
            self.window,
            &self.ticket_id,
            &self.x_prev_hash,
            &self.c_prev,
            &self.d_prev,
            &self.d,
        )
    }

    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        // Structural check: the digest link must be internally consistent.
        let expected_d = digest_step(&self.d_prev, self.tick, self.pos, &self.h, self.bucket, &self.ticket_id);
        if expected_d != self.d {
            return false;
        }
        crate::identity::verify_signed(&self.message(), &self.sig, sequencer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum RejectReason {
    Malformed = 1,
    WrongSize = 2,
    BadTicket = 3,
    TicketConsumed = 4,
    Duplicate = 5,
    WindowNotOpen = 6,
    WindowExpired = 7,
    BadDelayClass = 8,
    Fenced = 9,
}

/// Signed rejection: ρ^rej = Sig(epoch, h, bucket, window, reason, C).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignedRejection {
    pub epoch: u64,
    pub h: [u8; 32],
    pub bucket: Bucket,
    pub window: Window,
    pub reason: RejectReason,
    pub c_latest: [u8; 32],
    pub sig: Sig65,
}

impl SignedRejection {
    pub fn message_of(
        epoch: u64,
        h: &[u8; 32],
        bucket: Bucket,
        window: Window,
        reason: RejectReason,
        c_latest: &[u8; 32],
    ) -> Vec<u8> {
        let mut m = Vec::with_capacity(128);
        m.extend_from_slice(b"\x03posq-rejection-v1");
        m.extend_from_slice(&epoch.to_be_bytes());
        m.extend_from_slice(h);
        m.push(bucket as u8);
        m.extend_from_slice(&window.start.to_be_bytes());
        m.extend_from_slice(&window.len.to_be_bytes());
        m.push(reason as u8);
        m.extend_from_slice(c_latest);
        m
    }

    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        crate::identity::verify_signed(
            &Self::message_of(self.epoch, &self.h, self.bucket, self.window, self.reason, &self.c_latest),
            &self.sig,
            sequencer,
        )
    }
}

/// Signed full-window response: ρ^full = Sig(epoch, bucket, window, capacity, C).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FullWindowResponse {
    pub epoch: u64,
    pub bucket: Bucket,
    pub window: Window,
    pub capacity: u32,
    pub c_latest: [u8; 32],
    pub sig: Sig65,
}

impl FullWindowResponse {
    pub fn message_of(
        epoch: u64,
        bucket: Bucket,
        window: Window,
        capacity: u32,
        c_latest: &[u8; 32],
    ) -> Vec<u8> {
        let mut m = Vec::with_capacity(96);
        m.extend_from_slice(b"\x04posq-full-window-v1");
        m.extend_from_slice(&epoch.to_be_bytes());
        m.push(bucket as u8);
        m.extend_from_slice(&window.start.to_be_bytes());
        m.extend_from_slice(&window.len.to_be_bytes());
        m.extend_from_slice(&capacity.to_be_bytes());
        m.extend_from_slice(c_latest);
        m
    }

    pub fn verify(&self, sequencer: &[u8; 20]) -> bool {
        crate::identity::verify_signed(
            &Self::message_of(self.epoch, self.bucket, self.window, self.capacity, &self.c_latest),
            &self.sig,
            sequencer,
        )
    }
}

/// Segment close: (epoch, s, y_s, x_end(s), π_s)   (Appendix A).
/// Carries the seed entropy r_s so watchers can recompute y_s from the
/// prior segment's published (x_end, C_end).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentSeal {
    pub epoch: u64,
    pub segment: u64,
    pub entropy: [u8; 32],
    pub y: BigUint,
    pub x_end: BigUint,
    pub proof: vdf::posq::SequentialProof,
    pub sig: Sig65,
}

impl SegmentSeal {
    pub fn signing_message(
        epoch: u64,
        segment: u64,
        entropy: &[u8; 32],
        y_hash: &[u8; 32],
        x_end_hash: &[u8; 32],
    ) -> Vec<u8> {
        let mut m = Vec::with_capacity(128);
        m.extend_from_slice(b"\x05posq-segment-seal-v1");
        m.extend_from_slice(&epoch.to_be_bytes());
        m.extend_from_slice(&segment.to_be_bytes());
        m.extend_from_slice(entropy);
        m.extend_from_slice(y_hash);
        m.extend_from_slice(x_end_hash);
        m
    }

    pub fn message(&self) -> Vec<u8> {
        Self::signing_message(
            self.epoch,
            self.segment,
            &self.entropy,
            &hash_group_element(&self.y),
            &hash_group_element(&self.x_end),
        )
    }

    pub fn verify_sig(&self, sequencer: &[u8; 20]) -> bool {
        crate::identity::verify_signed(&self.message(), &self.sig, sequencer)
    }
}

/// The three accountable responses (Definition 6.1): exactly one per
/// syntactically valid commitment in an active window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AdmissionOutcome {
    Admitted(Receipt),
    Rejected(SignedRejection),
    FullWindow(FullWindowResponse),
}

/// Convenience: sign helpers on Identity.
pub trait SignRecords {
    fn sign_message_addr(&self, message: &[u8]) -> (Sig65, [u8; 20]);
}

impl SignRecords for Identity {
    fn sign_message_addr(&self, message: &[u8]) -> (Sig65, [u8; 20]) {
        (self.sign(message), self.address())
    }
}

pub fn recover_signer(message: &[u8], sig: &Sig65) -> Option<[u8; 20]> {
    recover_address(&keccak256(message), sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merkle_path_roundtrip() {
        let leaves: Vec<[u8; 32]> = (0..7u8).map(|i| keccak256(&[i])).collect();
        let root = merkle_root(&leaves);
        for i in 0..leaves.len() {
            let path = merkle_path(&leaves, i).unwrap();
            assert!(merkle_verify(&root, &leaves[i], i, &path), "leaf {i}");
            // A different leaf under the same path must fail.
            assert!(!merkle_verify(&root, &keccak256(b"forged"), i, &path));
        }
    }

    #[test]
    fn single_leaf_and_empty() {
        let leaves = vec![keccak256(b"only")];
        assert_eq!(merkle_root(&leaves), leaves[0]);
        assert_eq!(merkle_root(&[]), empty_batch_root());
    }

    #[test]
    fn receipt_digest_link_enforced_in_verify() {
        let id = Identity::from_seed(b"seq");
        let d_prev = digest_genesis(0, 5);
        let h = keccak256(b"commitment");
        let d = digest_step(&d_prev, 5, 0, &h, Bucket::Majors, &[3u8; 16]);
        let msg = Receipt::signing_message(
            0, 5, 0, &h, Bucket::Majors,
            Window { start: 4, len: 4 },
            &[3u8; 16], &[0u8; 32], &log_genesis(0), &d_prev, &d,
        );
        let receipt = Receipt {
            epoch: 0, tick: 5, pos: 0, h, bucket: Bucket::Majors,
            window: Window { start: 4, len: 4 },
            ticket_id: [3u8; 16], x_prev_hash: [0u8; 32],
            c_prev: log_genesis(0), d_prev, d,
            sig: id.sign(&msg),
        };
        assert!(receipt.verify(&id.address()));
        let mut bad = receipt.clone();
        bad.d = keccak256(b"wrong");
        assert!(!bad.verify(&id.address()));
    }

    #[test]
    fn log_chain_deterministic() {
        let c0 = log_genesis(0);
        let root = empty_batch_root();
        let x = keccak256(b"x1");
        let c1 = log_step(&c0, 1, &root, &x);
        assert_eq!(c1, log_step(&c0, 1, &root, &x));
        assert_ne!(c1, log_step(&c0, 2, &root, &x));
    }
}
