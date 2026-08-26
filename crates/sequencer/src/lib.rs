//! Continuum sequencer — Proof of Sequence (2026) MVD implementation.
//!
//! Module map (whitepaper section in parentheses):
//! - `identity`   sequencer/client keys, secp256k1 + keccak (ecrecover-ready)
//! - `params`     Table-1 parameters and the no-look inequality (eq. 1)
//! - `envelope`   fixed-size envelopes, fee tickets, intents (§7)
//! - `records`    receipts, tick records, digest/log chains, merkle (§4.5, §8)
//! - `admission`  windows and the three accountable responses (§6)
//! - `clock`      work-defined ticks, segments, per-segment proofs (§4.4, §5)
//! - `store`      canonical tape, deterministic gaps, DA layer (§9)
//! - `transcript` sequencing-validity predicate + proof backends (§4.6)
//! - `anchor`     host-chain anchors and the finality ladder (§10.1)
//! - `fraud`      fraud-proof constructors/verifiers (§10.2)
//! - `node`       the coordinator wiring all of the above
//! - `service`    gRPC surface
//! - `client`     client-side envelope construction (SDK core)
//!
//! The Ethereum host contract lives in `contracts/PoSqHost.sol`; its
//! encodings mirror `records`/`anchor` byte-for-byte.

pub mod admission;
pub mod anchor;
pub mod client;
pub mod clock;
pub mod envelope;
pub mod fraud;
pub mod identity;
pub mod node;
pub mod params;
pub mod records;
pub mod service;
pub mod store;
pub mod transcript;

pub mod proto {
    tonic::include_proto!("continuum.sequencer.v2");
}

pub use node::{NodeConfig, SequencerNode};
pub use params::PosqParams;
