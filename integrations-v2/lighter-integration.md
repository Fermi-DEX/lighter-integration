# Lighter Integration v2 — Binding a Verifiable Exchange to a Verifiable Order

> **Historical design.** V3 replaces optimistic production settlement with an
> independent `SequenceTransitionProof`, an atomic `C_bind` join, typed
> terminal inputs, versioned blob binding, and fail-closed finality. See
> `../docs/lighter-integration-spec-v3.md`.

**Supersedes** the architecture-level path document (v1, drafted before the
official Lighter design was reconciled). This revision is grounded in three
verified sources: the **Lighter whitepaper** ("Lighter Protocol: Order Book
Matching and Liquidations with Transparent and Verifiable Computation",
Elliot Technologies, Oct 2025), the **zksecurity public circuit report**, and
**this repository's implementation** (`crates/sequencer`, `crates/vdf`,
`contracts/PoSqHost.sol`). Where the two systems' code disagrees with their
papers, this spec follows code-as-built and says so.

**Question**: Lighter proves *execution* — price-time-priority matching,
liquidations, funding, margin — in SNARKs verified on Ethereum. Its sequencer
still picks the transaction order unverifiably: the circuit report states
plainly that *the circuit does not enforce ordering fairness*, and the
whitepaper concedes millisecond-scale reordering windows (§1) and names
"transaction encryption and pre-commitment schemes … fair sequencing
techniques" as open future work (§8, §2.3). How do we bind Lighter's proven
execution to PoSq's receipted, blindly-admitted tape — at the lowest level,
with the smallest change surface on either side?

**Answer in one line**: in Lighter, time priority is not a timestamp — it is
the per-market, per-side **order nonce**, assigned in execution order and
baked into the Order Book Tree leaf index (`I = p·2^O + o`, whitepaper
§3.7.1); since the whole STF is already proven as a deterministic function of
the transaction sequence, **every fairness property reduces to one binding:
the batch's transaction stream equals the opened prefix of the PoSq tape.**
Enforce that single stream equality — first optimistically behind a fraud
proof, ultimately inside Lighter's own validity proof — and the circuits
Lighter already ships transitively prove *PoSq-fair* price-time priority end
to end. Blind admission comes free from the envelope layer and is literally
the whitepaper's own §8 roadmap item, delivered.

This is the historical **Pattern B′** integration path. Its production
replacement is
[`docs/lighter-integration-spec-v3.md`](../docs/lighter-integration-spec-v3.md)
(zk-appchain: ordering adherence becomes a validity-proven statement, not a
fraud-proven one). It is **not Pattern C**: the freeze-authority airlock in
`integrations-v2/direct-spec.md` exists because L1 AMMs cannot be modified.
Lighter *is* the venue — enforcement moves into its STF and circuits, and the
airlock/cMint machinery is N/A.

---

## 1. The reduction: why one equality suffices

Facts established from the Lighter design (whitepaper §§2.3–2.4, 3.7, 4;
circuit report):

- **L1** The sequencer's only protocol power is choosing the transaction
  order. "Since transaction execution and order matching are verified by the
  Prover, all valid state transitions can be deterministically inferred from
  the transaction order and oracle data" (§2.3).
- **L2** Time priority = order nonce, incremented per new order per side per
  market, i.e. assigned **in execution order**. Priority is structural: ask
  leaf index `I = p·2^O + o`, bid `I = p·2^O + (2^O−1−o)`; the circuits prove
  no-crossing on insert and highest-priority-first on match against these
  indexes (§§3.7.1, 4.3–4.4).
- **L3** The block commitment — a public input to the proof — is a composite
  hash over old/new state roots **and the block's transaction data** (§4);
  aggregation carries it into the single batch commitment stored by the
  Ethereum contracts (§2.5, §5).

L1+L2 ⇒ nonce assignment is itself part of the proven STF; the only free
variable in the whole system is the per-batch transaction sequence. L3 ⇒ that
sequence is already cryptographically committed on Ethereum. Therefore:

> **Bind Lighter's committed transaction stream to the PoSq tape's opened
> prefix, and ordering fairness is inherited by everything the circuits
> already prove.** No per-order `(t, j)` witnessing, no VDF-in-SNARK, no
> timelock-in-SNARK — the VDF and timelock stay in PoSq's accountability
> layer (`PoSqHost` verifies Wesolowski natively through the modexp
> precompile), and Lighter proves only conformance to a stream commitment it
> takes as public input.

What the tape supplies (all implemented in `crates/sequencer`):

| PoSq object | Fields (as built) | Role in this integration |
|---|---|---|
| Receipt ρ_{t,j} | `(epoch, tick, pos, h, bucket, window, ticket_id, x_prev_hash, c_prev, d_prev, d, sig)` — `records.rs` | Signed order-ack: position `(t,j)` fixed at admission, digest-chained (`digest_step`), slashable. **This is the "pre-commitment scheme" Lighter's §2.3 promises**, with a bond behind it |
| Tick record R_t | `(epoch, segment, tick, x, c_prev, c_t, batch_root, da_ref, sig)` | Per-tick sealed batch: `batch_root` is a keccak merkle over `BatchEntry(tick, pos, h, bucket, ticket_id, receipt_sig)` leaves |
| Tape entry | `StreamTape` → `TapeEntryMsg(tick, pos, h, bucket, delay_class, outcome ∈ {pending, opened, gap:<reason>}, intent, mature_at)` — `proto/` | The adapter's live input; `intent` is the bincode `envelope::Intent` once opened |
| Anchor | `(epoch, segment span, tick span, x_b_hash, c_b, segment_seals[], receipts_root, da_attestation, transcript, sig)` — `anchor.rs` / `PoSqHost.submitAnchor` | Host-chain checkpoint the bridge binds batches to |
| Fraud proofs | equivocation, reorder, omission, invalid-log, forced-default — `PoSqHost.sol` proofs 1/2/3/5/6 | Tape-internal accountability (receipt ↔ batch ↔ chain); §5 adds the one cross-system proof |

---

## 2. Object mapping (wire level)

### 2.1 Intent ⊃ Lighter transaction

The PoSq plaintext intent (`envelope.rs`) carries the Lighter transaction
whole:

```
Intent {
  namespace:   u64        — the Lighter deployment's namespace id
  account:     [u8; 20]   — the Lighter main account's L1 address
  nonce:       u64        — PoSq intent nonce (replay scope: the tape)
  expiry_tick: u64        — tape expiry; maps to order time-in-force floor
  payload:     Vec<u8>    — canonical Lighter tx bytes, INCLUDING its
                            API-key signature (CreateOrder, CancelOrder,
                            Transfer, Withdraw, ChangePubKey, …)
  signature:   Sig65      — outer secp256k1 accountability signature
}
```

Two signatures by design, three nonce spaces — each with a distinct owner:

| Object | Signed/assigned by | Checked by | Purpose |
|---|---|---|---|
| Outer intent sig (secp256k1/keccak) | user's L1 key | adapter at reveal (stateless) | objective gap attribution: "client sent garbage" vs "operator gapped me" |
| Inner API-key sig (Lighter's circuit-friendly scheme) | user's Lighter API key | Lighter circuits (§4.2) | execution authorization, as today — untouched |
| PoSq intent nonce | client | sequencer dedup (`seen_h` + ticket nullifier) | tape-level replay protection |
| Lighter API-key nonce | client | circuits vs API Key Tree | Lighter-level replay, as today |
| Lighter order nonce `o` | **STF, in tape order** | circuits (leaf index) | time priority — the thing this whole spec pins down |

Size check: envelope body budget is `S − BODY_OFFSET = 1024 − 382 = 642`
bytes (`envelope.rs`). A Lighter tx is tens of bytes (account/market indexes,
price/size **steps** per the quote-multiplier encoding, flags, client order
index) plus a ~64-byte signature; the bincode Intent wrapper adds ~110. One
fixed envelope size covers every rollup tx type — which is exactly what
operation indistinguishability requires.

### 2.2 What the envelope hides and shows

Visible surface (`header_prefix`, 39 bytes AAD): bucket, namespace, window,
delay class, nonce, plus the fee ticket. Hidden until reveal: **everything
Lighter cares about** — op type, market, side, price, size, account.
Consequences:

- **Cancels race trades through the same blind FIFO.** Stale-quote sniping is
  structurally removed. This requires the bucket taxonomy to never encode op
  type (deployment rule 6): buckets are market-*class* only
  (`Majors`/`LongTail` as built), never market, never op.
- **No selective admission** by direction/size/account: the sequencer admits
  ciphertexts and must answer every one accountably
  (`AdmissionOutcome::{Admitted, Rejected(reason), FullWindow}` — Definition
  6.1, implemented in `admission.rs` with public per-tick capacity).
- **Fee tickets ↔ Lighter's zero-fee model**: tickets are spam control, not
  revenue. Issue them through the existing API-server quota machinery
  (per-account rate tiers become ticket allowances, issuer-signed,
  denomination = delay class). Blind issuance for unlinkability stays the
  documented deferral.

### 2.3 Reveal path, as built

`crates/vdf/src/tlk.rs` implements the **solve-only profile**: fixed-base
timelock KEM (per class `k`: `(g, h_k = g^{2^{T_k}}, π_k)` transparent
setup), Enc is two exponentiations client-side, and openings come exclusively
from **public solving** — `w = u^{2^{T_k}}` with a Wesolowski proof; AEAD is
ChaCha20-Poly1305 with the envelope header as AAD, so a mauled ciphertext
opens to a verifiable error and lands as a paid gap. There is **no
client-assisted fast reveal in the current code** (the DLEQ machinery was
deliberately dropped). Plan latency accordingly (§7): honest-case fill
knowledge is bounded by solve time `D_k = ⌈T_k/q⌉` ticks, and the
**fast-reveal responder** (client publishes `w` + DLEQ on receipt) is the
single highest-leverage PoSq-side upgrade for this integration — it moves
fill latency from ~28 ms to ~network RTT without touching Lighter.

---

## 3. The validity split: gap vs proven no-op

Today Lighter's sequencer pre-validates before ordering; under blind
admission it cannot. Every failure mode must land in exactly one of two
deterministic bins, chosen by *what data decides it*:

| Bin | Predicate decidable from | Examples | Effect |
|---|---|---|---|
| **Gap** at `(t, j)` | plaintext alone (stateless, public) | body doesn't decode as Intent; outer sig invalid; inner API-key sig malformed; unknown tx type/market id out of range; `expiry_tick` < execution tick; AEAD failure (mauled ciphertext) | tape position consumed, **no Lighter tx exists**; permanent in the audit trail |
| **Proven no-op** | Lighter state at execution | stale API-key nonce; insufficient margin; reduce-only violation; OI limit; order-size limits | tape position consumed **and** a Lighter tx exists that the STF processes to a failed outcome |

The line matters because of the binding (§5): the opened stream is defined as
"stateless-valid plaintexts in `(t, j)` order", and a challenger must be able
to adjudicate membership from public data alone. Stateful outcomes cannot be
filtered out of the stream — they must flow into the STF and be *proven* as
no-ops.

**This is the one genuine Lighter circuit delta** (needed at binding level B3,
optional before): an execution-cycle outcome `TxFailed` that verifies the
signature, consumes nothing but the API-key nonce (policy choice: consume, to
keep nonce semantics simple), and leaves state otherwise unchanged. Note
Lighter's circuits already prove *conditional* failures (e.g. FoK kill paths),
so this is an outcome variant, not a new circuit family.

Gap semantics inherited from the tape (`admission.rs::fence`, gap discipline):
a gap consumes a tape position, never a Lighter nonce of any kind; the user
resubmits with the same Lighter nonces. Unlike Fermi's ring (docs3/07 §3.1
item 4), there is nothing to reclaim or compact — the tape *is* the queue, and
gaps stay visible forever.

---

## 4. The ingest adapter: every residual discretion, pinned

The adapter replaces Lighter's mempool→sequencer pipeline. It consumes
`PosqSequencer.StreamTape` + `StreamTickRecords` (gRPC, already implemented)
and emits Lighter blocks. It holds a cursor `(t, j)` and is **fully
deterministic** — two honest adapters given the same tape, oracle feed, and L1
queue produce byte-identical blocks. The rules, exhaustively:

- **R1 — Block boundaries are tick ranges.** Block `b` covers ticks
  `[b·k, (b+1)·k)` for a per-epoch constant `k`; batch spans align to whole
  PoSq segments (so anchors and batches share boundaries). Block timestamp
  := `genesis_wall_time + t·Δ` — attested by the clock, not by the operator's
  wall clock. Funding-due checks (§4.1 pre-execution) read this derived
  timestamp.
- **R2 — Oracle application.** Index-price round `r` (from the decentralized
  oracle feed, itself signed and timestamped) applies at the pre-execution of
  the first block whose tick range contains `r`'s publication tick. Mark
  price/premium recomputation follows the whitepaper's own deterministic
  formulas from there. No "apply when convenient".
- **R3 — User flow.** Consume tape entries with `namespace = lighter_ns`
  strictly in `(t, j)` order. An entry executes when `outcome = opened`; the
  head blocks until opened or gapped. An entry unrevealed at
  `mature_at + grace` becomes `gap:unopened` (sequencer-side, already
  implemented). Worst-case head-of-line stall is therefore bounded:
  `D_k + G` ticks (§7).
- **R4 — Validity split** per §3: stateless-invalid ⇒ gap (adapter emits no
  tx); stateful outcomes ⇒ tx enters the block and the STF decides.
- **R5 — Priority (L1-authenticated) transactions** (Deposit, FullExit,
  CreateOrderBook, UpdateOrderBook) interleave at **batch boundaries only**,
  in L1 queue order — they are censorship-resistant by construction and gain
  nothing from blind admission; fixed placement removes the last insertion
  freedom.
- **R6 — Continuation ("fake") transactions** (ClaimOrder/ExitOrder): already
  circuit-forced — the next user tx executes only when the Instruction Stack
  is empty (§3.6), so multi-cycle expansion contains no discretion. No change.
- **R7 — Liquidations and DMS.** Exchange-initiated events are scheduled
  deterministically: after each pre-execution that changes a mark price, the
  liquidation sweep visits eligible accounts in ascending account-index order
  before any user flow of that block; DMS (dead-man's-switch) cancel-alls
  trigger on the R1-derived timestamp. (Alternative — liquidator bots racing
  through the blind queue — is strictly worse: it reintroduces a latency race
  for a flow that is already proven correct in-circuit.)

Adapter output, besides blocks: the `(tick, pos) ↔ (block, txIndex)` map,
published through the Indexer so any observer can join the two systems'
histories; and per-span, the stream commitment of §5.

---

## 5. The binding ladder

Four levels; each subsumes the previous. Names avoid the v1 doc's `L1–L4`
(which collided with layer-1/2 terminology).

### B0 — Attested (pilot)

The Lighter sequencer runs the adapter internally; PoSq receipts become the
order-ack in the API/WS feed. Trust-based, zero contract/circuit change.
Works against the current repo (`StreamTape` already carries opened intents)
and the transparent lane (plaintext bodies) — no KEM required to start.

### B1 — Co-anchored

Each Ethereum state-update proposal additionally carries
`(posq_epoch, firstTick, lastTick, anchorId)`; the bridge contract checks
`PoSqHost.anchors[anchorId]` covers exactly that span (fields
`firstTick/lastTick/cB/receiptsRoot` exist today). Receipt-vs-tape violations
are already slashable on `PoSqHost` (proofs 2/3: `proveReorder`,
`proveOmission`); B1 puts the *claimed correspondence* on-chain so B2 has
something to challenge. Note the forced-inclusion entry point is
`PoSqHost.forceInclude` / `dischargeForced` / `proveForcedDefault` (the v1
doc's `submitForced` does not exist).

### B2 — Stream equality, optimistic

Define, per batch span, the **opened-stream commitment**:

```
s_0 = keccak("lighter-stream-genesis-v1" ‖ epoch ‖ firstTick)
s_i = keccak("lighter-stream-v1" ‖ s_{i-1} ‖ tick_i ‖ pos_i ‖ keccak(tx_bytes_i))
```

over stateless-valid opened plaintexts in `(t, j)` order, namespace-filtered.
Anyone can recompute it from public data: envelope DA (preimages of `h`,
which certify the namespace), solve/reveal witnesses, and the per-tick batch
roots. The operator posts `s_n` with each proposal; the bridge holds it
challengeable for `challengeWindow`, and **withdrawal finality gates on the
window passing** (Lighter already gates on proof verification; this adds one
more condition on the same rung).

Challenge = exhibit one index `i` where Lighter's committed tx ≠ stream:

1. Merkle path of `BatchEntry(t, j, h, …)` to the signed tick record's
   `batch_root` — `PoSqHost.merkleVerify` + `checkTickRecordSig`, existing.
2. Envelope preimage: bytes with `commitment_hash(epoch, bytes) = h` — binds
   namespace, window, ciphertext.
3. Opening: verify `w = u^{2^{T_k}}` (Wesolowski, via the existing modexp
   path) — or the DLEQ once fast-reveal ships — then ChaCha20-Poly1305
   decrypt of ≤642 bytes on-chain. This is the expensive step (~1M gas-order,
   Solidity implementation; acceptable for a fraud path that should fire
   never). If it offends, wrap steps 2–3 in a helper SNARK — but build the
   dumb version first.
4. Inclusion proof of `tx_i` at stream index `i` in Lighter's batch
   commitment. **This is the single disclosure required from Lighter**: the
   internal structure of the block/batch commitment over transaction data
   (hash chain vs merkle), so a per-index opening is verifiable on-chain.
   Everything else in this spec uses only published facts.
5. Compare; on mismatch, slash the PoSq bond (ordering is the sequencing
   layer's warranty; execution remains covered by Lighter's validity proof)
   and flip the bridge to the degraded mode of §6.

### B3 — Validity-proven (the endgame)

Move the equality in-circuit: the block circuits take `s` (or the PoSq
receipt-digest chain `d_{t,j}` directly) as public input and constrain each
executed transaction to hash-link into it; gaps are witnessed as chain steps
with no execution. The single PLONK-wrapped proof then attests **execution
correctness ∧ order adherence**, and the B2 challenge game retires.

Hash alignment makes this cheap: Lighter merkleizes with **Poseidon2**
(whitepaper fn. 3; the earlier audited circuits used MiMC), and
`records.rs` explicitly reserves the Poseidon2 migration for ZK backends
("Changing the hash re-geneses the chain — the epoch field exists for exactly
that"). So: bump the epoch, switch the digest/log chains for the Lighter
namespace to Poseidon2, and the in-circuit cost is ~one permutation per tx —
noise against a matching cycle. The other circuit delta is §3's `TxFailed`
outcome. Nothing else: no VDF, no timelock, no envelope parsing in-SNARK.

Optional PoSq-side simplification for B3: add `namespace` to the
`BatchEntry` leaf (or run per-namespace digest chains) so the circuit doesn't
need envelope preimages to justify namespace filtering. Small, epoch-gated,
worth doing during the same re-genesis.

---

## 6. Contracts and the joint failure matrix

**Bridge** (new, small — an extension of Lighter's rollup contract or a
standalone `LighterPosqBridge` referencing both):

| Function | Effect |
|---|---|
| `proposeSpan(batchId, epoch, firstTick, lastTick, anchorId, streamCommitment)` | B1 span check against `PoSqHost.anchors`; stores `s_n` for the challenge window |
| `challengeStream(i, batchEntry, merklePath, tickRecord+sig, envelopeBytes, opening, lighterTxProof)` | the §5 B2 game; on success calls into slashing + degradation |
| `setDegraded()` / degradation hooks | see failure matrix |

**Forced inclusion, unified.** Two inboxes, deliberately kept two:

- *Market operations* needing censorship resistance →
  `PoSqHost.forceInclude(h)` with the envelope posted as calldata/blob; the
  sequencer must exhibit a receipt (`dischargeForced`) within `F_force` ticks
  or be slashed (`proveForcedDefault`, proof 6). This upgrades "include by
  deadline" from a freeze trigger to a slashing condition.
- *Asset exits* stay on **Lighter's own priority queue** (whitepaper §2.2.2)
  ending in the Escape Hatch (§6): they are L1-authenticated and must survive
  a total PoSq failure independently. Deadline ordering: `F_force` (slash)
  fires strictly before Lighter's escape deadline (freeze) — money-out is the
  backstop of last resort, not the first response.

**Failure matrix** (each row: detected by → response → user impact):

| Failure | Detection | Response | Degradation |
|---|---|---|---|
| PoSq cadence fault (clock stalls) | fence machinery (`admission.rs::fence`, cadence faults in `Status`) | windows fenced, tickets released; adapter idles — Lighter halts *admission*, never state | trading pauses; exits unaffected (priority queue live) |
| Tape/anchor fraud (reorder, omission, equivocation, invalid log) | `PoSqHost` proofs 1/2/3/5 | bond slashed, `rescueMode` | bridge flips Lighter to **declared FCFS mode** — i.e. exactly Lighter-today semantics, publicly flagged; degraded but live (the direct analogue of rescue §10.4 / `emergency_thaw`) |
| Stream mismatch (batch ≠ tape) | B2 challenge | PoSq bond slashed; batch rejected before finality | same declared-FCFS degradation |
| Lighter prover failure / invalid state transition | Lighter's own verifier — **out of PoSq scope (N2)** | proposals stop verifying | Lighter's existing model; PoSq unaffected |
| Priority tx omitted past deadline | Lighter contracts (as today) | Escape Hatch: freeze + merkle exits vs frozen root at last mark price | full stop, assets recoverable — unchanged |
| Sequencer omits forced envelope | proof 6 | slash | forced flow migrates to declared-FCFS or exit |

The invariant across all rows: **PoSq failure degrades Lighter to what
Lighter already is today — never below it.** The integration is strictly
additive in trust.

---

## 7. Latency and throughput budget (reference profile, `params.rs`)

Δ = 100 µs, q = 3600 sq/tick (R = 3.6·10⁷ sq/s), W = 50, Γ = 3, segment
F = 256 ticks = 25.6 ms, anchor every 8 segments ≈ 205 ms of tape,
S = 1024 B, capacity 64/tick/bucket.

| Quantity | Value | Notes |
|---|---|---|
| Order-ack (receipt) | sub-ms + RTT | position `(t,j)` fixed and signed; the "pre-commitment" Lighter planned, with slashing |
| Fill known, **as built** (solve-only KEM) | ≈ `D_0` = ⌈10⁶/3600⌉ = 278 ticks ≈ 28 ms after tick seal | public solver at frontier rate; batched solves amortize |
| Fill known, **with fast-reveal upgrade** | ≈ RTT after receipt (~few ms) | client publishes `w` + DLEQ on ack; the one PoSq roadmap item this integration should pull forward |
| Worst-case head-of-line stall | `D_0 + G` = 278 + 256 ticks ≈ 53 ms | non-revealing head entry; then a permanent paid gap |
| Withdrawal finality | anchored + proven + B2 window | rung 3 of the finality ladder; never rung 1 |
| Throughput ceiling | 64 env/tick/bucket → 640k env/s/bucket | Lighter's "tens of thousands of ops/s" fits with >10× headroom; DA ≈ 10 MB/s at 10⁴ env/s, routed to the high-throughput DA layer, referenced from batches by `(t, j, h)` — no double-posting |
| Blindness tax vs Lighter today | ~few ms honest-case (with fast reveal) | against ms-scale current latency; §1's "millisecond-scale windows" go to zero *by construction*, not by being hard to exploit |

**Delay-menu caveat (from our own formal verification)**: the code's no-look
bound (`min_delay_squarings` ≈ 597,600 for W=50) omits the paper Eq. 1's
δ_sub/σ_fence terms; the paper-faithful minimum is 2,469,600 (divergence
D001, proven in `formal_verification/`). T₁ = 10⁶ passes code, fails paper.
**Size the Lighter deployment's menu against the paper bound** (e.g. T₁ ≥
2.5·10⁶ ⇒ solve-only fill ≈ 70 ms, another reason to ship fast-reveal) until
D001 is resolved.

---

## 8. Change surface per component

| Component | Change | Size |
|---|---|---|
| Lighter sequencer | mempool → ingest adapter (§4); emit `(t,j)↔(block,txIdx)` map | the core engineering item; matching engine untouched |
| Lighter API servers | become relayer + SDK host: envelope build (KEM Enc = 2 modexps), ticket attach, receipt/rejection surfacing; later the fast-reveal responder | client-side library work |
| Lighter circuits | **B3 only**: stream-link constraint (~1 Poseidon2/tx) + `TxFailed` cycle outcome | small relative to matching circuits |
| Lighter contracts | bridge (§6, 3 functions); disclose tx-commitment structure (§5.4) | ~200 lines + one spec disclosure |
| PoSq sequencer | namespace + bucket config; nothing structural (admission, receipts, tape, anchors all as-built) | config |
| PoSq crates | fast-reveal path (reinstate DLEQ in `tlk.rs`) — pulled forward; Poseidon2 chain migration + optional namespace-in-leaf, epoch-gated, at B3 | the two scheduled roadmap items this integration needs |
| `PoSqHost.sol` | none for B0–B1; B2 challenge verifier (Wesolowski path reused, ChaCha20-Poly1305 verify added) | ~300 lines, fraud path only |

---

## 9. Build plan

1. **P0 — transparent pilot (B0).** Adapter against the dev profile
   (`PosqParams::dev`), plaintext lane: `StreamTape` → blocks, receipts as
   acks, R1–R7 replay-tested (two adapters, byte-identical blocks). No
   Lighter circuit/contract change.
2. **P1 — co-anchor (B1).** Bridge on testnet; spans aligned to segments;
   `PoSqHost` fraud-proof drills (reorder/omission against a misbehaving-
   sequencer harness).
3. **P2 — blind admission.** KEM envelopes end to end; solver daemon;
   validity-split conformance tests (every §3 row lands in its bin);
   fast-reveal responder if the DLEQ path has landed.
4. **P3 — stream equality (B2).** Requires the Lighter tx-commitment
   disclosure. Challenge game on testnet including the on-chain decrypt;
   withdrawal gating on the challenge window.
5. **P4 — in-circuit binding (B3).** Poseidon2 epoch migration on the PoSq
   chains; stream-link constraint + `TxFailed` outcome in the block circuits;
   retire the B2 game.

**Verification checklist before mainnet:**

- [ ] Adapter determinism: independent replay from public data reproduces
      every block byte-for-byte (R1–R7 have no hidden inputs).
- [ ] Gap positions are permanent and visible in the joined history; no
      Lighter nonce is ever consumed by a gap.
- [ ] `TxFailed` (or pre-B3 equivalent) proven for every stateful-failure
      class; a stateless-invalid entry can never reach the STF.
- [ ] Bucket taxonomy leak test: envelope traffic across op types is
      indistinguishable (size, header, timing) within a bucket.
- [ ] Delay menu sized against paper Eq. 1 (D001), not the code bound.
- [ ] Stream commitment reproducible by a third party from DA + reveals only.
- [ ] Forced-inclusion drill: `forceInclude` → `dischargeForced` inside
      `F_force`; `proveForcedDefault` fires on an omitting sequencer;
      Lighter's escape-hatch deadline verified to sit strictly after.
- [ ] Degradation drill: induced `rescueMode` flips the bridge to declared
      FCFS with no state loss and live exits.
- [ ] Bond sized to the rung-1→rung-3 exposure window at target open
      interest (trading risk, not bridge risk).

---

## 10. Deltas from the v1 path document

For reviewers of the earlier draft: (1) the in-circuit design is simplified
from "witness `(t,j)` keys and assert monotonic consumption" to the single
stream-equality binding — the order-nonce observation (§1 L2) makes per-entry
keys redundant; (2) v1's open question 1 (where is the order committed?) is
resolved by whitepaper §§2.5/4/5 — committed in the block/batch commitment;
only its per-index structure needs disclosure; (3) the validity split (§3),
determinism rules R1–R7 (§4), failure matrix (§6), and real latency numbers
(§7) are new; (4) the KEM is solve-only as built — fast reveal is an upgrade,
not an assumption; (5) `PoSqHost` function names corrected
(`forceInclude`, not `submitForced`); (6) phases renamed B0–B3/P0–P4 to stop
colliding with L1/L2 chain terminology; (7) the D001 formal-verification
divergence now constrains the deployable delay menu.
