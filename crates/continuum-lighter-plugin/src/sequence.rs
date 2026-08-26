use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    binding::{
        compute_c_bind, compute_execution_stream_root, compute_ordered_item_root, BindingInputsV3,
    },
    hash::LighterNativeHash,
    join::SequencePublicV3,
    types::{DerivedItemV3, ExecutionItemV3, Hash32, ResolutionV3},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SequenceStateV3 {
    pub domain_hash: Hash32,
    pub epoch: u64,
    pub global_cursor: u64,
    pub namespace_item_count: u64,
    pub transcript_root: Hash32,
    pub receipt_chain_root: Hash32,
    pub frame_plan_root: Hash32,
    pub da_commitment: Hash32,
    pub config_hash: Hash32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReceiptV3 {
    pub epoch: u64,
    pub global_cursor: u64,
    pub tick: u64,
    pub position: u32,
    pub namespace_id: u64,
    pub envelope_hash: Hash32,
    pub previous_receipt_digest: Hash32,
    pub receipt_digest: Hash32,
    pub da_leaf: Hash32,
    pub signature: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolvedItemV3 {
    pub receipt_digest: Hash32,
    pub derived: DerivedItemV3,
    pub execution: ExecutionItemV3,
    pub opening_commitment: Hash32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SequenceTransitionPublicV3 {
    pub namespace_id: u64,
    pub old_state: SequenceStateV3,
    pub new_state: SequenceStateV3,
    pub binding: BindingInputsV3,
    pub c_bind: Hash32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SequenceTransitionWitnessV3 {
    pub receipts: Vec<ReceiptV3>,
    pub resolved_items: Vec<ResolvedItemV3>,
}

impl SequenceTransitionPublicV3 {
    /// Project the sequence transition onto the exact values consumed by the
    /// atomic settlement join after the sequence proof has verified.
    pub fn to_join_public(&self, verifier_id: Hash32) -> SequencePublicV3 {
        SequencePublicV3 {
            domain_hash: self.old_state.domain_hash,
            verifier_id,
            epoch: self.old_state.epoch,
            old_global_cursor: self.old_state.global_cursor,
            new_global_cursor: self.new_state.global_cursor,
            old_transcript_root: self.old_state.transcript_root,
            new_transcript_root: self.new_state.transcript_root,
            old_namespace_count: self.old_state.namespace_item_count,
            new_namespace_count: self.new_state.namespace_item_count,
            ordered_item_root: self.binding.ordered_item_root,
            execution_stream_root: self.binding.execution_stream_root,
            ordered_item_count: self.binding.ordered_item_count,
            priority_start: self.binding.priority_start,
            priority_end: self.binding.priority_end,
            c_bind: self.c_bind,
        }
    }
}

/// Cryptographic checks supplied by the Continuum implementation or the
/// sequence circuit. The structural verifier below is shared by both.
pub trait TransitionAuthenticator {
    fn verify_receipt(&self, receipt: &ReceiptV3) -> bool;
    fn verify_da(&self, receipt: &ReceiptV3, expected_da_commitment: Hash32) -> bool;
    fn verify_resolution(&self, receipt: &ReceiptV3, item: &ResolvedItemV3) -> bool;
    fn verify_transcript_transition(
        &self,
        old_transcript_root: Hash32,
        receipts: &[ReceiptV3],
        new_transcript_root: Hash32,
    ) -> bool;
    fn verify_frame_commitments(
        &self,
        witness: &SequenceTransitionWitnessV3,
        public: &SequenceTransitionPublicV3,
    ) -> bool;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceTransitionError {
    Domain,
    Epoch,
    Config,
    EmptySpan,
    Cursor,
    ReceiptOrder,
    ReceiptChain,
    DuplicateReceipt,
    DuplicateEnvelope,
    InvalidReceipt,
    InvalidDa,
    MissingResolution,
    DuplicateResolution,
    OrphanResolution,
    ResolutionBinding,
    InvalidTerminalSemantics,
    InvalidExecutionLeaf,
    Transcript,
    FrameCommitment,
    NewState,
    OrderedItemRoot,
    ExecutionStreamRoot,
    Binding,
}

pub fn verify_sequence_transition(
    hasher: &impl LighterNativeHash,
    authenticator: &impl TransitionAuthenticator,
    public: &SequenceTransitionPublicV3,
    witness: &SequenceTransitionWitnessV3,
) -> Result<(), SequenceTransitionError> {
    if public.old_state.domain_hash != public.new_state.domain_hash
        || public.old_state.domain_hash != public.binding.domain_hash
    {
        return Err(SequenceTransitionError::Domain);
    }
    if public.old_state.epoch != public.new_state.epoch
        || public.old_state.epoch != public.binding.epoch
    {
        return Err(SequenceTransitionError::Epoch);
    }
    if public.old_state.config_hash != public.new_state.config_hash {
        return Err(SequenceTransitionError::Config);
    }
    if witness.receipts.is_empty() {
        return Err(SequenceTransitionError::EmptySpan);
    }

    let mut expected_cursor = public
        .old_state
        .global_cursor
        .checked_add(1)
        .ok_or(SequenceTransitionError::Cursor)?;
    let mut previous_position: Option<(u64, u32)> = None;
    let mut previous_digest = public.old_state.receipt_chain_root;
    let mut seen_receipts = HashSet::with_capacity(witness.receipts.len());
    let mut seen_envelopes = HashSet::with_capacity(witness.receipts.len());
    let mut receipt_by_digest = HashMap::with_capacity(witness.receipts.len());

    for receipt in &witness.receipts {
        if receipt.epoch != public.old_state.epoch {
            return Err(SequenceTransitionError::Epoch);
        }
        if receipt.global_cursor != expected_cursor {
            return Err(SequenceTransitionError::Cursor);
        }
        expected_cursor = expected_cursor
            .checked_add(1)
            .ok_or(SequenceTransitionError::Cursor)?;

        if let Some(previous) = previous_position {
            if (receipt.tick, receipt.position) <= previous {
                return Err(SequenceTransitionError::ReceiptOrder);
            }
        }
        previous_position = Some((receipt.tick, receipt.position));

        if receipt.previous_receipt_digest != previous_digest {
            return Err(SequenceTransitionError::ReceiptChain);
        }
        previous_digest = receipt.receipt_digest;

        if !seen_receipts.insert(receipt.receipt_digest) {
            return Err(SequenceTransitionError::DuplicateReceipt);
        }
        if !seen_envelopes.insert(receipt.envelope_hash) {
            return Err(SequenceTransitionError::DuplicateEnvelope);
        }
        if !authenticator.verify_receipt(receipt) {
            return Err(SequenceTransitionError::InvalidReceipt);
        }
        if !authenticator.verify_da(receipt, public.new_state.da_commitment) {
            return Err(SequenceTransitionError::InvalidDa);
        }
        receipt_by_digest.insert(receipt.receipt_digest, receipt);
    }

    let mut resolution_by_receipt = HashMap::with_capacity(witness.resolved_items.len());
    for item in &witness.resolved_items {
        if resolution_by_receipt
            .insert(item.receipt_digest, item)
            .is_some()
        {
            return Err(SequenceTransitionError::DuplicateResolution);
        }
        if !receipt_by_digest.contains_key(&item.receipt_digest) {
            return Err(SequenceTransitionError::OrphanResolution);
        }
    }

    let protected_receipts: Vec<&ReceiptV3> = witness
        .receipts
        .iter()
        .filter(|receipt| receipt.namespace_id == public.namespace_id)
        .collect();

    let mut derived_items = Vec::with_capacity(protected_receipts.len());
    let mut execution_items = Vec::with_capacity(protected_receipts.len());

    for (logical_index, receipt) in protected_receipts.iter().enumerate() {
        let item = resolution_by_receipt
            .get(&receipt.receipt_digest)
            .ok_or(SequenceTransitionError::MissingResolution)?;

        if item.derived.domain_hash != public.old_state.domain_hash
            || item.derived.tick != receipt.tick
            || item.derived.position != receipt.position
            || item.derived.envelope_hash != receipt.envelope_hash
            || item.derived.receipt_digest != receipt.receipt_digest
        {
            return Err(SequenceTransitionError::ResolutionBinding);
        }
        if !authenticator.verify_resolution(receipt, item) {
            return Err(SequenceTransitionError::ResolutionBinding);
        }

        let terminal = matches!(
            item.derived.resolution,
            ResolutionV3::BadAead | ResolutionV3::BadEncoding | ResolutionV3::L1Cancelled
        );
        if terminal {
            if !item.execution.terminal_noop
                || item.execution.tx_type != 0
                || item.execution.tx_hash != [0; 5]
                || item.execution.outcome_class != item.derived.resolution as u16
                || item.derived.cleartext_length != 0
                || item.derived.terminal_reason != item.execution.outcome_class
            {
                return Err(SequenceTransitionError::InvalidTerminalSemantics);
            }
        } else if item.execution.terminal_noop
            || item.execution.tx_type == 0
            || item.derived.cleartext_length == 0
            || item.derived.terminal_reason != 0
        {
            return Err(SequenceTransitionError::InvalidTerminalSemantics);
        }

        if item.execution.logical_index != logical_index as u64 {
            return Err(SequenceTransitionError::InvalidExecutionLeaf);
        }

        derived_items.push(item.derived.clone());
        execution_items.push(item.execution.clone());
    }

    if resolution_by_receipt.len() != protected_receipts.len() {
        return Err(SequenceTransitionError::OrphanResolution);
    }

    if !authenticator.verify_transcript_transition(
        public.old_state.transcript_root,
        &witness.receipts,
        public.new_state.transcript_root,
    ) {
        return Err(SequenceTransitionError::Transcript);
    }
    if !authenticator.verify_frame_commitments(witness, public) {
        return Err(SequenceTransitionError::FrameCommitment);
    }

    let item_count = derived_items.len() as u64;
    let expected_new_cursor = witness.receipts.last().expect("nonempty").global_cursor;
    let expected_namespace_count = public
        .old_state
        .namespace_item_count
        .checked_add(item_count)
        .ok_or(SequenceTransitionError::NewState)?;
    if public.new_state.global_cursor != expected_new_cursor
        || public.new_state.namespace_item_count != expected_namespace_count
        || public.new_state.receipt_chain_root != previous_digest
        || public.new_state.frame_plan_root != public.binding.frame_plan_root
        || public.new_state.transcript_root != public.binding.new_transcript_root
        || public.old_state.transcript_root != public.binding.old_transcript_root
        || public.old_state.global_cursor != public.binding.old_global_cursor
        || public.new_state.global_cursor != public.binding.new_global_cursor
    {
        return Err(SequenceTransitionError::NewState);
    }

    let ordered_item_root = compute_ordered_item_root(
        hasher,
        public.old_state.domain_hash,
        derived_items.first().map(|item| item.frame_id).unwrap_or(0),
        public.old_state.global_cursor,
        &derived_items,
    );
    if ordered_item_root != public.binding.ordered_item_root
        || item_count != public.binding.ordered_item_count
    {
        return Err(SequenceTransitionError::OrderedItemRoot);
    }

    let execution_stream_root = compute_execution_stream_root(
        hasher,
        public.old_state.domain_hash,
        public.old_state.global_cursor,
        &execution_items,
    );
    if execution_stream_root != public.binding.execution_stream_root {
        return Err(SequenceTransitionError::ExecutionStreamRoot);
    }

    if compute_c_bind(hasher, &public.binding) != public.c_bind {
        return Err(SequenceTransitionError::Binding);
    }

    Ok(())
}
