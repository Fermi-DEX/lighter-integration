//! Client-side envelope construction (SDK core, used by tests and any
//! integration): sign the intent, encrypt it into the fixed-base KEM with
//! the envelope header as AAD, and wrap it in the fixed-size envelope.
//!
//! Under the solve-only profile there is nothing else for a client to do
//! after submission: the position is receipted, the public solver opens the
//! ciphertext at maturity, and the outcome lands on the tape.

use crate::envelope::{header_prefix, Bucket, Envelope, Intent, Ticket, Window};
use crate::identity::Identity;
use vdf::tlk::{self, TlkParams};

pub struct EnvelopeRequest<'a> {
    pub signer: &'a Identity,
    pub ticket: Ticket,
    pub bucket: Bucket,
    pub namespace: u64,
    pub window: Window,
    pub delay_class: u8,
    pub nonce: [u8; 16],
    pub intent_nonce: u64,
    pub expiry_tick: u64,
    pub payload: Vec<u8>,
    /// Entropy for the KEM exponent r; must be fresh per envelope.
    pub kem_seed: Vec<u8>,
}

/// Build a submit-ready envelope of exactly `envelope_size` bytes.
pub fn build_envelope(
    tlk: &TlkParams,
    envelope_size: usize,
    req: EnvelopeRequest<'_>,
) -> anyhow::Result<Vec<u8>> {
    let intent = Intent::sign_new(
        req.signer,
        req.namespace,
        req.intent_nonce,
        req.expiry_tick,
        req.payload,
    );
    let aad = header_prefix(req.bucket, req.namespace, req.window, req.delay_class, &req.nonce);
    let ciphertext = tlk::encrypt(tlk, req.delay_class, &intent.to_bytes(), &aad, &req.kem_seed)
        .map_err(|e| anyhow::anyhow!("kem encrypt: {e}"))?;
    let env = Envelope {
        bucket: req.bucket,
        namespace: req.namespace,
        window: req.window,
        delay_class: req.delay_class,
        nonce: req.nonce,
        ticket: req.ticket,
        ciphertext,
    };
    env.encode(envelope_size)
        .map_err(|e| anyhow::anyhow!("envelope encode: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vdf::posq::Group;

    #[test]
    fn built_envelope_decodes_and_opens_via_solve() {
        let tlk = TlkParams::setup(Group::default_rsa2048(), &[64]);
        let client = Identity::from_seed(b"client");
        let issuer = Identity::from_seed(b"issuer");
        let ticket = Ticket::issue(&issuer, [5u8; 16], 0, Bucket::Majors);
        let bytes = build_envelope(
            &tlk,
            1024,
            EnvelopeRequest {
                signer: &client,
                ticket,
                bucket: Bucket::Majors,
                namespace: 3,
                window: Window { start: 1, len: 4 },
                delay_class: 0,
                nonce: [1u8; 16],
                intent_nonce: 42,
                expiry_tick: 10_000,
                payload: b"limit buy 10 @ 99.5".to_vec(),
                kem_seed: b"fresh entropy".to_vec(),
            },
        )
        .unwrap();
        assert_eq!(bytes.len(), 1024);

        let env = Envelope::decode(&bytes, 1024).unwrap();
        let opening = tlk::solve(&tlk, &env.ciphertext).unwrap();
        let aad = &bytes[..crate::envelope::HEADER_AAD_LEN];
        let plaintext = tlk::open(&env.ciphertext, &opening.w, aad).unwrap();
        let intent = Intent::from_bytes(&plaintext).unwrap();
        assert!(intent.verify());
        assert_eq!(intent.payload, b"limit buy 10 @ 99.5");
        assert_eq!(intent.account, client.address());
    }
}
