//! REST + SSE surface for the dashboard. Everything the dashboard displays
//! is fetchable here in byte-faithful form; the page re-derives hashes,
//! signature recoveries, merkle roots, and stream commitments client-side.

use crate::dto::{self, hx};
use crate::state::App;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

#[derive(Deserialize)]
pub struct Limit {
    limit: Option<usize>,
    from: Option<u64>,
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/params", get(params))
        .route("/api/status", get(status))
        .route("/api/tape", get(tape))
        .route("/api/ticks", get(ticks))
        .route("/api/blocks", get(blocks))
        .route("/api/book", get(book))
        .route("/api/trades", get(trades))
        .route("/api/submits", get(submits))
        .route("/api/segments", get(segments))
        .route("/api/anchors", get(anchors))
        .route("/api/scenario/{name}", post(crate::scenarios::run_scenario))
        .route("/api/scenario-log", get(scenario_log))
        .route("/api/events", get(events))
        .with_state(app)
}

async fn params(State(app): State<Arc<App>>) -> impl IntoResponse {
    let node = app.node.read().unwrap().clone();
    let Some(shared) = node else {
        return Json(json!({"ready": false}));
    };
    let p = &shared.params;
    let eth = app.eth.lock().unwrap().clone();
    Json(json!({
        "ready": true,
        "epoch": p.epoch,
        "delta_micros": p.delta_micros,
        "q_squarings": p.q_squarings,
        "segment_ticks": p.segment_ticks,
        "window_ticks": p.window_ticks,
        "gamma_ticks": p.gamma_ticks,
        "delay_classes": p.delay_classes,
        "delay_ticks_class0": p.delay_ticks(0),
        "grace_ticks": p.grace_ticks(),
        "envelope_bytes": p.envelope_bytes,
        "capacity_per_tick_per_bucket": p.capacity_per_tick_per_bucket,
        "anchor_every_segments": p.anchor_every_segments,
        "mandated_rate_sq_per_s": p.mandated_rate(),
        "min_delay_for_window": p.min_delay_squarings(p.window_ticks),
        "sequencer_address": hx(&shared.sequencer_address),
        // RSA-2048 group modulus (256-byte big-endian) so the browser can
        // verify Wesolowski segment proofs from /api/segments itself.
        "modulus_n": hx(&vdf::posq::pad256(&vdf::posq::Group::default_rsa2048().modulus)),
        "block_ticks": app.cfg.block_ticks,
        "namespace": crate::lighter_sim::LIGHTER_NAMESPACE,
        "eth": eth,
    }))
}

async fn status(State(app): State<Arc<App>>) -> impl IntoResponse {
    Json(crate::status_json(&app))
}

async fn tape(State(app): State<Arc<App>>, Query(q): Query<Limit>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(60).min(500);
    let node = app.node.read().unwrap().clone();
    let Some(shared) = node else { return Json(json!([])) };
    let store = shared.store.lock().unwrap();
    let items: Vec<_> = store
        .tape
        .iter()
        .rev()
        .take(limit)
        .map(|(_, p)| dto::tape_json(p))
        .collect();
    Json(json!(items))
}

/// Sealed ticks with batch entries + receipts (verification panel). Only
/// non-empty ticks unless `from` pins a range.
async fn ticks(State(app): State<Arc<App>>, Query(q): Query<Limit>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(10).min(100);
    let node = app.node.read().unwrap().clone();
    let Some(shared) = node else { return Json(json!([])) };
    let store = shared.store.lock().unwrap();
    let iter: Box<dyn Iterator<Item = (&u64, &sequencer::store::SealedTick)>> = match q.from {
        Some(f) => Box::new(store.ticks.range(f..).map(|(k, v)| (k, v))),
        None => Box::new(store.ticks.iter().rev().filter(|(_, s)| !s.entries.is_empty())),
    };
    let items: Vec<_> = iter
        .take(limit)
        .map(|(_, sealed)| {
            json!({
                "record": dto::tick_record_json(&sealed.record),
                "entries": sealed.entries.iter().map(|e| json!({
                    "tick": e.tick, "pos": e.pos, "h": hx(&e.h),
                    "bucket": e.bucket as u8, "ticket_id": hx(&e.ticket_id),
                    "receipt_sig": hx(e.receipt_sig.as_bytes()),
                })).collect::<Vec<_>>(),
                "receipts": sealed.receipts.iter().map(dto::receipt_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    Json(json!(items))
}

async fn blocks(State(app): State<Arc<App>>, Query(q): Query<Limit>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(20).min(200);
    let sim = app.sim.lock().unwrap();
    let items: Vec<_> = sim.blocks.iter().rev().take(limit).cloned().collect();
    Json(json!(items))
}

async fn book(State(app): State<Arc<App>>) -> impl IntoResponse {
    let sim = app.sim.lock().unwrap();
    Json(json!({
        "snapshot": sim.book.snapshot(15),
        "head_blocked": sim.head_blocked,
        "cursor": {"tick": sim.cursor_tick, "pos": sim.cursor_pos},
    }))
}

async fn trades(State(app): State<Arc<App>>, Query(q): Query<Limit>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).min(500);
    let sim = app.sim.lock().unwrap();
    let items: Vec<_> = sim.trades.iter().rev().take(limit).cloned().collect();
    Json(json!(items))
}

async fn submits(State(app): State<Arc<App>>, Query(q): Query<Limit>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).min(256);
    let m = app.metrics.lock().unwrap();
    let items: Vec<_> = m.recent.iter().rev().take(limit).cloned().collect();
    Json(json!(items))
}

/// Sealed segments with their full Wesolowski proof material, encoded exactly
/// as `PoSqHost.verifySegmentProof(y, xEnd, pi, t, l)` consumes it (256-byte
/// big-endian group elements, challenge prime `l` recomputed here as a decimal
/// string). The browser — or a `cast call` against the deployed host — can
/// verify any of these directly.
async fn segments(State(app): State<Arc<App>>, Query(q): Query<Limit>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(8).min(32);
    let node = app.node.read().unwrap().clone();
    let Some(shared) = node else { return Json(json!([])) };
    let store = shared.store.lock().unwrap();
    let iter: Box<dyn Iterator<Item = &sequencer::records::SegmentSeal>> = match q.from {
        Some(f) => Box::new(store.segments.range(f..).map(|(_, s)| s)),
        None => Box::new(store.segments.values().rev()),
    };
    let items: Vec<_> = iter
        .take(limit)
        .map(|s| {
            let t = s.proof.iterations;
            let l = vdf::posq::challenge_prime(
                sequencer::clock::SEGMENT_PROOF_DOMAIN,
                &s.y,
                &s.x_end,
                t,
            );
            json!({
                "epoch": s.epoch,
                "segment": s.segment,
                "entropy": hx(&s.entropy),
                "y": hx(&vdf::posq::pad256(&s.y)),
                "x_end": hx(&vdf::posq::pad256(&s.x_end)),
                "x_end_hash": hx(&sequencer::records::hash_group_element(&s.x_end)),
                "pi": hx(&vdf::posq::pad256(&s.proof.pi)),
                "t": t,
                "l": l.to_string(),
                "sig": hx(s.sig.as_bytes()),
            })
        })
        .collect();
    Json(json!(items))
}

async fn anchors(State(app): State<Arc<App>>, Query(q): Query<Limit>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(30).min(200);
    let a = app.anchors.lock().unwrap();
    let items: Vec<_> = a.iter().rev().take(limit).cloned().collect();
    Json(json!(items))
}

async fn scenario_log(State(app): State<Arc<App>>) -> impl IntoResponse {
    let log = app.scenario_log.lock().unwrap();
    Json(json!(log.iter().rev().cloned().collect::<Vec<_>>()))
}

async fn events(
    State(app): State<Arc<App>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = app.events_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|v| match v {
        Ok(v) => Some(Ok(Event::default().data(v.to_string()))),
        Err(_) => None, // lagged; dashboard re-syncs via REST
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
