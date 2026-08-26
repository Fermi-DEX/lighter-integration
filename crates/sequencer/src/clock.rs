//! The segment clock (§4.4, §5.1): work-defined ticks.
//!
//! A dedicated thread advances the chain state by pure squaring — the tick
//! *is* q squarings, not a timer — organized into segments of F ticks with
//! one Wesolowski proof per segment. The segment seed welds the prior
//! segment's end state, its log commitment, and fresh external entropy into
//! the timeline:
//!
//!   y_s = H→QR_N(epoch ‖ s ‖ x_end(s−1) ‖ C_end(s−1) ‖ r_s)
//!
//! Freshness bounds precomputation to one segment (§4.4). Because C_end is
//! produced by the coordinator (it depends on the sealed batches), the
//! clock rendezvouses at each segment boundary: it emits `SegmentEnd`,
//! waits for the seed input, then starts the next segment while a separate
//! thread computes π_s (the two-rig discipline: proving must not stall the
//! cadence).
//!
//! Global tick numbering: segment s covers ticks s·F + 1 ..= (s+1)·F.

use num_bigint::BigUint;
use std::sync::mpsc::{Receiver, Sender};
use vdf::posq::{Group, SequentialProof};

pub const SEGMENT_SEED_DOMAIN: &[u8] = b"posq-segment-seed-v1";
pub const SEGMENT_PROOF_DOMAIN: &[u8] = b"posq-segment-proof-v1";

#[derive(Debug, Clone)]
pub enum ClockEvent {
    /// End-of-tick state x_t after q squarings.
    Tick { segment: u64, tick: u64, x: BigUint },
    /// Segment finished its F ticks; proof follows asynchronously.
    SegmentEnd { segment: u64, y: BigUint, x_end: BigUint },
    /// π_s for y_s → x_end(s) over q·F squarings.
    SegmentProof { segment: u64, proof: SequentialProof },
}

/// Input the coordinator supplies at each segment boundary.
#[derive(Debug, Clone)]
pub struct SeedInput {
    /// C_end(s−1): the log commitment after the segment's last tick sealed.
    pub c_end: [u8; 32],
    /// r_s = H(beacon ‖ L1 hash) — external freshness.
    pub entropy: [u8; 32],
}

/// Genesis inputs for segment 0.
pub fn genesis_x_end_hash(epoch: u64) -> [u8; 32] {
    crate::identity::keccak256(&[b"posq-genesis-x-v1".as_slice(), &epoch.to_be_bytes()].concat())
}

pub fn segment_seed(
    group: &Group,
    epoch: u64,
    segment: u64,
    x_end_prev_hash: &[u8; 32],
    c_end_prev: &[u8; 32],
    entropy: &[u8; 32],
) -> BigUint {
    let mut input = Vec::with_capacity(112);
    input.extend_from_slice(&epoch.to_be_bytes());
    input.extend_from_slice(&segment.to_be_bytes());
    input.extend_from_slice(x_end_prev_hash);
    input.extend_from_slice(c_end_prev);
    input.extend_from_slice(entropy);
    group.hash_to_group(SEGMENT_SEED_DOMAIN, &input)
}

pub struct ClockConfig {
    pub group: Group,
    pub epoch: u64,
    pub q: u64,
    pub segment_ticks: u64,
    /// Stop after this many segments (None = run until channel closes).
    pub max_segments: Option<u64>,
    /// Entropy for segment 0 (later segments' entropy arrives in SeedInput).
    pub genesis_entropy: [u8; 32],
    /// C_end(-1) = log genesis.
    pub genesis_c: [u8; 32],
}

/// Run the clock until the event channel closes or `max_segments` is hit.
/// Blocking; intended for its own OS thread.
pub fn run_clock(cfg: ClockConfig, events: Sender<ClockEvent>, seeds: Receiver<SeedInput>) {
    let mut x_end_prev_hash = genesis_x_end_hash(cfg.epoch);
    let mut c_end_prev = cfg.genesis_c;
    let mut entropy = cfg.genesis_entropy;
    let mut segment: u64 = 0;

    loop {
        if cfg.max_segments.is_some_and(|m| segment >= m) {
            return;
        }
        let y = segment_seed(&cfg.group, cfg.epoch, segment, &x_end_prev_hash, &c_end_prev, &entropy);
        let mut x = y.clone();
        for i in 0..cfg.segment_ticks {
            x = cfg.group.tick_advance(&x, cfg.q);
            let tick = segment * cfg.segment_ticks + i + 1;
            if events.send(ClockEvent::Tick { segment, tick, x: x.clone() }).is_err() {
                return;
            }
        }
        let x_end = x;
        if events
            .send(ClockEvent::SegmentEnd { segment, y: y.clone(), x_end: x_end.clone() })
            .is_err()
        {
            return;
        }

        // Prove the segment on a separate thread so the cadence never waits
        // on proving.
        {
            let group = cfg.group.clone();
            let events = events.clone();
            let (y, x_end) = (y.clone(), x_end.clone());
            let t = cfg.q * cfg.segment_ticks;
            let s = segment;
            std::thread::spawn(move || {
                let pi = group.prove_with_output(SEGMENT_PROOF_DOMAIN, &y, &x_end, t);
                let proof = SequentialProof { y: x_end, pi, iterations: t };
                let _ = events.send(ClockEvent::SegmentProof { segment: s, proof });
            });
        }

        // Rendezvous: the next seed needs C_end(s), which the coordinator
        // computes after sealing this segment's final tick.
        let Ok(seed) = seeds.recv() else { return };
        x_end_prev_hash = crate::records::hash_group_element(&x_end);
        c_end_prev = seed.c_end;
        entropy = seed.entropy;
        segment += 1;
    }
}

/// Verify a sealed segment against its claimed seed inputs (watcher lane 2).
pub fn verify_segment(
    group: &Group,
    epoch: u64,
    segment: u64,
    x_end_prev_hash: &[u8; 32],
    c_end_prev: &[u8; 32],
    q: u64,
    segment_ticks: u64,
    seal: &crate::records::SegmentSeal,
) -> bool {
    let expected_y =
        segment_seed(group, epoch, segment, x_end_prev_hash, c_end_prev, &seal.entropy);
    if seal.y != expected_y || seal.proof.iterations != q * segment_ticks {
        return false;
    }
    if seal.proof.y != seal.x_end {
        return false;
    }
    group
        .verify_sequential(SEGMENT_PROOF_DOMAIN, &seal.y, &seal.proof)
        .unwrap_or(false)
}

/// Measure the local squaring rate and derive q at 90% of it (§5.1: the
/// mandated rate is measured, not chosen).
pub fn calibrate_q(group: &Group, delta_micros: u64) -> u64 {
    let x = group.hash_to_group(b"calibrate", b"probe");
    let probe: u64 = 2_000;
    let start = std::time::Instant::now();
    let _ = group.tick_advance(&x, probe);
    let elapsed = start.elapsed().as_secs_f64();
    let rate = probe as f64 / elapsed; // squarings per second
    let q = (0.9 * rate * (delta_micros as f64 / 1e6)) as u64;
    q.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn clock_emits_ticks_and_segments_with_valid_proofs() {
        let group = Group::default_rsa2048();
        let (etx, erx) = mpsc::channel();
        let (stx, srx) = mpsc::channel();
        let cfg = ClockConfig {
            group: group.clone(),
            epoch: 0,
            q: 4,
            segment_ticks: 3,
            max_segments: Some(2),
            genesis_entropy: [7u8; 32],
            genesis_c: crate::records::log_genesis(0),
        };
        let handle = std::thread::spawn(move || run_clock(cfg, etx, srx));

        let mut ticks = Vec::new();
        let mut ends = Vec::new();
        let mut proofs = Vec::new();
        while let Ok(ev) = erx.recv() {
            match ev {
                ClockEvent::Tick { tick, x, .. } => ticks.push((tick, x)),
                ClockEvent::SegmentEnd { segment, y, x_end } => {
                    ends.push((segment, y, x_end));
                    let _ = stx.send(SeedInput { c_end: [segment as u8; 32], entropy: [9u8; 32] });
                }
                ClockEvent::SegmentProof { segment, proof } => proofs.push((segment, proof)),
            }
        }
        handle.join().unwrap();

        assert_eq!(ticks.iter().map(|(t, _)| *t).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(ends.len(), 2);
        assert_eq!(proofs.len(), 2);

        // Tick states chain by q squarings within a segment.
        let (_, y0, x_end0) = &ends[0];
        let mut x = y0.clone();
        for i in 0..3 {
            x = group.tick_advance(&x, 4);
            assert_eq!(x, ticks[i].1);
        }
        assert_eq!(&x, x_end0);

        // Segment proofs verify.
        for (s, proof) in &proofs {
            let (_, y, x_end) = ends.iter().find(|(seg, _, _)| seg == s).unwrap();
            assert_eq!(&proof.y, x_end);
            assert!(group.verify_sequential(SEGMENT_PROOF_DOMAIN, y, proof).unwrap());
        }

        // Segment 1's seed depends on segment 0's outputs (braiding).
        let x0_hash = crate::records::hash_group_element(&ends[0].2);
        let expected_y1 = segment_seed(&group, 0, 1, &x0_hash, &[0u8; 32], &[9u8; 32]);
        assert_eq!(ends[1].1, expected_y1);
    }

    #[test]
    fn seed_binds_all_inputs() {
        let g = Group::default_rsa2048();
        let base = segment_seed(&g, 0, 1, &[1; 32], &[2; 32], &[3; 32]);
        assert_ne!(base, segment_seed(&g, 0, 2, &[1; 32], &[2; 32], &[3; 32]));
        assert_ne!(base, segment_seed(&g, 0, 1, &[9; 32], &[2; 32], &[3; 32]));
        assert_ne!(base, segment_seed(&g, 0, 1, &[1; 32], &[9; 32], &[3; 32]));
        assert_ne!(base, segment_seed(&g, 0, 1, &[1; 32], &[2; 32], &[9; 32]));
    }
}
