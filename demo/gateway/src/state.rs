//! Shared gateway state: outlives epoch rotations (the node itself is
//! bounded per epoch; the demo rotates epochs like the real deployment
//! rotates on operator replacement — here it doubles as memory bounding).

use crate::lighter_sim::SimState;
use sequencer::node::Shared;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct DemoCfg {
    pub http_addr: String,
    pub delta_micros: u64,
    pub segment_ticks: u64,
    pub epoch_segments: u64,
    pub block_ticks: u64,
    pub anchor_every_segments: u64,
    pub window_ticks: u16,
    pub eth_rpc: String,
    pub eth_key_file: String,
}

impl DemoCfg {
    pub fn from_env() -> Self {
        let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        Self {
            http_addr: get("DEMO_HTTP_ADDR", "0.0.0.0:8080"),
            delta_micros: get("DEMO_DELTA_US", "10000").parse().unwrap_or(10_000),
            segment_ticks: get("DEMO_SEGMENT_TICKS", "128").parse().unwrap_or(128),
            epoch_segments: get("DEMO_EPOCH_SEGMENTS", "2800").parse().unwrap_or(2800),
            block_ticks: get("DEMO_BLOCK_TICKS", "8").parse().unwrap_or(8),
            anchor_every_segments: get("DEMO_ANCHOR_EVERY", "4").parse().unwrap_or(4),
            window_ticks: get("DEMO_WINDOW_TICKS", "12").parse().unwrap_or(12),
            eth_rpc: get("DEMO_ETH_RPC", "https://ethereum-sepolia-rpc.publicnode.com"),
            eth_key_file: get("DEMO_ETH_KEY_FILE", "demo/.sepolia-key"),
        }
    }
}

/// One admission outcome, kept for the dashboard's accountability panel.
#[derive(Debug, Clone, Serialize)]
pub struct SubmitLog {
    pub at_ms: u64,
    pub kind: String, // admitted | rejected:<reason> | full-window
    pub latency_us: u64,
    pub tick: u64,
    pub pos: u32,
    pub h: String,
    pub bot: String,
    /// Full signed outcome object (hex fields) for in-browser verification.
    pub outcome: serde_json::Value,
    /// The exact envelope bytes submitted (hex): 39-byte visible header +
    /// opaque ciphertext. Lets the browser recompute
    /// h = commitment_hash(epoch, bytes) and see that the tape is blind.
    pub envelope_hex: Option<String>,
}

#[derive(Default)]
pub struct Metrics {
    pub submitted: u64,
    pub admitted: u64,
    pub rejected: u64,
    pub full_window: u64,
    pub latencies_us: VecDeque<u64>,
    pub recent: VecDeque<SubmitLog>,
}

impl Metrics {
    pub fn record(&mut self, log: SubmitLog) {
        self.submitted += 1;
        match log.kind.as_str() {
            "admitted" => self.admitted += 1,
            "full-window" => self.full_window += 1,
            _ => self.rejected += 1,
        }
        if self.latencies_us.len() >= 4096 {
            self.latencies_us.pop_front();
        }
        self.latencies_us.push_back(log.latency_us);
        if self.recent.len() >= 256 {
            self.recent.pop_front();
        }
        self.recent.push_back(log);
    }

    pub fn percentiles(&self) -> (u64, u64, u64) {
        if self.latencies_us.is_empty() {
            return (0, 0, 0);
        }
        let mut v: Vec<u64> = self.latencies_us.iter().copied().collect();
        v.sort_unstable();
        let pick = |p: f64| v[((v.len() as f64 * p) as usize).min(v.len() - 1)];
        (pick(0.5), pick(0.95), *v.last().unwrap())
    }
}

/// Anchor posting record (local + Sepolia status).
#[derive(Debug, Clone, Serialize)]
pub struct AnchorLog {
    pub epoch: u64,
    pub first_segment: u64,
    pub last_segment: u64,
    pub first_tick: u64,
    pub last_tick: u64,
    pub c_b: String,
    pub receipts_root: String,
    // Remaining PoSqHost.submitAnchor calldata (byte-faithful hex) so the
    // poster and the dashboard can reconstruct the full on-chain call.
    pub x_b_hash: String,
    pub da_attestation: String,
    pub transcript_commitment: String,
    pub sig: String,
    pub segment_x_end_hashes: Vec<String>,
    pub at_ms: u64,
    /// none | skipped (throttle) | pending | tx hash | error:<msg>
    pub sepolia: String,
    pub stream_commitment: Option<String>,
    pub bridge_tx: Option<String>,
    /// PoSqHost.submitAnchor tx hash, when the full anchor was posted.
    pub host_tx: Option<String>,
}

pub struct App {
    pub cfg: DemoCfg,
    /// Current epoch's node internals; swapped at rotation.
    pub node: RwLock<Option<Arc<Shared>>>,
    pub sim: Arc<Mutex<SimState>>,
    pub metrics: Mutex<Metrics>,
    pub anchors: Mutex<VecDeque<AnchorLog>>,
    pub events_tx: broadcast::Sender<serde_json::Value>,
    pub started_ms: u64,
    pub eth: Mutex<crate::eth::EthStatus>,
    pub scenario_log: Mutex<VecDeque<serde_json::Value>>,
}

impl App {
    pub fn emit(&self, ev: serde_json::Value) {
        let _ = self.events_tx.send(ev);
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
