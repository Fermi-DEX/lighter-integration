//! Fixed-size envelopes, fee tickets, and plaintext intents (§7).
//!
//! Every market-action envelope is exactly `S` bytes (reference 1024): for
//! a given namespace and epoch there is exactly one valid envelope size, so
//! size cannot fingerprint flow. The visible surface is the header (bucket,
//! namespace, window, delay class, nonce) and the fee ticket; the operation
//! type lives inside the ciphertext (operation indistinguishability, §7.2).
//!
//! Fee tickets here are prepaid bearer credentials in the simplified MVD
//! form: issuer-signed fixed denominations with a nullifier set, consumed at
//! receipt issuance and never refunded (G4). Blind signing (unlinkability)
//! is the documented deferral. The ticket denomination doubles as the delay
//! class, since the delay menu is implied by the visible fee class (§4.2).

use crate::identity::{keccak256, Identity, Sig65};
use serde::{Deserialize, Serialize};
use vdf::tlk::Ciphertext;

pub const ENVELOPE_MAGIC: [u8; 2] = *b"PQ";
pub const ENVELOPE_VERSION: u8 = 1;
/// Header prefix bound as AEAD associated data (bytes 0..39).
pub const HEADER_AAD_LEN: usize = 39;
const U_MAX: usize = 256;
const BODY_OFFSET: usize = 382;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Bucket {
    Majors = 0,
    LongTail = 1,
}

impl Bucket {
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(Bucket::Majors),
            1 => Some(Bucket::LongTail),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub start: u64,
    pub len: u16,
}

impl Window {
    pub fn contains(&self, tick: u64) -> bool {
        tick >= self.start && tick < self.start + self.len as u64
    }
    pub fn end_inclusive(&self) -> u64 {
        self.start + self.len as u64 - 1
    }
}

/// Prepaid bearer fee ticket (simplified MVD form).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    pub id: [u8; 16],
    /// Denomination == delay class (fee class implies the delay menu entry).
    pub denomination: u8,
    pub bucket: Bucket,
    pub issuer_sig: Sig65,
}

impl Ticket {
    pub fn signing_message(id: &[u8; 16], denomination: u8, bucket: Bucket) -> Vec<u8> {
        let mut m = Vec::with_capacity(64);
        m.extend_from_slice(b"posq-ticket-v1");
        m.extend_from_slice(id);
        m.push(denomination);
        m.push(bucket as u8);
        m
    }

    pub fn issue(issuer: &Identity, id: [u8; 16], denomination: u8, bucket: Bucket) -> Self {
        let sig = issuer.sign(&Self::signing_message(&id, denomination, bucket));
        Self { id, denomination, bucket, issuer_sig: sig }
    }

    pub fn verify(&self, issuer_address: &[u8; 20]) -> bool {
        crate::identity::verify_signed(
            &Self::signing_message(&self.id, self.denomination, self.bucket),
            &self.issuer_sig,
            issuer_address,
        )
    }
}

/// The visible envelope contents after parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub bucket: Bucket,
    pub namespace: u64,
    pub window: Window,
    pub delay_class: u8,
    pub nonce: [u8; 16],
    pub ticket: Ticket,
    pub ciphertext: Ciphertext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvelopeError {
    WrongSize,
    BadMagic,
    BadVersion,
    BadBucket,
    BadField,
}

/// The 39-byte header prefix (magic ‖ version ‖ bucket ‖ namespace ‖
/// window ‖ delay class ‖ nonce) — the AEAD associated data. Clients build
/// it before encrypting; `encode` writes the identical bytes.
pub fn header_prefix(
    bucket: Bucket,
    namespace: u64,
    window: Window,
    delay_class: u8,
    nonce: &[u8; 16],
) -> [u8; HEADER_AAD_LEN] {
    let mut out = [0u8; HEADER_AAD_LEN];
    out[0..2].copy_from_slice(&ENVELOPE_MAGIC);
    out[2] = ENVELOPE_VERSION;
    out[3] = bucket as u8;
    out[4..12].copy_from_slice(&namespace.to_be_bytes());
    out[12..20].copy_from_slice(&window.start.to_be_bytes());
    out[20..22].copy_from_slice(&window.len.to_be_bytes());
    out[22] = delay_class;
    out[23..39].copy_from_slice(nonce);
    out
}

impl Envelope {
    /// Serialize to exactly `size` bytes; fails if the body doesn't fit.
    pub fn encode(&self, size: usize) -> Result<Vec<u8>, EnvelopeError> {
        let u_bytes = self.ciphertext.u.to_bytes_be();
        if u_bytes.len() > U_MAX {
            return Err(EnvelopeError::BadField);
        }
        let body = &self.ciphertext.body;
        if BODY_OFFSET + body.len() > size || body.len() > u16::MAX as usize {
            return Err(EnvelopeError::WrongSize);
        }
        let mut out = vec![0u8; size];
        out[..HEADER_AAD_LEN].copy_from_slice(&header_prefix(
            self.bucket,
            self.namespace,
            self.window,
            self.delay_class,
            &self.nonce,
        ));
        out[39..55].copy_from_slice(&self.ticket.id);
        out[55] = self.ticket.denomination;
        out[56] = self.ticket.bucket as u8;
        out[57..122].copy_from_slice(self.ticket.issuer_sig.as_bytes());
        out[122..124].copy_from_slice(&(u_bytes.len() as u16).to_be_bytes());
        out[124 + (U_MAX - u_bytes.len())..124 + U_MAX].copy_from_slice(&u_bytes);
        out[380..382].copy_from_slice(&(body.len() as u16).to_be_bytes());
        out[BODY_OFFSET..BODY_OFFSET + body.len()].copy_from_slice(body);
        // ciphertext delay class must match the visible one; carried
        // implicitly (single field).
        Ok(out)
    }

    pub fn decode(bytes: &[u8], expected_size: usize) -> Result<Self, EnvelopeError> {
        if bytes.len() != expected_size {
            return Err(EnvelopeError::WrongSize);
        }
        if bytes[0..2] != ENVELOPE_MAGIC {
            return Err(EnvelopeError::BadMagic);
        }
        if bytes[2] != ENVELOPE_VERSION {
            return Err(EnvelopeError::BadVersion);
        }
        let bucket = Bucket::from_u8(bytes[3]).ok_or(EnvelopeError::BadBucket)?;
        let ticket_bucket = Bucket::from_u8(bytes[56]).ok_or(EnvelopeError::BadBucket)?;
        let namespace = u64::from_be_bytes(bytes[4..12].try_into().unwrap());
        let window = Window {
            start: u64::from_be_bytes(bytes[12..20].try_into().unwrap()),
            len: u16::from_be_bytes(bytes[20..22].try_into().unwrap()),
        };
        if window.len == 0 {
            return Err(EnvelopeError::BadField);
        }
        let delay_class = bytes[22];
        let nonce: [u8; 16] = bytes[23..39].try_into().unwrap();
        let ticket = Ticket {
            id: bytes[39..55].try_into().unwrap(),
            denomination: bytes[55],
            bucket: ticket_bucket,
            issuer_sig: Sig65(bytes[57..122].try_into().unwrap()),
        };
        let u_len = u16::from_be_bytes(bytes[122..124].try_into().unwrap()) as usize;
        if u_len > U_MAX {
            return Err(EnvelopeError::BadField);
        }
        let u = num_bigint::BigUint::from_bytes_be(&bytes[124 + (U_MAX - u_len)..124 + U_MAX]);
        let body_len = u16::from_be_bytes(bytes[380..382].try_into().unwrap()) as usize;
        if BODY_OFFSET + body_len > expected_size {
            return Err(EnvelopeError::BadField);
        }
        let body = bytes[BODY_OFFSET..BODY_OFFSET + body_len].to_vec();
        // Padding must be zero (uniformity: no covert channel in the pad).
        if bytes[BODY_OFFSET + body_len..].iter().any(|&b| b != 0) {
            return Err(EnvelopeError::BadField);
        }
        Ok(Self {
            bucket,
            namespace,
            window,
            delay_class,
            nonce,
            ticket,
            ciphertext: Ciphertext { class: delay_class, u, body },
        })
    }

    /// The AEAD associated data: the header prefix.
    pub fn header_aad(bytes: &[u8]) -> &[u8] {
        &bytes[..HEADER_AAD_LEN]
    }

    /// Commitment hash h = keccak(commit-domain ‖ epoch ‖ envelope-bytes).
    /// The client nonce η and ciphertext are inside the envelope bytes.
    pub fn commitment_hash(epoch: u64, envelope_bytes: &[u8]) -> [u8; 32] {
        let mut m = Vec::with_capacity(envelope_bytes.len() + 24);
        m.extend_from_slice(b"posq-commit-v1");
        m.extend_from_slice(&epoch.to_be_bytes());
        m.extend_from_slice(envelope_bytes);
        keccak256(&m)
    }
}

/// The plaintext intent carried inside the AEAD body (§7.1):
/// i = (namespace, account, nonce, expiry, payload), σ_i.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub namespace: u64,
    pub account: [u8; 20],
    pub nonce: u64,
    pub expiry_tick: u64,
    pub payload: Vec<u8>,
    pub signature: Sig65,
}

impl Intent {
    pub fn signing_message(
        namespace: u64,
        account: &[u8; 20],
        nonce: u64,
        expiry_tick: u64,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut m = Vec::with_capacity(96);
        m.extend_from_slice(b"posq-intent-v1");
        m.extend_from_slice(&namespace.to_be_bytes());
        m.extend_from_slice(account);
        m.extend_from_slice(&nonce.to_be_bytes());
        m.extend_from_slice(&expiry_tick.to_be_bytes());
        m.extend_from_slice(&keccak256(payload));
        m
    }

    pub fn sign_new(
        signer: &Identity,
        namespace: u64,
        nonce: u64,
        expiry_tick: u64,
        payload: Vec<u8>,
    ) -> Self {
        let account = signer.address();
        let sig = signer.sign(&Self::signing_message(
            namespace, &account, nonce, expiry_tick, &payload,
        ));
        Self { namespace, account, nonce, expiry_tick, payload, signature: sig }
    }

    pub fn verify(&self) -> bool {
        crate::identity::verify_signed(
            &Self::signing_message(
                self.namespace,
                &self.account,
                self.nonce,
                self.expiry_tick,
                &self.payload,
            ),
            &self.signature,
            &self.account,
        )
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("intent serialization")
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;

    fn sample_envelope() -> Envelope {
        let issuer = Identity::from_seed(b"issuer");
        Envelope {
            bucket: Bucket::Majors,
            namespace: 7,
            window: Window { start: 100, len: 4 },
            delay_class: 0,
            nonce: [9u8; 16],
            ticket: Ticket::issue(&issuer, [1u8; 16], 0, Bucket::Majors),
            ciphertext: Ciphertext {
                class: 0,
                u: BigUint::from(123456789u64),
                body: vec![5u8; 200],
            },
        }
    }

    #[test]
    fn envelope_roundtrip_fixed_size() {
        let e = sample_envelope();
        let bytes = e.encode(1024).unwrap();
        assert_eq!(bytes.len(), 1024);
        let d = Envelope::decode(&bytes, 1024).unwrap();
        assert_eq!(d, e);
    }

    #[test]
    fn wrong_size_rejected() {
        let e = sample_envelope();
        let bytes = e.encode(1024).unwrap();
        assert_eq!(
            Envelope::decode(&bytes[..1023], 1024),
            Err(EnvelopeError::WrongSize)
        );
    }

    #[test]
    fn nonzero_padding_rejected() {
        let e = sample_envelope();
        let mut bytes = e.encode(1024).unwrap();
        *bytes.last_mut().unwrap() = 1;
        assert_eq!(Envelope::decode(&bytes, 1024), Err(EnvelopeError::BadField));
    }

    #[test]
    fn ticket_verify() {
        let issuer = Identity::from_seed(b"issuer");
        let t = Ticket::issue(&issuer, [2u8; 16], 1, Bucket::LongTail);
        assert!(t.verify(&issuer.address()));
        assert!(!t.verify(&Identity::from_seed(b"other").address()));
    }

    #[test]
    fn intent_sign_verify() {
        let client = Identity::from_seed(b"client");
        let i = Intent::sign_new(&client, 7, 1, 5000, b"place order".to_vec());
        assert!(i.verify());
        let mut bad = i.clone();
        bad.nonce = 2;
        assert!(!bad.verify());
        let rt = Intent::from_bytes(&i.to_bytes()).unwrap();
        assert_eq!(rt, i);
    }

    #[test]
    fn commitment_hash_binds_epoch_and_bytes() {
        let e = sample_envelope();
        let bytes = e.encode(1024).unwrap();
        let h0 = Envelope::commitment_hash(0, &bytes);
        let h1 = Envelope::commitment_hash(1, &bytes);
        assert_ne!(h0, h1);
    }
}
