use serde::{Deserialize, Serialize};

use crate::{
    accumulator::OrderedAccumulator,
    canonical::{put_fixed, put_u64},
    hash::LighterNativeHash,
    types::{DerivedItemV3, ExecutionItemV3, Hash32},
};

pub const BINDING_DOMAIN: &[u8] = b"LIGHTER_CONTINUUM_BATCH_V3";
pub const EXECUTION_STREAM_INIT_DOMAIN: &[u8] = b"CONTINUUM_LIGHTER_EXEC_INIT_V3_1";
pub const EXECUTION_STREAM_STEP_DOMAIN: &[u8] = b"CONTINUUM_LIGHTER_EXEC_STEP_V3_1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BindingInputsV3 {
    pub domain_hash: Hash32,
    pub epoch: u64,
    pub old_global_cursor: u64,
    pub new_global_cursor: u64,
    pub old_transcript_root: Hash32,
    pub new_transcript_root: Hash32,
    pub frame_plan_root: Hash32,
    pub ordered_item_root: Hash32,
    pub execution_stream_root: Hash32,
    pub ordered_item_count: u64,
    pub priority_start: u64,
    pub priority_end: u64,
    pub priority_root: Hash32,
    pub oracle_snapshot_root: Hash32,
    pub protocol_event_root: Hash32,
    pub l1_origin_hash: Hash32,
    pub policy_hash: Hash32,
    pub decryption_module_id: Hash32,
}

impl BindingInputsV3 {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(336);
        put_fixed(&mut out, &self.domain_hash);
        put_u64(&mut out, self.epoch);
        put_u64(&mut out, self.old_global_cursor);
        put_u64(&mut out, self.new_global_cursor);
        put_fixed(&mut out, &self.old_transcript_root);
        put_fixed(&mut out, &self.new_transcript_root);
        put_fixed(&mut out, &self.frame_plan_root);
        put_fixed(&mut out, &self.ordered_item_root);
        put_fixed(&mut out, &self.execution_stream_root);
        put_u64(&mut out, self.ordered_item_count);
        put_u64(&mut out, self.priority_start);
        put_u64(&mut out, self.priority_end);
        put_fixed(&mut out, &self.priority_root);
        put_fixed(&mut out, &self.oracle_snapshot_root);
        put_fixed(&mut out, &self.protocol_event_root);
        put_fixed(&mut out, &self.l1_origin_hash);
        put_fixed(&mut out, &self.policy_hash);
        put_fixed(&mut out, &self.decryption_module_id);
        out
    }
}

pub fn compute_execution_stream_root(
    hasher: &impl LighterNativeHash,
    domain_hash: Hash32,
    start_cursor: u64,
    items: &[ExecutionItemV3],
) -> Hash32 {
    let mut seed = Vec::with_capacity(12);
    seed.extend(hash32_u32_limbs(domain_hash));
    seed.push(start_cursor >> 32);
    seed.push(start_cursor & u32::MAX as u64);
    let item_count = items.len() as u64;
    seed.push(item_count >> 32);
    seed.push(item_count & u32::MAX as u64);
    let mut digest = hasher.hash_fields(EXECUTION_STREAM_INIT_DOMAIN, &seed);

    for item in items {
        let mut fields = Vec::with_capacity(14);
        fields.extend(hash32_u64_limbs(digest));
        fields.push(item.logical_index >> 32);
        fields.push(item.logical_index & u32::MAX as u64);
        fields.push(item.tx_type as u64);
        fields.extend(item.tx_hash);
        fields.push(item.outcome_class as u64);
        fields.push(item.terminal_noop as u64);
        digest = hasher.hash_fields(EXECUTION_STREAM_STEP_DOMAIN, &fields);
    }
    digest
}

fn hash32_u32_limbs(value: Hash32) -> [u64; 8] {
    core::array::from_fn(|index| {
        let offset = index * 4;
        u32::from_be_bytes(value[offset..offset + 4].try_into().expect("four bytes")) as u64
    })
}

fn hash32_u64_limbs(value: Hash32) -> [u64; 4] {
    core::array::from_fn(|index| {
        let offset = index * 8;
        u64::from_be_bytes(value[offset..offset + 8].try_into().expect("eight bytes"))
    })
}

pub fn compute_c_bind(hasher: &impl LighterNativeHash, inputs: &BindingInputsV3) -> Hash32 {
    hasher.hash(BINDING_DOMAIN, &inputs.canonical_bytes())
}

pub fn compute_ordered_item_root(
    hasher: &impl LighterNativeHash,
    domain_hash: Hash32,
    frame_id: u64,
    start_cursor: u64,
    items: &[DerivedItemV3],
) -> Hash32 {
    let mut accumulator = OrderedAccumulator::new(
        hasher,
        domain_hash,
        frame_id,
        start_cursor,
        items.len() as u64,
    );
    for (index, item) in items.iter().enumerate() {
        accumulator
            .advance(hasher, index as u64, item)
            .expect("enumerate is consecutive");
    }
    accumulator
        .finish()
        .expect("enumeration consumes the declared item count")
}
