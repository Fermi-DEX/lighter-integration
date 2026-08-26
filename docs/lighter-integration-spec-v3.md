# Continuum × Lighter Integration v3.1

## A proof-carrying sequencer feed for validity-proven order and execution

**Status:** proposed production specification; team-test implementation included
**Original date:** 12 July 2026
**V3.1 update:** 26 August 2026
**Repository baseline:** `cryptohariseldon/continuum-monorepo`, including `main` and branch `lighter-integration-v1` at `9418a528`  
**Target:** Lighter Core, an application-specific validity rollup settled on Ethereum

---

## 1. Decision

Lighter already proves that a chosen transaction stream executed correctly. It proves signatures, nonce transitions, risk checks, price-time-priority matching, liquidations, state transitions, recursive aggregation, and the correspondence between execution public data and Ethereum blob data. The remaining discretionary input is the transaction stream chosen by the sequencer.

Continuum should supply that stream as a **proof-carrying sequencer feed**.

The production integration consists of two validity proofs:

```text
SequenceTransitionProof
    proves that an ordered Lighter input stream is the exact deterministic
    projection of a valid Continuum transcript

LighterExecutionProof
    proves that Lighter executed that exact ordered input stream correctly

Ethereum settlement
    accepts the state transition only when both proofs expose the same C_bind
```

The proofs remain independent and are generated in parallel. Ethereum joins them by equality of one circuit-native commitment. Recursive aggregation into a single proof is a later gas optimization; it is not the security boundary.

This design closes the precise gap in Lighter’s present architecture. Lighter’s documentation says that the sequencer coordinates FIFO ordering and that execution is deterministic from transaction order and oracle data. Its whitepaper also identifies the sequencer’s ordering power and residual millisecond-scale reordering as the remaining fairness surface. The execution proof establishes price-time priority *after* an order has entered Lighter’s state. Continuum establishes which signed transaction entered first.

The integration must fail closed. A Lighter batch without a valid sequence proof may be exposed as provisional soft state, but it cannot advance the Ethereum-settled state. A Continuum failure moves the system to priority-only operation and then Lighter’s existing Escape Hatch if necessary. It never silently falls back to an unprotected sequencer.

### 1.1 The minimal kernel

The kernel has four parts:

1. Users encrypt already-signed Lighter transactions and submit fixed-size envelopes to Continuum.
2. Continuum fixes each envelope at a receipted position before plaintext is available.
3. A sequence proof derives the exact Lighter input stream from a contiguous, validity-proven transcript transition.
4. Lighter’s proof and the sequence proof commit to the same ordered-input root.

Everything else—receipts, DA, force paths, operating modes, versioning, and solver capacity—exists to make this kernel usable under faults.

---

## 2. Verified Lighter architecture and the actual gap

This specification is grounded in Lighter’s current [technical architecture](https://docs.lighter.xyz/about-lighter/technical-architecture-lighter-core), [October 2025 whitepaper](https://assets.lighter.xyz/whitepaper.pdf), [current API documentation](https://apidocs.lighter.xyz/docs/get-started), and [published circuit and contract audits](https://docs.lighter.xyz/security/security-audits).

### 2.1 What Lighter proves

Lighter is an application-specific Ethereum L2. Its sequencer receives signed L2 transactions, executes them, and constructs blocks and batches. Its prover proves the resulting state transition. Ethereum contracts custody assets, store the canonical state root, verify batch proofs, execute withdrawals, and maintain a priority-operation queue.

The audited proving stack uses Plonky2 over the Goldilocks field with recursive transaction, block, segment, and batch aggregation. The final proof is wrapped for Ethereum verification. Lighter’s wrapper also proves that its public account-delta data corresponds to the EIP-4844 blob committed on Ethereum.

The execution relation covers:

- Lighter API-key signatures and per-key nonce progression;
- order placement, modification, cancellation, and other L2 operations;
- margin, health, reduce-only, slippage, and protocol risk constraints;
- deterministic instruction-stack continuation for multi-cycle operations;
- price-time-priority matching;
- liquidations and protocol accounting;
- old-to-new state-root continuity;
- priority-operation prefix consumption;
- account-delta and market-data correspondence to the posted blob.

Lighter’s Order Book Tree encodes price and an internally assigned order nonce into leaf position. At an equal price, the lower nonce is older and has higher priority. The proof therefore establishes that matching respects the order already represented in state.

### 2.2 What it does not prove

The public specification does not establish:

- a cryptographic ingress point at which arrival becomes final;
- a byte-level merge rule across API servers, regions, or connections;
- that transaction order is independent of plaintext;
- that an API acknowledgement binds the sequencer to a position;
- that every acknowledged transaction appears in the executed stream;
- that batch boundaries and oracle choices were fixed before order contents were known.

Lighter’s execution proof can approve either of two valid transaction orders if both produce valid state transitions. It proves execution validity, not fair selection of the execution input.

### 2.3 Why stream equality is the correct reduction

For user flow, Lighter’s state transition is deterministic once the following are fixed:

```text
old state
ordered logical L2 transactions
priority-operation prefix
oracle and frame inputs
protocol configuration
```

The existing `lighter-integration-v1` branch correctly identified the key reduction: if the complete logical input stream is bound to Continuum’s order, Lighter’s existing matching proof inherits that order. No change to the matching algorithm is required.

The v1 branch did not complete the binding. Its B3 circuit accepted an opened-stream hash as a public input but did not prove that this hash came from a valid Continuum transcript. A malicious operator could choose an arbitrary transaction list, choose its matching stream hash, and produce a valid Lighter proof. The demo bridge compounded this by accepting a Boolean off-chain mismatch assertion and by never binding a span to a Lighter state root or execution proof.

V3 adds the missing `SequenceTransitionProof` and makes equality of the two proof outputs a settlement condition.

---

## 3. Design-space decision

| Design | Guarantee | Main defect | Verdict |
|---|---|---|---|
| Continuum gateway and signed receipts | Fast accountable order acknowledgements | Lighter can settle a different order | Shadow mode only |
| Co-anchor a PoSq root beside a Lighter batch | Public correlation between systems | A root alone does not prove exact consumption | Insufficient |
| Optimistic stream-equality challenge | Detectable divergence after a challenge window | Full transaction data is not in Lighter’s blob; on-chain TLP opening and insertion/deletion proofs are impractical | Reject |
| Put the complete PoSq verifier in every Lighter transaction circuit | One monolithic proof | Non-native RSA, Keccak, receipt, and TLP verification bloats the execution hot path | Reject |
| Independent sequence proof and execution proof joined on one commitment | Validity-enforced order and execution; parallel proving; modular upgrades | Requires one additional proof relation and a small wrapper/contract change | **Selected** |
| Recursively verify the sequence proof inside Lighter’s final wrapper | Same security with one Ethereum proof | Tighter proof-system and upgrade coupling | Later optimization |

The selected design preserves the clean separation of concerns:

- Continuum proves ordering and deterministic derivation.
- Lighter proves execution.
- Ethereum proves that both statements refer to the same batch.

---

## 4. Security objective and scope

### 4.1 Primary property

For every Ethereum-finalized protected Lighter batch, the ordered logical user transactions consumed by Lighter are exactly the Lighter namespace projection of the accepted Continuum transcript range, with no insertion, deletion, duplication, or reordering.

### 4.2 Supporting properties

Subject to the assumptions in §4.4:

- a Continuum receipt fixes an envelope’s position before plaintext can be used by the sequencer;
- transaction contents cannot influence admission position within the protected namespace;
- batch and frame cuts are committed before the corresponding plaintext opens;
- every consumed transcript position has one objectively proven resolution;
- state-dependent transaction rejection occurs inside Lighter’s execution proof rather than in an opaque prefilter;
- a signed orphan receipt is slashable;
- a stalled operator cannot settle a conflicting state transition;
- Lighter’s L1 priority queue and Escape Hatch remain independent of Continuum availability.

### 4.3 Explicit non-goals

The design does not prove:

- that a message censored before receipt issuance reached the sequencer;
- equal network latency across users or regions;
- physical-time FCFS between Ethereum priority requests and off-chain ingress;
- secrecy if the user leaks its own signed transaction out of band;
- absolute wall-clock delay independent of the calibrated sequential-work assumption;
- oracle correctness beyond Lighter’s existing oracle model;
- successful execution of a validly sequenced transaction under later state, nonce, margin, price, or slippage checks;
- ordering fairness for any unprotected lane sharing the same mutable order book.

### 4.4 Assumptions

The final property depends on:

- soundness of Lighter’s execution proof and the new sequence proof;
- collision resistance of the pinned hash functions;
- unforgeability of Lighter API-key signatures and Continuum receipts;
- correct Ethereum settlement and Lighter custody contracts;
- sequentiality and calibrated hardware margin of the selected time-lock module;
- availability of the receipted Continuum data needed to generate the sequence proof;
- at least one honest public solver for non-cooperative recovery;
- correct, version-pinned canonical encodings;
- governance not replacing the verifier or implementation outside the declared upgrade process.

Zero knowledge is optional for the sequencing statement. Succinct validity is essential. The proof may keep Lighter transaction bytes private where Lighter’s existing validium-like state design permits it.

---

## 5. System architecture

```text
Lighter SDK
   │ sign ordinary Lighter L2 transaction with existing API key
   │ encrypt and pad into Continuum envelope
   ▼
Continuum admission
   │ signed receipt fixes (epoch, tick, position)
   │ fixed frame closes before opening
   ▼
Permissionless time-lock solve
   │ canonical cleartext or objective terminal invalidity
   ▼
Sequence prover ───────────────► π_seq, C_bind
   │ derives complete ordered input
   │
   └────────► Lighter sequencer / witness generation
                    │ executes logical inputs
                    ▼
              Lighter prover ──► π_exec, C_bind
                                    │
                                    ▼
Ethereum LighterCoreV3
   verify π_seq
   verify π_exec
   require C_bind(seq) == C_bind(exec) == blob_header.C_bind
   advance Lighter state + Continuum cursor atomically
```

### 5.1 Logical roles

- **Lighter client:** creates the existing signed transaction and encrypts it.
- **Continuum ingress:** validates only the visible envelope surface and assigns the receipted order.
- **Continuum DA nodes:** retain envelopes, receipts, tick records, and opening material.
- **Public solver network:** begins mandatory time-lock work and publishes verified openings.
- **Sequence prover:** proves the Continuum transition and derives Lighter’s canonical inputs.
- **Lighter sequencer:** becomes an execution coordinator. It no longer chooses protected user order.
- **Lighter prover:** proves the existing state transition plus a small ordered-input accumulator.
- **Ethereum contracts:** join both proofs, store the consumed Continuum cursor, and preserve the priority/escape path.

The same company may initially operate several roles. Security follows from the proofs and the encryption boundary, not organizational separation.

---

## 6. Versioned deployment domain

Every receipt, envelope, proof, frame, and settlement commitment is bound to one deployment domain.

```text
DeploymentDomainV3 {
    ethereum_chain_id:        u64,
    lighter_proxy:            address,
    lighter_impl_codehash:    bytes32,
    lighter_exec_vk_hash:     bytes32,
    posq_host:                address,
    posq_genesis_hash:        bytes32,
    posq_verifier_id:         bytes32,
    namespace_id:             bytes32,
    policy_hash:              bytes32,
    encoding_version:         u16,
    decryption_module_id:     bytes32
}

domain_hash = keccak256("CONTINUUM_LIGHTER_DOMAIN_V3" || canonical(domain))
```

The domain prevents replay across Lighter deployments, forks, verifier upgrades, Continuum epochs, or policy versions. A batch cannot cross a domain change. Any verifier, implementation, encoding, policy, or decryption-module change starts a new epoch from a state checkpoint jointly committed by the old and new configurations.

Production configuration pins:

- the Lighter proxy and implementation code hash;
- the execution verification key;
- the sequence verification key;
- the PoSq genesis and transcript verifier;
- the envelope and transaction encodings;
- the protected scheduling policy;
- the decryption module and parameters;
- all internal hash identifiers and serialization rules.

---

## 7. Client and envelope protocol

### 7.1 Preserve Lighter authorization

The protected payload is the exact canonical signed Lighter transaction produced by Lighter’s current SDK. Lighter API keys use a circuit-friendly Schnorr construction and maintain an independent nonce per API key. Continuum does not replace that authorization scheme.

```text
CanonicalSignedLighterTx {
    tx_version,
    tx_type,
    exact Lighter transaction fields,
    account_index,
    api_key_index,
    api_key_nonce_and_attributes,
    Lighter API-key signature
}
```

There is no mandatory outer L1-wallet signature. The v1 proposal’s extra secp256k1 user signature would break bot key isolation, subaccount operations, smart-wallet use, and unattended HFT. The one-time admission ticket pays for malformed or unauthorized submissions. Lighter remains the sole authority on whether the recovered transaction is authorized.

### 7.2 Envelope

```text
LighterEnvelopeV3 {
    magic:                    [u8; 8],
    version:                  u16,
    size_class:               u16,
    domain_hash:              bytes32,
    posq_epoch:               u64,
    inclusion_window_start:   u64,
    inclusion_window_end:     u64,
    admission_ticket:         TicketV3,
    decryption_module_id:     bytes32,
    puzzle_public_data:       fixed[module, size_class],
    ciphertext:               fixed[size_class],
    padding:                  zeroes
}
```

All integers use the canonical byte order specified by the test-vector suite. Lists include explicit length and maximum length. Merkle roots commit to leaf count. The production protocol does not use Rust `bincode` as a consensus encoding.

The v1 branch assumes a 1,024-byte envelope with a 642-byte body. V3 treats 1,024 bytes as a candidate, not a fact. Before freezing a size, the integration test suite must serialize every currently supported Lighter transaction, including grouped orders, pool actions, transfers, withdrawals, key changes, subaccount actions, and maximum signatures. The selected protected trading class is the smallest fixed power-of-two size that fits all price- and order-affecting transaction types.

If one class cannot cover every administrative transaction, those operations may use a separate fixed class. Every class must remain operation-indistinguishable within its declared scope. Market, side, price, size, account, create/cancel/modify type, and API-key identity remain encrypted.

### 7.3 Admission tickets

Tickets are prepaid, one-time bearer credentials with a nullifier. They enforce spam cost and Lighter quotas without revealing account identity to ingress.

```text
TicketV3 {
    issuer_id:       bytes32,
    class_id:        u16,
    nullifier:       bytes32,
    expiry_epoch:    u64,
    issuer_sig:      bytes
}
```

Ticket issuance may reflect Lighter account tier, staking, rate limit, or commercial policy. Ticket class cannot change ordering within the protected stream. It controls admission quota and price only.

Ingress atomically consumes the nullifier when it signs a receipt. A crash after receipt issuance cannot restore the ticket. Nullifier state is durable and is part of the proved PoSq transition.

### 7.4 Protected batch submission

`sendTxBatch` is not a sequencing unit. The protected SDK decomposes an API batch into separately encrypted logical transactions and returns one receipt per transaction. This prevents a client-selected batch from becoming an opaque internal ordering island.

Protocol-native grouped, OCO, or atomic order types remain one logical transaction if Lighter’s existing state transition already treats them atomically. Their internal semantics remain Lighter’s responsibility.

For transactions from one API key with consecutive nonces, the SDK waits for admission of nonce `n` before sending `n+1`, or uses Lighter’s documented skip-nonce semantics. The protocol never silently reorders transactions to repair a client nonce race.

### 7.5 API surface

The protected client surface is:

```text
POST /v3/protected/tx
    request:  fixed LighterEnvelopeV3 bytes
    response: AdmissionReceipt | SignedRejection | SignedFullWindow

GET /v3/protected/receipt/{commitment}
GET /v3/protected/frame/{frame_id}
GET /v3/protected/tape?from={cursor}&limit={n}
WS  /v3/protected/stream?from={cursor}
```

The Lighter SDK exposes:

```text
sendProtectedTx(...)
sendProtectedBatch(...): Receipt[]
getProtectedStatus(receipt)
subscribeProtectedAccount(account_index)
```

User-visible states are:

```text
SUBMITTED
SEQUENCED          // signed position held
FRAME_CLOSED       // order and frame fixed
OPENED             // canonical cleartext available
EXECUTED           // Lighter soft state
SEQUENCE_PROVEN
EXECUTION_PROVEN
ETHEREUM_FINALIZED

REJECTED(reason)
TERMINAL_INVALID(reason)
PRIORITY_ONLY
```

The HTTP `200` currently returned by Lighter means only that API syntax was accepted. The protected route returns the actual signed Continuum admission outcome.

---

## 8. Ordering policy and frame construction

### 8.1 One protected stream

All L2 transactions capable of changing trading state must use the protected stream:

- create, modify, cancel, and cancel-all;
- market, limit, trigger, TWAP, and grouped order operations;
- externally initiated liquidation transactions;
- leverage, margin, transfer, withdrawal, pool, or account operations whose placement relative to orders can change execution validity or risk.

A direct transaction path into the same mutable book defeats the guarantee. Read-only API calls are outside the stream. L1 priority operations use the separate safety lane in §12.

### 8.2 Uniform protected eligibility

Lighter currently applies different speed bumps by account and transaction class: Standard accounts list 200 ms maker/cancel and 300 ms taker delay, while Premium maker/cancel flow can receive a 0 ms path and Premium takers receive 140–200 ms delay. These are public product policies, but preserving them would require a post-decryption priority scheduler and would make the guarantee “FCFS after policy-defined eligibility” rather than strict receipt order.

V3 selects one uniform minimum execution delay for the protected stream:

```text
eligible_frame(item) = admission_frame(item) + protected_delay_frames
```

The delay is identical for create, cancel, modify, maker, taker, account tier, and market. Fees, staking discounts, and rate limits may remain differentiated. Latency priority does not.

This is the smallest rule that preserves strict blind FIFO, keeps cancels indistinguishable from trades, and avoids a large scheduling circuit. The exact delay is an epoch parameter derived from the opening and proving latency budget. It is not hard-coded in this document.

A future policy-aware scheduler may support differential speed classes by proving `eligibility = admission_position + declared_policy_delay` and maintaining a proved pending-set root. Such a deployment must be marketed as policy-aware ordering and uses a different `policy_hash`. It is outside V3.

### 8.3 Fixed frames

Frame boundaries are fixed before any payload in the frame can open.

```text
FrameId(tick) = floor(tick / ticks_per_frame)

FrameRange(f) =
    [f * ticks_per_frame, (f + 1) * ticks_per_frame)
```

The sequencer cannot close a frame early because it is empty, full of expensive operations, or contains adverse flow. If one frame exceeds the Lighter block capacity, it is divided into deterministic chunks by `(tick, position)` with a fixed maximum item count. Overflow carries to the next chunk without changing relative order.

```text
FramePlanV3 {
    domain_hash,
    frame_id,
    chunk_id,
    global_start_cursor,
    global_end_cursor,
    namespace_start_index,
    namespace_end_index,
    frame_close_tick,
    execute_not_before_frame,
    max_item_count,
    l1_origin,
    priority_start,
    priority_end,
    oracle_snapshot_root,
    protocol_event_root,
    decryption_module_id,
    policy_hash
}
```

`FramePlanV3` is committed in the Continuum transcript before an opening for that frame is accepted. This removes content-dependent discretion over block cuts, priority ranges, and oracle snapshots.

### 8.4 Time semantics

A VDF proves sequential work. It does not prove wall-clock time. V3 does not derive Lighter’s economic timestamp as `genesis_time + tick × target_tick_duration`.

Lighter keeps its existing proved timestamp, oracle, funding, trigger, expiry, and dead-man-switch semantics. The integration adds one constraint: the exact `l1_origin`, oracle snapshot, and protocol-event roots used for a frame are committed before user plaintext opens. This prevents order-aware input selection while avoiding a new claim that PoSq ticks are a trusted wall clock.

Any stronger schedule—such as “use the latest oracle round available before frame close”—must define availability through signed provider data or a transparent Continuum oracle namespace and must be proven under a new `policy_hash`.

---

## 9. Opening and terminal resolution

### 9.1 Permissionless solve

Every receipted envelope is opened through a permissionless time-lock mechanism. Solving begins as soon as the envelope is receipted. Publication or use of the opening remains gated by the protocol’s maturity rule. Starting work at receipt avoids the current implementation error in which solving begins only at maturity and completes roughly one full delay too late.

The decryption interface is versioned:

```text
trait DecryptionModuleV3 {
    module_id() -> bytes32
    validate_envelope(public_data) -> bool
    derive_maturity(receipt, params) -> Tick
    verify_opening(envelope, opening) -> bool
    open(envelope, opening) -> bytes | BadAead
    verify_aggregate(frame, aggregate_opening) -> bool
}
```

The sequence proof and deployment domain bind `decryption_module_id`.

### 9.2 Mandatory resolution

A receipted position has exactly one of these final states:

```text
CLEAR(bytes)          // authenticated decryption succeeded
BAD_AEAD              // a valid puzzle opening derives a key that fails AEAD
BAD_ENCODING          // CLEAR bytes fail the pinned Lighter parser
L1_CANCELLED          // explicit settlement-enforced cancellation event
```

`TLP_UNAVAILABLE`, `DATA_UNAVAILABLE`, and `SOLVER_TIMEOUT` are not terminal no-ops. They stall the sequence proof at that cursor. Allowing an unavailable opening to become a gap would let an operator with a private decryption advantage learn the transaction, suppress the public opening, and omit adverse flow.

DA is therefore checked before receipt finality. Once a receipt is issued, the system either produces the objective opening result or stops finalizing later protected state.

### 9.3 Execution versus sequence validity

Continuum decides only whether authenticated cleartext exists and whether it parses under the pinned byte grammar. It does not decide Lighter authorization or state validity.

For `CLEAR(bytes)` that pass the parser, Lighter’s circuit evaluates:

- API-key signature validity;
- API-key nonce and skip-nonce rules;
- expiry and time-in-force;
- margin and health;
- order flags and market state;
- slippage, reduce-only, and protocol limits;
- the exact transaction-specific success or failure path.

Invalid signatures, stale nonces, insufficient margin, bad price bounds, and state-dependent failures cannot be filtered by the adapter. They become circuit-proven Lighter outcomes.

For `BAD_AEAD` and `BAD_ENCODING`, Lighter’s ordered-input circuit consumes the derived item, proves a no-state-change terminal result, and consumes no Lighter API nonce. The admission ticket remains spent.

### 9.4 Failure-semantics table

Before production, Lighter must publish a table for every supported transaction type:

| Field | Required definition |
|---|---|
| Canonical parser | exact bytes, bounds, and version |
| Authentication | key lookup and signature predicate |
| Nonce rule | increment, skip, consume-on-failure behavior |
| Stateful checks | exact ordered predicates |
| Failure result | stable code and state delta |
| Retry rule | whether the same signed transaction may be retried |
| Internal expansion | instruction-stack cycles generated by one logical input |

Blind admission removes API-server stateful prevalidation. This table is a consensus artifact, not an SDK note.

---

## 10. Canonical derivation stream

### 10.1 Namespace projection

Continuum may carry multiple applications on one global tape. The envelope header exposes `namespace_id`, while all economically sensitive Lighter fields remain encrypted.

For a global cursor range `(old_cursor, new_cursor]`, the sequence proof:

1. verifies the complete global receipt/tick transition;
2. selects every entry with the pinned Lighter namespace;
3. proves that no Lighter entry in the range is omitted;
4. preserves global `(tick, position)` order;
5. resolves each selected entry exactly once.

Lighter stores both the last consumed global cursor and the namespace-local item count. The latter prevents ambiguity when a global range contains zero Lighter items.

### 10.2 Derived item

```text
DerivedItemV3 {
    domain_hash:          bytes32,
    frame_id:             u64,
    chunk_id:             u32,
    tick:                 u64,
    position:             u32,
    envelope_hash:        bytes32,
    receipt_digest:       bytes32,
    resolution:           u8,
    cleartext_length:     u32,
    cleartext_hash:       LighterHash,
    terminal_reason:      u16
}
```

The logical transaction bytes are a private witness where permitted. Their length and hash are public to the proof relation. Length is always committed.

### 10.3 Dual Lighter-native accumulators (V3.1)

The sequence prover computes the rich ordered-item accumulator:

```text
D_0 = H_L(
    "CONTINUUM_LIGHTER_ITEMS_V3",
    domain_hash,
    frame_id,
    start_cursor,
    item_count
)

D_{i+1} = H_L(D_i, canonical_field_encode(DerivedItemV3_i))

ordered_item_root = D_n
```

The sequence proof also proves a one-to-one projection into a compact
execution stream. Both the sequence prover and Lighter compute:

```text
ExecutionItemV3 {
    logical_index: u64,
    tx_type: u16,
    tx_hash: [Goldilocks; 5],
    outcome_class: u16,
    terminal_noop: bool
}

E_0 = H_L(
    "CONTINUUM_LIGHTER_EXEC_INIT_V3_1",
    domain_hash[8 × u32],
    start_cursor[2 × u32],
    item_count[2 × u32]
)

E_{i+1} = H_L(
    "CONTINUUM_LIGHTER_EXEC_STEP_V3_1",
    E_i[4 × Goldilocks],
    logical_index[2 × u32],
    tx_type,
    tx_hash[5 × Goldilocks],
    outcome_class,
    terminal_noop
)

execution_stream_root = E_n
```

The compact item is folded directly into one accumulator preimage. There is
no separately hashed execution leaf. Lighter reuses its existing five-field
transaction hash, so the per-item hot path adds one tagged Poseidon2 hash and
one 64-bit index split. Receipt, envelope, opening, and cleartext metadata stay
in the sequence proof.

`H_L` is the exact hash, field, width, round constants, and padding rule pinned to Lighter’s audited circuit fork. “Poseidon2-compatible” is not a specification. A shared vector suite fixes:

- field modulus and canonical limb range;
- field-element ordering;
- byte-to-field packing;
- length binding;
- domain constants;
- output limb order;
- serialization to Ethereum `bytes32`.

Reference `bytes32` serialization is four canonical 64-bit output limbs in declared order. The final choice follows Lighter’s circuit conventions and must be frozen by vectors before implementation.

The existing global Keccak receipt/log commitments remain. V3.1 adds the rich
namespace accumulator and compact execution accumulator, then proves their
one-to-one relationship. It does not migrate unrelated Continuum tenants or
discard cheap EVM accountability commitments.

### 10.4 Complete derivation input

The user root alone is insufficient. Lighter’s batch also depends on L1 priority operations, oracle data, and protocol-generated events.

```text
DerivationInputV3 {
    ordered_user_item_root,
    ordered_user_item_count,
    priority_start,
    priority_end,
    priority_root,
    oracle_snapshot_root,
    protocol_event_root,
    l1_origin,
    frame_plan_root
}
```

The protected claim is strongest for user ordering. Other roots ensure that their merge with user flow is fixed before reveal and that Lighter’s proof and sequence proof refer to the same complete frame inputs.

Internal instruction-stack continuation is not separately sequenced. It is a deterministic expansion of one logical Lighter transaction and remains inside Lighter’s execution relation.

Protocol-created operations must satisfy one of two rules:

- the execution circuit proves a unique deterministic trigger and placement; or
- an external signed operation enters through Continuum like any other user transaction.

Discretionary internal transactions are forbidden in protected mode.

---

## 11. SequenceTransitionProof

### 11.1 State

```text
PoSqApplicationStateV3 {
    domain_hash,
    epoch,
    global_cursor,
    namespace_item_count,
    transcript_root,
    receipt_chain_root,
    ticket_nullifier_root,
    frame_plan_root,
    da_commitment,
    config_hash
}
```

The state transition is:

```text
R_seq(
    old_posq_state,
    new_posq_state,
    ordered_item_root,
    execution_stream_root,
    ordered_item_count,
    priority_commitment,
    oracle_snapshot_root,
    protocol_event_root,
    opening_root,
    receipt_vector_root,
    C_bind
)
```

### 11.2 Public inputs

```text
SequencePublicV3 {
    domain_hash,
    verifier_id,
    epoch,
    old_global_cursor,
    new_global_cursor,
    old_transcript_root,
    new_transcript_root,
    old_namespace_count,
    new_namespace_count,
    frame_plan_root,
    ordered_item_root,
    execution_stream_root,
    ordered_item_count,
    receipt_vector_root,
    opening_root,
    priority_start,
    priority_end,
    priority_root,
    oracle_snapshot_root,
    protocol_event_root,
    l1_origin_hash,
    da_commitment,
    policy_hash,
    decryption_module_id,
    C_bind
}
```

### 11.3 Private witness

The witness contains:

- the relevant receipt, tick-record, log, and segment data;
- fixed envelopes and their DA membership proofs;
- time-lock openings or aggregate opening proofs;
- frame plans and their pre-opening commitments;
- namespace membership and projection paths;
- recovered cleartext bytes;
- canonical parse results;
- priority and oracle commitment witnesses;
- persistent ticket/nullifier updates.

### 11.4 Required checks

`π_seq` proves all of the following:

1. The old state equals the previously accepted application state.
2. Every tick and transcript transition follows the pinned PoSq verifier.
3. Receipt signatures, epoch, tick, position, envelope hash, and digest-chain links are valid.
4. Each batch leaf contains the exact canonical receipt reference.
5. Positions and list lengths are unambiguous; Merkle roots bind leaf count.
6. No envelope hash, ticket nullifier, or receipt position is duplicated across spans.
7. DA for every receipted envelope was committed before receipt finality.
8. Frame boundaries follow the fixed pre-decryption rule.
9. `FramePlanV3` was committed before any opening for the frame.
10. Every Lighter namespace position in the consumed range resolves exactly once.
11. Every accepted opening passes the pinned decryption verifier.
12. `BAD_AEAD` and `BAD_ENCODING` satisfy their objective predicates.
13. Availability or solver failure is not converted into a terminal item.
14. The Lighter namespace projection preserves lexicographic `(tick, position)` order.
15. `DerivedItemV3`, `ExecutionItemV3`, both stream roots, `receipt_vector_root`, and `opening_root` are correctly computed, with exactly one compact item for each rich item.
16. Priority, oracle, protocol-event, L1-origin, and policy commitments match the frame plan.
17. The new cursor and persistent roots are the unique transition outputs.
18. `C_bind` is computed from the exact public transition data in §13.

The current repository `ReplayProver` and `PoSqHost.submitAnchor` do not satisfy this relation. The existing transcript predicate omits receipt verification and chain equality checks, resets some duplicate state across spans, and does not validate openings or gap reasons. Production requires a new normative verifier and differential vectors against the Rust implementation.

### 11.5 Proof backend

The relation is normative; the backend is versioned.

The reference path is a recursively aggregatable proof over the same field family used by Lighter’s pinned prover fork, followed by an Ethereum-verifiable wrapper. The implementation may instead use a zkVM/STARK plus wrapper if benchmarks are better. Lighter does not import RSA or TLP verification into each matching circuit. The sequence proof amortizes transcript and delay verification over a complete settlement span.

Only a `ZK_FINALIZED` sequence state may authorize production Lighter settlement. The repository’s optimistic transcript backend remains useful for shadow mode and fault drills, not for a protected batch.

---

## 12. Ethereum priority and force inclusion

### 12.1 One settlement-enforced inbox

Lighter already maintains an Ethereum priority queue and an Escape Hatch. Current public interfaces include withdrawal, cancel-all, pool-share exit, key change, and related safety operations; the architecture also describes reduce-only exits. These operations are ordered by the L1 queue and must be processed before their deadline or the protocol enters Desert/Escape mode.

V3 uses this as the single settlement-enforced force path. It does not create a second independent PoSq market-operation queue.

The deterministic merge for each frame is:

```text
1. frame pre-execution under the committed oracle/protocol roots
2. all eligible L1 priority operations, in priority-request ID order
3. the Continuum protected user items, in receipted order
4. deterministic post-execution and instruction-stack completion
```

The proof and contract advance one authoritative priority cursor.

### 12.2 Scope of force operations

Initial production scope retains risk-reducing and asset-safety operations:

- cancel-all;
- supported reduce-only IOC close;
- secure withdrawal/full exit;
- public-pool exit;
- emergency API-key change where already supported.

Arbitrary delayed maker or leveraged taker orders are not force-included in V3. An L1-delayed trading order is frequently stale and creates a public front-running surface. Pre-receipt censorship of ordinary market flow remains detectable through relayers and probes but is not converted into a guaranteed original market position.

### 12.3 Receipt challenge

Post-receipt omission is objectively accountable.

Each frame publishes a length-bound `receipt_vector_root` over:

```text
(domain_hash, cursor, envelope_hash, receipt_digest, receipt_signature_hash)
```

The user receives a Merkle path after frame closure. If a signed receipt names a cursor already behind the settled head but does not match the finalized vector root, the user submits it to the host contract with a challenge bond. The operator must provide the matching inclusion proof within the response window. Failure or mismatch proves equivocation/omission, slashes the Continuum bond, and moves protected settlement to `SEQUENCE_STALLED`.

A receipt ahead of the settled head may be promoted after an SLA timeout as evidence of a stalled sequence. It cannot regain its original market position through L1; the safe response is to pause protected settlement and preserve cancel/exit rights.

### 12.4 Pre-receipt censorship

A single client cannot prove that an unacknowledged packet reached the ingress boundary. The production service uses:

- several independently operated relayers;
- signed rejection/full-window outcomes;
- deterministic capacity commitments;
- observer probes through the same paths;
- public regional latency and miss-rate telemetry.

These make censorship measurable. They do not turn network delivery into a cryptographic fact.

---

## 13. Atomic proof join

### 13.1 Binding commitment

Both proofs compute:

```text
C_bind = H_L(
    "LIGHTER_CONTINUUM_BATCH_V3",
    domain_hash,
    epoch,
    old_global_cursor,
    new_global_cursor,
    old_transcript_root,
    new_transcript_root,
    frame_plan_root,
    ordered_item_root,
    execution_stream_root,
    ordered_item_count,
    priority_start,
    priority_end,
    priority_root,
    oracle_snapshot_root,
    protocol_event_root,
    l1_origin_hash,
    policy_hash,
    decryption_module_id
)
```

The sequence proof derives both roots from the validity-proven Continuum
transcript. Lighter derives `execution_stream_root` and the count from the
logical inputs it actually executes, carries the rich `ordered_item_root` as a
public binding value, and recomputes `C_bind`. The settlement join requires
equality of both roots, count, and `C_bind`; Lighter cannot substitute the rich
root without breaking the sequence proof or join.

### 13.2 Lighter circuit delta

Lighter adds:

1. the compact field-native `ExecutionItemV3` accumulator, advanced once per logical input;
2. terminal no-state-change handling for `BAD_AEAD` and `BAD_ENCODING`;
3. explicit public roots for priority, oracle, and protocol-event inputs;
4. `C_bind` in block, segment, batch, and wrapper aggregation;
5. exact continuity checks for the prior and new Continuum cursor.

Internal matching cycles do not advance the accumulator. A taker that consumes ten makers remains one logical sequenced transaction followed by deterministic instruction-stack work.

The V3.1 preimage removes the avoidable second leaf hash and byte decomposition,
so one Poseidon2 hash is the design minimum per logical item. The measured
prover regression is still unknown: the accumulator also affects aggregation
and wrapper connections. Constraint count, proof latency, memory, and recursion
depth must be benchmarked against Lighter’s exact audited prover fork.

### 13.3 Blob integration

The audited Lighter blob is 126,976 bytes. Its first 34 bytes are currently constrained to zero:

```text
bytes [0..1]   version
bytes [2..33]  reserved
```

V3 uses this existing surface:

```text
version        = CONTINUUM_BINDING_V3
reserved[32]   = canonical_bytes32(C_bind)
```

The wrapper circuit binds the word to the execution proof’s `C_bind`. The sequence proof exposes the same canonical word. The Lighter batch commitment already binds the blob’s KZG commitment and execution public data; the version bump makes the ordering binding part of that proved data without increasing blob size.

This follows the EIP-4844 pattern in which a ZK rollup uses its own internal commitment plus the blob commitment and proves equivalence between them. The blob remains inaccessible to EVM execution except through its commitment.

### 13.4 Contract flow

The current `commitBatch`, `verifyBatch`, and `executeBatches` structure becomes:

```solidity
commitBatchV3(batchCommitment, blobVersionedHash, cBind)

verifyBatchV3(
    batchId,
    executionProof,
    sequenceProofOrCertificate,
    ExecutionPublicV3 execPublic,
    SequencePublicV3 seqPublic
)

executeBatches(...)
```

`verifyBatchV3` requires:

```text
mode == PROTECTED
seqPublic.domain_hash == pinned_domain
execPublic.domain_hash == pinned_domain
seqPublic.old_global_cursor == stored_continuum_cursor
seqPublic.old_transcript_root == stored_transcript_root
seqPublic.priority_start == stored_priority_head
seqPublic.C_bind == execPublic.C_bind
seqPublic.C_bind == committed_blob_header.C_bind
sequence proof verifier == pinned sequence verifier
execution proof verifier == pinned Lighter verifier
epoch and verifier configuration do not change inside the batch
```

Only after both proofs verify does the contract mark the batch verified and advance:

```text
Lighter state root
Continuum global cursor
Continuum transcript root
namespace item count
priority head
last C_bind
```

The transition is atomic. A valid execution proof cannot settle without sequence validity. A valid sequence proof cannot change Lighter state without execution validity.

### 13.5 Contract state

```solidity
struct ContinuumBindingStateV3 {
    bytes32 domainHash;
    bytes32 sequenceVerifierId;
    bytes32 executionVerifierId;
    bytes32 decryptionModuleId;
    bytes32 policyHash;
    uint64  epoch;
    Cursor  continuumCursor;
    uint64  namespaceItemCount;
    bytes32 transcriptRoot;
    bytes32 lastCBind;
    uint64  priorityHead;
    Mode    mode;
}
```

The contract never performs a linear scan for an anchor. Proof certificates are addressed by hash and must extend the stored state exactly.

### 13.6 Recursive aggregation

After production measurements, Lighter may recursively verify `π_seq` inside its final aggregation and submit one wrapped proof to Ethereum. The public relation and `C_bind` remain unchanged.

Recursion is adopted only if it reduces total cost without:

- serializing sequence and execution proving;
- preventing independent verifier upgrades;
- increasing settlement latency beyond the SLA;
- importing an unpinned upstream proof library;
- obscuring which verifier configuration authorized a batch.

---

## 14. PoSq host and core changes

The current `PoSqHost.sol` is a prototype. V3 requires a stateful validity host.

### 14.1 Finality modes

```text
UNVERIFIED
OPTIMISTIC
ZK_FINALIZED
```

Shadow-mode roots may be `OPTIMISTIC`. Lighter production settlement accepts only `ZK_FINALIZED` sequence transitions.

### 14.2 Required host properties

The V3 host must:

- verify or register the exact sequence proof and verifier ID;
- enforce old-to-new transcript and cursor continuity;
- bind proof certificates to domain, epoch, namespace, DA, openings, and configuration;
- store receipt-vector roots and response deadlines;
- tie fraud evidence to one accepted span;
- enforce bond floor at admission and checkpoint time;
- use canonical low-`s` ECDSA where ECDSA remains;
- use identical Rust, circuit, and Solidity challenge derivation;
- persist ticket/nullifier and capacity commitments;
- reject duplicate-last Merkle ambiguity by binding leaf count;
- pause protected settlement on proven receipt equivocation.

The v1 branch discovered and fixed a Rust/Solidity mismatch in Wesolowski challenge derivation. That fix and all other cross-language cryptography must be represented by permanent differential vectors.

### 14.3 Durable consumer APIs

Live broadcasts are insufficient for a settlement-critical adapter. Continuum adds:

```text
GetTapeRange(start_cursor, end_cursor)
GetTickRange(first_tick, last_tick)
GetFramePlan(frame_id)
GetReceiptVector(frame_id)
GetOpeningRange(start_cursor, end_cursor)
SubscribeFrom(cursor)
GetSequenceCertificate(certificate_hash)
```

Every API response has a canonical hash and can be independently reconstructed from DA.

---

## 15. Data availability

### 15.1 Preserve Lighter’s hybrid DA

Lighter publishes compressed account deltas and market data in Ethereum blobs, sufficient for its public account-state reconstruction and Escape Hatch. It does not publish the complete high-frequency transaction and order-book state.

V3 preserves that model. The Lighter blob adds only the 32-byte `C_bind` word in its existing reserved header.

### 15.2 Continuum DA requirements

Continuum DA retains:

- fixed envelopes;
- admission receipts and receipt vectors;
- tick records, segment data, and frame plans;
- opening witnesses and aggregate opening proofs;
- sequence proof inputs required for independent reproduction;
- namespace projection paths;
- sequence proof certificates.

DA-before-receipt is mandatory: a receipt reaches final status only after the envelope is retrievable from the configured DA quorum and its commitment appears in the signed tick record.

The full 1,024-byte-or-larger envelope stream is not copied into Ethereum blobs. At 20,000 envelopes/s, 1,024-byte envelopes alone produce 20.48 MB/s before erasure coding and proofs. This requires a dedicated high-throughput DA network with replicated archival nodes and a retrieval SLA.

### 15.3 Withholding behavior

If transcript data is withheld, `π_seq` cannot be produced. Lighter may continue to expose unfinalized soft state, but Ethereum does not advance. Safety holds; liveness moves through the states in §17.

Users retain receipts and receipt paths locally. Watchers mirror the receipt-vector roots and sequence certificates. Retention must exceed the receipt challenge window, Lighter proof delay, Ethereum reorganization margin, and recovery period.

---

## 16. Decryption throughput and required Continuum improvements

### 16.1 Current implementation limit

The current repository uses one fixed-base solve-only time-lock puzzle per ciphertext. It aggregates verification proofs, not the sequential work. At arrival rate `λ`, delay work `T`, solver redundancy `r`, and operating headroom `h`:

```text
required aggregate squaring rate = λ × T × r × h
live puzzle lanes                = λ × delay_seconds × r × h
```

At 10,000 envelopes/s and `T = 2.5 million` squarings, one solver copy requires 25 billion squarings/s. At 20,000 envelopes/s, two replicas, and 1.5× headroom, the requirement is 150 billion squarings/s. Proof aggregation does not reduce it.

The v1 branch’s admission capacity of 640,000 envelopes/s therefore says little about end-to-end capacity. Decryption is the limiting path.

### 16.2 Production profiles

V3 defines the module interface now and separates launch profiles:

1. **Per-item TLP profile:** permitted only for capped beta volume below a measured adversarial solver ceiling.
2. **Transparent batch-wave profile:** required for unrestricted Lighter-wide deployment once a concrete construction, implementation, security proof, and hardware benchmark are selected.
3. **Threshold-decryption profile:** optional weaker mode if Lighter accepts committee collusion and availability assumptions. It uses a distinct `decryption_module_id` and product label.

The target full-scale module performs one delay-dominant solve per maturity wave, not one per transaction. This document does not pretend that the current `batch_solve` implementation already has that property.

### 16.3 Solve timeline

For each wave:

```text
t_admit       receipt issued; DA confirmed; solve begins
t_close       frame and order fixed
t_mature      opening may become canonical
t_execute     fixed protected-delay frame; cleartext must be available
```

Required inequalities:

```text
t_close < t_mature
t_mature + open_verify_p99 + ingest_margin <= t_execute
```

The no-look delay is derived from the full exposure window and adversarial hardware advantage. The current code formula omits terms identified in the repository’s formal-verification plan. Production uses the paper-faithful bound and refuses to start if configured parameters fail it.

### 16.4 Launch gate

Unrestricted protected mainnet launch is blocked until tests demonstrate, under adversarial withholding:

- peak and sustained opening throughput above Lighter’s target load;
- two independent solver implementations;
- p99 opening within the protected-delay budget;
- proof generation within the settlement SLA;
- hardware-advantage margin with documented benchmark methodology;
- no optional reveal dependency on the Continuum or Lighter sequencer.

---

## 17. Operating modes and recovery

```text
PROTECTED
    │ sequence proof or DA stalls
    ▼
SEQUENCE_STALLED
    │ timeout / safety action
    ▼
PRIORITY_ONLY
    │ Lighter priority deadline missed
    ▼
DESERT / ESCAPE_HATCH

SEQUENCE_STALLED or PRIORITY_ONLY
    │ clean proof from last finalized head + new epoch activation
    ▼
RECOVERY_PENDING
    │ verifier/config checks and activation delay complete
    ▼
PROTECTED
```

### 17.1 Protected

Normal user admission, execution, sequence proving, execution proving, and Ethereum settlement are live.

### 17.2 Sequence stalled

New receipts stop. Lighter may finish proving a prefix already fully opened. No batch beyond the last proved Continuum cursor can settle. Users may cancel or reduce risk through the priority lane where supported.

### 17.3 Priority only

Normal trading is disabled. Lighter processes settlement-enforced safety operations in priority-request order. Assets remain under Lighter’s existing custody and proof rules.

### 17.4 Desert / Escape Hatch

Lighter’s existing trigger freezes the last verified state and permits independent exits from Ethereum-posted public account data. Continuum adds no new custody or withdrawal dependency.

### 17.5 Recovery

Recovery starts from the last jointly verified Lighter state, Continuum cursor, transcript root, and priority head. A new epoch proves continuity or explicitly records a terminal abandoned suffix. Governance cannot reinterpret or silently skip a receipted finalized position.

### 17.6 No automatic unprotected downgrade

The v1 proposal degraded to “declared FCFS” after ordering fraud. V3 rejects that behavior. A faulting sequencer would benefit from forcing the protection downgrade.

Unprotected trading, if Lighter ever offers it, is a separate deployment or user-selected market state with a new domain, new epoch, explicit UI, and no Continuum claim. It cannot share a mutable protected book during the same epoch.

---

## 18. Upgrade and governance safety

Lighter’s contracts and verifier stack are upgradeable. Published audits identify privileged validator, governor, security-council, and upgrade-gatekeeper roles. V3 treats these as part of the trust surface.

Protected status requires:

- verifier and implementation hashes pinned in `DeploymentDomainV3`;
- a nonzero public activation delay for normal upgrades;
- emergency authority limited to pausing, priority-only mode, and asset safety;
- verifier changes activated only at epoch boundaries;
- old and new verifier sets jointly committing to the transition checkpoint;
- no batch spanning an implementation, policy, encoding, or decryption change;
- two independent audits for changes to either proof relation;
- permanent public test vectors and reproducible verifier builds.

If governance can bypass the advertised notice period and install arbitrary logic, the protected guarantee remains conditional on that governance. The UI and risk documentation must say so directly.

---

## 19. End-to-end security properties

Let `B` be an Ethereum-finalized protected Lighter batch. Under the assumptions in §4.4:

### 19.1 Exact consumption

Every logical protected item consumed by `B` corresponds to exactly one Lighter namespace position in the proved Continuum range. No additional protected item can affect the state transition.

### 19.2 Order preservation

If two protected Lighter items `a` and `b` have receipted positions `p_a < p_b`, and both are in the consumed range, Lighter processes `a` before `b`.

### 19.3 No silent omission

Every Lighter namespace position in the consumed range contributes either an executable logical transaction or an objectively proven terminal invalid item. Availability failure prevents finalization.

### 19.4 Pre-content frame commitment

The frame boundary, priority range, oracle root, protocol-event root, and execution eligibility delay are committed before any protected payload in the frame becomes usable by the sequencer.

### 19.5 Execution inheritance

Because Lighter’s proof executes the exact derived stream, its existing price-time-priority, risk, liquidation, and state-transition guarantees apply to the Continuum-fixed order.

### 19.6 Receipt accountability

A valid signed receipt inconsistent with the finalized receipt vector yields objective slash evidence and pauses later protected settlement.

### 19.7 Liveness containment

A Continuum or solver failure can stop new protected settlement. It cannot authorize a conflicting Lighter state or block Lighter’s existing L1 safety and escape mechanisms.

---

## 20. Performance budget and targets

Lighter advertises tens of thousands of operations per second and millisecond engine latency. The integration must preserve that execution profile while adding a proof and opening pipeline outside the matching hot path.

Production targets are measured, not assumed:

| Metric | Capped beta gate | Full production gate |
|---|---:|---:|
| Sustained protected ingress | target workload + 2× headroom | Lighter peak + 2× headroom |
| Receipt p99 | ≤ regional network budget + sub-ms service budget | same under target load |
| Frame closure jitter | zero content-dependent cuts | deterministic replay equality |
| Opening p99 | within protected-delay budget | within budget under adversarial withholding |
| Sequence proof latency | below Lighter settlement interval | below interval at peak load |
| Execution-proof overhead | measured versus unmodified prover | agreed maximum regression |
| Ethereum verification | one sequence + one execution verification per settlement batch | recursion optional after cost study |
| DA retrieval p99 | sufficient for independent proof generation | multi-provider SLA |

No per-segment proof is verified directly on Ethereum at a 25.6 ms cadence. The v1 demo measured a single native Wesolowski verification, which demonstrates feasibility of the primitive, not economic feasibility at segment frequency. Sequence proofs recursively aggregate many ticks and segments into one Lighter settlement certificate.

---

## 21. Implementation plan

### Phase 0 — Freeze the interface

- Obtain Lighter’s exact current transaction codecs, block/batch public-input layout, prover fork, and verifier hashes.
- Serialize every current Lighter transaction and choose envelope size classes.
- Publish `DeploymentDomainV3`, `DerivedItemV3`, `ExecutionItemV3`, both stream roots, `SequencePublicV3`, and `C_bind` vectors.
- Replace `bincode` and length-ambiguous Merkle trees in the consensus path.
- Write the complete transaction failure-semantics table.

**Exit:** Rust, Go, Python, circuit, and Solidity implementations produce identical vectors.

### Phase 1 — Shadow feed

- Run the protected SDK, Continuum receipts, fixed frames, permissionless opening, and deterministic adapter.
- Execute against a Lighter state replica without affecting production settlement.
- Compare native Lighter order with Continuum-derived order and measure latency, rejection, and solver load.

**Label:** observable pilot, no validity-enforced fairness claim.

### Phase 2 — Sequence proof

- Implement the normative PoSq transition verifier.
- Add durable tape/frame/opening APIs and persistent nullifier state.
- Build `SequenceTransitionProof` and `ZK_FINALIZED` host state.
- Prove typed resolution and receipt vectors.
- Adversarially test false gaps, orphan receipts, cross-span duplicates, malformed openings, and DA withholding.

**Exit:** independent replay and proof verification from the last accepted state.

### Phase 3 — Lighter circuit and blob binding

- Add the logical-input accumulator and typed terminal-invalid cycle.
- Bind priority, oracle, protocol-event, and frame roots.
- Enable the reserved blob-header word under a version bump.
- Add `C_bind` through every recursive aggregation layer.
- Implement `commitBatchV3` and `verifyBatchV3` on testnet.

**Exit:** any mutation, deletion, insertion, duplicate, or reordering between proofs causes settlement rejection.

### Phase 4 — Capped protected mainnet

- Activate one domain and one protected policy with hard notional and throughput caps.
- Use the measured per-item TLP profile only below its adversarial solver ceiling.
- Fund the receipt-accountability bond above the maximum permitted value at risk per settlement frame.
- Drill `SEQUENCE_STALLED`, `PRIORITY_ONLY`, receipt challenge, and Escape Hatch.

**Exit:** sustained operation through the full proof and Ethereum finality path.

### Phase 5 — Full-scale deployment

- Activate only after a transparent batch-wave opening module or equivalent capacity passes the launch gate in §16.4.
- Remove capped beta throughput limits gradually.
- Evaluate recursive proof aggregation only after measuring parallel two-proof production.

---

## 22. Verification matrix

### 22.1 Encoding and cryptography

- [ ] Canonical bytes for every object and transaction type.
- [ ] Domain separation binds chain, contracts, implementation, verifier, epoch, namespace, policy, and decryption module.
- [ ] Merkle roots bind list length.
- [ ] ECDSA uses canonical low-`s`; Rust/Solidity/circuit signature behavior matches.
- [ ] VDF/TLP challenge derivation and primality checks are identical across implementations.
- [ ] Hash-to-field and `C_bind` serialization vectors cover boundary values.

### 22.2 Ordering

- [ ] Receipt position is allocated atomically with ticket consumption.
- [ ] Independent adapters replay byte-identical frame plans and streams.
- [ ] Frame cuts remain identical under different plaintexts and execution costs.
- [ ] Every Lighter namespace position in a range appears exactly once.
- [ ] Cross-span duplicate and omitted receipt tests fail the proof.
- [ ] Batch submission creates separate receipts unless the Lighter transaction is protocol-atomic.

### 22.3 Opening

- [ ] Solve begins at receipt.
- [ ] Order and frame close before maturity.
- [ ] No unavailable opening can become a finalizable no-op.
- [ ] Bad AEAD and bad encoding have unique objective predicates.
- [ ] Adversarial withholding load fits the measured solver cap.
- [ ] Two independent solver implementations agree on every vector.

### 22.4 Lighter execution

- [ ] Every current transaction type is covered.
- [ ] Signature, nonce, expiry, and stateful failure semantics match current protocol behavior or an explicitly versioned migration.
- [ ] Logical transaction accumulator advances exactly once despite multi-cycle matching.
- [ ] Rich and compact stream roots have a proved one-to-one mapping and identical declared count.
- [ ] Priority operations advance in Ethereum request-ID order.
- [ ] Discretionary protocol-created transaction paths are removed or sequenced.
- [ ] Oracle and frame roots are committed before opening.

### 22.5 Proof join and settlement

- [ ] `π_seq` alone cannot change Lighter state.
- [ ] `π_exec` alone cannot settle a protected batch.
- [ ] Both proofs agree on `execution_stream_root`, item count, and `C_bind`; the sequence proof additionally binds `ordered_item_root` into `C_bind`.
- [ ] Cursor, transcript root, priority head, and state root advance atomically.
- [ ] Overlapping, skipping, or cross-epoch certificates are rejected.
- [ ] Only `ZK_FINALIZED` sequence state is accepted.

### 22.6 Faults and recovery

- [ ] Orphan receipt challenge slashes and pauses.
- [ ] DA withholding stalls without corrupting state.
- [ ] Continuum outage preserves priority cancel/exit.
- [ ] No automatic unprotected downgrade exists.
- [ ] Recovery begins at the last jointly verified head.
- [ ] Escape Hatch works from the last finalized Lighter root with Continuum fully offline.

---

## 23. Production blockers in the current repository

The following are deployment-blocking:

1. The current transcript predicate does not fully verify receipt signatures, receipt-chain equality, one-receipt-per-entry, cross-span duplicates, openings, or terminal reasons.
2. `PoSqHost.submitAnchor` does not make segment or transcript verification a state-transition condition.
3. The host lacks strict anchor continuity and a proof-backed `ZK_FINALIZED` mode.
4. Duplicate-last Merkle roots do not bind leaf count.
5. `bincode` is used where a permanent multi-language consensus encoding is required.
6. Ticket, capacity, and duplicate state are not sufficiently durable across restart.
7. Cadence fencing does not consistently materialize position-preserving terminal entries.
8. Per-ciphertext solving is linear in flow; current batch code aggregates proofs, not work.
9. Solving starts too late relative to maturity and grace.
10. Current local DA is not a production availability layer.
11. The v1 bridge is demo-only and does not bind to a Lighter execution proof or state root.
12. Lighter transaction-size coverage, exact transaction commitment, and failure semantics remain undisclosed or unmeasured.
13. Lighter’s current verifier and upgrade-delay guarantees must be pinned from the live deployment.

---

## 24. Changes from `lighter-integration-v1`

V3 keeps:

- the observation that Lighter’s order nonce makes input-stream binding the correct integration point;
- encrypted, fixed-size Lighter transactions inside the Continuum tape;
- deterministic frame replay;
- separation of state-independent terminal invalidity from Lighter state-dependent rejection;
- Lighter’s existing matching and execution circuits as the execution authority;
- Ethereum as the joint settlement surface;
- the L1 priority queue and Escape Hatch as the asset-safety backstop.

V3 changes:

1. The B3 public stream hash is replaced by a real `SequenceTransitionProof`.
2. Optimistic stream challenges are removed from the production path.
3. The sequence and execution proofs are joined atomically on `C_bind`.
4. The existing 32-byte reserved Lighter blob word carries `C_bind` under a version bump.
5. The derivation commits user, priority, oracle, frame, and protocol-event inputs.
6. Frame boundaries are fixed before reveal.
7. Lighter’s differential speed bumps are replaced by one uniform protected delay.
8. The mandatory outer L1-wallet signature is removed.
9. Consensus encoding is versioned and test-vector-defined; `bincode` is removed.
10. Availability or solver failure stalls finality instead of becoming an omittable gap.
11. Lighter’s existing priority queue becomes the single settlement-enforced force path.
12. The system fails to priority-only mode, not unprotected trading.
13. Per-item TLP is capped; full scale requires a real batch-wave opening module or an explicitly weaker threshold profile.
14. PoSqHost gains a proof-backed `ZK_FINALIZED` state and exact continuity.
15. Receipt-vector challenges cover orphan receipts outside the canonical proof.

---

## 25. Required disclosures from Lighter

Implementation cannot proceed beyond shadow mode without:

1. the exact current logical L2 transaction codecs and maximum sizes;
2. transaction-by-transaction signature, nonce, rejection, and retry semantics;
3. the current transaction/public-input commitment structure;
4. the exact Plonky2 fork, field/hash parameters, recursion layout, and verification keys;
5. the final wrapper and Ethereum public-input serialization;
6. the current blob serializer and versioning process;
7. priority-operation types, deadlines, and circuit merge rules;
8. oracle, block-time, pre-execution, liquidation, trigger, and TWAP scheduling rules;
9. live proxy implementation, governance roles, and effective upgrade notice;
10. proof-generation and settlement latency distributions under production load.

Until these are available, constraint counts and latency effects are engineering estimates, not deployable parameters.

---

## 26. Sources

### Lighter primary sources

- [Technical Architecture: Lighter Core](https://docs.lighter.xyz/about-lighter/technical-architecture-lighter-core)
- [Lighter Protocol whitepaper, October 2025](https://assets.lighter.xyz/whitepaper.pdf)
- [Order Types & Matching](https://docs.lighter.xyz/trading/order-types-and-matching)
- [Trading Fees and latency tiers](https://docs.lighter.xyz/trading/trading-fees)
- [API account types](https://apidocs.lighter.xyz/docs/account-types)
- [API keys and nonce behavior](https://apidocs.lighter.xyz/docs/api-keys)
- [Signing transactions](https://apidocs.lighter.xyz/docs/trading)
- [Priority transactions](https://apidocs.lighter.xyz/docs/priority-transactions)
- [Official Lighter Go signing implementation](https://github.com/elliottech/lighter-go/blob/main/signer/key_manager.go)
- [Security audit index](https://docs.lighter.xyz/security/security-audits)
- [Nethermind LighterCore contract audit](https://1186887628-files.gitbook.io/~/files/v0/b/gitbook-x-prod.appspot.com/o/spaces%2FXuISSHTfjHCg60BNss6v%2Fuploads%2FpbieGJkDU9ReEfZC3bUC%2F22-09-2025_Nethermind_LighterCore.pdf?alt=media)
- [zkSecurity block-circuit audit](https://1186887628-files.gitbook.io/~/files/v0/b/gitbook-x-prod.appspot.com/o/spaces%2FXuISSHTfjHCg60BNss6v%2Fuploads%2Fx29oIB5HInlZotXSFFdK%2F08-04-2025_Block_audit.pdf?alt=media)
- [zkSecurity wrapper-circuit audit](https://1186887628-files.gitbook.io/~/files/v0/b/gitbook-x-prod.appspot.com/o/spaces%2FXuISSHTfjHCg60BNss6v%2Fuploads%2FP5Vou5dAIWFzkyW9TL31%2F10-10-2025_Wrapper_audit.pdf?alt=media)

### Continuum repository sources

- [`lighter-integration-v1` branch](https://github.com/cryptohariseldon/continuum-monorepo/tree/lighter-integration-v1)
- [`integrations-v2/lighter-integration.md` on v1](https://github.com/cryptohariseldon/continuum-monorepo/blob/lighter-integration-v1/integrations-v2/lighter-integration.md)
- [`docs3/07-integration-roadmaps.md`](https://github.com/cryptohariseldon/continuum-monorepo/blob/main/docs3/07-integration-roadmaps.md)
- [`crates/sequencer/src/records.rs`](https://github.com/cryptohariseldon/continuum-monorepo/blob/main/crates/sequencer/src/records.rs)
- [`crates/sequencer/src/transcript.rs`](https://github.com/cryptohariseldon/continuum-monorepo/blob/main/crates/sequencer/src/transcript.rs)
- [`crates/sequencer/src/admission.rs`](https://github.com/cryptohariseldon/continuum-monorepo/blob/main/crates/sequencer/src/admission.rs)
- [`crates/vdf/src/tlk.rs`](https://github.com/cryptohariseldon/continuum-monorepo/blob/main/crates/vdf/src/tlk.rs)
- [`contracts/PoSqHost.sol`](https://github.com/cryptohariseldon/continuum-monorepo/blob/main/contracts/PoSqHost.sol)

### Cryptographic and Ethereum references

- Boneh, Bonneau, Bünz, and Fisch, [Verifiable Delay Functions](https://eprint.iacr.org/2018/601)
- Wesolowski, [Efficient Verifiable Delay Functions](https://eprint.iacr.org/2018/623)
- [EIP-4844: Shard Blob Transactions](https://eips.ethereum.org/EIPS/eip-4844)

---

## 27. Final specification statement

A protected Lighter state transition is valid only if:

```text
1. Continuum validity-proves the canonical encrypted admission transcript;
2. the sequence proof derives one exact ordered Lighter input stream;
3. Lighter validity-proves execution over that exact stream;
4. both proofs and the Ethereum blob expose the same C_bind;
5. Ethereum advances the Lighter state and Continuum cursor atomically.
```

This turns Lighter’s remaining sequencer discretion into a validity-proven input and completes the end-to-end settlement statement.
