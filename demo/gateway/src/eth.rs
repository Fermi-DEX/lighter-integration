//! Sepolia edge: anchor posting and chain status via the foundry `cast`
//! CLI (no heavy RPC crate pulled into the gateway). The poster activates
//! only when a key file + `DEMO_POSQ_HOST` address are configured; without
//! them the demo runs fully local and anchors are marked "local-only".
//!
//! Posting is throttled (default 1 anchor bundle/min): the tape anchors
//! every few seconds locally and the poster forwards the newest un-posted
//! anchor per interval — the "anchors are kilobytes, cadence is a policy
//! knob" posture of the spec.

use crate::state::{App, DemoCfg};
use serde::Serialize;
use std::sync::Arc;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Default)]
pub struct EthStatus {
    pub enabled: bool,
    pub rpc: String,
    pub chain_id: u64,
    pub poster_address: Option<String>,
    pub poster_balance_eth: Option<String>,
    pub posq_host: Option<String>,
    pub bridge: Option<String>,
    pub posq_host_sacrificial: Option<String>,
    pub anchors_posted: u64,
    pub spans_posted: u64,
    pub last_anchor_tx: Option<String>,
    pub last_span_tx: Option<String>,
    pub last_error: Option<String>,
}

fn cast_bin() -> String {
    std::env::var("DEMO_CAST_BIN").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        let p = format!("{home}/.foundry/bin/cast");
        if std::path::Path::new(&p).exists() {
            p
        } else {
            "cast".to_string()
        }
    })
}

async fn cast(rpc: &str, args: &[&str]) -> anyhow::Result<String> {
    let mut cmd = Command::new(cast_bin());
    cmd.args(args).arg("--rpc-url").arg(rpc);
    let out = cmd.output().await?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Probe the configured RPC/addresses at boot.
pub async fn spawn_probe(cfg: &DemoCfg) -> EthStatus {
    let mut st = EthStatus { rpc: cfg.eth_rpc.clone(), ..Default::default() };
    let host = std::env::var("DEMO_POSQ_HOST").ok();
    let key_present = std::path::Path::new(&cfg.eth_key_file).exists()
        || std::env::var("DEMO_ETH_KEY").is_ok();
    st.posq_host = host.clone();
    st.bridge = std::env::var("DEMO_BRIDGE").ok();
    st.posq_host_sacrificial = std::env::var("DEMO_POSQ_HOST_SACRIFICIAL").ok();

    // Chain id is a cheap liveness check for the RPC even without a key.
    match cast(&cfg.eth_rpc, &["chain-id"]).await {
        Ok(s) => st.chain_id = s.parse().unwrap_or(0),
        Err(e) => {
            st.last_error = Some(format!("rpc chain-id: {e}"));
            return st;
        }
    }
    if let Some(addr) = poster_address(cfg).await {
        st.poster_address = Some(addr.clone());
        if let Ok(bal) = cast(&cfg.eth_rpc, &["balance", &addr, "--ether"]).await {
            st.poster_balance_eth = Some(bal);
        }
    }
    st.enabled = key_present && host.is_some() && st.chain_id != 0;
    st
}

async fn poster_address(cfg: &DemoCfg) -> Option<String> {
    if let Ok(a) = std::env::var("DEMO_POSTER_ADDRESS") {
        return Some(a);
    }
    // Derive from the key file via cast.
    let key = read_key(cfg)?;
    let out = Command::new(cast_bin())
        .args(["wallet", "address", "--private-key", &key])
        .output()
        .await
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn read_key(cfg: &DemoCfg) -> Option<String> {
    if let Ok(k) = std::env::var("DEMO_ETH_KEY") {
        return Some(k);
    }
    std::fs::read_to_string(&cfg.eth_key_file).ok().map(|s| s.trim().to_string())
}

/// Throttled anchor forwarder. Posts the newest local anchor lacking a tx to
/// `PoSqHost.submitAnchor`, then to the bridge's `proposeSpan` if configured.
/// Real submission uses `cast send`; the anchor calldata is reconstructed
/// from the local `AnchorLog` plus the demo's known segment layout.
pub async fn run_poster(app: Arc<App>, mut stop: tokio::sync::watch::Receiver<bool>) {
    let cfg = app.cfg.clone();
    let interval_secs: u64 = std::env::var("DEMO_ANCHOR_POST_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let enabled = app.eth.lock().unwrap().enabled;
    if !enabled {
        return; // local-only mode
    }
    let key = match read_key(&cfg) {
        Some(k) => k,
        None => return,
    };
    let host = match std::env::var("DEMO_POSQ_HOST") {
        Ok(h) => h,
        Err(_) => return,
    };
    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(interval_secs)) => {}
            _ = stop.changed() => { if *stop.borrow() { return; } }
        }
        // Find the newest anchor whose sepolia status is still "pending".
        let target = {
            let anchors = app.anchors.lock().unwrap();
            anchors.iter().rev().find(|a| a.sepolia == "pending").cloned()
        };
        let Some(a) = target else { continue };
        // 1. Full anchor to PoSqHost.submitAnchor — the AnchorLog carries the
        // complete calldata (segment proofs are verified separately via
        // verifySegmentProof to bound calldata; submitAnchor takes the
        // per-segment x_end hashes bound into the signed message).
        {
            let sig = "submitAnchor(uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes,uint64,uint64,bytes32[])";
            let hashes = format!("[{}]", a.segment_x_end_hashes.join(","));
            let args = [
                "send",
                &host,
                sig,
                &a.first_tick.to_string(),
                &a.last_tick.to_string(),
                &a.x_b_hash,
                &a.c_b,
                &a.receipts_root,
                &a.da_attestation,
                &a.transcript_commitment,
                &a.sig,
                &a.first_segment.to_string(),
                &a.last_segment.to_string(),
                &hashes,
                "--private-key",
                &key,
            ];
            match cast(&cfg.eth_rpc, &args).await {
                Ok(receipt) => {
                    let tx = extract_tx_hash(&receipt);
                    let mut anchors = app.anchors.lock().unwrap();
                    if let Some(rec) = anchors
                        .iter_mut()
                        .rev()
                        .find(|x| x.first_tick == a.first_tick && x.sepolia == "pending")
                    {
                        rec.host_tx = tx.clone();
                    }
                    let mut eth = app.eth.lock().unwrap();
                    eth.anchors_posted += 1;
                    eth.last_anchor_tx = tx;
                }
                Err(e) => {
                    let mut eth = app.eth.lock().unwrap();
                    eth.last_error = Some(format!("submitAnchor: {e}"));
                }
            }
        }
        // 2. Lightweight span binding to the bridge (what the challenge-window
        // demo verifies on-chain).
        if let Ok(bridge) = std::env::var("DEMO_BRIDGE") {
            let stream = a.stream_commitment.clone().unwrap_or_else(|| format!("0x{}", "00".repeat(32)));
            let sig = format!(
                "proposeSpan(uint64,uint64,uint64,uint64,bytes32,bytes32)"
            );
            let args = [
                "send",
                &bridge,
                &sig,
                &a.epoch.to_string(),
                &a.first_tick.to_string(),
                &a.last_tick.to_string(),
                &a.last_segment.to_string(),
                &a.c_b,
                &stream,
                "--private-key",
                &key,
            ];
            match cast(&cfg.eth_rpc, &args).await {
                Ok(receipt) => {
                    let tx = extract_tx_hash(&receipt);
                    let mut anchors = app.anchors.lock().unwrap();
                    if let Some(rec) = anchors.iter_mut().rev().find(|x| x.first_tick == a.first_tick && x.sepolia == "pending") {
                        rec.sepolia = tx.clone().unwrap_or_else(|| "posted".into());
                        rec.bridge_tx = tx.clone();
                    }
                    let mut eth = app.eth.lock().unwrap();
                    eth.spans_posted += 1;
                    eth.last_span_tx = tx;
                }
                Err(e) => {
                    let mut eth = app.eth.lock().unwrap();
                    eth.last_error = Some(format!("proposeSpan: {e}"));
                }
            }
        }
    }
}

fn extract_tx_hash(receipt: &str) -> Option<String> {
    // `cast send` prints a receipt table; grab the transactionHash line.
    for line in receipt.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("transactionHash") {
            return Some(rest.trim().to_string());
        }
        if l.starts_with("0x") && l.len() == 66 {
            return Some(l.to_string());
        }
    }
    None
}
