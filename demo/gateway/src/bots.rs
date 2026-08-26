//! Traffic bots: makers maintaining two-sided quotes (place → cancel →
//! replace) and takers crossing the book — all through the blind envelope
//! path, so the tape sees only ciphertexts. Submission cadence is bursty on
//! purpose: envelopes admitted in the same tick share a maturity wave, which
//! is what the batched solver amortizes.

use crate::lighter_sim::{LighterTx, OrderKind, Side, SimState, LIGHTER_NAMESPACE};
use crate::state::{now_ms, App, SubmitLog};
use rand::Rng;
use sequencer::client::{build_envelope, EnvelopeRequest};
use sequencer::envelope::{Bucket, Window};
use sequencer::identity::Identity;
use sequencer::node::Shared;
use sequencer::records::AdmissionOutcome;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

pub struct Bot {
    pub name: String,
    pub identity: Identity,
    pub intent_nonce: u64,
    pub next_coi: u64,
    pub live_quotes: Vec<u64>, // cois currently believed resting
}

impl Bot {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            identity: Identity::from_seed(name.as_bytes()),
            intent_nonce: 1,
            next_coi: 1,
            live_quotes: Vec::new(),
        }
    }
}

/// Build and submit one envelope; record the accountable outcome. Returns
/// the assigned (tick, pos) when admitted.
pub fn submit_tx(app: &App, shared: &Arc<Shared>, bot: &mut Bot, tx: &LighterTx) -> Option<(u64, u32)> {
    let payload = serde_json::to_vec(tx).expect("tx json");
    submit_payload(app, shared, &bot.name, &bot.identity, &mut bot.intent_nonce, payload)
}

pub fn submit_payload(
    app: &App,
    shared: &Arc<Shared>,
    bot_name: &str,
    identity: &Identity,
    intent_nonce: &mut u64,
    payload: Vec<u8>,
) -> Option<(u64, u32)> {
    let params = &shared.params;
    let cur = shared.current_tick.load(Ordering::Relaxed).max(1);
    let window = Window { start: cur, len: params.window_ticks.min(10) };
    let ticket = shared.faucet.lock().unwrap().mint(0, Bucket::Majors);
    let mut rng = rand::thread_rng();
    let req = EnvelopeRequest {
        signer: identity,
        ticket,
        bucket: Bucket::Majors,
        namespace: LIGHTER_NAMESPACE,
        window,
        delay_class: 0,
        nonce: rng.gen::<[u8; 16]>(),
        intent_nonce: *intent_nonce,
        expiry_tick: cur + 2_000,
        payload,
        kem_seed: rng.gen::<[u8; 32]>().to_vec(),
    };
    *intent_nonce += 1;
    let bytes = match build_envelope(&shared.tlk, params.envelope_bytes, req) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("envelope build failed: {e}");
            return None;
        }
    };
    let start = std::time::Instant::now();
    let outcome = {
        let store = shared.store.lock().unwrap();
        let da = store.da();
        let mut adm = shared.admission.lock().unwrap();
        adm.submit(&bytes, da)
    };
    let latency_us = start.elapsed().as_micros() as u64;

    let (kind, tick, pos, h, outcome_json) = match &outcome {
        AdmissionOutcome::Admitted(r) => (
            "admitted".to_string(),
            r.tick,
            r.pos,
            crate::dto::hx(&r.h),
            crate::dto::receipt_json(r),
        ),
        AdmissionOutcome::Rejected(r) => (
            format!("rejected:{:?}", r.reason),
            0,
            0,
            crate::dto::hx(&r.h),
            crate::dto::rejection_json(r),
        ),
        AdmissionOutcome::FullWindow(f) => (
            "full-window".to_string(),
            0,
            0,
            String::new(),
            crate::dto::full_window_json(f),
        ),
    };
    let log = SubmitLog {
        at_ms: now_ms(),
        kind: kind.clone(),
        latency_us,
        tick,
        pos,
        h,
        bot: bot_name.to_string(),
        outcome: outcome_json,
        envelope_hex: Some(hex::encode(&bytes)),
    };
    app.emit(serde_json::json!({"type": "submit", "data": &log}));
    app.metrics.lock().unwrap().record(log);
    match outcome {
        AdmissionOutcome::Admitted(r) => Some((r.tick, r.pos)),
        _ => None,
    }
}

fn mid_price(sim: &Arc<Mutex<SimState>>) -> u64 {
    let s = sim.lock().unwrap();
    let book = &s.book;
    match (book.best_bid(), book.best_ask()) {
        (Some(b), Some(a)) => (b + a) / 2,
        _ => book.last_trade_price.unwrap_or(25_000),
    }
}

async fn sleep_jitter(base_ms: u64) {
    let j = rand::thread_rng().gen_range(0..base_ms / 2 + 1);
    tokio::time::sleep(std::time::Duration::from_millis(base_ms + j)).await;
}

pub async fn run_maker(
    app: Arc<App>,
    shared: Arc<Shared>,
    sim: Arc<Mutex<SimState>>,
    name: String,
    stop: tokio::sync::watch::Receiver<bool>,
) {
    let mut bot = Bot::new(&name);
    loop {
        if *stop.borrow() {
            return;
        }
        let mid = mid_price(&sim) as i64;
        let (half_spread, bid_size, ask_size, drift) = {
            let mut rng = rand::thread_rng();
            (
                rng.gen_range(2..7) as i64,             // half-spread in steps
                rng.gen_range(500..3000) as u64,        // bid size
                rng.gen_range(500..3000) as u64,        // ask size
                rng.gen_range(-3..=3) as i64,           // drift
            )
        };
        // Cancel-replace: cancels race through the same blind FIFO (the
        // cancel-priority property the spec sells).
        let stale: Vec<u64> = bot.live_quotes.drain(..).collect();
        for coi in stale {
            let tx = LighterTx::Cancel { market: 0, coi };
            submit_tx(&app, &shared, &mut bot, &tx);
        }
        let center = (mid + drift).max(100);
        let (bid_px, ask_px) = ((center - half_spread) as u64, (center + half_spread) as u64);
        for (side, price, size) in [(Side::Bid, bid_px, bid_size), (Side::Ask, ask_px, ask_size)] {
            let coi = bot.next_coi;
            bot.next_coi += 1;
            let tx = LighterTx::Create { market: 0, side, kind: OrderKind::Limit, price, size, coi };
            if submit_tx(&app, &shared, &mut bot, &tx).is_some() {
                bot.live_quotes.push(coi);
            }
        }
        sleep_jitter(2_000).await;
    }
}

pub async fn run_taker(
    app: Arc<App>,
    shared: Arc<Shared>,
    _sim: Arc<Mutex<SimState>>,
    name: String,
    stop: tokio::sync::watch::Receiver<bool>,
) {
    let mut bot = Bot::new(&name);
    loop {
        if *stop.borrow() {
            return;
        }
        let (side, size) = {
            let mut rng = rand::thread_rng();
            (
                if rng.gen_bool(0.5) { Side::Bid } else { Side::Ask },
                rng.gen_range(300..2000) as u64,
            )
        };
        let coi = bot.next_coi;
        bot.next_coi += 1;
        let tx = LighterTx::Create { market: 0, side, kind: OrderKind::Market, price: 0, size, coi };
        submit_tx(&app, &shared, &mut bot, &tx);
        sleep_jitter(2_600).await;
    }
}
