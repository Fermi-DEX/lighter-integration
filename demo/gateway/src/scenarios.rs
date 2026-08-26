//! Scripted scenarios — the demo's spine. Each returns a structured result
//! the dashboard renders as a before/after or an evidence object. The
//! on-chain ones (fraud, forced inclusion) produce ready-to-submit calldata
//! matching PoSqHost.sol; submission is done by the browser wallet or the
//! `post-proof` helper, so the gateway needs no signing key for them.

use crate::lighter_sim::{Book, ExecResult, LighterTx, OrderKind, Side};
use crate::state::{now_ms, App};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use sequencer::identity::Identity;
use sequencer::records::{SignRecords, TickRecord};
use serde_json::{json, Value};
use std::sync::Arc;

fn acct(b: u8) -> [u8; 20] {
    [b; 20]
}
fn hx(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// A self-contained book replay proving the property, independent of live
/// traffic (so the demo is deterministic on click).
fn scenario_frontrun() -> Value {
    // Victim posts a resting ask; an attacker who could see plaintext would
    // insert a buy ahead of the victim's own market buy to lift liquidity
    // first. Under blind FIFO the attacker only sees a ciphertext, so its
    // position is whatever the tape assigned — it cannot jump the queue.
    let recipe = |blind: bool| {
        let mut book = Book::default();
        // resting liquidity
        book.execute(acct(9), &LighterTx::Create { market: 0, side: Side::Ask, kind: OrderKind::Limit, price: 100, size: 10, coi: 1 }, 1, 0, 0);
        book.execute(acct(9), &LighterTx::Create { market: 0, side: Side::Ask, kind: OrderKind::Limit, price: 101, size: 10, coi: 2 }, 1, 1, 0);
        // Tape order (what PoSq fixed): victim buy at pos 2, then attacker at pos 3.
        // Plaintext-FCFS attacker, having seen the victim's buy, front-runs to pos 2.5
        // (modelled by swapping execution order).
        let mut trades = vec![];
        let mut push = |bk: &mut Book, a: u8, size: u64, coi: u64, tick_pos: (u64, u32)| {
            if let ExecResult::Applied { trades: t, .. } = bk.execute(
                acct(a),
                &LighterTx::Create { market: 0, side: Side::Bid, kind: OrderKind::Market, price: 0, size, coi },
                tick_pos.0, tick_pos.1, 0,
            ) {
                for tr in t {
                    trades.push(json!({"taker": tr.taker, "price": tr.price, "size": tr.size}));
                }
            }
        };
        if blind {
            push(&mut book, 5, 10, 10, (1, 2)); // victim first (tape order)
            push(&mut book, 7, 10, 11, (1, 3)); // attacker second
        } else {
            push(&mut book, 7, 10, 11, (1, 2)); // attacker front-ran
            push(&mut book, 5, 10, 10, (1, 3)); // victim gets worse price
        }
        trades
    };
    json!({
        "title": "Front-run attempt fails under blind admission",
        "blind_posq": {
            "note": "PoSq order is fixed on ciphertext; victim keeps the best fill",
            "fills": recipe(true),
        },
        "plaintext_fcfs": {
            "note": "sequencer sees plaintext; attacker inserts ahead, victim pays 101",
            "fills": recipe(false),
        },
        "verdict": "Victim's average fill price is strictly better under PoSq blind FIFO.",
    })
}

fn scenario_cancel_priority() -> Value {
    let outcome = |cancel_first: bool| {
        let mut book = Book::default();
        book.execute(acct(9), &LighterTx::Create { market: 0, side: Side::Ask, kind: OrderKind::Limit, price: 100, size: 10, coi: 1 }, 1, 0, 0);
        let mut sniped = 0u64;
        let mut run = |bk: &mut Book, seq: &[(&str, u8, u32)]| {
            for (kind, a, pos) in seq {
                match *kind {
                    "cancel" => {
                        bk.execute(acct(*a), &LighterTx::Cancel { market: 0, coi: 1 }, 1, *pos as u32, 0);
                    }
                    _ => {
                        if let ExecResult::Applied { trades, .. } = bk.execute(
                            acct(*a),
                            &LighterTx::Create { market: 0, side: Side::Bid, kind: OrderKind::Market, price: 0, size: 10, coi: 2 },
                            1, *pos as u32, 0,
                        ) {
                            sniped += trades.iter().map(|t| t.size).sum::<u64>();
                        }
                    }
                }
            }
        };
        if cancel_first {
            run(&mut book, &[("cancel", 9, 1), ("take", 7, 2)]);
        } else {
            run(&mut book, &[("take", 7, 1), ("cancel", 9, 2)]);
        }
        sniped
    };
    json!({
        "title": "Cancel priority — stale-quote sniping is structurally removed",
        "cancel_wins_race_blind": {
            "sniped_size": outcome(true),
            "note": "cancel is envelope-indistinguishable from a trade, so it races fairly; quote pulled, nothing sniped",
        },
        "cancel_loses_plaintext": {
            "sniped_size": outcome(false),
            "note": "if the sequencer can prioritize the taker (seeing op-type), the maker's stale quote is picked off",
        },
        "verdict": "Under PoSq the market maker's cancel gets the same blind FIFO treatment as any trade.",
    })
}

/// Accountable admission: force a rejection and a full-window, return both as
/// signed objects the dashboard verifies client-side. Uses the live node's
/// admission so the sequencer's real key signs them.
fn scenario_accountable(app: &App) -> Value {
    let node = app.node.read().unwrap().clone();
    let Some(shared) = node else { return json!({"error": "node not ready"}) };
    // A malformed (wrong-size) envelope → signed rejection.
    let store = shared.store.lock().unwrap();
    let da = store.da();
    let mut adm = shared.admission.lock().unwrap();
    let bad = vec![0u8; 16]; // wrong size
    let rej = adm.submit(&bad, da);
    drop(adm);
    drop(store);
    let rej_json = match rej {
        sequencer::records::AdmissionOutcome::Rejected(r) => crate::dto::rejection_json(&r),
        other => json!({"unexpected": format!("{other:?}")}),
    };
    json!({
        "title": "Accountable admission — no silent outcomes (Definition 6.1)",
        "rejection": rej_json,
        "note": "Every syntactically-checked submission yields a signed receipt, signed rejection, or signed full-window. 'My order vanished' becomes cryptographic evidence. Verify the signature in your browser (panel recovers the sequencer address).",
        "sequencer_address": hx(&shared.sequencer_address),
    })
}

/// Fabricate a reorder and build PoSqHost.proveReorder calldata against it.
/// We produce two *conflicting* signed tick records style demonstration: a
/// receipt for h at (t,j) plus a signed batch that places a different entry
/// at j. Signed by a throwaway "malicious sequencer" identity so the demo is
/// self-contained; the sacrificial PoSqHost is deployed with that address.
fn scenario_fraud_reorder(_app: &App) -> Value {
    use sequencer::envelope::Bucket;
    use sequencer::records::{
        digest_genesis, digest_step, log_genesis, merkle_path, merkle_root, BatchEntry, Receipt,
    };
    let evil = Identity::from_seed(b"demo-malicious-sequencer");
    let epoch = 0u64;
    let tick = 7u64;
    let pos = 0u32;
    // The honest receipt the user holds: commitment h_user at (7,0).
    let h_user = sequencer::identity::keccak256(b"user-commitment");
    let ticket_id = [3u8; 16];
    let d_prev = digest_genesis(epoch, tick);
    let d = digest_step(&d_prev, tick, pos, &h_user, Bucket::Majors, &ticket_id);
    let c_prev = log_genesis(epoch);
    let window = sequencer::envelope::Window { start: 6, len: 4 };
    let rmsg = Receipt::signing_message(
        epoch, tick, pos, &h_user, Bucket::Majors, window, &ticket_id,
        &[0u8; 32], &c_prev, &d_prev, &d,
    );
    let receipt = Receipt {
        epoch, tick, pos, h: h_user, bucket: Bucket::Majors, window,
        ticket_id, x_prev_hash: [0u8; 32], c_prev, d_prev, d,
        sig: evil.sign(&rmsg),
    };
    // The malicious batch: a DIFFERENT commitment placed at (7,0).
    let h_evil = sequencer::identity::keccak256(b"attacker-inserted");
    let conflicting = BatchEntry {
        tick, pos, h: h_evil, bucket: Bucket::Majors, ticket_id,
        receipt_sig: sequencer::identity::Sig65([0u8; 65]),
    };
    let leaves = vec![conflicting.leaf()];
    let batch_root = merkle_root(&leaves);
    let path = merkle_path(&leaves, 0).unwrap_or_default();
    let x_hash = sequencer::records::hash_group_element(&num_bigint::BigUint::from(1u32));
    let c_t = sequencer::records::log_step(&c_prev, tick, &batch_root, &x_hash);
    let trmsg = TickRecord::signing_message(epoch, 0, tick, &x_hash, &c_prev, &c_t, &batch_root, &batch_root);
    let (rec_sig, evil_addr) = evil.sign_message_addr(&trmsg);

    json!({
        "title": "On-chain fraud proof — reorder detected and slashable",
        "explanation": "The user holds a signed receipt placing commitment h_user at (tick 7, pos 0). The malicious sequencer publishes a signed batch that places a DIFFERENT commitment at (7,0). PoSqHost.proveReorder verifies both signatures, the merkle inclusion of the conflicting entry, and slashes the bond.",
        "malicious_sequencer": hx(&evil_addr),
        "receipt": crate::dto::receipt_json(&receipt),
        "calldata": {
            "target": "PoSqHost(sacrificial).proveReorder",
            "receipt": {
                "tick": tick, "pos": pos, "h": hx(&h_user), "bucket": 0u8,
                "windowStart": window.start, "windowLen": window.len,
                "ticketId": hx(&ticket_id), "xPrevHash": hx(&[0u8;32]),
                "cPrev": hx(&c_prev), "dPrev": hx(&d_prev), "d": hx(&d),
            },
            "receiptSig": hx(receipt.sig.as_bytes()),
            "record": {
                "segment": 0u64, "tick": tick, "xHash": hx(&x_hash),
                "cPrev": hx(&c_prev), "cT": hx(&c_t),
                "batchRoot": hx(&batch_root), "daRef": hx(&batch_root),
            },
            "recordSig": hx(rec_sig.as_bytes()),
            "conflicting": {
                "tick": tick, "pos": pos, "h": hx(&h_evil), "bucket": 0u8,
                "ticketId": hx(&ticket_id),
                "receiptSigHash": hx(&sequencer::identity::keccak256(&[0u8;65])),
            },
            "path": path.iter().map(|p| hx(p)).collect::<Vec<_>>(),
        },
        "note": "This calldata is ready to submit to the sacrificial PoSqHost (whose configured sequencer == the malicious address above). On success it emits FraudProven + RescueMode and pays the challenger a bounty. The MAIN demo host is unaffected.",
    })
}

fn scenario_forced_inclusion() -> Value {
    let h = sequencer::identity::keccak256(b"forced-envelope-demo");
    json!({
        "title": "Forced inclusion — censorship resistance is a slashing condition",
        "flow": [
            "1. User calls PoSqHost.forceInclude(h) on-chain (envelope posted as calldata).",
            "2. Sequencer must exhibit a receipt for h within F_force ticks via dischargeForced.",
            "3. If it fails, anyone calls proveForcedDefault → bond slashed.",
        ],
        "commitment_hash": hx(&h),
        "note": "This upgrades Lighter's 'include priority tx by deadline' from a freeze trigger to a slashing condition, while asset-exit escape-hatch stays as the deeper backstop.",
    })
}

fn scenario_stream_mismatch(app: &App) -> Value {
    // Pull the newest *non-empty* sealed block and show that flipping one byte
    // of its last tx changes the stream commitment — i.e. any batch ≠ tape is
    // detectable.
    let sim = app.sim.lock().unwrap();
    let Some(idx) = sim.blocks.iter().rposition(|b| !b.txs.is_empty()) else {
        return json!({"error": "no transactions in any block yet — let traffic run a few seconds"});
    };
    let blk = &sim.blocks[idx];
    let honest = blk.stream_commitment.clone();
    let t = blk.txs.last().expect("non-empty by rposition");
    let decode32 = |h: &str| -> Option<[u8; 32]> { hex::decode(h).ok()?.try_into().ok() };
    // Stream state *before* the last tx: the previous tx's post-state, else the
    // commitment carried into this block, else the epoch's stream genesis
    // (blocks restart at tick 1 on epoch rotation).
    let s_prev = if blk.txs.len() >= 2 {
        decode32(&blk.txs[blk.txs.len() - 2].stream_after)
    } else if idx > 0 {
        decode32(&sim.blocks[idx - 1].stream_commitment)
    } else if blk.first_tick == 1 {
        Some(crate::lighter_sim::stream_genesis(blk.epoch, 1))
    } else {
        None
    };
    let Some(s_prev) = s_prev else {
        return json!({"error": "cannot determine the pre-state of the last tx — click again"});
    };
    let payload = hex::decode(&t.payload_hex).unwrap_or_default();
    // Self-check: replaying the honest payload from s_prev must land exactly on
    // the tx's recorded post-state (which the block commitment extends).
    let recomputed_honest = crate::lighter_sim::stream_step(&s_prev, t.tick, t.pos, &payload);
    let honest_matches = hex::encode(recomputed_honest) == t.stream_after;
    let mut tampered_payload = payload;
    if let Some(b) = tampered_payload.last_mut() {
        *b ^= 0x01;
    }
    let tampered =
        crate::lighter_sim::stream_step(&s_prev, t.tick, t.pos, &tampered_payload);
    json!({
        "title": "Stream equality — any batch ≠ tape is detectable (B2)",
        "block": blk.number,
        "tampered_tx": { "tick": t.tick, "pos": t.pos },
        "stream_prev": hx(&s_prev),
        "recomputed_honest_step": hx(&recomputed_honest),
        "honest_recomputation_matches": honest_matches,
        "honest_stream_commitment": honest,
        "tampered_stream_commitment": hx(&tampered),
        "note": "The opened-stream commitment is recomputable by anyone from DA + reveals. A sequencer that reorders or drops even one tx produces a different commitment than the tape implies — the on-chain challenge (challengeStream) fires.",
    })
}

pub async fn run_scenario(State(app): State<Arc<App>>, Path(name): Path<String>) -> impl IntoResponse {
    let result = match name.as_str() {
        "frontrun" => scenario_frontrun(),
        "cancel-priority" => scenario_cancel_priority(),
        "accountable" => scenario_accountable(&app),
        "fraud-reorder" => scenario_fraud_reorder(&app),
        "forced-inclusion" => scenario_forced_inclusion(),
        "stream-mismatch" => scenario_stream_mismatch(&app),
        _ => json!({"error": format!("unknown scenario: {name}")}),
    };
    let entry = json!({"at_ms": now_ms(), "name": name, "result": result});
    {
        let mut log = app.scenario_log.lock().unwrap();
        if log.len() >= 64 {
            log.pop_front();
        }
        log.push_back(entry.clone());
    }
    app.emit(json!({"type": "scenario", "data": &entry}));
    Json(entry)
}
