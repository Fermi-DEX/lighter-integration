//! JSON encodings of the signed protocol objects, byte-faithful (hex) so the
//! dashboard can re-verify signatures and hash links client-side.

use sequencer::records::{FullWindowResponse, Receipt, SignedRejection, TickRecord};
use sequencer::store::{Outcome, TapePosition};
use serde_json::json;

pub fn hx(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

pub fn receipt_json(r: &Receipt) -> serde_json::Value {
    json!({
        "type": "receipt",
        "epoch": r.epoch,
        "tick": r.tick,
        "pos": r.pos,
        "h": hx(&r.h),
        "bucket": r.bucket as u8,
        "window_start": r.window.start,
        "window_len": r.window.len,
        "ticket_id": hx(&r.ticket_id),
        "x_prev_hash": hx(&r.x_prev_hash),
        "c_prev": hx(&r.c_prev),
        "d_prev": hx(&r.d_prev),
        "d": hx(&r.d),
        "sig": hx(r.sig.as_bytes()),
    })
}

pub fn rejection_json(r: &SignedRejection) -> serde_json::Value {
    json!({
        "type": "rejection",
        "epoch": r.epoch,
        "h": hx(&r.h),
        "bucket": r.bucket as u8,
        "window_start": r.window.start,
        "window_len": r.window.len,
        "reason": format!("{:?}", r.reason),
        "reason_code": r.reason as u8,
        "c_latest": hx(&r.c_latest),
        "sig": hx(r.sig.as_bytes()),
    })
}

pub fn full_window_json(f: &FullWindowResponse) -> serde_json::Value {
    json!({
        "type": "full-window",
        "epoch": f.epoch,
        "bucket": f.bucket as u8,
        "window_start": f.window.start,
        "window_len": f.window.len,
        "capacity": f.capacity,
        "c_latest": hx(&f.c_latest),
        "sig": hx(f.sig.as_bytes()),
    })
}

pub fn tick_record_json(r: &TickRecord) -> serde_json::Value {
    json!({
        "epoch": r.epoch,
        "segment": r.segment,
        "tick": r.tick,
        "x_hash": hx(&r.x_hash()),
        "c_prev": hx(&r.c_prev),
        "c_t": hx(&r.c_t),
        "batch_root": hx(&r.batch_root),
        "da_ref": hx(&r.da_ref),
        "sig": hx(r.sig.as_bytes()),
    })
}

pub fn tape_json(p: &TapePosition) -> serde_json::Value {
    let (outcome, detail) = match &p.outcome {
        Outcome::Pending { mature_at } => ("pending".to_string(), json!({"mature_at": mature_at})),
        Outcome::Opened { intent } => (
            "opened".to_string(),
            json!({
                "account": hx(&intent.account),
                "namespace": intent.namespace,
                "nonce": intent.nonce,
                "expiry_tick": intent.expiry_tick,
                "payload_hex": hex::encode(&intent.payload),
                "payload_utf8": String::from_utf8_lossy(&intent.payload),
            }),
        ),
        Outcome::Gap { reason } => (format!("gap:{reason:?}"), json!({})),
    };
    json!({
        "tick": p.tick,
        "pos": p.pos,
        "h": hx(&p.h),
        "bucket": p.bucket as u8,
        "delay_class": p.delay_class,
        "outcome": outcome,
        "detail": detail,
    })
}
