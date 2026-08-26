use crate::{
    hash::LighterNativeHash,
    types::{DerivedItemV3, Hash32},
};

pub const INIT_DOMAIN: &[u8] = b"CONTINUUM_LIGHTER_ITEMS_V3";
pub const STEP_DOMAIN: &[u8] = b"CONTINUUM_LIGHTER_ITEM_STEP_V3";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccumulatorError {
    NonConsecutiveIndex { expected: u64, actual: u64 },
    CountMismatch { expected: u64, actual: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderedAccumulator {
    next_index: u64,
    expected_count: u64,
    digest: Hash32,
}

impl OrderedAccumulator {
    pub fn new(
        hasher: &impl LighterNativeHash,
        domain_hash: Hash32,
        frame_id: u64,
        start_cursor: u64,
        item_count: u64,
    ) -> Self {
        let mut seed = Vec::with_capacity(56);
        seed.extend_from_slice(&domain_hash);
        seed.extend_from_slice(&frame_id.to_be_bytes());
        seed.extend_from_slice(&start_cursor.to_be_bytes());
        seed.extend_from_slice(&item_count.to_be_bytes());
        Self {
            next_index: 0,
            expected_count: item_count,
            digest: hasher.hash(INIT_DOMAIN, &seed),
        }
    }

    /// Advance once for one logical input. Matching sub-cycles must never call
    /// this method.
    pub fn advance(
        &mut self,
        hasher: &impl LighterNativeHash,
        logical_index: u64,
        item: &DerivedItemV3,
    ) -> Result<(), AccumulatorError> {
        if logical_index != self.next_index {
            return Err(AccumulatorError::NonConsecutiveIndex {
                expected: self.next_index,
                actual: logical_index,
            });
        }
        if self.next_index >= self.expected_count {
            return Err(AccumulatorError::CountMismatch {
                expected: self.expected_count,
                actual: self.next_index + 1,
            });
        }

        let item_bytes = item.canonical_bytes();
        let mut payload = Vec::with_capacity(32 + 8 + item_bytes.len());
        payload.extend_from_slice(&self.digest);
        payload.extend_from_slice(&logical_index.to_be_bytes());
        payload.extend_from_slice(&item_bytes);
        self.digest = hasher.hash(STEP_DOMAIN, &payload);
        self.next_index += 1;
        Ok(())
    }

    pub fn digest(&self) -> Hash32 {
        self.digest
    }

    pub fn consumed(&self) -> u64 {
        self.next_index
    }

    pub fn finish(self) -> Result<Hash32, AccumulatorError> {
        if self.next_index != self.expected_count {
            return Err(AccumulatorError::CountMismatch {
                expected: self.expected_count,
                actual: self.next_index,
            });
        }
        Ok(self.digest)
    }
}
