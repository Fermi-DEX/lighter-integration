//! The PoSq timelock KEM (2026 whitepaper §4.2), **solve-only profile**.
//!
//! Fixed-base, public-coin: a one-time transparent setup publishes, per
//! delay class k, (g, h_k = g^(2^T_k) mod N, π_k) — a public sequential
//! computation with a Wesolowski proof; no secret, no toxic waste.
//!
//! - Enc (client, fast): sample r; u = g^r; K = H(kem ‖ h_k^r);
//!   c = AEAD_K(plaintext; AAD = envelope header). Ciphertext ct = (k, u, c).
//! - Solve (anyone): w = u^(2^T_k) by T_k sequential squarings, with a
//!   Wesolowski proof. By construction u^(2^T_k) = g^(r·2^T_k) = h_k^r, so
//!   the solve yields the same witness and hence the same key.
//! - Open: recompute K = H(kem ‖ w), decrypt.
//!
//! This deployment profile has **no client-assisted reveal path**: openings
//! come exclusively from public solving (per-ciphertext, or batched — see
//! `batch_solve`). Dropping the reveal drops the DLEQ machinery; nothing
//! else changes. h_k remains required — it is what lets the *encryptor*
//! compute the session key without sequential work.
//!
//! AEAD is ChaCha20-Poly1305 with the envelope header bound as associated
//! data (CCA discipline, §4.2): a mauled ciphertext opens to garbage and
//! lands as a paid gap.

use crate::posq::{Group, SequentialProof};
use crate::{VdfError, VdfResult};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SOLVE_DOMAIN: &[u8] = b"posq-tlk-solve-v1";
const SETUP_DOMAIN: &[u8] = b"posq-tlk-setup-v1";
const GENERATOR_DOMAIN: &[u8] = b"posq-tlk-generator-v1";

/// One delay class: T_k squarings, with h_k = g^(2^T_k) and its proof.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DelayClass {
    pub t_squarings: u64,
    pub h: BigUint,
    pub proof: SequentialProof,
}

/// The public timelock parameters: (N, g, {h_k, T_k, π_k}).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TlkParams {
    pub group: Group,
    pub g: BigUint,
    pub classes: Vec<DelayClass>,
}

/// A timelock ciphertext: ct = (k, u, c).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ciphertext {
    pub class: u8,
    pub u: BigUint,
    pub body: Vec<u8>,
}

impl TlkParams {
    /// One-time transparent setup: derive the nothing-up-my-sleeve generator
    /// and compute each h_k by public sequential work with a proof. Slow at
    /// production T_k (by design); run once and serialize.
    pub fn setup(group: Group, delays: &[u64]) -> Self {
        let g = group.hash_to_group(GENERATOR_DOMAIN, b"posq fixed base g");
        let classes = delays
            .iter()
            .map(|&t| {
                let proof = group.prove_sequential(SETUP_DOMAIN, &g, t);
                DelayClass { t_squarings: t, h: proof.y.clone(), proof }
            })
            .collect();
        Self { group, g, classes }
    }

    /// Verify a received setup (anyone can: the computation is public-coin).
    pub fn verify(&self) -> VdfResult<bool> {
        let expected_g = self
            .group
            .hash_to_group(GENERATOR_DOMAIN, b"posq fixed base g");
        if self.g != expected_g {
            return Ok(false);
        }
        for c in &self.classes {
            if c.proof.y != c.h || c.proof.iterations != c.t_squarings {
                return Ok(false);
            }
            if !self.group.verify_sequential(SETUP_DOMAIN, &self.g, &c.proof)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn class(&self, k: u8) -> VdfResult<&DelayClass> {
        self.classes
            .get(k as usize)
            .ok_or(VdfError::InvalidInput)
    }
}

fn session_key(w: &BigUint) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"posq-kem-key-v1");
    h.update(w.to_bytes_be());
    h.finalize().into()
}

fn nonce_from_u(u: &BigUint) -> [u8; 12] {
    // The key is unique per fresh r, so a u-derived nonce never repeats
    // under one key.
    let mut h = Sha256::new();
    h.update(b"posq-kem-nonce-v1");
    h.update(u.to_bytes_be());
    let out = h.finalize();
    let mut n = [0u8; 12];
    n.copy_from_slice(&out[..12]);
    n
}

/// Client-side encryption: two modexps, no sequential work.
pub fn encrypt(
    params: &TlkParams,
    class: u8,
    plaintext: &[u8],
    aad: &[u8],
    rng_seed: &[u8],
) -> VdfResult<Ciphertext> {
    let dc = params.class(class)?;
    let n = &params.group.modulus;
    // r from caller-supplied entropy, wide enough to be uniform mod ord(g).
    let mut wide = Vec::with_capacity(32 * 10);
    for i in 0u32..10 {
        let mut h = Sha256::new();
        h.update(b"posq-kem-r");
        h.update(i.to_be_bytes());
        h.update(rng_seed);
        wide.extend_from_slice(&h.finalize());
    }
    let r = BigUint::from_bytes_be(&wide);
    let u = params.g.modpow(&r, n);
    let w = dc.h.modpow(&r, n);

    let cipher = ChaCha20Poly1305::new(Key::from_slice(&session_key(&w)));
    let body = cipher
        .encrypt(
            Nonce::from_slice(&nonce_from_u(&u)),
            Payload { msg: plaintext, aad },
        )
        .map_err(|_| VdfError::ComputationError("aead encrypt".into()))?;

    Ok(Ciphertext { class, u, body })
}

/// A verified opening witness for one ciphertext.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub w: BigUint,
    pub proof: SequentialProof,
}

/// Public solve: T_k sequential squarings from u, with a Wesolowski proof.
/// The element is squared on ingest (QR_N discipline); encryptors must
/// account for this by treating u as u² — `encrypt` output is already in
/// QR_N because g is, so `to_qr` is a no-op for honest ciphertexts only in
/// exponent terms; we therefore solve from u directly but reject u ∉ [1, N).
pub fn solve(params: &TlkParams, ct: &Ciphertext) -> VdfResult<Opening> {
    let dc = params.class(ct.class)?;
    if ct.u >= params.group.modulus || ct.u.bits() == 0 {
        return Err(VdfError::InvalidInput);
    }
    let proof = params
        .group
        .prove_sequential(SOLVE_DOMAIN, &ct.u, dc.t_squarings);
    Ok(Opening { w: proof.y.clone(), proof })
}

/// Verify an opening against its ciphertext.
pub fn verify_opening(params: &TlkParams, ct: &Ciphertext, opening: &Opening) -> VdfResult<bool> {
    let dc = params.class(ct.class)?;
    if opening.proof.iterations != dc.t_squarings || opening.proof.y != opening.w {
        return Ok(false);
    }
    params
        .group
        .verify_sequential(SOLVE_DOMAIN, &ct.u, &opening.proof)
}

/// Open: recompute the key from the witness and decrypt.
/// Returns Err for a mauled/garbage ciphertext — the caller records a gap.
pub fn open(ct: &Ciphertext, w: &BigUint, aad: &[u8]) -> VdfResult<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&session_key(w)));
    cipher
        .decrypt(
            Nonce::from_slice(&nonce_from_u(&ct.u)),
            Payload { msg: ct.body.as_slice(), aad },
        )
        .map_err(|_| VdfError::InvalidProof)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_params() -> TlkParams {
        TlkParams::setup(Group::default_rsa2048(), &[64, 256])
    }

    #[test]
    fn setup_verifies() {
        let p = test_params();
        assert!(p.verify().unwrap());
    }

    #[test]
    fn tampered_setup_rejected() {
        let mut p = test_params();
        p.classes[0].h = (&p.classes[0].h + BigUint::from(1u32)) % &p.group.modulus;
        assert!(!p.verify().unwrap());
    }

    #[test]
    fn encrypt_solve_open_roundtrip() {
        let p = test_params();
        let aad = b"envelope-header";
        let ct = encrypt(&p, 0, b"the intent", aad, b"client entropy 1").unwrap();
        let opening = solve(&p, &ct).unwrap();
        assert!(verify_opening(&p, &ct, &opening).unwrap());
        assert_eq!(open(&ct, &opening.w, aad).unwrap(), b"the intent");
    }

    #[test]
    fn solve_matches_encryptor_witness() {
        // u^(2^T) == h^r: both paths yield the same witness.
        let p = test_params();
        let ct = encrypt(&p, 1, b"x", b"", b"seed").unwrap();
        let opening = solve(&p, &ct).unwrap();
        // The opening decrypts — meaning the solved w equals the encryptor's
        // h^r (checked implicitly by AEAD authentication).
        assert!(open(&ct, &opening.w, b"").is_ok());
    }

    #[test]
    fn wrong_aad_fails_open() {
        let p = test_params();
        let ct = encrypt(&p, 0, b"payload", b"header-A", b"seed2").unwrap();
        let opening = solve(&p, &ct).unwrap();
        assert!(open(&ct, &opening.w, b"header-B").is_err());
    }

    #[test]
    fn mauled_ciphertext_opens_to_error_not_plaintext() {
        let p = test_params();
        let mut ct = encrypt(&p, 0, b"payload", b"hdr", b"seed3").unwrap();
        ct.body[0] ^= 0xff;
        let opening = solve(&p, &ct).unwrap();
        assert!(open(&ct, &opening.w, b"hdr").is_err());
    }

    #[test]
    fn copied_ciphertext_yields_same_plaintext() {
        // Ciphertext-copying is harmless (§4.2): the copier gets the victim's
        // signed intent, which fails downstream nonce/signature checks.
        let p = test_params();
        let ct = encrypt(&p, 0, b"victim intent", b"hdr", b"seed4").unwrap();
        let copy = ct.clone();
        let opening = solve(&p, &copy).unwrap();
        assert_eq!(open(&copy, &opening.w, b"hdr").unwrap(), b"victim intent");
    }
}
