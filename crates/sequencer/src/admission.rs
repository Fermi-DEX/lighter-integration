//! Admission: inclusion windows, deterministic (epoch, t, j) assignment,
//! and the three accountable responses (§6, §8.1).
//!
//! Every syntactically valid commitment in an active window gets exactly
//! one of: an admission receipt, a signed rejection with a protocol-valid
//! reason, or a signed full-window response — no silent outcome
//! (Definition 6.1). The capacity rule is public and deterministic
//! (`capacity_per_tick_per_bucket`), and a full-window response is issued
//! only when **every** remaining tick of the window is exhausted for the
//! bucket — issuing it earlier would be false fullness, which is objective
//! fraud evidence (§6.2). If the current tick is full but the window has
//! room later, the commitment spills to the earliest tick with capacity;
//! its position (t, j) and receipt digest chain are fixed at reservation
//! time, so the receipt is still issued immediately.
//!
//! Fee tickets are consumed (nullified) exactly when a receipt is issued
//! (§7.3, deployment rule 4); rejections and full-window responses do not
//! consume the ticket.
//!
//! Fencing (Definition 5.2): a cadence fault fences every window
//! overlapping the fault interval. Fenced reservations are released, their
//! tickets un-consumed, and re-submission is answered with `Fenced` while
//! the fence is active (re-servable without penalty).

use crate::envelope::{Bucket, Envelope, Ticket, Window};
use crate::identity::Identity;
use crate::params::PosqParams;
use crate::records::{
    digest_genesis, digest_step, AdmissionOutcome, FullWindowResponse, Receipt, RejectReason,
    SignedRejection,
};
use crate::store::DaStore;
use std::collections::{BTreeMap, HashMap, HashSet};

/// One reserved slot, fixed at admission.
#[derive(Debug, Clone)]
pub struct PendingEntry {
    pub tick: u64,
    pub pos: u32,
    pub h: [u8; 32],
    pub bucket: Bucket,
    pub delay_class: u8,
    pub ticket_id: [u8; 16],
    pub envelope: Envelope,
    pub envelope_bytes: Vec<u8>,
    pub receipt: Receipt,
}

#[derive(Default)]
struct TickBuffer {
    next_pos: u32,
    d_last: Option<[u8; 32]>,
    bucket_counts: HashMap<u8, u32>,
    entries: Vec<PendingEntry>,
}

pub struct Admission {
    params: PosqParams,
    identity: Identity,
    ticket_issuer: [u8; 20],
    /// Tick currently accepting commitments; sealed ticks are immutable.
    open_tick: u64,
    /// Latest sealed chain state, bound into receipts.
    x_prev_hash: [u8; 32],
    c_prev: [u8; 32],
    /// Buffers for the open tick and future (spilled) ticks.
    ticks: BTreeMap<u64, TickBuffer>,
    consumed_tickets: HashSet<[u8; 16]>,
    seen_h: HashSet<[u8; 32]>,
    /// Windows overlapping [fence_from, fence_until] are fenced.
    fence_until: Option<u64>,
}

impl Admission {
    pub fn new(params: PosqParams, identity: Identity, ticket_issuer: [u8; 20]) -> Self {
        let c_prev = crate::records::log_genesis(params.epoch);
        Self {
            params,
            identity,
            ticket_issuer,
            open_tick: 1,
            x_prev_hash: [0u8; 32],
            c_prev,
            ticks: BTreeMap::new(),
            consumed_tickets: HashSet::new(),
            seen_h: HashSet::new(),
            fence_until: None,
        }
    }

    pub fn open_tick(&self) -> u64 {
        self.open_tick
    }

    fn reject(&self, h: [u8; 32], bucket: Bucket, window: Window, reason: RejectReason) -> AdmissionOutcome {
        let msg = SignedRejection::message_of(self.params.epoch, &h, bucket, window, reason, &self.c_prev);
        AdmissionOutcome::Rejected(SignedRejection {
            epoch: self.params.epoch,
            h,
            bucket,
            window,
            reason,
            c_latest: self.c_prev,
            sig: self.identity.sign(&msg),
        })
    }

    /// The admission flow. `da` receives the envelope body before the
    /// receipt is issued (no data, no receipt-finality).
    pub fn submit(&mut self, envelope_bytes: &[u8], da: &DaStore) -> AdmissionOutcome {
        let epoch = self.params.epoch;
        let h = Envelope::commitment_hash(epoch, envelope_bytes);

        // 1. Fixed size (§7.1: exactly one valid envelope size).
        if envelope_bytes.len() != self.params.envelope_bytes {
            return self.reject(h, Bucket::Majors, Window { start: 0, len: 1 }, RejectReason::WrongSize);
        }
        // 2. Parse the visible surface.
        let env = match Envelope::decode(envelope_bytes, self.params.envelope_bytes) {
            Ok(e) => e,
            Err(_) => {
                return self.reject(h, Bucket::Majors, Window { start: 0, len: 1 }, RejectReason::Malformed)
            }
        };
        let (bucket, window) = (env.bucket, env.window);

        // 3. Window length is bounded by the profile W — inequality (1) is
        //    derived for it (deployment rule 1).
        if window.len > self.params.window_ticks {
            return self.reject(h, bucket, window, RejectReason::Malformed);
        }
        // 4. Delay class must exist; ticket fee class implies delay class.
        if self.params.delay_ticks(env.delay_class).is_none() {
            return self.reject(h, bucket, window, RejectReason::BadDelayClass);
        }
        if env.ticket.denomination != env.delay_class
            || env.ticket.bucket != bucket
            || !env.ticket.verify(&self.ticket_issuer)
        {
            return self.reject(h, bucket, window, RejectReason::BadTicket);
        }

        // 4. Window vs the open tick.
        if self.fence_until.is_some_and(|until| window.start <= until) {
            return self.reject(h, bucket, window, RejectReason::Fenced);
        }
        if self.open_tick < window.start {
            return self.reject(h, bucket, window, RejectReason::WindowNotOpen);
        }
        if self.open_tick > window.end_inclusive() {
            return self.reject(h, bucket, window, RejectReason::WindowExpired);
        }

        // 5. Ticket nullifier and commitment dedup.
        if self.consumed_tickets.contains(&env.ticket.id) {
            return self.reject(h, bucket, window, RejectReason::TicketConsumed);
        }
        if self.seen_h.contains(&h) {
            return self.reject(h, bucket, window, RejectReason::Duplicate);
        }

        // 6. Capacity: earliest tick in [open, window end] with room.
        let cap = self.params.capacity_per_tick_per_bucket;
        let target = (self.open_tick..=window.end_inclusive()).find(|t| {
            self.ticks
                .get(t)
                .and_then(|b| b.bucket_counts.get(&(bucket as u8)))
                .copied()
                .unwrap_or(0)
                < cap
        });
        let Some(t) = target else {
            // Every remaining tick of the window is exhausted: honest
            // full-window response under the public capacity rule.
            let msg = FullWindowResponse::message_of(epoch, bucket, window, cap, &self.c_prev);
            return AdmissionOutcome::FullWindow(FullWindowResponse {
                epoch,
                bucket,
                window,
                capacity: cap,
                c_latest: self.c_prev,
                sig: self.identity.sign(&msg),
            });
        };

        // 7. Persist the envelope body before issuing the receipt.
        if da.put_envelope(&h, envelope_bytes).is_err() {
            // Storage failure: accountable rejection rather than a receipt
            // that could never be data-available.
            return self.reject(h, bucket, window, RejectReason::Malformed);
        }

        // 8. Reserve (t, j), extend the digest chain, sign the receipt,
        //    consume the ticket.
        let buffer = self.ticks.entry(t).or_default();
        let pos = buffer.next_pos;
        buffer.next_pos += 1;
        *buffer.bucket_counts.entry(bucket as u8).or_insert(0) += 1;
        let d_prev = buffer.d_last.unwrap_or_else(|| digest_genesis(epoch, t));
        let d = digest_step(&d_prev, t, pos, &h, bucket, &env.ticket.id);
        buffer.d_last = Some(d);

        let msg = Receipt::signing_message(
            epoch, t, pos, &h, bucket, window, &env.ticket.id,
            &self.x_prev_hash, &self.c_prev, &d_prev, &d,
        );
        let receipt = Receipt {
            epoch,
            tick: t,
            pos,
            h,
            bucket,
            window,
            ticket_id: env.ticket.id,
            x_prev_hash: self.x_prev_hash,
            c_prev: self.c_prev,
            d_prev,
            d,
            sig: self.identity.sign(&msg),
        };
        self.consumed_tickets.insert(env.ticket.id);
        self.seen_h.insert(h);
        buffer.entries.push(PendingEntry {
            tick: t,
            pos,
            h,
            bucket,
            delay_class: env.delay_class,
            ticket_id: env.ticket.id,
            envelope: env,
            envelope_bytes: envelope_bytes.to_vec(),
            receipt: receipt.clone(),
        });
        AdmissionOutcome::Admitted(receipt)
    }

    /// Take tick `t`'s entries (in receipt-chain order) without advancing.
    /// The coordinator computes root(B_t) and C_t from these, then calls
    /// `advance_open`.
    pub fn take_tick(&mut self, t: u64) -> Vec<PendingEntry> {
        debug_assert_eq!(t, self.open_tick);
        self.ticks.remove(&t).map(|b| b.entries).unwrap_or_default()
    }

    /// Open tick t+1, binding subsequent receipts to the sealed state.
    pub fn advance_open(&mut self, t: u64, x_hash: [u8; 32], c_t: [u8; 32]) {
        debug_assert_eq!(t, self.open_tick);
        self.open_tick = t + 1;
        self.x_prev_hash = x_hash;
        self.c_prev = c_t;
    }

    /// Convenience: take + advance in one call (tests).
    pub fn seal_tick(&mut self, t: u64, x_hash: [u8; 32], c_t: [u8; 32]) -> Vec<PendingEntry> {
        let entries = self.take_tick(t);
        self.advance_open(t, x_hash, c_t);
        entries
    }

    pub fn latest_log_commitment(&self) -> [u8; 32] {
        self.c_prev
    }

    /// Cadence fault covering tick `fault_tick`: fence every window
    /// overlapping it. Releases unsealed reservations in fenced windows and
    /// un-consumes their tickets; returns the released commitment hashes.
    pub fn fence(&mut self, fault_tick: u64) -> Vec<[u8; 32]> {
        self.fence_until = Some(fault_tick.max(self.fence_until.unwrap_or(0)));
        let mut released = Vec::new();
        for buffer in self.ticks.values_mut() {
            buffer.entries.retain(|e| {
                let overlaps = e.receipt.window.start <= fault_tick
                    && fault_tick <= e.receipt.window.end_inclusive();
                if overlaps {
                    self.consumed_tickets.remove(&e.ticket_id);
                    self.seen_h.remove(&e.h);
                    released.push(e.h);
                    // Note: position (t, j) is abandoned, not reused — the
                    // digest chain keeps the hole; the batch emits a gap.
                }
                !overlaps
            });
        }
        released
    }

    pub fn lift_fence(&mut self) {
        self.fence_until = None;
    }

    pub fn ticket_consumed(&self, id: &[u8; 16]) -> bool {
        self.consumed_tickets.contains(id)
    }
}

/// Dev ticket faucet: mints issuer-signed tickets. Economics (payment in a
/// settlement asset, burn share, blind signing) are the documented deferral.
pub struct TicketFaucet {
    issuer: Identity,
    counter: u64,
}

impl TicketFaucet {
    pub fn new(issuer: Identity) -> Self {
        Self { issuer, counter: 0 }
    }

    pub fn issuer_address(&self) -> [u8; 20] {
        self.issuer.address()
    }

    pub fn mint(&mut self, denomination: u8, bucket: Bucket) -> Ticket {
        self.counter += 1;
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&self.counter.to_be_bytes());
        id[8..].copy_from_slice(&rand::random::<[u8; 8]>());
        Ticket::issue(&self.issuer, id, denomination, bucket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Intent;
    use num_bigint::BigUint;

    fn setup() -> (Admission, TicketFaucet, DaStore, PosqParams) {
        let params = PosqParams::dev(8, 16);
        let seq = Identity::from_seed(b"sequencer");
        let faucet = TicketFaucet::new(Identity::from_seed(b"issuer"));
        let adm = Admission::new(params.clone(), seq, faucet.issuer_address());
        (adm, faucet, DaStore::temp().unwrap(), params)
    }

    fn envelope_bytes(
        faucet: &mut TicketFaucet,
        params: &PosqParams,
        window: Window,
        seed: &str,
    ) -> Vec<u8> {
        let ticket = faucet.mint(0, Bucket::Majors);
        let env = Envelope {
            bucket: Bucket::Majors,
            namespace: 1,
            window,
            delay_class: 0,
            nonce: crate::identity::keccak256(seed.as_bytes())[..16].try_into().unwrap(),
            ticket,
            ciphertext: vdf::tlk::Ciphertext {
                class: 0,
                u: BigUint::from(42u32) + BigUint::from(seed.len()),
                body: seed.as_bytes().to_vec(),
            },
        };
        env.encode(params.envelope_bytes).unwrap()
    }

    #[test]
    fn admitted_gets_receipt_with_chained_digest() {
        let (mut adm, mut faucet, da, params) = setup();
        let w = Window { start: 1, len: 4 };
        let b1 = envelope_bytes(&mut faucet, &params, w, "one");
        let b2 = envelope_bytes(&mut faucet, &params, w, "two");
        let AdmissionOutcome::Admitted(r1) = adm.submit(&b1, &da) else { panic!() };
        let AdmissionOutcome::Admitted(r2) = adm.submit(&b2, &da) else { panic!() };
        assert_eq!((r1.tick, r1.pos), (1, 0));
        assert_eq!((r2.tick, r2.pos), (1, 1));
        assert_eq!(r2.d_prev, r1.d);
        assert!(da.has_envelope(&r1.h));
        let seq_addr = Identity::from_seed(b"sequencer").address();
        assert!(r1.verify(&seq_addr) && r2.verify(&seq_addr));
    }

    #[test]
    fn duplicate_and_consumed_ticket_rejected() {
        let (mut adm, mut faucet, da, params) = setup();
        let w = Window { start: 1, len: 4 };
        let b = envelope_bytes(&mut faucet, &params, w, "dup");
        assert!(matches!(adm.submit(&b, &da), AdmissionOutcome::Admitted(_)));
        let AdmissionOutcome::Rejected(rej) = adm.submit(&b, &da) else { panic!() };
        // Same bytes → the ticket nullifier fires (checked before dedup);
        // either reason is a protocol-valid accountable rejection.
        assert!(matches!(
            rej.reason,
            RejectReason::TicketConsumed | RejectReason::Duplicate
        ));
    }

    #[test]
    fn window_checks() {
        let (mut adm, mut faucet, da, params) = setup();
        let early = envelope_bytes(&mut faucet, &params, Window { start: 10, len: 2 }, "e");
        let AdmissionOutcome::Rejected(r) = adm.submit(&early, &da) else { panic!() };
        assert_eq!(r.reason, RejectReason::WindowNotOpen);

        adm.seal_tick(1, [1; 32], [2; 32]);
        adm.seal_tick(2, [1; 32], [2; 32]);
        let late = envelope_bytes(&mut faucet, &params, Window { start: 1, len: 2 }, "l");
        let AdmissionOutcome::Rejected(r) = adm.submit(&late, &da) else { panic!() };
        assert_eq!(r.reason, RejectReason::WindowExpired);
    }

    #[test]
    fn spill_then_full_window() {
        let (mut adm, mut faucet, da, mut params) = setup();
        params.capacity_per_tick_per_bucket = 1;
        let seq = Identity::from_seed(b"sequencer");
        let mut adm2 = Admission::new(params.clone(), seq, faucet.issuer_address());
        let w = Window { start: 1, len: 2 };
        let a = envelope_bytes(&mut faucet, &params, w, "a");
        let b = envelope_bytes(&mut faucet, &params, w, "b");
        let c = envelope_bytes(&mut faucet, &params, w, "c");
        let AdmissionOutcome::Admitted(ra) = adm2.submit(&a, &da) else { panic!() };
        assert_eq!(ra.tick, 1);
        // Tick 1 full → spills to tick 2, receipt issued immediately.
        let AdmissionOutcome::Admitted(rb) = adm2.submit(&b, &da) else { panic!() };
        assert_eq!((rb.tick, rb.pos), (2, 0));
        // Whole window exhausted → signed full-window response.
        let AdmissionOutcome::FullWindow(fw) = adm2.submit(&c, &da) else { panic!() };
        assert_eq!(fw.capacity, 1);
        assert!(fw.verify(&Identity::from_seed(b"sequencer").address()));
        let _ = adm;
    }

    #[test]
    fn fence_releases_reservations_and_tickets() {
        let (mut adm, mut faucet, da, params) = setup();
        let w = Window { start: 1, len: 4 };
        let b = envelope_bytes(&mut faucet, &params, w, "fenced");
        let AdmissionOutcome::Admitted(r) = adm.submit(&b, &da) else { panic!() };
        assert!(adm.ticket_consumed(&r.ticket_id));
        let released = adm.fence(2);
        assert_eq!(released, vec![r.h]);
        assert!(!adm.ticket_consumed(&r.ticket_id));
        // While fenced, resubmission is answered accountably.
        let AdmissionOutcome::Rejected(rej) = adm.submit(&b, &da) else { panic!() };
        assert_eq!(rej.reason, RejectReason::Fenced);
        // After the fence lifts, the same commitment is re-servable.
        adm.lift_fence();
        adm.seal_tick(1, [1; 32], [2; 32]);
        assert!(matches!(adm.submit(&b, &da), AdmissionOutcome::Admitted(_)));
    }
}
