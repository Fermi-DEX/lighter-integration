//! gRPC surface over the node's shared state.

use crate::envelope::{Bucket, Intent};
use crate::node::Shared;
use crate::proto::{self, posq_sequencer_server::PosqSequencer};
use crate::records::AdmissionOutcome;
use crate::store::{GapReason, Outcome, TapePosition};
use std::pin::Pin;
use std::sync::Arc;
use tonic::{Request, Response, Status};

pub struct PosqService {
    pub shared: Arc<Shared>,
}

fn tape_to_proto(p: &TapePosition) -> proto::TapeEntryMsg {
    let (outcome, intent, mature_at) = match &p.outcome {
        Outcome::Pending { mature_at } => ("pending".to_string(), Vec::new(), *mature_at),
        Outcome::Opened { intent } => ("opened".to_string(), bincode::serialize(intent).unwrap_or_default(), 0),
        Outcome::Gap { reason } => (
            format!(
                "gap:{}",
                match reason {
                    GapReason::Undecryptable => "undecryptable",
                    GapReason::InvalidIntent => "invalid-intent",
                    GapReason::Unopened => "unopened",
                }
            ),
            Vec::new(),
            0,
        ),
    };
    proto::TapeEntryMsg {
        tick: p.tick,
        pos: p.pos,
        h: p.h.to_vec(),
        bucket: p.bucket as u32,
        delay_class: p.delay_class as u32,
        outcome,
        intent,
        mature_at,
    }
}

fn record_to_proto(r: &crate::records::TickRecord) -> proto::TickRecordMsg {
    proto::TickRecordMsg {
        epoch: r.epoch,
        segment: r.segment,
        tick: r.tick,
        x: r.x.to_bytes_be(),
        c_prev: r.c_prev.to_vec(),
        c_t: r.c_t.to_vec(),
        batch_root: r.batch_root.to_vec(),
        da_ref: r.da_ref.to_vec(),
        sig: r.sig.as_bytes().to_vec(),
    }
}

#[tonic::async_trait]
impl PosqSequencer for PosqService {
    async fn get_info(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::Info>, Status> {
        let p = &self.shared.params;
        let issuer = self.shared.faucet.lock().unwrap().issuer_address();
        Ok(Response::new(proto::Info {
            epoch: p.epoch,
            sequencer_address: self.shared.sequencer_address.to_vec(),
            q_squarings: p.q_squarings,
            segment_ticks: p.segment_ticks,
            window_ticks: p.window_ticks as u32,
            delta_micros: p.delta_micros,
            envelope_bytes: p.envelope_bytes as u32,
            delay_classes: p.delay_classes.clone(),
            tlk_params: bincode::serialize(&self.shared.tlk)
                .map_err(|e| Status::internal(e.to_string()))?,
            ticket_issuer_address: issuer.to_vec(),
        }))
    }

    async fn mint_ticket(
        &self,
        request: Request<proto::MintTicketRequest>,
    ) -> Result<Response<proto::TicketMsg>, Status> {
        let req = request.into_inner();
        let bucket = Bucket::from_u8(req.bucket as u8)
            .ok_or_else(|| Status::invalid_argument("unknown bucket"))?;
        if req.denomination as usize >= self.shared.params.delay_classes.len() {
            return Err(Status::invalid_argument("unknown denomination/delay class"));
        }
        let ticket = self.shared.faucet.lock().unwrap().mint(req.denomination as u8, bucket);
        Ok(Response::new(proto::TicketMsg {
            ticket: bincode::serialize(&ticket).map_err(|e| Status::internal(e.to_string()))?,
        }))
    }

    async fn submit_envelope(
        &self,
        request: Request<proto::SubmitEnvelopeRequest>,
    ) -> Result<Response<proto::AdmissionResponse>, Status> {
        let bytes = request.into_inner().envelope;
        // Admission is synchronous and lock-scoped; the DA write happens
        // inside (before the receipt exists).
        let outcome = {
            let store = self.shared.store.lock().unwrap();
            let da_ok: &crate::store::DaStore = store.da();
            // Admission needs &DaStore while holding its own lock; the store
            // lock only guards the maps, DaStore itself is filesystem-backed.
            let mut adm = self.shared.admission.lock().unwrap();
            adm.submit(&bytes, da_ok)
        };
        let resp = match outcome {
            AdmissionOutcome::Admitted(r) => proto::admission_response::Outcome::Receipt(proto::ReceiptMsg {
                epoch: r.epoch,
                tick: r.tick,
                pos: r.pos,
                h: r.h.to_vec(),
                bucket: r.bucket as u32,
                window_start: r.window.start,
                window_len: r.window.len as u32,
                ticket_id: r.ticket_id.to_vec(),
                x_prev_hash: r.x_prev_hash.to_vec(),
                c_prev: r.c_prev.to_vec(),
                d_prev: r.d_prev.to_vec(),
                d: r.d.to_vec(),
                sig: r.sig.as_bytes().to_vec(),
            }),
            AdmissionOutcome::Rejected(r) => proto::admission_response::Outcome::Rejection(proto::RejectionMsg {
                epoch: r.epoch,
                h: r.h.to_vec(),
                bucket: r.bucket as u32,
                window_start: r.window.start,
                window_len: r.window.len as u32,
                reason: r.reason as u32,
                c_latest: r.c_latest.to_vec(),
                sig: r.sig.as_bytes().to_vec(),
            }),
            AdmissionOutcome::FullWindow(f) => proto::admission_response::Outcome::FullWindow(proto::FullWindowMsg {
                epoch: f.epoch,
                bucket: f.bucket as u32,
                window_start: f.window.start,
                window_len: f.window.len as u32,
                capacity: f.capacity,
                c_latest: f.c_latest.to_vec(),
                sig: f.sig.as_bytes().to_vec(),
            }),
        };
        Ok(Response::new(proto::AdmissionResponse { outcome: Some(resp) }))
    }

    async fn get_status(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::Status>, Status> {
        let s = self.shared.status.lock().unwrap().clone();
        Ok(Response::new(proto::Status {
            current_tick: s.current_tick,
            current_segment: s.current_segment,
            admitted: s.admitted,
            opened: s.opened,
            gaps: s.gaps,
            cadence_faults: s.cadence_faults,
        }))
    }

    type StreamTickRecordsStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<proto::TickRecordMsg, Status>> + Send>>;

    async fn stream_tick_records(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<Self::StreamTickRecordsStream>, Status> {
        let mut rx = self.shared.record_tx.subscribe();
        let (tx, out) = tokio::sync::mpsc::channel(256);
        tokio::spawn(async move {
            while let Ok(record) = rx.recv().await {
                if tx.send(Ok(record_to_proto(&record))).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(Box::pin(tokio_stream::wrappers::ReceiverStream::new(out))))
    }

    type StreamTapeStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<proto::TapeEntryMsg, Status>> + Send>>;

    async fn stream_tape(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<Self::StreamTapeStream>, Status> {
        let mut rx = self.shared.tape_tx.subscribe();
        let (tx, out) = tokio::sync::mpsc::channel(1024);
        tokio::spawn(async move {
            while let Ok(pos) = rx.recv().await {
                if tx.send(Ok(tape_to_proto(&pos))).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(Box::pin(tokio_stream::wrappers::ReceiverStream::new(out))))
    }

    async fn list_anchors(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::AnchorList>, Status> {
        let anchors = self.shared.anchors.lock().unwrap();
        Ok(Response::new(proto::AnchorList {
            anchors: anchors
                .iter()
                .map(|a| bincode::serialize(a).unwrap_or_default())
                .collect(),
        }))
    }
}

// Re-export Intent for SDK consumers decoding tape entries.
pub type TapeIntent = Intent;
