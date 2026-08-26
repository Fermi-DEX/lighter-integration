use serde::{Deserialize, Serialize};

use crate::types::Hash32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SequencePublicV3 {
    pub domain_hash: Hash32,
    pub verifier_id: Hash32,
    pub epoch: u64,
    pub old_global_cursor: u64,
    pub new_global_cursor: u64,
    pub old_transcript_root: Hash32,
    pub new_transcript_root: Hash32,
    pub old_namespace_count: u64,
    pub new_namespace_count: u64,
    pub ordered_item_root: Hash32,
    pub execution_stream_root: Hash32,
    pub ordered_item_count: u64,
    pub priority_start: u64,
    pub priority_end: u64,
    pub c_bind: Hash32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionPublicV3 {
    pub domain_hash: Hash32,
    pub verifier_id: Hash32,
    pub epoch: u64,
    pub old_state_root: Hash32,
    pub new_state_root: Hash32,
    pub ordered_item_root: Hash32,
    pub execution_stream_root: Hash32,
    pub ordered_item_count: u64,
    pub blob_c_bind: Hash32,
    pub c_bind: Hash32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SettlementHeadV3 {
    pub domain_hash: Hash32,
    pub sequence_verifier_id: Hash32,
    pub execution_verifier_id: Hash32,
    pub epoch: u64,
    pub global_cursor: u64,
    pub namespace_count: u64,
    pub transcript_root: Hash32,
    pub lighter_state_root: Hash32,
    pub priority_head: u64,
    pub last_c_bind: Hash32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JoinError {
    Domain,
    Epoch,
    SequenceVerifier,
    ExecutionVerifier,
    CursorContinuity,
    NamespaceContinuity,
    TranscriptContinuity,
    StateRootContinuity,
    PriorityContinuity,
    BindingMismatch,
    BlobBindingMismatch,
    StreamMismatch,
    CountMismatch,
}

/// Apply the public-input relation after both cryptographic verifiers have
/// accepted. Returning the new head models the contract's atomic update.
pub fn advance_head(
    head: &SettlementHeadV3,
    seq: &SequencePublicV3,
    exec: &ExecutionPublicV3,
) -> Result<SettlementHeadV3, JoinError> {
    if seq.domain_hash != head.domain_hash || exec.domain_hash != head.domain_hash {
        return Err(JoinError::Domain);
    }
    if seq.epoch != head.epoch || exec.epoch != head.epoch {
        return Err(JoinError::Epoch);
    }
    if seq.verifier_id != head.sequence_verifier_id {
        return Err(JoinError::SequenceVerifier);
    }
    if exec.verifier_id != head.execution_verifier_id {
        return Err(JoinError::ExecutionVerifier);
    }
    if seq.old_global_cursor != head.global_cursor
        || seq.new_global_cursor <= seq.old_global_cursor
    {
        return Err(JoinError::CursorContinuity);
    }
    if seq.old_namespace_count != head.namespace_count
        || seq.new_namespace_count < seq.old_namespace_count
    {
        return Err(JoinError::NamespaceContinuity);
    }
    if seq.old_transcript_root != head.transcript_root {
        return Err(JoinError::TranscriptContinuity);
    }
    if exec.old_state_root != head.lighter_state_root {
        return Err(JoinError::StateRootContinuity);
    }
    if seq.priority_start != head.priority_head || seq.priority_end < seq.priority_start {
        return Err(JoinError::PriorityContinuity);
    }
    if seq.c_bind != exec.c_bind {
        return Err(JoinError::BindingMismatch);
    }
    if exec.blob_c_bind != exec.c_bind {
        return Err(JoinError::BlobBindingMismatch);
    }
    if seq.ordered_item_root != exec.ordered_item_root
        || seq.execution_stream_root != exec.execution_stream_root
    {
        return Err(JoinError::StreamMismatch);
    }
    if seq.ordered_item_count != exec.ordered_item_count
        || seq.new_namespace_count - seq.old_namespace_count != seq.ordered_item_count
    {
        return Err(JoinError::CountMismatch);
    }

    Ok(SettlementHeadV3 {
        domain_hash: head.domain_hash,
        sequence_verifier_id: head.sequence_verifier_id,
        execution_verifier_id: head.execution_verifier_id,
        epoch: head.epoch,
        global_cursor: seq.new_global_cursor,
        namespace_count: seq.new_namespace_count,
        transcript_root: seq.new_transcript_root,
        lighter_state_root: exec.new_state_root,
        priority_head: seq.priority_end,
        last_c_bind: seq.c_bind,
    })
}
