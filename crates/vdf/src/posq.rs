//! PoSq group and proof primitives (2026 whitepaper §4.1, §4.4).
//!
//! Everything sequential in PoSq — the tick VDF and the timelock — is
//! repeated squaring in QR_N of one RSA-2048 modulus. This module provides:
//!
//! - `hash_to_group`: domain-separated hash into QR_N (expand to 2048 bits,
//!   reduce, square — squaring confines the element to the quadratic
//!   residues and kills small-order components, §4.1),
//! - element-based Wesolowski prove/verify with a **streaming quotient**
//!   (sound at arbitrary T; the challenge prime comes from the fixed
//!   Miller-Rabin-backed `hash_to_prime`),
//! - `tick_advance`: the per-tick state transition x_t = x_{t-1}^(2^q).
//!
//! Streaming quotient: π = x^⌊2^T / l⌋ is accumulated by long division —
//! per step, π = π² · x^b where b is the next quotient bit — so no 2^T-sized
//! integer is ever materialized and T is unbounded.

use crate::{hash_to_prime, is_probably_prime, VdfError, VdfResult};
use num_bigint::BigUint;
use num_traits::One;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The shared group: QR_N of an RSA-2048 modulus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub modulus: BigUint,
}

impl Group {
    pub fn new(modulus: BigUint) -> Self {
        Self { modulus }
    }

    /// The reference modulus (RSA-2048 challenge number; ceremony output in
    /// production — provenance placeholder, size-correct).
    pub fn default_rsa2048() -> Self {
        Self::new(crate::backend::VdfParams::default().modulus)
    }

    /// Domain-separated hash into QR_N: expand input to 2048+ bits via
    /// counter-mode SHA-256, reduce mod N, then square.
    pub fn hash_to_group(&self, domain: &[u8], input: &[u8]) -> BigUint {
        let mut wide = Vec::with_capacity(32 * 9);
        for counter in 0u32..9 {
            let mut h = Sha256::new();
            h.update(b"posq-hash-to-group");
            h.update((domain.len() as u64).to_be_bytes());
            h.update(domain);
            h.update(counter.to_be_bytes());
            h.update(input);
            wide.extend_from_slice(&h.finalize());
        }
        let x = BigUint::from_bytes_be(&wide) % &self.modulus;
        // Square into QR_N.
        (&x * &x) % &self.modulus
    }

    /// Confine an externally received element to QR_N (square on ingest).
    pub fn to_qr(&self, x: &BigUint) -> BigUint {
        (x * x) % &self.modulus
    }

    /// x^(2^q) mod N by q sequential squarings.
    pub fn tick_advance(&self, x: &BigUint, q: u64) -> BigUint {
        let mut y = x.clone();
        for _ in 0..q {
            y = (&y * &y) % &self.modulus;
        }
        y
    }
}

/// A Wesolowski proof that y = x^(2^t) mod N.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequentialProof {
    pub y: BigUint,
    pub pi: BigUint,
    pub iterations: u64,
}

/// Pad a group element to 256 bytes big-endian (EVM word-aligned encoding;
/// the host contract reconstructs challenge preimages byte-for-byte).
pub fn pad256(x: &BigUint) -> [u8; 256] {
    let bytes = x.to_bytes_be();
    let mut out = [0u8; 256];
    out[256 - bytes.len()..].copy_from_slice(&bytes);
    out
}

/// Fiat-Shamir challenge prime l = HashToPrime(domain ‖ x ‖ y ‖ t), with x
/// and y in fixed 256-byte encoding for on-chain reconstruction.
pub fn challenge_prime(domain: &[u8], x: &BigUint, y: &BigUint, t: u64) -> BigUint {
    let mut h = Sha256::new();
    h.update(b"posq-wesolowski-challenge");
    h.update((domain.len() as u64).to_be_bytes());
    h.update(domain);
    h.update(pad256(x));
    h.update(pad256(y));
    h.update(t.to_be_bytes());
    let l = hash_to_prime(&h.finalize());
    debug_assert!(is_probably_prime(&l));
    l
}

/// Compute x^⌊2^t / l⌋ mod N by streaming long division (never materializes
/// 2^t). Cost: t iterations of one squaring plus an occasional multiply.
pub fn quotient_power(group: &Group, x: &BigUint, t: u64, l: &BigUint) -> BigUint {
    let n = &group.modulus;
    let mut pi = BigUint::one();
    let mut r = BigUint::one(); // remainder of 2^i / l, starts at 2^0 = 1
    for _ in 0..t {
        // shift in the next power of two
        r <<= 1;
        let bit = if r >= *l {
            r -= l;
            true
        } else {
            false
        };
        pi = (&pi * &pi) % n;
        if bit {
            pi = (&pi * x) % n;
        }
    }
    pi
}

/// 2^t mod l without materializing 2^t.
pub fn two_pow_mod(t: u64, l: &BigUint) -> BigUint {
    BigUint::from(2u32).modpow(&BigUint::from(t), l)
}

impl Group {
    /// Prove y = x^(2^t): performs the t squarings and the quotient pass.
    /// (A production prover overlaps the quotient pass with the next
    /// segment's advance on separate hardware; here they are sequential.)
    pub fn prove_sequential(&self, domain: &[u8], x: &BigUint, t: u64) -> SequentialProof {
        let y = self.tick_advance(x, t);
        let pi = self.prove_with_output(domain, x, &y, t);
        SequentialProof { y, pi, iterations: t }
    }

    /// Produce the proof when the output y = x^(2^t) is already known
    /// (e.g. computed incrementally by the clock).
    pub fn prove_with_output(
        &self,
        domain: &[u8],
        x: &BigUint,
        y: &BigUint,
        t: u64,
    ) -> BigUint {
        let l = challenge_prime(domain, x, y, t);
        quotient_power(self, x, t, &l)
    }

    /// Verify a Wesolowski proof: pi^l · x^(2^t mod l) == y (mod N).
    pub fn verify_sequential(
        &self,
        domain: &[u8],
        x: &BigUint,
        proof: &SequentialProof,
    ) -> VdfResult<bool> {
        let n = &self.modulus;
        if proof.y >= *n || proof.pi >= *n {
            return Err(VdfError::InvalidInput);
        }
        let l = challenge_prime(domain, x, &proof.y, proof.iterations);
        let r = two_pow_mod(proof.iterations, &l);
        let lhs = (proof.pi.modpow(&l, n) * x.modpow(&r, n)) % n;
        Ok(lhs == proof.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_group() -> Group {
        Group::default_rsa2048()
    }

    #[test]
    fn miller_rabin_rejects_composites() {
        assert!(!is_probably_prime(&BigUint::from(9u32)));
        assert!(!is_probably_prime(&BigUint::from(561u32))); // Carmichael
        assert!(!is_probably_prime(&BigUint::from(1105u32))); // Carmichael
        assert!(is_probably_prime(&BigUint::from(104729u32)));
        // The old stub accepted any odd number > 3.
        assert!(!is_probably_prime(&BigUint::from(15u32)));
    }

    #[test]
    fn hash_to_group_lands_in_range_and_differs_by_domain() {
        let g = small_group();
        let a = g.hash_to_group(b"a", b"input");
        let b = g.hash_to_group(b"b", b"input");
        assert!(a < g.modulus && b < g.modulus);
        assert_ne!(a, b);
    }

    #[test]
    fn prove_verify_roundtrip_various_t() {
        let g = small_group();
        let x = g.hash_to_group(b"test", b"base");
        for t in [1u64, 2, 27, 100, 3000, 5000] {
            let proof = g.prove_sequential(b"test", &x, t);
            assert!(g.verify_sequential(b"test", &x, &proof).unwrap(), "t={t}");
        }
    }

    #[test]
    fn tampered_proof_rejected() {
        let g = small_group();
        let x = g.hash_to_group(b"test", b"base");
        let mut proof = g.prove_sequential(b"test", &x, 3000);
        proof.y = (&proof.y + BigUint::one()) % &g.modulus;
        assert!(!g.verify_sequential(b"test", &x, &proof).unwrap());

        let mut proof2 = g.prove_sequential(b"test", &x, 3000);
        proof2.pi = (&proof2.pi + BigUint::one()) % &g.modulus;
        assert!(!g.verify_sequential(b"test", &x, &proof2).unwrap());
    }

    #[test]
    fn wrong_domain_or_t_rejected() {
        // Note: for t below the ~256-bit challenge size the quotient is zero
        // and verification degenerates to honest recomputation (still sound,
        // just not succinct); the domain/t binding only has teeth at t > 256.
        let g = small_group();
        let x = g.hash_to_group(b"test", b"base");
        let proof = g.prove_sequential(b"test", &x, 3000);
        assert!(!g.verify_sequential(b"other", &x, &proof).unwrap());
        let mut wrong_t = proof.clone();
        wrong_t.iterations = 2999;
        assert!(!g.verify_sequential(b"test", &x, &wrong_t).unwrap());
    }

    #[test]
    fn quotient_power_matches_naive_for_small_t() {
        // For small t, ⌊2^t/l⌋ can be computed directly; cross-check streaming.
        let g = small_group();
        let x = g.hash_to_group(b"q", b"x");
        let l = BigUint::from(1000003u64);
        let t = 40u64;
        let naive_q = (BigUint::one() << t) / &l;
        let expected = x.modpow(&naive_q, &g.modulus);
        assert_eq!(quotient_power(&g, &x, t, &l), expected);
    }

    #[test]
    fn tick_advance_composes() {
        let g = small_group();
        let x = g.hash_to_group(b"c", b"x");
        let a = g.tick_advance(&g.tick_advance(&x, 10), 15);
        let b = g.tick_advance(&x, 25);
        assert_eq!(a, b);
    }
}
