//! lighter-sim: the ingest adapter (spec §4, rules R1–R7) over a toy
//! Lighter-style CLOB with real order-nonce time priority (whitepaper
//! §3.7.1: per-market, per-side monotonic nonces; priority = (price, nonce)).
//!
//! The adapter consumes the PoSq tape strictly in (t, j) order — the head
//! blocks until its position resolves (Opened/Gap), exactly the R3
//! discipline — assembles fixed tick-range blocks (R1), applies the
//! validity split (R4: stateless-invalid ⇒ gap, stateful-fail ⇒ recorded
//! failed tx), and maintains the opened-stream commitment of spec §5/B2.

use sequencer::identity::keccak256;
use sequencer::node::Shared;
use sequencer::store::{GapReason, Outcome};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub const LIGHTER_NAMESPACE: u64 = 1;

// ---------------------------------------------------------------------------
// The demo Lighter transaction language (JSON payload inside the intent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderKind {
    Limit,
    Market,
    Ioc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum LighterTx {
    /// Price/size are integer steps (Lighter's quote-multiplier encoding).
    Create {
        market: u32,
        side: Side,
        kind: OrderKind,
        #[serde(default)]
        price: u64,
        size: u64,
        coi: u64,
    },
    Cancel { market: u32, coi: u64 },
}

// ---------------------------------------------------------------------------
// Order book: price-time priority via per-side monotonic nonces
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RestingOrder {
    pub account: String, // 0x-hex
    pub coi: u64,
    pub side: Side,
    pub price: u64,
    pub size: u64,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub tick: u64,
    pub pos: u32,
    pub price: u64,
    pub size: u64,
    pub maker: String,
    pub taker: String,
    pub maker_coi: u64,
    pub taker_coi: u64,
    pub taker_side: Side,
    pub block: u64,
}

#[derive(Default)]
pub struct Book {
    /// Asks keyed (price, nonce): best = first. Bids keyed (Reverse(price), nonce).
    pub asks: BTreeMap<(u64, u64), RestingOrder>,
    pub bids: BTreeMap<(Reverse<u64>, u64), RestingOrder>,
    pub next_ask_nonce: u64,
    pub next_bid_nonce: u64,
    /// (account, coi) → side+key for cancels.
    index: HashMap<([u8; 20], u64), (Side, u64, u64)>, // (side, price, nonce)
    pub last_trade_price: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ExecResult {
    Applied {
        trades: Vec<Trade>,
        rested: Option<RestingOrder>,
        order_nonce: Option<u64>,
    },
    /// Stateful failure: the tx exists in the stream but no-ops (spec §3).
    Failed { reason: String },
}

fn hex_addr(a: &[u8; 20]) -> String {
    format!("0x{}", hex::encode(a))
}

impl Book {
    pub fn best_bid(&self) -> Option<u64> {
        self.bids.keys().next().map(|(Reverse(p), _)| *p)
    }
    pub fn best_ask(&self) -> Option<u64> {
        self.asks.keys().next().map(|(p, _)| *p)
    }

    pub fn execute(&mut self, account: [u8; 20], tx: &LighterTx, tick: u64, pos: u32, block: u64) -> ExecResult {
        match tx {
            LighterTx::Cancel { coi, .. } => match self.index.remove(&(account, *coi)) {
                None => ExecResult::Failed { reason: "unknown or inactive order".into() },
                Some((side, price, nonce)) => {
                    match side {
                        Side::Ask => self.asks.remove(&(price, nonce)),
                        Side::Bid => self.bids.remove(&(Reverse(price), nonce)),
                    };
                    ExecResult::Applied { trades: vec![], rested: None, order_nonce: Some(nonce) }
                }
            },
            LighterTx::Create { side, kind, price, size, coi, .. } => {
                if *size == 0 {
                    return ExecResult::Failed { reason: "zero size".into() };
                }
                if matches!(kind, OrderKind::Limit) && *price == 0 {
                    return ExecResult::Failed { reason: "zero price on limit".into() };
                }
                if self.index.contains_key(&(account, *coi)) {
                    return ExecResult::Failed { reason: "client order index in use".into() };
                }
                // Lighter §3.7.1: every new order on a side gets the next nonce.
                let nonce = match side {
                    Side::Ask => {
                        let n = self.next_ask_nonce;
                        self.next_ask_nonce += 1;
                        n
                    }
                    Side::Bid => {
                        let n = self.next_bid_nonce;
                        self.next_bid_nonce += 1;
                        n
                    }
                };
                let mut remaining = *size;
                let mut trades = Vec::new();
                let taker_hex = hex_addr(&account);
                // Cross against the opposite side in strict (price, nonce) priority.
                loop {
                    if remaining == 0 {
                        break;
                    }
                    let crossed = match side {
                        Side::Bid => self
                            .asks
                            .keys()
                            .next()
                            .copied()
                            .filter(|(p, _)| matches!(kind, OrderKind::Market) || *p <= *price)
                            .map(|k| (Side::Ask, k.0, k.1)),
                        Side::Ask => self
                            .bids
                            .keys()
                            .next()
                            .copied()
                            .filter(|(Reverse(p), _)| matches!(kind, OrderKind::Market) || *p >= *price)
                            .map(|(Reverse(p), n)| (Side::Bid, p, n)),
                    };
                    let Some((maker_side, mprice, mnonce)) = crossed else { break };
                    let (fill, maker_done, maker) = {
                        let maker = match maker_side {
                            Side::Ask => self.asks.get_mut(&(mprice, mnonce)).unwrap(),
                            Side::Bid => self.bids.get_mut(&(Reverse(mprice), mnonce)).unwrap(),
                        };
                        let fill = remaining.min(maker.size);
                        maker.size -= fill;
                        (fill, maker.size == 0, maker.clone())
                    };
                    remaining -= fill;
                    trades.push(Trade {
                        tick,
                        pos,
                        price: mprice,
                        size: fill,
                        maker: maker.account.clone(),
                        taker: taker_hex.clone(),
                        maker_coi: maker.coi,
                        taker_coi: *coi,
                        taker_side: *side,
                        block,
                    });
                    self.last_trade_price = Some(mprice);
                    if maker_done {
                        match maker_side {
                            Side::Ask => self.asks.remove(&(mprice, mnonce)),
                            Side::Bid => self.bids.remove(&(Reverse(mprice), mnonce)),
                        };
                        let maker_acct: [u8; 20] = hex::decode(&maker.account[2..])
                            .ok()
                            .and_then(|v| v.try_into().ok())
                            .unwrap_or_default();
                        self.index.remove(&(maker_acct, maker.coi));
                    }
                }
                // Rest the remainder (limit only).
                let mut rested = None;
                if remaining > 0 && matches!(kind, OrderKind::Limit) {
                    let o = RestingOrder {
                        account: taker_hex,
                        coi: *coi,
                        side: *side,
                        price: *price,
                        size: remaining,
                        nonce,
                    };
                    match side {
                        Side::Ask => {
                            self.asks.insert((*price, nonce), o.clone());
                        }
                        Side::Bid => {
                            self.bids.insert((Reverse(*price), nonce), o.clone());
                        }
                    }
                    self.index.insert((account, *coi), (*side, *price, nonce));
                    rested = Some(o);
                }
                ExecResult::Applied { trades, rested, order_nonce: Some(nonce) }
            }
        }
    }

    pub fn snapshot(&self, depth: usize) -> BookSnapshot {
        BookSnapshot {
            bids: self.bids.values().take(depth).cloned().collect(),
            asks: self.asks.values().take(depth).cloned().collect(),
            next_ask_nonce: self.next_ask_nonce,
            next_bid_nonce: self.next_bid_nonce,
            last_trade_price: self.last_trade_price,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BookSnapshot {
    pub bids: Vec<RestingOrder>,
    pub asks: Vec<RestingOrder>,
    pub next_ask_nonce: u64,
    pub next_bid_nonce: u64,
    pub last_trade_price: Option<u64>,
}

// ---------------------------------------------------------------------------
// Blocks and the opened-stream commitment (spec §5, B2)
// ---------------------------------------------------------------------------

pub fn stream_genesis(epoch: u64, first_tick: u64) -> [u8; 32] {
    keccak256(
        &[b"lighter-stream-genesis-v1".as_slice(), &epoch.to_be_bytes(), &first_tick.to_be_bytes()]
            .concat(),
    )
}

pub fn stream_step(s_prev: &[u8; 32], tick: u64, pos: u32, payload: &[u8]) -> [u8; 32] {
    let mut m = Vec::with_capacity(96);
    m.extend_from_slice(b"lighter-stream-v1");
    m.extend_from_slice(s_prev);
    m.extend_from_slice(&tick.to_be_bytes());
    m.extend_from_slice(&pos.to_be_bytes());
    m.extend_from_slice(&keccak256(payload));
    keccak256(&m)
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockTx {
    pub tick: u64,
    pub pos: u32,
    pub account: String,
    /// Exact payload bytes (hex) — what the stream commitment hashes.
    pub payload_hex: String,
    /// Parsed for display.
    pub tx: LighterTx,
    pub result: ExecResult,
    /// Cumulative stream commitment after this tx.
    pub stream_after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockGap {
    pub tick: u64,
    pub pos: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LighterBlock {
    pub number: u64,
    pub epoch: u64,
    pub first_tick: u64,
    pub last_tick: u64,
    /// R1: derived from the tape, not the operator's wall clock.
    pub timestamp_ms: u64,
    pub txs: Vec<BlockTx>,
    pub gaps: Vec<BlockGap>,
    pub stream_commitment: String,
    pub book_after: BookSnapshot,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HeadBlock {
    pub tick: u64,
    pub pos: u32,
    pub mature_at: u64,
    pub since_ms: u64,
}

pub struct SimState {
    pub book: Book,
    pub blocks: VecDeque<LighterBlock>,
    pub trades: VecDeque<Trade>,
    pub block_counter: u64,
    pub stream_s: [u8; 32],
    pub epoch: u64,
    pub epoch_start_ms: u64,
    /// Cursor: next (tick, pos) to consume in the current epoch.
    pub cursor_tick: u64,
    pub cursor_pos: u32,
    pub head_blocked: Option<HeadBlock>,
    // accumulators for the in-progress block
    cur_txs: Vec<BlockTx>,
    cur_gaps: Vec<BlockGap>,
    cur_first_tick: u64,
    // counters
    pub opened: u64,
    pub applied: u64,
    pub failed: u64,
    pub adapter_gaps: u64,
    pub tape_gaps: u64,
    pub trades_total: u64,
}

impl SimState {
    pub fn new() -> Self {
        Self {
            book: Book::default(),
            blocks: VecDeque::with_capacity(512),
            trades: VecDeque::with_capacity(2048),
            block_counter: 0,
            stream_s: [0u8; 32],
            epoch: 0,
            epoch_start_ms: 0,
            cursor_tick: 1,
            cursor_pos: 0,
            head_blocked: None,
            cur_txs: Vec::new(),
            cur_gaps: Vec::new(),
            cur_first_tick: 1,
            opened: 0,
            applied: 0,
            failed: 0,
            adapter_gaps: 0,
            tape_gaps: 0,
            trades_total: 0,
        }
    }

    /// Re-genesis at epoch rotation: the stream commitment restarts (chain
    /// re-genesis discipline), the book state carries across (Lighter state
    /// persists across batches).
    pub fn begin_epoch(&mut self, epoch: u64) {
        self.epoch = epoch;
        self.epoch_start_ms = now_ms();
        self.cursor_tick = 1;
        self.cursor_pos = 0;
        self.stream_s = stream_genesis(epoch, 1);
        self.cur_txs.clear();
        self.cur_gaps.clear();
        self.cur_first_tick = 1;
        self.head_blocked = None;
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// The adapter task
// ---------------------------------------------------------------------------

/// Consume the tape for one epoch. Returns when the node reports finished
/// and the tape is fully drained, or when `stop` flips.
pub async fn run_sim(
    shared: Arc<Shared>,
    sim: Arc<Mutex<SimState>>,
    events: broadcast::Sender<serde_json::Value>,
    block_ticks: u64,
    stop: tokio::sync::watch::Receiver<bool>,
) {
    let delta_ms = shared.params.delta_micros / 1000;
    {
        let mut s = sim.lock().unwrap();
        s.begin_epoch(shared.params.epoch);
    }
    loop {
        if *stop.borrow() {
            return;
        }
        let finished = shared.status.lock().unwrap().finished;
        let made_progress = advance(&shared, &sim, &events, block_ticks, delta_ms);
        if !made_progress {
            if finished {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
}

/// Consume as many resolved positions as possible. Returns whether progress
/// was made. Strict (t, j): a Pending head blocks everything behind it (R3).
fn advance(
    shared: &Arc<Shared>,
    sim: &Arc<Mutex<SimState>>,
    events: &broadcast::Sender<serde_json::Value>,
    block_ticks: u64,
    delta_ms: u64,
) -> bool {
    // Snapshot what we need under the store lock, then release it before
    // executing (the coordinator thread needs the lock every tick).
    struct Resolved {
        tick: u64,
        pos: u32,
        outcome: Outcome,
    }
    let mut batch: Vec<Resolved> = Vec::new();
    let mut ticks_completed: Vec<u64> = Vec::new();
    let (mut ct, mut cp) = {
        let s = sim.lock().unwrap();
        (s.cursor_tick, s.cursor_pos)
    };
    let mut head_block: Option<HeadBlock> = None;
    {
        let store = shared.store.lock().unwrap();
        'outer: loop {
            let Some(sealed) = store.ticks.get(&ct) else { break };
            let entries = &sealed.entries;
            while (cp as usize) < entries.len() {
                let key = (ct, cp);
                match store.tape.get(&key).map(|p| p.outcome.clone()) {
                    Some(Outcome::Pending { mature_at }) => {
                        head_block = Some(HeadBlock {
                            tick: ct,
                            pos: cp,
                            mature_at,
                            since_ms: now_ms(),
                        });
                        break 'outer;
                    }
                    Some(outcome) => {
                        batch.push(Resolved { tick: ct, pos: cp, outcome });
                        cp += 1;
                    }
                    None => break 'outer, // record/tape race; retry next poll
                }
            }
            if (cp as usize) < entries.len() {
                break;
            }
            ticks_completed.push(ct);
            ct += 1;
            cp = 0;
            if batch.len() >= 512 {
                break; // bound work per pass
            }
        }
    }

    if batch.is_empty() && ticks_completed.is_empty() {
        let mut s = sim.lock().unwrap();
        s.head_blocked = head_block;
        return false;
    }

    let mut s = sim.lock().unwrap();
    let mut sealed_blocks: Vec<LighterBlock> = Vec::new();
    let mut new_trades: Vec<Trade> = Vec::new();
    // Replay the resolved positions in order, interleaving block seals at
    // tick-range boundaries (R1).
    let mut batch_iter = batch.into_iter().peekable();
    for done_tick in &ticks_completed {
        while let Some(r) = batch_iter.peek() {
            if r.tick > *done_tick {
                break;
            }
            let r = batch_iter.next().unwrap();
            process_position(&mut s, r.tick, r.pos, &r.outcome, &mut new_trades);
        }
        // Block seal when the last tick of a range completes.
        if done_tick % block_ticks == 0 {
            let blk = seal_block(&mut s, *done_tick, block_ticks, delta_ms);
            sealed_blocks.push(blk);
        }
        s.cursor_tick = done_tick + 1;
        s.cursor_pos = 0;
    }
    // Positions of the (incomplete) current tick.
    for r in batch_iter {
        process_position(&mut s, r.tick, r.pos, &r.outcome, &mut new_trades);
        s.cursor_tick = r.tick;
        s.cursor_pos = r.pos + 1;
    }
    s.head_blocked = head_block;
    drop(s);

    for t in &new_trades {
        let _ = events.send(serde_json::json!({"type": "trade", "data": t}));
    }
    for b in &sealed_blocks {
        let _ = events.send(serde_json::json!({"type": "block", "data": b}));
    }
    true
}

fn process_position(
    s: &mut SimState,
    tick: u64,
    pos: u32,
    outcome: &Outcome,
    new_trades: &mut Vec<Trade>,
) {
    match outcome {
        Outcome::Pending { .. } => unreachable!("pending never reaches process_position"),
        Outcome::Gap { reason } => {
            s.tape_gaps += 1;
            s.cur_gaps.push(BlockGap {
                tick,
                pos,
                reason: match reason {
                    GapReason::Undecryptable => "undecryptable (mauled ciphertext)".into(),
                    GapReason::InvalidIntent => "invalid intent (sig/namespace/expiry)".into(),
                    GapReason::Unopened => "unopened by cutoff".into(),
                },
            });
        }
        Outcome::Opened { intent } => {
            s.opened += 1;
            if intent.namespace != LIGHTER_NAMESPACE {
                s.adapter_gaps += 1;
                s.cur_gaps.push(BlockGap { tick, pos, reason: "foreign namespace".into() });
                return;
            }
            match serde_json::from_slice::<LighterTx>(&intent.payload) {
                Err(_) => {
                    // Stateless-invalid ⇒ gap; excluded from the stream (§3).
                    s.adapter_gaps += 1;
                    s.cur_gaps.push(BlockGap { tick, pos, reason: "unparseable payload".into() });
                }
                Ok(tx) => {
                    // Stream commitment covers every stateless-valid entry,
                    // including ones that fail statefully (§3, §5).
                    s.stream_s = stream_step(&s.stream_s, tick, pos, &intent.payload);
                    let block_no = s.block_counter;
                    let result = s.book.execute(intent.account, &tx, tick, pos, block_no);
                    match &result {
                        ExecResult::Applied { trades, .. } => {
                            s.applied += 1;
                            s.trades_total += trades.len() as u64;
                            for t in trades {
                                if s.trades.len() >= 2048 {
                                    s.trades.pop_front();
                                }
                                s.trades.push_back(t.clone());
                                new_trades.push(t.clone());
                            }
                        }
                        ExecResult::Failed { .. } => s.failed += 1,
                    }
                    s.cur_txs.push(BlockTx {
                        tick,
                        pos,
                        account: hex_addr(&intent.account),
                        payload_hex: hex::encode(&intent.payload),
                        tx,
                        result,
                        stream_after: hex::encode(s.stream_s),
                    });
                }
            }
        }
    }
}

fn seal_block(s: &mut SimState, last_tick: u64, block_ticks: u64, delta_ms: u64) -> LighterBlock {
    let blk = LighterBlock {
        number: s.block_counter,
        epoch: s.epoch,
        first_tick: s.cur_first_tick,
        last_tick,
        timestamp_ms: s.epoch_start_ms + last_tick * delta_ms,
        txs: std::mem::take(&mut s.cur_txs),
        gaps: std::mem::take(&mut s.cur_gaps),
        stream_commitment: hex::encode(s.stream_s),
        book_after: s.book.snapshot(10),
    };
    s.block_counter += 1;
    s.cur_first_tick = last_tick + 1;
    if s.blocks.len() >= 512 {
        s.blocks.pop_front();
    }
    s.blocks.push_back(blk.clone());
    let _ = block_ticks;
    blk
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct(b: u8) -> [u8; 20] {
        [b; 20]
    }

    #[test]
    fn price_time_priority_by_nonce() {
        let mut book = Book::default();
        // Two asks at the same price: the earlier nonce must fill first.
        let a1 = LighterTx::Create { market: 0, side: Side::Ask, kind: OrderKind::Limit, price: 100, size: 5, coi: 1 };
        let a2 = LighterTx::Create { market: 0, side: Side::Ask, kind: OrderKind::Limit, price: 100, size: 5, coi: 2 };
        book.execute(acct(1), &a1, 1, 0, 0);
        book.execute(acct(2), &a2, 1, 1, 0);
        let taker = LighterTx::Create { market: 0, side: Side::Bid, kind: OrderKind::Market, price: 0, size: 5, coi: 3 };
        let ExecResult::Applied { trades, .. } = book.execute(acct(3), &taker, 1, 2, 0) else {
            panic!()
        };
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].maker, hex_addr(&acct(1)), "earlier nonce fills first");
        // a2 remains.
        assert_eq!(book.asks.len(), 1);
        assert_eq!(book.asks.values().next().unwrap().coi, 2);
    }

    #[test]
    fn cancel_then_taker_misses() {
        let mut book = Book::default();
        let quote = LighterTx::Create { market: 0, side: Side::Ask, kind: OrderKind::Limit, price: 100, size: 5, coi: 1 };
        book.execute(acct(1), &quote, 1, 0, 0);
        // Cancel arrives before the taker in tape order → sniping fails.
        let cancel = LighterTx::Cancel { market: 0, coi: 1 };
        assert!(matches!(book.execute(acct(1), &cancel, 1, 1, 0), ExecResult::Applied { .. }));
        let take = LighterTx::Create { market: 0, side: Side::Bid, kind: OrderKind::Market, price: 0, size: 5, coi: 2 };
        let ExecResult::Applied { trades, .. } = book.execute(acct(2), &take, 1, 2, 0) else {
            panic!()
        };
        assert!(trades.is_empty(), "cancel won the race; nothing to snipe");
    }

    #[test]
    fn stateful_failures_are_failed_not_gaps() {
        let mut book = Book::default();
        let cancel = LighterTx::Cancel { market: 0, coi: 99 };
        assert!(matches!(book.execute(acct(1), &cancel, 1, 0, 0), ExecResult::Failed { .. }));
    }

    #[test]
    fn stream_commitment_is_order_sensitive() {
        let g = stream_genesis(0, 1);
        let a = stream_step(&stream_step(&g, 1, 0, b"x"), 1, 1, b"y");
        let b = stream_step(&stream_step(&g, 1, 0, b"y"), 1, 1, b"x");
        assert_ne!(a, b);
    }
}
