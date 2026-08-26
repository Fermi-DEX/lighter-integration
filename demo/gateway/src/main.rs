//! demo-gateway: the single process behind the PoSq × Lighter dashboard.
//!
//! It embeds a real PoSq `SequencerNode` (dev profile), runs the lighter-sim
//! ingest adapter over its tape, drives blind traffic bots, mirrors anchors
//! into the local log (and optionally posts them to Sepolia), and serves the
//! REST/SSE API plus the static dashboard. Everything the dashboard shows is
//! a real object emitted by the sequencer crate — nothing is mocked.

mod api;
mod bots;
mod dto;
mod eth;
mod lighter_sim;
mod scenarios;
mod state;

use anchor_bridge::PosterHost;
use axum::Router;
use lighter_sim::SimState;
use sequencer::anchor::{Anchor, HostClient};
use sequencer::node::{NodeConfig, SequencerNode, Shared};
use sequencer::params::PosqParams;
use state::{now_ms, AnchorLog, App, DemoCfg};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, watch};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;

/// Host client that records anchors into the app log and forwards them to the
/// optional Sepolia poster.
mod anchor_bridge {
    use super::*;
    pub struct PosterHost {
        pub app: Arc<App>,
    }
    impl HostClient for PosterHost {
        fn post_anchor(&self, anchor: &Anchor) -> anyhow::Result<()> {
            let stream = {
                let sim = self.app.sim.lock().unwrap();
                // The block covering last_tick carries the span's stream head.
                sim.blocks
                    .iter()
                    .rev()
                    .find(|b| b.last_tick <= anchor.last_tick)
                    .map(|b| b.stream_commitment.clone())
            };
            let log = AnchorLog {
                epoch: anchor.epoch,
                first_segment: anchor.first_segment,
                last_segment: anchor.last_segment,
                first_tick: anchor.first_tick,
                last_tick: anchor.last_tick,
                c_b: format!("0x{}", hex::encode(anchor.c_b)),
                receipts_root: format!("0x{}", hex::encode(anchor.receipts_root)),
                x_b_hash: format!("0x{}", hex::encode(anchor.x_b_hash)),
                da_attestation: format!("0x{}", hex::encode(anchor.da_attestation)),
                transcript_commitment: format!(
                    "0x{}",
                    hex::encode(anchor.transcript_commitment())
                ),
                sig: format!("0x{}", hex::encode(anchor.sig.as_bytes())),
                segment_x_end_hashes: anchor
                    .segment_seals
                    .iter()
                    .map(|s| {
                        format!(
                            "0x{}",
                            hex::encode(sequencer::records::hash_group_element(&s.x_end))
                        )
                    })
                    .collect(),
                at_ms: now_ms(),
                sepolia: if self.app.eth.lock().unwrap().enabled {
                    "pending".into()
                } else {
                    "local-only".into()
                },
                stream_commitment: stream,
                bridge_tx: None,
                host_tx: None,
            };
            {
                let mut a = self.app.anchors.lock().unwrap();
                if a.len() >= 256 {
                    a.pop_front();
                }
                a.push_back(log.clone());
            }
            self.app.emit(serde_json::json!({"type": "anchor", "data": log}));
            Ok(())
        }
    }
}

fn build_params(cfg: &DemoCfg, epoch: u64, q: u64) -> PosqParams {
    // Dev profile scaled so a tick is ~Δ on this host and delays clear in a
    // few hundred ms. The delay menu is derived from the *calibrated* q so
    // the no-look inequality (deployment rule 1) holds by construction —
    // deriving it from a different q and overwriting q afterward would break
    // the bound.
    let mut p = PosqParams::dev(q, cfg.segment_ticks);
    p.epoch = epoch;
    p.delta_micros = cfg.delta_micros;
    p.segment_ticks = cfg.segment_ticks;
    p.window_ticks = cfg.window_ticks;
    p.anchor_every_segments = cfg.anchor_every_segments;
    p
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let cfg = DemoCfg::from_env();
    info!("demo-gateway starting: http={} Δ={}µs", cfg.http_addr, cfg.delta_micros);

    // Calibrate q once so a tick is ~Δ on this machine. DEMO_Q pins it
    // instead — required for on-chain parity: the deployed PoSqHost checks
    // t == q * segmentTicks, so a live instance whose proofs must verify on
    // Sepolia has to run the exact q the host was deployed with (the tick
    // then lasts however long q squarings take here, rather than ~Δ).
    let group = vdf::posq::Group::default_rsa2048();
    let q = match std::env::var("DEMO_Q").ok().and_then(|s| s.parse::<u64>().ok()) {
        Some(pinned) => {
            info!("q pinned via DEMO_Q = {pinned} squarings per tick (on-chain parity mode)");
            pinned
        }
        None => {
            let q = sequencer::clock::calibrate_q(&group, cfg.delta_micros);
            info!("calibrated q = {q} squarings per {}µs tick", cfg.delta_micros);
            q
        }
    };

    let (events_tx, _) = broadcast::channel::<serde_json::Value>(8192);
    let sim = Arc::new(Mutex::new(SimState::new()));
    let eth = eth::spawn_probe(&cfg).await;

    let app = Arc::new(App {
        cfg: cfg.clone(),
        node: RwLock::new(None),
        sim: sim.clone(),
        metrics: Mutex::new(Default::default()),
        anchors: Mutex::new(Default::default()),
        events_tx: events_tx.clone(),
        started_ms: now_ms(),
        eth: Mutex::new(eth),
        scenario_log: Mutex::new(Default::default()),
    });

    // Static dir: prefer the crate-local copy; fall back to CWD.
    let static_dir = {
        let a = concat!(env!("CARGO_MANIFEST_DIR"), "/static");
        if std::path::Path::new(a).is_dir() {
            a.to_string()
        } else {
            "demo/gateway/static".to_string()
        }
    };
    let router = Router::new()
        .merge(api::router(app.clone()))
        .fallback_service(ServeDir::new(static_dir))
        .layer(CorsLayer::permissive());

    let http_app = app.clone();
    let addr = cfg.http_addr.clone();
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind http");
        info!("dashboard on http://{addr}");
        axum::serve(listener, router.into_make_service()).await.expect("serve");
        let _ = http_app;
    });

    // Epoch supervisor: each epoch spins a fresh bounded node; the adapter,
    // bots, and anchor poster attach to it; on completion the loop rotates.
    let epoch_counter = AtomicU64::new(0);
    loop {
        let epoch = epoch_counter.fetch_add(1, Ordering::Relaxed);
        let params = build_params(&cfg, epoch, q);
        run_epoch(app.clone(), params, cfg.epoch_segments).await?;
        info!("epoch {epoch} complete; rotating");
    }
}

async fn run_epoch(app: Arc<App>, params: PosqParams, max_segments: u64) -> anyhow::Result<()> {
    let host: Arc<dyn HostClient> = Arc::new(PosterHost { app: app.clone() });
    let cfg = NodeConfig {
        params: params.clone(),
        identity_seed: b"demo-sequencer".to_vec(),
        data_dir: None,
        max_segments: Some(max_segments),
        enforce_timeliness: false,
        induce_late_tick: None,
    };
    let node = SequencerNode::start(cfg, host)?;
    let shared: Arc<Shared> = node.shared.clone();
    *app.node.write().unwrap() = Some(shared.clone());
    // Reset the sim for the new epoch (state carries via begin_epoch inside).
    app.sim.lock().unwrap().begin_epoch(params.epoch);

    let (stop_tx, stop_rx) = watch::channel(false);

    // Adapter.
    let adapter = tokio::spawn(lighter_sim::run_sim(
        shared.clone(),
        app.sim.clone(),
        app.events_tx.clone(),
        app.cfg.block_ticks,
        stop_rx.clone(),
    ));

    // Bots: two makers, two takers.
    let mut bot_handles = Vec::new();
    for name in ["maker-alice", "maker-bob"] {
        bot_handles.push(tokio::spawn(bots::run_maker(
            app.clone(), shared.clone(), app.sim.clone(), name.to_string(), stop_rx.clone(),
        )));
    }
    for name in ["taker-carol", "taker-dave"] {
        bot_handles.push(tokio::spawn(bots::run_taker(
            app.clone(), shared.clone(), app.sim.clone(), name.to_string(), stop_rx.clone(),
        )));
    }

    // Sepolia anchor forwarder (throttled), if enabled.
    let poster = tokio::spawn(eth::run_poster(app.clone(), stop_rx.clone()));

    // Wait for the node to finish this epoch (bounded by max_segments).
    let shared_join = shared.clone();
    tokio::task::spawn_blocking(move || {
        // Busy-wait on the finished flag; the node's coordinator sets it.
        loop {
            if shared_join.status.lock().unwrap().finished {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    })
    .await
    .ok();

    // Tear down the epoch's tasks.
    let _ = stop_tx.send(true);
    let _ = adapter.await;
    for h in bot_handles {
        h.abort();
    }
    poster.abort();
    Ok(())
}

/// Aggregate status snapshot (also used by /api/status).
pub fn status_json(app: &App) -> serde_json::Value {
    let node = app.node.read().unwrap().clone();
    let (tick, segment, node_status) = match &node {
        Some(s) => {
            let st = s.status.lock().unwrap().clone();
            (st.current_tick, st.current_segment, st)
        }
        None => (0, 0, Default::default()),
    };
    let (p50, p95, pmax) = app.metrics.lock().unwrap().percentiles();
    let m = app.metrics.lock().unwrap();
    let sim = app.sim.lock().unwrap();
    let anchors = app.anchors.lock().unwrap();
    serde_json::json!({
        "uptime_ms": now_ms() - app.started_ms,
        "epoch": node.as_ref().map(|s| s.params.epoch),
        "tick": tick,
        "segment": segment,
        "admitted": node_status.admitted,
        "opened": node_status.opened,
        "tape_gaps": node_status.gaps,
        "cadence_faults": node_status.cadence_faults,
        "submits": {
            "total": m.submitted, "admitted": m.admitted,
            "rejected": m.rejected, "full_window": m.full_window,
            "latency_us": {"p50": p50, "p95": p95, "max": pmax},
        },
        "lighter": {
            "blocks": sim.block_counter,
            "opened": sim.opened,
            "applied": sim.applied,
            "failed": sim.failed,
            "adapter_gaps": sim.adapter_gaps,
            "tape_gaps": sim.tape_gaps,
            "trades": sim.trades_total,
            "best_bid": sim.book.best_bid(),
            "best_ask": sim.book.best_ask(),
            "last_price": sim.book.last_trade_price,
            "head_blocked": sim.head_blocked.clone(),
        },
        "anchors_local": anchors.len(),
        "eth": app.eth.lock().unwrap().clone(),
    })
}
