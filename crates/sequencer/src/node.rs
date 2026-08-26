//! The sequencer node: coordinates the clock, admission, tape, solver
//! waves, timeliness, segment seals, and anchors.
//!
//! Threading model: the clock runs on its own OS thread (work-defined
//! ticks); segment proving and wave solving run on further threads (the
//! two-rig discipline — nothing on the accountability path waits on
//! proving or solving); this coordinator consumes a single event channel
//! and owns all sequencing state transitions. gRPC handlers touch only the
//! `Shared` state (admission for submits, store for reads).

use crate::admission::{Admission, TicketFaucet};
use crate::anchor::{Anchor, HostClient, MockHost};
use crate::clock::{self, ClockConfig, ClockEvent, SeedInput};
use crate::envelope::Intent;
use crate::identity::{keccak256, Identity};
use crate::params::PosqParams;
use crate::records::{
    hash_group_element, log_genesis, log_step, merkle_root, BatchEntry, SegmentSeal, TickRecord,
};
use crate::store::{ChainStore, DaStore, GapReason, Outcome, SealedTick, TapePosition};
use crate::transcript::{ReplayProver, TranscriptProver};
use num_bigint::BigUint;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::{info, warn};
use vdf::batch_solve::{self, WaveSolution};
use vdf::posq::Group;
use vdf::tlk::{Ciphertext, TlkParams};

pub struct NodeConfig {
    pub params: PosqParams,
    pub identity_seed: Vec<u8>,
    pub data_dir: Option<PathBuf>,
    /// Stop after this many segments (tests); None runs until dropped.
    pub max_segments: Option<u64>,
    pub enforce_timeliness: bool,
    /// Test hook: treat this tick as having missed its deadline.
    pub induce_late_tick: Option<u64>,
}

impl NodeConfig {
    pub fn dev(max_segments: u64) -> Self {
        Self {
            params: PosqParams::dev(2_000, 8),
            identity_seed: b"dev-sequencer".to_vec(),
            data_dir: None,
            max_segments: Some(max_segments),
            enforce_timeliness: false,
            induce_late_tick: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NodeStatus {
    pub current_tick: u64,
    pub current_segment: u64,
    pub admitted: u64,
    pub opened: u64,
    pub gaps: u64,
    pub cadence_faults: Vec<u64>,
    pub finished: bool,
}

/// A solved wave kept for audit/serving.
#[derive(Debug, Clone)]
pub struct StoredWave {
    pub mature_tick: u64,
    pub class: u8,
    pub positions: Vec<(u64, u32)>,
    pub solution: WaveSolution,
}

pub struct Shared {
    pub params: PosqParams,
    pub group: Group,
    pub tlk: TlkParams,
    pub sequencer_address: [u8; 20],
    pub admission: Mutex<Admission>,
    pub store: Mutex<ChainStore>,
    pub faucet: Mutex<TicketFaucet>,
    pub anchors: Mutex<Vec<Anchor>>,
    pub waves: Mutex<Vec<StoredWave>>,
    pub status: Mutex<NodeStatus>,
    pub record_tx: broadcast::Sender<TickRecord>,
    pub tape_tx: broadcast::Sender<TapePosition>,
    pub current_tick: AtomicU64,
}

struct WaveItem {
    tick: u64,
    pos: u32,
    namespace: u64,
    ct: Ciphertext,
    aad: Vec<u8>,
}

enum NodeEvent {
    Clock(ClockEvent),
    ClockClosed,
    Wave {
        mature_tick: u64,
        class: u8,
        items: Vec<WaveItem>,
        solution: Result<WaveSolution, String>,
    },
}

pub struct SequencerNode {
    pub shared: Arc<Shared>,
    coordinator: Option<std::thread::JoinHandle<()>>,
}

impl SequencerNode {
    /// Build the node: verifies parameters (Equation (1) startup invariant),
    /// runs the one-time timelock setup, and spawns clock + coordinator.
    pub fn start(cfg: NodeConfig, host: Arc<dyn HostClient>) -> anyhow::Result<Self> {
        if let Err((k, t, min_t)) = cfg.params.check_no_look() {
            anyhow::bail!(
                "delay class {k} (T={t}) violates the no-look inequality (needs > {min_t}); \
                 refusing to serve (deployment rule 1)"
            );
        }
        let group = Group::default_rsa2048();
        info!(
            "running timelock setup for {} delay classes (public sequential work)",
            cfg.params.delay_classes.len()
        );
        let tlk = TlkParams::setup(group.clone(), &cfg.params.delay_classes);
        let identity = Identity::from_seed(&cfg.identity_seed);
        let faucet = TicketFaucet::new(Identity::from_seed(&[&cfg.identity_seed[..], b"-tickets"].concat()));
        let da = match &cfg.data_dir {
            Some(dir) => DaStore::open(dir.clone())?,
            None => DaStore::temp()?,
        };
        let admission = Admission::new(cfg.params.clone(), identity.clone(), faucet.issuer_address());
        let (record_tx, _) = broadcast::channel(1024);
        let (tape_tx, _) = broadcast::channel(4096);

        let shared = Arc::new(Shared {
            params: cfg.params.clone(),
            group: group.clone(),
            tlk,
            sequencer_address: identity.address(),
            admission: Mutex::new(admission),
            store: Mutex::new(ChainStore::new(da)),
            faucet: Mutex::new(faucet),
            anchors: Mutex::new(Vec::new()),
            waves: Mutex::new(Vec::new()),
            status: Mutex::new(NodeStatus::default()),
            record_tx,
            tape_tx,
            current_tick: AtomicU64::new(0),
        });

        // Clock thread.
        let (clock_tx, clock_rx) = mpsc::channel::<ClockEvent>();
        let (seed_tx, seed_rx) = mpsc::channel::<SeedInput>();
        let genesis_entropy = entropy_for(cfg.params.epoch, 0);
        let clock_cfg = ClockConfig {
            group: group.clone(),
            epoch: cfg.params.epoch,
            q: cfg.params.q_squarings,
            segment_ticks: cfg.params.segment_ticks,
            max_segments: cfg.max_segments,
            genesis_entropy,
            genesis_c: log_genesis(cfg.params.epoch),
        };
        std::thread::Builder::new()
            .name("posq-clock".into())
            .spawn(move || clock::run_clock(clock_cfg, clock_tx, seed_rx))?;

        // Event fan-in: clock events + solver completions on one channel.
        let (event_tx, event_rx) = mpsc::channel::<NodeEvent>();
        {
            let event_tx = event_tx.clone();
            std::thread::Builder::new().name("posq-clock-fwd".into()).spawn(move || {
                while let Ok(ev) = clock_rx.recv() {
                    if event_tx.send(NodeEvent::Clock(ev)).is_err() {
                        return;
                    }
                }
                let _ = event_tx.send(NodeEvent::ClockClosed);
            })?;
        }

        let coord_shared = shared.clone();
        let coordinator = std::thread::Builder::new().name("posq-coordinator".into()).spawn(move || {
            run_coordinator(
                coord_shared, cfg, identity, host, event_rx, event_tx, seed_tx, genesis_entropy,
            )
        })?;

        Ok(Self { shared, coordinator: Some(coordinator) })
    }

    /// Wait for the coordinator to finish (only with max_segments set).
    pub fn join(mut self) -> Arc<Shared> {
        if let Some(h) = self.coordinator.take() {
            let _ = h.join();
        }
        self.shared
    }
}

fn entropy_for(epoch: u64, segment: u64) -> [u8; 32] {
    // External freshness stand-in: production wires the Observer beacon and
    // the latest anchored L1 block hash here (§4.4); the stated degradation
    // of a weaker source is a wider precomputation bound, not an ordering
    // risk. Published in each SegmentSeal for watcher verification.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    keccak256(&[b"posq-local-entropy".as_slice(), &epoch.to_be_bytes(), &segment.to_be_bytes(), &now.to_be_bytes()].concat())
}

fn wave_binding(epoch: u64, mature_tick: u64, class: u8) -> Vec<u8> {
    let mut m = Vec::with_capacity(32);
    m.extend_from_slice(b"posq-wave");
    m.extend_from_slice(&epoch.to_be_bytes());
    m.extend_from_slice(&mature_tick.to_be_bytes());
    m.push(class);
    m
}

#[allow(clippy::too_many_arguments)]
fn run_coordinator(
    shared: Arc<Shared>,
    cfg: NodeConfig,
    identity: Identity,
    host: Arc<dyn HostClient>,
    events: mpsc::Receiver<NodeEvent>,
    event_tx: mpsc::Sender<NodeEvent>,
    seeds: mpsc::Sender<SeedInput>,
    genesis_entropy: [u8; 32],
) {
    let params = &shared.params;
    let epoch = params.epoch;
    let t0 = std::time::Instant::now();
    let mut c_prev = log_genesis(epoch);
    // Waves waiting to mature: (mature_tick, class) → items.
    let mut waves: BTreeMap<(u64, u8), Vec<WaveItem>> = BTreeMap::new();
    // Segment ends seen / proofs pending, for shutdown accounting.
    let mut clock_closed = false;
    let mut proofs_pending: u64 = 0;
    let mut solver_outstanding: u64 = 0;
    // Entropy issued per segment, bound into that segment's seal.
    let mut entropies: BTreeMap<u64, [u8; 32]> = BTreeMap::new();
    entropies.insert(0, genesis_entropy);
    // (y_s, x_end) stashed at SegmentEnd, consumed when the proof arrives.
    let mut seal_bases: BTreeMap<u64, (BigUint, BigUint)> = BTreeMap::new();
    let mut next_anchor_after_segment = params.anchor_every_segments.saturating_sub(1);
    let mut fence_active = false;

    loop {
        let Ok(ev) = events.recv() else { break };
        match ev {
            NodeEvent::Clock(ClockEvent::Tick { segment, tick, x }) => {
                let x_hash = hash_group_element(&x);
                let (entries, receipts, batch_entries) = {
                    let mut adm = shared.admission.lock().unwrap();
                    let entries = adm.take_tick(tick);
                    let receipts: Vec<_> = entries.iter().map(|e| e.receipt.clone()).collect();
                    let batch_entries: Vec<BatchEntry> = entries
                        .iter()
                        .map(|e| BatchEntry {
                            tick: e.tick,
                            pos: e.pos,
                            h: e.h,
                            bucket: e.bucket,
                            ticket_id: e.ticket_id,
                            receipt_sig: e.receipt.sig,
                        })
                        .collect();
                    (entries, receipts, batch_entries)
                };
                let leaves: Vec<[u8; 32]> = batch_entries.iter().map(|e| e.leaf()).collect();
                let batch_root = merkle_root(&leaves);
                let c_t = log_step(&c_prev, tick, &batch_root, &x_hash);
                let msg = TickRecord::signing_message(
                    epoch, segment, tick, &x_hash, &c_prev, &c_t, &batch_root, &batch_root,
                );
                let record = TickRecord {
                    epoch,
                    segment,
                    tick,
                    x,
                    c_prev,
                    c_t,
                    batch_root,
                    da_ref: batch_root,
                    sig: identity.sign(&msg),
                };
                {
                    let mut store = shared.store.lock().unwrap();
                    if let Err(e) = store.insert_tick(SealedTick {
                        record: record.clone(),
                        entries: batch_entries,
                        receipts,
                    }) {
                        warn!("DA write failed for tick {tick}: {e}");
                    }
                    // Tape positions + wave scheduling.
                    for e in &entries {
                        let d_ticks = params.delay_ticks(e.delay_class).unwrap_or(u64::MAX);
                        let mature_at = tick + d_ticks;
                        let p = TapePosition {
                            tick: e.tick,
                            pos: e.pos,
                            h: e.h,
                            bucket: e.bucket,
                            delay_class: e.delay_class,
                            outcome: Outcome::Pending { mature_at },
                        };
                        store.add_position(p.clone());
                        let _ = shared.tape_tx.send(p);
                        waves.entry((mature_at, e.delay_class)).or_default().push(WaveItem {
                            tick: e.tick,
                            pos: e.pos,
                            namespace: e.envelope.namespace,
                            ct: e.envelope.ciphertext.clone(),
                            aad: e.envelope_bytes[..crate::envelope::HEADER_AAD_LEN].to_vec(),
                        });
                    }
                }
                // Advance admission to the new sealed state.
                shared.admission.lock().unwrap().advance_open(tick, x_hash, c_t);
                c_prev = c_t;

                // Timeliness (Definition 5.1/5.2): late ticks do not count —
                // fence overlapping windows.
                let deadline_micros = tick * params.delta_micros + params.gamma_ticks * params.delta_micros;
                let late = cfg.induce_late_tick == Some(tick)
                    || (cfg.enforce_timeliness
                        && t0.elapsed().as_micros() as u64 > deadline_micros);
                if late {
                    let released = shared.admission.lock().unwrap().fence(tick);
                    fence_active = true;
                    let mut status = shared.status.lock().unwrap();
                    status.cadence_faults.push(tick);
                    warn!(
                        "cadence fault at tick {tick}: fenced, released {} reservations",
                        released.len()
                    );
                } else if fence_active {
                    shared.admission.lock().unwrap().lift_fence();
                    fence_active = false;
                }

                // Dispatch matured waves to solver threads.
                let due: Vec<(u64, u8)> =
                    waves.range(..=(tick, u8::MAX)).map(|(k, _)| *k).collect();
                for key in due {
                    let items = waves.remove(&key).unwrap();
                    let (mature_tick, class) = key;
                    let tlk = shared.tlk.clone();
                    let tx = event_tx.clone();
                    solver_outstanding += 1;
                    std::thread::spawn(move || {
                        let cts: Vec<Ciphertext> = items.iter().map(|i| i.ct.clone()).collect();
                        let binding = wave_binding(epoch, mature_tick, class);
                        let solution = batch_solve::solve_wave(&tlk, class, &binding, &cts)
                            .map_err(|e| e.to_string());
                        let _ = tx.send(NodeEvent::Wave { mature_tick, class, items, solution });
                    });
                }

                // Execution cutoff: unresolved past u_mat + G become gaps.
                {
                    let mut store = shared.store.lock().unwrap();
                    for (t, j) in store.overdue_positions(tick, params.grace_ticks()) {
                        store.set_outcome(t, j, Outcome::Gap { reason: GapReason::Unopened });
                        if let Some(p) = store.tape.get(&(t, j)) {
                            let _ = shared.tape_tx.send(p.clone());
                        }
                        shared.status.lock().unwrap().gaps += 1;
                    }
                }

                shared.current_tick.store(tick, Ordering::Relaxed);
                {
                    let mut status = shared.status.lock().unwrap();
                    status.current_tick = tick;
                    status.current_segment = segment;
                    status.admitted += entries.len() as u64;
                }
                let _ = shared.record_tx.send(record);
            }

            NodeEvent::Clock(ClockEvent::SegmentEnd { segment, y, x_end }) => {
                proofs_pending += 1;
                seal_bases.insert(segment, (y, x_end));
                let next_entropy = entropy_for(epoch, segment + 1);
                entropies.insert(segment + 1, next_entropy);
                // Rendezvous: hand the clock C_end(s) and fresh entropy.
                let _ = seeds.send(SeedInput { c_end: c_prev, entropy: next_entropy });
            }

            NodeEvent::Clock(ClockEvent::SegmentProof { segment, proof }) => {
                proofs_pending = proofs_pending.saturating_sub(1);
                let entropy = entropies.get(&segment).copied().unwrap_or_default();
                if let Some((y, x_end)) = seal_bases.remove(&segment) {
                    debug_assert_eq!(proof.y, x_end);
                    let msg = SegmentSeal::signing_message(
                        epoch,
                        segment,
                        &entropy,
                        &hash_group_element(&y),
                        &hash_group_element(&x_end),
                    );
                    let seal = SegmentSeal {
                        epoch,
                        segment,
                        entropy,
                        y,
                        x_end,
                        proof,
                        sig: identity.sign(&msg),
                    };
                    let mut store = shared.store.lock().unwrap();
                    let _ = store.insert_segment(seal);
                    drop(store);
                    maybe_anchor(&shared, &identity, &host, &mut next_anchor_after_segment);
                }
            }

            NodeEvent::Wave { mature_tick, class, items, solution } => {
                solver_outstanding = solver_outstanding.saturating_sub(1);
                match solution {
                    Ok(sol) => {
                        apply_wave(&shared, mature_tick, class, &items, &sol);
                        shared.waves.lock().unwrap().push(StoredWave {
                            mature_tick,
                            class,
                            positions: items.iter().map(|i| (i.tick, i.pos)).collect(),
                            solution: sol,
                        });
                    }
                    Err(e) => warn!("wave solve failed (mature {mature_tick}, class {class}): {e}"),
                }
            }

            NodeEvent::ClockClosed => {
                clock_closed = true;
            }
        }

        // Waves maturing beyond the final tick can never dispatch once the
        // clock has closed; they resolve as gaps via the final cutoff pass.
        if clock_closed && proofs_pending == 0 && solver_outstanding == 0 {
            // Final cutoff pass so trailing pendings resolve deterministically.
            let final_tick = shared.current_tick.load(Ordering::Relaxed);
            let mut store = shared.store.lock().unwrap();
            for (t, j) in store.overdue_positions(final_tick, params.grace_ticks()) {
                store.set_outcome(t, j, Outcome::Gap { reason: GapReason::Unopened });
            }
            drop(store);
            shared.status.lock().unwrap().finished = true;
            break;
        }
    }
}

fn apply_wave(shared: &Arc<Shared>, _mature_tick: u64, _class: u8, items: &[WaveItem], sol: &WaveSolution) {
    let mut store = shared.store.lock().unwrap();
    for (item, w) in items.iter().zip(sol.witnesses.iter()) {
        let outcome = match vdf::tlk::open(&item.ct, w, &item.aad) {
            Err(_) => Outcome::Gap { reason: GapReason::Undecryptable },
            Ok(plaintext) => match Intent::from_bytes(&plaintext) {
                Some(intent)
                    if intent.verify()
                        && intent.namespace == item.namespace
                        && intent.expiry_tick >= item.tick =>
                {
                    Outcome::Opened { intent }
                }
                _ => Outcome::Gap { reason: GapReason::InvalidIntent },
            },
        };
        let is_gap = matches!(outcome, Outcome::Gap { .. });
        store.set_outcome(item.tick, item.pos, outcome);
        if let Some(p) = store.tape.get(&(item.tick, item.pos)) {
            let _ = shared.tape_tx.send(p.clone());
        }
        let mut status = shared.status.lock().unwrap();
        if is_gap {
            status.gaps += 1;
        } else {
            status.opened += 1;
        }
    }
}

/// Build any anchor whose span of segments is fully sealed.
fn maybe_anchor(
    shared: &Arc<Shared>,
    identity: &Identity,
    host: &Arc<dyn HostClient>,
    next_anchor_after_segment: &mut u64,
) {
    let params = &shared.params;
    loop {
        let span_end = *next_anchor_after_segment;
        let span_start = span_end + 1 - params.anchor_every_segments;
        let store = shared.store.lock().unwrap();
        let seals: Vec<SegmentSeal> = (span_start..=span_end)
            .filter_map(|s| store.segments.get(&s).cloned())
            .collect();
        if seals.len() as u64 != params.anchor_every_segments {
            return; // span not fully sealed yet
        }
        let first_tick = span_start * params.segment_ticks + 1;
        let last_tick = (span_end + 1) * params.segment_ticks;
        let Some(last_record) = store.ticks.get(&last_tick).map(|s| s.record.clone()) else {
            return;
        };
        let c_before = store
            .ticks
            .get(&first_tick)
            .map(|s| s.record.c_prev)
            .unwrap_or_else(|| log_genesis(params.epoch));
        // Receipts root: per-tick final digest, in tick order.
        let mut receipt_leaves = Vec::new();
        for t in first_tick..=last_tick {
            if let Some(sealed) = store.ticks.get(&t) {
                let d_last = sealed
                    .receipts
                    .last()
                    .map(|r| r.d)
                    .unwrap_or_else(|| crate::records::digest_genesis(params.epoch, t));
                receipt_leaves.push(keccak256(&[&t.to_be_bytes()[..], &d_last].concat()));
            }
        }
        let receipts_root = merkle_root(&receipt_leaves);
        let da_attestation = store.da().attest_span(first_tick..=last_tick);
        // Transcript validity over the span (ZK backend slots in here).
        let ticks_fn = |t: u64| store.ticks.get(&t).cloned();
        let transcript = match ReplayProver.prove(
            params.epoch,
            &shared.sequencer_address,
            c_before,
            first_tick,
            last_tick,
            &ticks_fn,
        ) {
            Ok((_, proof)) => proof,
            Err(v) => {
                warn!("transcript predicate failed for anchor span: {v:?}");
                return;
            }
        };
        let x_b_hash = hash_group_element(&seals.last().unwrap().x_end);
        drop(store);

        let anchor = Anchor {
            epoch: params.epoch,
            first_segment: span_start,
            last_segment: span_end,
            first_tick,
            last_tick,
            x_b_hash,
            c_b: last_record.c_t,
            segment_seals: seals,
            receipts_root,
            da_attestation,
            transcript,
            sig: crate::identity::Sig65([0u8; 65]),
        }
        .sign(identity);
        if let Err(e) = host.post_anchor(&anchor) {
            warn!("anchor post failed: {e}");
        }
        shared.anchors.lock().unwrap().push(anchor);
        *next_anchor_after_segment += params.anchor_every_segments;
    }
}

/// Convenience constructor for tests and the binary: mock host.
pub fn start_dev_node(max_segments: u64) -> anyhow::Result<SequencerNode> {
    SequencerNode::start(NodeConfig::dev(max_segments), Arc::new(MockHost::default()))
}
