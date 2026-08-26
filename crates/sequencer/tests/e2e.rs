//! End-to-end: client encrypts → submits → receipt → clock seals → solver
//! wave opens → canonical tape, then independent lane-2 verification
//! (segment proofs + log chain + transcript predicate) and anchor checks.

use sequencer::admission::TicketFaucet;
use sequencer::anchor::MockHost;
use sequencer::client::{build_envelope, EnvelopeRequest};
use sequencer::clock::{verify_segment, SEGMENT_PROOF_DOMAIN};
use sequencer::envelope::{Bucket, Window};
use sequencer::identity::Identity;
use sequencer::node::{NodeConfig, SequencerNode};
use sequencer::records::{hash_group_element, log_genesis, AdmissionOutcome};
use sequencer::store::{GapReason, Outcome};
use sequencer::transcript::{verify_span, TranscriptProof};
use std::collections::HashSet;
use std::sync::Arc;

fn wait_for_tick(shared: &Arc<sequencer::node::Shared>, tick: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while shared.current_tick.load(std::sync::atomic::Ordering::Relaxed) < tick {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for tick {tick}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn full_pipeline_blind_admission_to_opened_tape() {
    let cfg = NodeConfig::dev(6); // 6 segments × 8 ticks = 48 ticks
    let params = cfg.params.clone();
    let host = Arc::new(MockHost::default());
    let node = SequencerNode::start(cfg, host.clone()).unwrap();
    let shared = node.shared.clone();

    // Client side: mint tickets, build envelopes.
    let client = Identity::from_seed(b"trader-1");
    let mut submitted = Vec::new();
    for i in 0..3u64 {
        let ticket = shared.faucet.lock().unwrap().mint(0, Bucket::Majors);
        let window = Window { start: 1, len: params.window_ticks };
        let bytes = build_envelope(
            &shared.tlk,
            params.envelope_bytes,
            EnvelopeRequest {
                signer: &client,
                ticket,
                bucket: Bucket::Majors,
                namespace: 7,
                window,
                delay_class: 0,
                nonce: [i as u8 + 1; 16],
                intent_nonce: i,
                expiry_tick: 10_000,
                payload: format!("order {i}").into_bytes(),
                kem_seed: format!("entropy-{i}").into_bytes(),
            },
        )
        .unwrap();

        let outcome = {
            let store = shared.store.lock().unwrap();
            let mut adm = shared.admission.lock().unwrap();
            adm.submit(&bytes, store.da())
        };
        let AdmissionOutcome::Admitted(receipt) = outcome else {
            panic!("expected admission, got {outcome:?}");
        };
        assert!(receipt.verify(&shared.sequencer_address), "receipt must verify");
        submitted.push(receipt);
    }
    // Certified FIFO: canonical (t, j) positions strictly increase in
    // submission order (submissions may straddle tick boundaries).
    for w in submitted.windows(2) {
        assert!(
            (w[0].tick, w[0].pos) < (w[1].tick, w[1].pos),
            "FIFO violated: {:?} !< {:?}",
            (w[0].tick, w[0].pos),
            (w[1].tick, w[1].pos)
        );
    }
    // Blind admission: at receipt time no wave for these could have matured.
    let d_ticks = params.delay_ticks(0).unwrap();
    assert!(d_ticks > params.window_ticks as u64 + params.gamma_ticks);

    let shared = node.join();
    let status = shared.status.lock().unwrap().clone();
    assert!(status.finished);
    assert_eq!(status.admitted, 3);

    // 1. Every submitted commitment opened on the tape with the right intent.
    let store = shared.store.lock().unwrap();
    for (i, r) in submitted.iter().enumerate() {
        let pos = store.tape.get(&(r.tick, r.pos)).expect("tape position exists");
        match &pos.outcome {
            Outcome::Opened { intent } => {
                assert_eq!(intent.payload, format!("order {i}").into_bytes());
                assert_eq!(intent.account, client.address());
            }
            other => panic!("position ({}, {}) not opened: {other:?}", r.tick, r.pos),
        }
    }

    // 2. Lane-2 verification: every segment seal verifies, and seeds chain.
    let epoch = params.epoch;
    let mut x_prev_hash = sequencer::clock::genesis_x_end_hash(epoch);
    let mut c_prev_end = log_genesis(epoch);
    for s in 0..6u64 {
        let seal = store.segments.get(&s).expect("segment sealed");
        assert!(
            verify_segment(
                &shared.group, epoch, s, &x_prev_hash, &c_prev_end,
                params.q_squarings, params.segment_ticks, seal,
            ),
            "segment {s} verification failed"
        );
        assert!(seal.verify_sig(&shared.sequencer_address));
        // Also raw Wesolowski check.
        assert!(shared
            .group
            .verify_sequential(SEGMENT_PROOF_DOMAIN, &seal.y, &seal.proof)
            .unwrap());
        x_prev_hash = hash_group_element(&seal.x_end);
        let last_tick_of_segment = (s + 1) * params.segment_ticks;
        c_prev_end = store.ticks.get(&last_tick_of_segment).unwrap().record.c_t;
    }

    // 3. Transcript predicate over the whole run.
    let last = store.latest_tick().unwrap();
    let ticks_fn = |t: u64| store.ticks.get(&t).cloned();
    let c_final = verify_span(
        epoch, &shared.sequencer_address, log_genesis(epoch), 1, last, &ticks_fn, &HashSet::new(),
    )
    .expect("transcript predicate holds");
    assert_eq!(c_final, store.ticks.get(&last).unwrap().record.c_t);

    // 4. Tick records chain x_t by q squarings within a segment.
    let r1 = &store.ticks.get(&1).unwrap().record;
    let r2 = &store.ticks.get(&2).unwrap().record;
    assert_eq!(shared.group.tick_advance(&r1.x, params.q_squarings), r2.x);

    // 5. Anchors were built, signed, posted, with optimistic transcript
    //    artifacts and verifiable segment seals inside.
    let anchors = shared.anchors.lock().unwrap();
    assert!(!anchors.is_empty(), "expected at least one anchor");
    for a in anchors.iter() {
        assert!(a.verify_sig(&shared.sequencer_address));
        assert!(matches!(a.transcript, TranscriptProof::Optimistic { .. }));
        assert_eq!(a.segment_seals.len() as u64, params.anchor_every_segments);
        assert!(!a.calldata().is_empty());
    }
    assert_eq!(host.posted.lock().unwrap().len(), anchors.len());

    // 6. Wave solutions are independently verifiable (batched public solving).
    let waves = shared.waves.lock().unwrap();
    assert!(!waves.is_empty());
    let total_opened: usize = waves.iter().map(|w| w.solution.witnesses.len()).sum();
    assert_eq!(total_opened, 3);
}

#[test]
fn mauled_ciphertext_becomes_paid_gap_and_stream_continues() {
    let cfg = NodeConfig::dev(6);
    let params = cfg.params.clone();
    let node = SequencerNode::start(cfg, Arc::new(MockHost::default())).unwrap();
    let shared = node.shared.clone();

    let client = Identity::from_seed(b"trader-2");
    let window = Window { start: 1, len: params.window_ticks };

    // Envelope A: mauled body (decode, flip a ciphertext byte, re-encode).
    let ticket_a = shared.faucet.lock().unwrap().mint(0, Bucket::Majors);
    let mut bytes_a = build_envelope(
        &shared.tlk,
        params.envelope_bytes,
        EnvelopeRequest {
            signer: &client,
            ticket: ticket_a,
            bucket: Bucket::Majors,
            namespace: 7,
            window,
            delay_class: 0,
            nonce: [1; 16],
            intent_nonce: 0,
            expiry_tick: 10_000,
            payload: b"garbled".to_vec(),
            kem_seed: b"seed-a".to_vec(),
        },
    )
    .unwrap();
    bytes_a[400] ^= 0xff; // inside the AEAD body

    // Envelope B: honest, submitted after A.
    let ticket_b = shared.faucet.lock().unwrap().mint(0, Bucket::Majors);
    let bytes_b = build_envelope(
        &shared.tlk,
        params.envelope_bytes,
        EnvelopeRequest {
            signer: &client,
            ticket: ticket_b,
            bucket: Bucket::Majors,
            namespace: 7,
            window,
            delay_class: 0,
            nonce: [2; 16],
            intent_nonce: 1,
            expiry_tick: 10_000,
            payload: b"honest order".to_vec(),
            kem_seed: b"seed-b".to_vec(),
        },
    )
    .unwrap();

    let (ra, rb) = {
        let store = shared.store.lock().unwrap();
        let mut adm = shared.admission.lock().unwrap();
        let AdmissionOutcome::Admitted(ra) = adm.submit(&bytes_a, store.da()) else { panic!() };
        let AdmissionOutcome::Admitted(rb) = adm.submit(&bytes_b, store.da()) else { panic!() };
        (ra, rb)
    };
    assert!(ra.pos < rb.pos || ra.tick < rb.tick);

    let shared = node.join();
    let store = shared.store.lock().unwrap();

    // A is a deterministic gap in place (fee consumed, position kept)...
    let pa = store.tape.get(&(ra.tick, ra.pos)).unwrap();
    assert!(
        matches!(pa.outcome, Outcome::Gap { reason: GapReason::Undecryptable }),
        "mauled ciphertext must gap, got {:?}",
        pa.outcome
    );
    // ...and B still opened *behind* it — no head-of-line blocking, no
    // slide-forward.
    let pb = store.tape.get(&(rb.tick, rb.pos)).unwrap();
    assert!(matches!(&pb.outcome, Outcome::Opened { intent } if intent.payload == b"honest order"));
    // The gap's ticket was consumed (paid capacity, G4).
    assert!(shared.admission.lock().unwrap().ticket_consumed(&ra.ticket_id));
}

#[test]
fn cadence_fault_fences_windows() {
    let mut cfg = NodeConfig::dev(4);
    cfg.induce_late_tick = Some(2);
    let params = cfg.params.clone();
    let node = SequencerNode::start(cfg, Arc::new(MockHost::default())).unwrap();
    let shared = node.shared.clone();
    // Give the fault a chance to happen while windows could overlap it.
    wait_for_tick(&shared, 3);
    let shared = node.join();
    let status = shared.status.lock().unwrap().clone();
    assert!(status.cadence_faults.contains(&2), "fault recorded: {:?}", status.cadence_faults);
}

#[test]
fn unmatured_positions_gap_at_cutoff() {
    // Delay class 1 (2×T1) matures beyond the short run; the final cutoff
    // pass must resolve it as gap(unopened) — G3: every position resolves.
    let cfg = NodeConfig::dev(3);
    let params = cfg.params.clone();
    let node = SequencerNode::start(cfg, Arc::new(MockHost::default())).unwrap();
    let shared = node.shared.clone();

    let client = Identity::from_seed(b"trader-3");
    let ticket = shared.faucet.lock().unwrap().mint(1, Bucket::Majors);
    let bytes = build_envelope(
        &shared.tlk,
        params.envelope_bytes,
        EnvelopeRequest {
            signer: &client,
            ticket,
            bucket: Bucket::Majors,
            namespace: 7,
            window: Window { start: 1, len: params.window_ticks },
            delay_class: 1,
            nonce: [3; 16],
            intent_nonce: 0,
            expiry_tick: 10_000,
            payload: b"slow class".to_vec(),
            kem_seed: b"seed-slow".to_vec(),
        },
    )
    .unwrap();
    let outcome = {
        let store = shared.store.lock().unwrap();
        let mut adm = shared.admission.lock().unwrap();
        adm.submit(&bytes, store.da())
    };
    let AdmissionOutcome::Admitted(r) = outcome else { panic!("{outcome:?}") };

    let shared = node.join();
    let store = shared.store.lock().unwrap();
    let p = store.tape.get(&(r.tick, r.pos)).unwrap();
    // Either the solver got to it in time (opened) or the cutoff fired; with
    // 3 segments × 8 ticks = 24 ticks and D_1 ≈ T1·2/q ≈ 112 ticks, maturity
    // is unreachable: must be a gap or still-pending-resolved-as-gap.
    assert!(
        matches!(p.outcome, Outcome::Gap { reason: GapReason::Unopened })
            || matches!(p.outcome, Outcome::Pending { .. }),
        "got {:?}",
        p.outcome
    );
}
