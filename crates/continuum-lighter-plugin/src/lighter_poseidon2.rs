//! Exact Poseidon2 implementation from Lighter's pinned Plonky2 fork.
//!
//! The protocol packing is deliberately narrow: a length field followed by
//! big-endian 32-bit limbs. This matches the existing Lighter blob-header
//! circuit convention and keeps every limb canonical in Goldilocks.

use plonky2::{
    field::{
        goldilocks_field::GoldilocksField,
        types::{Field, PrimeField64},
    },
    hash::{hash_types::HashOut, poseidon2::hash::Poseidon2Hash},
    plonk::config::Hasher,
};

use crate::{hash::LighterNativeHash, types::Hash32};

#[derive(Clone, Copy, Debug, Default)]
pub struct LighterPoseidon2Hasher;

impl LighterPoseidon2Hasher {
    fn pack_bytes(bytes: &[u8], out: &mut Vec<GoldilocksField>) {
        out.push(GoldilocksField::from_canonical_u64(bytes.len() as u64));
        for chunk in bytes.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            out.push(GoldilocksField::from_canonical_u64(
                u32::from_be_bytes(word) as u64,
            ));
        }
    }

    fn serialize(hash: HashOut<GoldilocksField>) -> Hash32 {
        let mut out = [0u8; 32];
        for (index, limb) in hash.elements.iter().enumerate() {
            out[index * 8..(index + 1) * 8]
                .copy_from_slice(&limb.to_canonical_u64().to_be_bytes());
        }
        out
    }
}

impl LighterNativeHash for LighterPoseidon2Hasher {
    fn hash(&self, domain: &'static [u8], payload: &[u8]) -> Hash32 {
        let mut fields = Vec::with_capacity(2 + domain.len().div_ceil(4) + payload.len().div_ceil(4));
        Self::pack_bytes(domain, &mut fields);
        Self::pack_bytes(payload, &mut fields);
        Self::serialize(Poseidon2Hash::hash_no_pad(&fields))
    }

    fn hash_fields(&self, domain: &'static [u8], payload: &[u64]) -> Hash32 {
        let mut fields = Vec::with_capacity(2 + domain.len().div_ceil(4) + payload.len());
        Self::pack_bytes(domain, &mut fields);
        fields.push(GoldilocksField::from_canonical_u64(payload.len() as u64));
        fields.extend(
            payload
                .iter()
                .map(|value| GoldilocksField::from_canonical_u64(*value)),
        );
        Self::serialize(Poseidon2Hash::hash_no_pad(&fields))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_and_length_are_bound() {
        let hasher = LighterPoseidon2Hasher;
        assert_ne!(hasher.hash(b"A", b"BC"), hasher.hash(b"AB", b"C"));
        assert_ne!(hasher.hash(b"A", b""), hasher.hash(b"A", &[0]));
    }

    #[test]
    fn serialization_is_four_big_endian_limbs() {
        let hash = HashOut {
            elements: [
                GoldilocksField::from_canonical_u64(1),
                GoldilocksField::from_canonical_u64(2),
                GoldilocksField::from_canonical_u64(3),
                GoldilocksField::from_canonical_u64(4),
            ],
        };
        let bytes = LighterPoseidon2Hasher::serialize(hash);
        assert_eq!(&bytes[0..8], &1u64.to_be_bytes());
        assert_eq!(&bytes[24..32], &4u64.to_be_bytes());
    }
}
