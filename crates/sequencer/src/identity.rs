//! Sequencer identity: secp256k1 (ECDSA) with keccak256 digests, so every
//! signed object — tick records, receipts, rejections, anchors — is
//! verifiable on the Ethereum host chain with `ecrecover` (§10: "fraud
//! proofs ... natively verifiable ... with no custom cryptography
//! on-chain"). Signatures are 65 bytes (r ‖ s ‖ v).
//!
//! Client intents use the same scheme, with the account identified by its
//! 20-byte Ethereum-style address.

use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha3::{Digest, Keccak256};

pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(data);
    h.finalize().into()
}

/// 20-byte Ethereum-style address of a verifying key.
pub fn address_of(vk: &VerifyingKey) -> [u8; 20] {
    let encoded = vk.to_encoded_point(false);
    let digest = keccak256(&encoded.as_bytes()[1..]); // skip 0x04 prefix
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&digest[12..]);
    addr
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sig65(pub [u8; 65]);

impl Sig65 {
    pub fn as_bytes(&self) -> &[u8; 65] {
        &self.0
    }
}

impl Serialize for Sig65 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sig65 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v: Vec<u8> = serde_bytes_compat(deserializer)?;
        let arr: [u8; 65] = v.try_into().map_err(|_| D::Error::custom("expected 65 bytes"))?;
        Ok(Sig65(arr))
    }
}

fn serde_bytes_compat<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<u8>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bytes")
        }
        fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
            Ok(v.to_vec())
        }
        fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
            Ok(v)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(b) = seq.next_element::<u8>()? {
                out.push(b);
            }
            Ok(out)
        }
    }
    d.deserialize_bytes(V)
}

#[derive(Clone)]
pub struct Identity {
    sk: SigningKey,
}

impl Identity {
    pub fn from_seed(seed: &[u8]) -> Self {
        // Derive a valid scalar deterministically from the seed.
        let mut counter = 0u32;
        loop {
            let mut material = keccak256(&[b"posq-identity".as_slice(), &counter.to_be_bytes(), seed].concat());
            material[0] &= 0x7f; // keep well below the curve order
            if let Ok(sk) = SigningKey::from_slice(&material) {
                return Self { sk };
            }
            counter += 1;
        }
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        *self.sk.verifying_key()
    }

    pub fn address(&self) -> [u8; 20] {
        address_of(&self.sk.verifying_key())
    }

    /// Sign a 32-byte digest, producing an ecrecover-compatible signature.
    pub fn sign_digest(&self, digest: &[u8; 32]) -> Sig65 {
        let (sig, rid) = self
            .sk
            .sign_prehash_recoverable(digest)
            .expect("signing cannot fail for a valid key");
        let mut out = [0u8; 65];
        out[..64].copy_from_slice(&sig.to_bytes());
        out[64] = 27 + rid.to_byte(); // Ethereum v convention
        Sig65(out)
    }

    /// Sign the keccak256 of an encoded message.
    pub fn sign(&self, message: &[u8]) -> Sig65 {
        self.sign_digest(&keccak256(message))
    }
}

/// Recover the signer address from a digest and 65-byte signature.
pub fn recover_address(digest: &[u8; 32], sig: &Sig65) -> Option<[u8; 20]> {
    let signature = Signature::from_slice(&sig.0[..64]).ok()?;
    let v = sig.0[64].checked_sub(27)?;
    let rid = RecoveryId::from_byte(v)?;
    let vk = VerifyingKey::recover_from_prehash(digest, &signature, rid).ok()?;
    Some(address_of(&vk))
}

/// Verify that `sig` over keccak256(message) recovers to `expected`.
pub fn verify_signed(message: &[u8], sig: &Sig65, expected: &[u8; 20]) -> bool {
    recover_address(&keccak256(message), sig).as_ref() == Some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_recover_roundtrip() {
        let id = Identity::from_seed(b"test seed");
        let msg = b"tick record encoding";
        let sig = id.sign(msg);
        assert!(verify_signed(msg, &sig, &id.address()));
        assert!(!verify_signed(b"different message", &sig, &id.address()));
    }

    #[test]
    fn deterministic_from_seed() {
        let a = Identity::from_seed(b"seed");
        let b = Identity::from_seed(b"seed");
        assert_eq!(a.address(), b.address());
        assert_ne!(a.address(), Identity::from_seed(b"other").address());
    }

    #[test]
    fn tampered_sig_recovers_wrong_or_none() {
        let id = Identity::from_seed(b"s");
        let mut sig = id.sign(b"m");
        sig.0[3] ^= 0xff;
        assert!(!verify_signed(b"m", &sig, &id.address()));
    }
}
