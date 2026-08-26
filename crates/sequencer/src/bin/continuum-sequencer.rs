//! The Continuum sequencer binary (PoSq 2026 MVD).
//!
//! Configuration via environment:
//!   POSQ_GRPC_ADDR        gRPC bind address     (default 0.0.0.0:9090)
//!   POSQ_DATA_DIR         DA/store directory    (default ./posq-data)
//!   POSQ_PROFILE          "reference" | "dev"   (default dev)
//!   POSQ_Q                per-tick squarings; "calibrate" measures the
//!                         local rate and takes 90% of it (§5.1)
//!   POSQ_ENFORCE_TIMELINESS  "1" to fence late ticks (default on for
//!                            reference profile, off for dev)

use sequencer::node::{NodeConfig, SequencerNode};
use sequencer::params::PosqParams;
use sequencer::proto::posq_sequencer_server::PosqSequencerServer;
use sequencer::service::PosqService;
use std::sync::Arc;
use tracing::info;
use vdf::posq::Group;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let grpc_addr = std::env::var("POSQ_GRPC_ADDR").unwrap_or_else(|_| "0.0.0.0:9090".into());
    let data_dir = std::env::var("POSQ_DATA_DIR").unwrap_or_else(|_| "./posq-data".into());
    let profile = std::env::var("POSQ_PROFILE").unwrap_or_else(|_| "dev".into());

    let mut params = match profile.as_str() {
        "reference" => PosqParams::reference(),
        _ => PosqParams::dev(64, 32),
    };

    match std::env::var("POSQ_Q").ok().as_deref() {
        Some("calibrate") => {
            let group = Group::default_rsa2048();
            params.q_squarings = sequencer::clock::calibrate_q(&group, params.delta_micros);
            info!(
                "calibrated q = {} squarings per {} µs tick (mandated rate {:.2e} sq/s)",
                params.q_squarings,
                params.delta_micros,
                params.mandated_rate()
            );
        }
        Some(q) => params.q_squarings = q.parse()?,
        None => {}
    }

    let enforce_timeliness = std::env::var("POSQ_ENFORCE_TIMELINESS")
        .map(|v| v == "1")
        .unwrap_or(profile == "reference");

    let cfg = NodeConfig {
        params,
        identity_seed: std::env::var("POSQ_IDENTITY_SEED")
            .unwrap_or_else(|_| "continuum-dev-identity".into())
            .into_bytes(),
        data_dir: Some(data_dir.into()),
        max_segments: None,
        enforce_timeliness,
        induce_late_tick: None,
    };

    info!(
        "starting PoSq sequencer: profile={profile}, q={}, F={}, W={}, delays={:?}",
        cfg.params.q_squarings,
        cfg.params.segment_ticks,
        cfg.params.window_ticks,
        cfg.params.delay_classes
    );

    // The host client is a logging mock until a JSON-RPC submitter is wired
    // to a deployed PoSqHost contract; anchors are still built, signed, and
    // exposed over ListAnchors with their calldata.
    let node = SequencerNode::start(cfg, Arc::new(sequencer::anchor::MockHost::default()))?;

    let service = PosqService { shared: node.shared.clone() };
    let addr = grpc_addr.parse()?;
    info!("gRPC listening on {addr}");
    tonic::transport::Server::builder()
        .add_service(PosqSequencerServer::new(service))
        .serve(addr)
        .await?;
    Ok(())
}
