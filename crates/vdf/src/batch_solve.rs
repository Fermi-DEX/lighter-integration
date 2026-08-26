//! Batched public solving for maturity waves (2026 whitepaper §4.3 path 2).
//!
//! Ciphertexts of the same delay class maturing in the same wave are solved
//! together. Two axes of batching:
//!
//! - **Work**: individual solves u_i^(2^T) are independent sequential chains;
//!   they are run in parallel across threads (the "second rig" model —
//!   solving hardware is horizontal). Amortizing the *sequential* work
//!   itself to one chain per wave requires the transparent batchable-TLP
//!   construction (Srinivasan et al., PKC 2023), which slots behind this
//!   interface later; this implementation batches proofs, not squarings.
//! - **Proof**: one aggregated Wesolowski proof per wave instead of one per
//!   ciphertext, via a random-linear-combination: with Fiat-Shamir scalars
//!   ρ_i, set U = Π u_i^ρ_i and W = Π w_i^ρ_i; then W = U^(2^T) holds iff
//!   (whp) every w_i = u_i^(2^T), and one proof certifies the wave.
//!
//! Verification cost per wave: the aggregation modexps (128-bit exponents)
//! plus one Wesolowski check — "one sequential computation of T_k steps plus
//! per-ciphertext bookkeeping" on the verifier side.

use crate::posq::{Group, SequentialProof};
use crate::tlk::{self, Ciphertext, TlkParams};
use crate::{VdfError, VdfResult};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const AGG_DOMAIN: &[u8] = b"posq-batch-agg-v1";

/// The witnesses for one maturity wave plus a single aggregated proof.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveSolution {
    pub class: u8,
    pub t_squarings: u64,
    /// (index-aligned with the input ciphertexts) solved witnesses w_i.
    pub witnesses: Vec<BigUint>,
    pub aggregate_proof: SequentialProof,
}

/// 128-bit Fiat-Shamir scalar for position i, bound to the whole wave.
fn rho(wave_binding: &[u8], i: usize, u: &BigUint, w: &BigUint) -> BigUint {
    let mut h = Sha256::new();
    h.update(AGG_DOMAIN);
    h.update((wave_binding.len() as u64).to_be_bytes());
    h.update(wave_binding);
    h.update((i as u64).to_be_bytes());
    h.update(u.to_bytes_be());
    h.update(w.to_bytes_be());
    BigUint::from_bytes_be(&h.finalize()[..16])
}

fn aggregate(
    group: &Group,
    wave_binding: &[u8],
    us: &[&BigUint],
    ws: &[BigUint],
) -> (BigUint, BigUint) {
    let n = &group.modulus;
    let mut cap_u = BigUint::from(1u32);
    let mut cap_w = BigUint::from(1u32);
    for (i, (u, w)) in us.iter().zip(ws.iter()).enumerate() {
        let e = rho(wave_binding, i, u, w);
        cap_u = (cap_u * u.modpow(&e, n)) % n;
        cap_w = (cap_w * w.modpow(&e, n)) % n;
    }
    (cap_u, cap_w)
}

/// Solve a wave: parallel per-ciphertext solves, one aggregated proof.
/// `wave_binding` should commit to the wave identity (epoch, maturity tick).
pub fn solve_wave(
    params: &TlkParams,
    class: u8,
    wave_binding: &[u8],
    cts: &[Ciphertext],
) -> VdfResult<WaveSolution> {
    let dc = params.class(class)?;
    let t = dc.t_squarings;
    for ct in cts {
        if ct.class != class || ct.u >= params.group.modulus || ct.u.bits() == 0 {
            return Err(VdfError::InvalidInput);
        }
    }

    // Parallel sequential solves (each chain is inherently sequential;
    // chains are independent).
    let group = params.group.clone();
    let witnesses: Vec<BigUint> = std::thread::scope(|scope| {
        let handles: Vec<_> = cts
            .iter()
            .map(|ct| {
                let g = &group;
                let u = ct.u.clone();
                scope.spawn(move || g.tick_advance(&u, t))
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("solver thread")).collect()
    });

    // Aggregate and produce one proof for W = U^(2^T).
    let us: Vec<&BigUint> = cts.iter().map(|c| &c.u).collect();
    let (cap_u, cap_w) = aggregate(&params.group, wave_binding, &us, &witnesses);
    let pi = params
        .group
        .prove_with_output(tlk::SOLVE_DOMAIN, &cap_u, &cap_w, t);
    let aggregate_proof = SequentialProof { y: cap_w, pi, iterations: t };

    Ok(WaveSolution { class, t_squarings: t, witnesses, aggregate_proof })
}

/// Verify a wave solution against its ciphertexts.
pub fn verify_wave(
    params: &TlkParams,
    wave_binding: &[u8],
    cts: &[Ciphertext],
    sol: &WaveSolution,
) -> VdfResult<bool> {
    let dc = params.class(sol.class)?;
    if sol.t_squarings != dc.t_squarings
        || sol.witnesses.len() != cts.len()
        || sol.aggregate_proof.iterations != dc.t_squarings
    {
        return Ok(false);
    }
    for (ct, w) in cts.iter().zip(sol.witnesses.iter()) {
        if ct.class != sol.class || *w >= params.group.modulus {
            return Ok(false);
        }
    }
    let us: Vec<&BigUint> = cts.iter().map(|c| &c.u).collect();
    let (cap_u, cap_w) = aggregate(&params.group, wave_binding, &us, &sol.witnesses);
    if cap_w != sol.aggregate_proof.y {
        return Ok(false);
    }
    params
        .group
        .verify_sequential(tlk::SOLVE_DOMAIN, &cap_u, &sol.aggregate_proof)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tlk::{encrypt, open, TlkParams};

    fn params() -> TlkParams {
        TlkParams::setup(Group::default_rsa2048(), &[64])
    }

    #[test]
    fn wave_roundtrip_and_open_all() {
        let p = params();
        let cts: Vec<Ciphertext> = (0..4)
            .map(|i| {
                encrypt(&p, 0, format!("intent {i}").as_bytes(), b"hdr", format!("seed{i}").as_bytes())
                    .unwrap()
            })
            .collect();
        let sol = solve_wave(&p, 0, b"epoch0-wave7", &cts).unwrap();
        assert!(verify_wave(&p, b"epoch0-wave7", &cts, &sol).unwrap());
        for (i, (ct, w)) in cts.iter().zip(sol.witnesses.iter()).enumerate() {
            assert_eq!(open(ct, w, b"hdr").unwrap(), format!("intent {i}").as_bytes());
        }
    }

    #[test]
    fn tampered_witness_fails_aggregate() {
        let p = params();
        let cts: Vec<Ciphertext> = (0..3)
            .map(|i| encrypt(&p, 0, b"x", b"", format!("s{i}").as_bytes()).unwrap())
            .collect();
        let mut sol = solve_wave(&p, 0, b"w", &cts).unwrap();
        sol.witnesses[1] = (&sol.witnesses[1] + BigUint::from(1u32)) % &p.group.modulus;
        assert!(!verify_wave(&p, b"w", &cts, &sol).unwrap());
    }

    #[test]
    fn wrong_wave_binding_fails() {
        let p = params();
        let cts = vec![encrypt(&p, 0, b"x", b"", b"s").unwrap()];
        let sol = solve_wave(&p, 0, b"wave-a", &cts).unwrap();
        assert!(!verify_wave(&p, b"wave-b", &cts, &sol).unwrap());
    }

    #[test]
    fn empty_wave_ok() {
        let p = params();
        let sol = solve_wave(&p, 0, b"w", &[]).unwrap();
        assert!(verify_wave(&p, b"w", &[], &sol).unwrap());
    }
}
