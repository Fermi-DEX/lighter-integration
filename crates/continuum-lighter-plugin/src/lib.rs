//! Standalone host-side reference for the Continuum × Lighter proof join.
//!
//! This crate deliberately does not pretend to be the production SNARK. It
//! freezes canonical host objects and the fail-closed settlement relation so
//! the Rust host, Lighter circuit patch, and Solidity integration can share
//! vectors. Production hashing must use Lighter's pinned native hash.

pub mod accumulator;
pub mod binding;
pub mod canonical;
pub mod hash;
pub mod join;
#[cfg(feature = "lighter-poseidon2")]
pub mod lighter_poseidon2;
pub mod sequence;
pub mod types;

pub use accumulator::{AccumulatorError, OrderedAccumulator};
pub use binding::{
    compute_c_bind, compute_execution_stream_root, compute_ordered_item_root, BindingInputsV3,
};
pub use hash::{LighterNativeHash, Sha256ReferenceHasher};
#[cfg(feature = "lighter-poseidon2")]
pub use lighter_poseidon2::LighterPoseidon2Hasher;
pub use join::{advance_head, ExecutionPublicV3, JoinError, SequencePublicV3, SettlementHeadV3};
pub use sequence::{
    verify_sequence_transition, ReceiptV3, ResolvedItemV3, SequenceStateV3,
    SequenceTransitionError, SequenceTransitionPublicV3, SequenceTransitionWitnessV3,
    TransitionAuthenticator,
};
pub use types::{DerivedItemV3, ExecutionItemV3, Hash32, LighterHash, ResolutionV3};
