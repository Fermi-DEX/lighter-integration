# Continuum × Lighter Integration v3.1

*A proof-carrying sequencer feed for validity-proven order and execution*

## How the system works

Continuum is an ordering layer that accepts encrypted transactions, signs their positions, and records them in a verifiable transcript.

For a plain-language example, read [Fair ordering basics](./fair-ordering-basics.md).

Lighter proves that its state transition follows a chosen transaction order. That proof does not prove that the sequencer chose the order fairly.

A sequencer that sees transaction contents can favor selected orders. It can add, omit, or reorder protected transactions before it proves valid execution.

Continuum adds a proof-carrying admission and ordering layer. Fair ordering means transaction contents cannot influence order after Continuum issues a receipt.

The protected flow has seven steps:

1. The user signs a normal Lighter transaction and encrypts it inside a fixed-size envelope.
2. Continuum assigns an ordered position and returns a signed receipt before it can read the transaction.
3. Time-lock work delays opening until the protocol fixes the order and frame inputs.
4. A public solver opens the envelope after maturity.
5. The sequence proof derives the exact ordered Lighter input stream from the receipted transcript.
6. Lighter proves correct execution of that same stream.
7. Ethereum verifies both proofs and their shared `C_bind`, then advances both states atomically.

`C_bind` commits the ordered roots, cursors, frame inputs, policy, and decryption module. Equality joins ordering validity with execution validity.

Atomic settlement updates both states after both proofs verify and expose equal commitments. Failed verification leaves both states unchanged.

This design keeps Lighter as the execution authority. Continuum proves which protected transaction entered first and whether Lighter consumed the complete ordered stream.

Section 4 defines the exact guarantee, assumptions, and scope limits.

### Key terms

| Term | Meaning |
|---|---|
| Transcript | The ordered record of admitted envelopes, receipts, openings, and state continuity |
| PoSq | Continuum's sequential-work mechanism for verifiable transcript progress |
| Data availability (DA) | Storage and retrieval of the data required to reconstruct and prove a transition |
| Time-lock puzzle (TLP) | An encryption puzzle that requires sequential work before public opening |
| Frame | A deterministic group of transcript positions with fixed execution inputs |
| Cursor | The last transcript position that settlement consumed |
| Maturity | The earliest protocol point at which an opening can become canonical |
| First-in, first-out (FIFO) | Processing in the same order that admission fixed |
| First-come, first-served (FCFS) | Eligibility based on an observable arrival rule |
| AEAD | Authenticated encryption that protects secrecy and detects changed ciphertext |
| Layer 1 (L1) | Ethereum settlement and custody contracts |
| Layer 2 (L2) | Lighter execution above Ethereum settlement |

---

## 1. Protocol design

Lighter already proves the correct execution of a chosen transaction stream. It proves signatures, nonce transitions, risk verification, price-time-priority matching, liquidations, state transitions, and recursive aggregation. It also proves the correspondence between execution public data and Ethereum blob data. The sequencer still chooses the transaction stream.

Continuum must supply that stream as a **proof-carrying sequencer feed**.

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

The proofs remain independent, and the provers generate them in parallel. Ethereum joins them by comparing one circuit-native commitment. Recursive aggregation can combine them to reduce gas costs. Recursion does not form the security boundary.

Lighter’s documentation says the sequencer coordinates first-in, first-out (FIFO) ordering. It also says transaction order and oracle data determine execution. The whitepaper identifies sequencer ordering power and residual millisecond-scale reordering as the remaining fairness surface. The execution proof establishes price-time priority *after* an order enters Lighter’s state. Continuum establishes which signed transaction enters first.

The integration must fail closed. Lighter can expose a batch without a valid sequence proof as provisional soft state. This batch cannot advance the Ethereum-settled state. A Continuum failure moves the system to priority-only operation. The system then uses Lighter’s existing Escape Hatch as necessary. It never silently uses an unprotected sequencer.

### 1.1 The minimal kernel

The kernel has four parts:

1. Users encrypt already-signed Lighter transactions and submit fixed-size envelopes to Continuum.
2. Continuum fixes each envelope at a receipted position before plaintext is available.
3. A sequence proof derives the exact Lighter input stream from a contiguous, validity-proven transcript transition.
4. Lighter’s proof and the sequence proof commit to the same ordered-input root.

Receipts, DA, force paths, operating modes, versioning, and solver capacity make this kernel usable during faults.

---

## 2. Lighter architecture and the ordering gap

These primary sources support this specification:

- [Lighter technical architecture](https://docs.lighter.xyz/about-lighter/technical-architecture-lighter-core)
- [Lighter whitepaper from October 2025](https://assets.lighter.xyz/whitepaper.pdf)
- [Current Lighter API documentation](https://apidocs.lighter.xyz/docs/get-started)
- [Published Lighter circuit and contract audits](https://docs.lighter.xyz/security/security-audits)

### 2.1 What Lighter proves

Lighter is an application-specific Ethereum L2. Its sequencer receives and executes signed L2 transactions. It then constructs blocks and batches. Its prover proves the resulting state transition. Ethereum contracts custody assets and store the canonical state root. They also verify batch proofs, execute withdrawals, and maintain a priority-operation queue.

The audited proving stack uses Plonky2 over the Goldilocks field. It recursively aggregates transactions, blocks, segments, and batches. A wrapper prepares the final proof for Ethereum verification. Lighter’s wrapper also proves a correspondence between public account-delta data and the EIP-4844 blob commitment on Ethereum.

The execution relation covers:

- Lighter API-key signatures and per-key nonce progression
- Order placement, modification, cancellation, and other L2 operations
- Margin, health, reduce-only, slippage, and protocol risk constraints
- Deterministic instruction-stack continuation for multi-cycle operations
- Price-time-priority matching
- Liquidations and protocol accounting
- Old-to-new state-root continuity
- Priority-operation prefix consumption
- Account-delta and market-data correspondence to the posted blob

Lighter’s Order Book Tree encodes price and an internally assigned order nonce into each leaf position. At one price, the lower nonce is older. It has higher priority. The proof establishes that matching respects the order already represented in state.

### 2.2 What it does not prove

The public specification does not establish:

- A cryptographic ingress point where arrival becomes final
- A byte-level merge rule across API servers, regions, or connections
- Proof that transaction order is independent of plaintext
- Proof that an API acknowledgement binds the sequencer to a position
- Proof that every acknowledged transaction appears in the executed stream
- Proof that the system fixed batch boundaries and oracle choices before it knew order contents

Lighter’s execution proof can approve either of two valid transaction orders. Both orders produce valid state transitions. The proof establishes execution validity. It does not establish fair selection of the execution input.

### 2.3 Why stream equality is the correct reduction

For user flow, these inputs determine Lighter’s state transition:

```text
old state
ordered logical L2 transactions
priority-operation prefix
oracle and frame inputs
protocol configuration
```

Binding the complete logical input stream to Continuum’s order transfers that order to Lighter’s existing matching proof. The matching algorithm needs no change.

A public stream hash alone does not prove that a valid Continuum transcript produced the stream. The `SequenceTransitionProof` proves this derivation.

Settlement also requires equality between the sequence-proof output and execution-proof output. This equality prevents an operator from proving execution over a substitute stream.

---

## 3. Why the design uses two proofs

Each alternative changes what settlement can verify:

| Approach | Result |
|---|---|
| Signed receipts alone | Provide accountability, but Lighter can settle a different stream |
| A PoSq root beside a Lighter batch | Correlates both systems, but does not prove exact consumption |
| An optimistic stream challenge | Requires unavailable blob data or impractical on-chain TLP and mutation proofs |
| A complete PoSq verifier in each Lighter transaction circuit | Adds non-native RSA, Keccak, receipt, and TLP work to the execution hot path |
| Independent proofs joined on one commitment | Gives validity-enforced ordering and execution with parallel proving and modular upgrades |
| Sequence-proof recursion inside the final wrapper | Keeps the same public relation with tighter proof-system and upgrade coupling |

The selected design assigns one proof responsibility to each layer:

- Continuum proves ordering and deterministic derivation.
- Lighter proves execution.
- Ethereum proves that both statements refer to the same batch.

---

## 4. Security objective and scope

### 4.1 Primary property

For each Ethereum-finalized protected Lighter batch, Lighter consumes the exact Lighter namespace projection of the accepted Continuum transcript range. No insertion, deletion, duplication, or reordering occurs.

### 4.2 Supporting properties

Subject to the assumptions in §4.4:

- A Continuum receipt fixes an envelope’s position before the sequencer can use the plaintext.
- Transaction contents cannot influence admission position within the protected namespace.
- The system commits batch and frame cuts before the corresponding plaintext opens.
- Every consumed transcript position has one objectively proven resolution.
- Lighter’s execution proof handles state-dependent transaction rejection instead of an opaque prefilter.
- A signed orphan receipt is slashable.
- A stalled operator cannot settle a conflicting state transition.
- Lighter’s L1 priority queue and Escape Hatch remain independent of Continuum availability.

### 4.3 Explicit non-goals

These properties remain outside the guarantee:

- Proof of pre-receipt delivery for a censored message
- Equal network latency across users or regions
- Physical-time first-come, first-served (FCFS) order between Ethereum priority requests and off-chain ingress
- Secrecy after a user leaks its own signed transaction out of band
- Absolute wall-clock delay independent of the calibrated sequential-work assumption
- Oracle correctness beyond Lighter’s existing oracle model
- Successful execution after later state, nonce, margin, price, or slippage predicates reject a validly sequenced transaction
- Ordering fairness for an unprotected lane that shares the same mutable order book

### 4.4 Assumptions

The final property depends on:

- Soundness of Lighter’s execution proof and the new sequence proof
- Collision resistance of the pinned hash functions
- Unforgeability of Lighter API-key signatures and Continuum receipts
- Correct Ethereum settlement and Lighter custody contracts
- Sequentiality and calibrated hardware margin of the selected time-lock module
- A ceremony-derived RSA modulus with unknown factors and no retained trapdoor
- Fresh high-entropy puzzle randomness for every envelope
- Availability of the receipted Continuum data that the sequence proof needs
- At least one honest public solver for non-cooperative recovery
- Correct, version-pinned canonical encodings
- Governance that does not replace the verifier or implementation outside the declared upgrade process

Zero knowledge is optional for the sequencing statement. Succinct validity is essential. The proof can keep Lighter transaction bytes private where Lighter’s validium-like state design permits privacy.

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

- **Lighter client:** Creates the existing signed transaction and encrypts it.
- **Continuum ingress:** Verifies only the visible envelope surface and assigns the receipted order.
- **Continuum DA nodes:** Retain envelopes, receipts, tick records, and opening material.
- **Public solver network:** Starts mandatory time-lock work and publishes verified openings.
- **Sequence prover:** Proves the Continuum transition and derives Lighter’s canonical inputs.
- **Lighter sequencer:** Coordinates execution. It no longer chooses protected user order.
- **Lighter prover:** Proves the existing state transition and a small ordered-input accumulator.
- **Ethereum contracts:** Join both proofs and store the consumed Continuum cursor. They also preserve the priority and escape path.

One operator can hold several roles. The proofs and encryption boundary provide security. Organizational separation does not provide this security.

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

The domain prevents replay across Lighter deployments, forks, verifier upgrades, Continuum epochs, and policy versions. A batch cannot cross a domain change. Each verifier, implementation, encoding, policy, or decryption-module change starts a new epoch. The old and new configurations jointly commit the state checkpoint.

Production configuration pins:

- The Lighter proxy and implementation code hash
- The execution verification key
- The sequence verification key
- The PoSq genesis and transcript verifier
- The envelope and transaction encodings
- The protected scheduling policy
- The decryption module and parameters
- The RSA modulus hash, group encoding, and delay classes inside the decryption module
- All internal hash identifiers and serialization rules

---

## 7. Client and envelope protocol

### 7.1 Preserve Lighter authorization

The protected payload is the exact canonical signed Lighter transaction from Lighter’s current SDK. Lighter API keys use a circuit-friendly Schnorr construction. Each API key maintains an independent nonce. Continuum does not replace that authorization scheme.

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

The protocol requires no outer L1-wallet signature. An extra secp256k1 user signature breaks bot key isolation and subaccount operations. It also breaks smart-wallet use and unattended HFT. The one-time admission ticket pays for malformed or unauthorized submissions. Lighter alone determines whether the recovered transaction has authorization.

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

All integers use the canonical byte order in the test-vector suite. Lists include an explicit length and maximum length. Merkle roots commit to leaf count. Consensus encoding must be language-independent and versioned.

Before receipt issuance, ingress must verify the envelope magic, version, size class, domain, epoch, inclusion window, and decryption module. It must also verify zero padding and canonical, group-valid puzzle public data. The sequence proof must verify the same admission predicate.

Envelope-size selection starts with a 1,024-byte candidate and a 642-byte body. Serialization tests must cover every supported Lighter transaction before size selection. They cover grouped orders, pool actions, transfers, withdrawals, key changes, subaccount actions, and maximum signatures. The selected class is the smallest fixed power-of-two size that fits every price-affecting and order-affecting transaction type.

A separate fixed class can cover administrative transactions that do not fit one class. Every class must remain operation-indistinguishable within its declared scope. Market, side, price, size, account, operation type, and API-key identity remain encrypted. Operation type includes create, cancel, and modify.

### 7.3 Admission tickets

Tickets are prepaid, one-time bearer credentials with a nullifier. They enforce spam cost and Lighter quotas. They do not reveal account identity to ingress.

```text
TicketV3 {
    issuer_id:       bytes32,
    class_id:        u16,
    nullifier:       bytes32,
    expiry_epoch:    u64,
    issuer_sig:      bytes
}
```

Ticket issuance can reflect Lighter account tier, staking, rate limit, or commercial policy. Ticket class cannot change ordering within the protected stream. It controls admission quota and price.

The admission transition must verify an authorized issuer, issuer signature, class, expiry, unused nullifier, and policy capacity. The pinned `policy_hash` binds the issuer set, class rules, and quota limits.

Ingress atomically consumes the nullifier as it signs a receipt. A crash after receipt issuance cannot restore the ticket. Nullifier state is durable. The proved PoSq transition includes this state.

### 7.4 Protected batch submission

`sendTxBatch` is not a sequencing unit. The protected SDK splits an API batch into separately encrypted logical transactions. It returns one receipt per transaction. This rule prevents a client-selected batch from becoming an opaque internal ordering island.

Protocol-native grouped, one-cancels-the-other (OCO), or atomic orders remain one logical transaction. Lighter remains responsible for their internal semantics.

For consecutive nonces from one API key, the protected SDK waits for admission of nonce `n` before it sends `n+1`. It can also use Lighter’s documented skip-nonce semantics. It must not reorder transactions to repair a client nonce race.

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

Lighter currently returns HTTP `200` only after it accepts the API syntax. The protected route returns the actual signed Continuum admission outcome.

---

## 8. Ordering policy and frame construction

### 8.1 One protected stream

All L2 transactions that can change trading state must use the protected stream:

- Create, modify, cancel, and cancel-all operations
- Market, limit, trigger, TWAP, and grouped order operations
- Externally initiated liquidation transactions
- Leverage, margin, transfer, withdrawal, pool, or account operations that can change execution validity or risk through their placement

A direct transaction path into the same mutable book defeats the guarantee. Read-only API calls remain outside the stream. L1 priority operations use the separate safety lane in §12.

### 8.2 Uniform protected eligibility

Lighter currently applies different speed bumps by account and transaction class. Standard accounts list 200 ms maker/cancel delay and 300 ms taker delay. Premium maker/cancel flow can receive a 0 ms path. Premium takers receive 140–200 ms delay. These public product policies require a post-decryption priority scheduler. Keeping them changes the guarantee to “FCFS after policy-defined eligibility” instead of strict receipt order.

V3 selects one uniform minimum execution delay for the protected stream:

```text
eligible_frame(item) = admission_frame(item) + protected_delay_frames
```

The delay is identical for create, cancel, modify, maker, taker, account tier, and market. Fees, staking discounts, and rate limits can remain different. Latency priority cannot differ.

This minimal rule preserves strict blind FIFO and keeps cancels indistinguishable from trades. It also avoids a large scheduling circuit. The opening and proving latency budget determines the exact epoch delay parameter. The deployment domain pins that parameter.

### 8.3 Fixed frames

The protocol fixes frame boundaries before any payload in the frame can open.

```text
FrameId(tick) = floor(tick / ticks_per_frame)

FrameRange(f) =
    [f * ticks_per_frame, (f + 1) * ticks_per_frame)
```

The sequencer cannot close a frame early because of its contents. This rule covers empty frames, expensive operations, and adverse flow. The protocol divides an oversized frame into deterministic `(tick, position)` chunks with a fixed maximum item count. It carries overflow forward without changing relative order.

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

The Continuum transcript commits `FramePlanV3` before it accepts an opening for that frame. This commitment removes content-dependent discretion over block cuts, priority ranges, and oracle snapshots.

### 8.4 Time semantics

A VDF proves sequential work. It does not prove wall-clock time. V3 does not use `genesis_time + tick × target_tick_duration` to derive Lighter’s economic timestamp.

Lighter keeps its existing proved timestamp, oracle, funding, trigger, expiry, and dead-man-switch semantics. The integration adds one constraint. The system commits the exact `l1_origin`, oracle snapshot, and protocol-event roots before user plaintext opens. This rule prevents order-aware input selection. It does not claim that PoSq ticks form a trusted wall clock.

A stronger schedule must define availability through signed provider data or a transparent Continuum oracle namespace. This rule covers schedules such as “use the latest oracle round available before frame close.” A new `policy_hash` must bind the proof.

---

## 9. Opening and terminal resolution

### 9.1 Permissionless solve

A permissionless time-lock mechanism opens every receipted envelope. Solving starts immediately after receipt issuance. The protocol’s maturity rule still gates publication and use of the opening. A solve that starts only at maturity finishes about one full delay late.

The deployment domain versions this decryption interface:

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

`L1_CANCELLED` is a deterministic recovery no-op. It is not an unavailable-opening outcome in `PROTECTED` mode.

At the pinned L1 fault deadline, the settlement contract moves to `PRIORITY_ONLY`. It atomically freezes the complete unsettled suffix from committed receipt data.

The freeze records the start cursor, end cursor, transcript root, and receipt-vector commitment. No later proof can execute an item in that range.

After the recovery timeout, anyone can call the pinned contract to emit one cancellation event for the complete frozen suffix.

The event activates the pinned stall penalty and recovery payout policy. Cancellation still triggers slashing and required compensation.

The event binds the domain, epoch, frozen range, transcript root, receipt-vector commitment, and L1 origin. It also binds the fault and timeout blocks.

The sequence proof must verify the event, contract, mode, deadlines, range coverage, event uniqueness, and transcript data for every position. The frozen range cannot depend on openings, plaintext, or operator choice.

Before this deterministic freeze, `TLP_UNAVAILABLE` and `SOLVER_TIMEOUT` stall the sequence proof. They are not terminal no-ops in `PROTECTED` mode.

`TRANSCRIPT_DATA_UNAVAILABLE` cannot use suffix cancellation. Without transcript data, the proof cannot account for every receipted position.

A gap from an unavailable opening can give a privately advantaged operator useful information. The operator cannot omit that item while protected settlement continues.

The system verifies DA before receipt finality. After receipt issuance, the system produces the objective opening result or stops finalizing later protected state.

### 9.3 Execution versus sequence validity

Continuum determines only whether authenticated cleartext exists and parses under the pinned byte grammar. It does not determine Lighter authorization or state validity.

For `CLEAR(bytes)` that pass the parser, Lighter’s circuit evaluates:

- API-key signature validity
- API-key nonce and skip-nonce rules
- Expiry and time-in-force
- Margin and health
- Order flags and market state
- Slippage, reduce-only, and protocol limits
- The exact transaction-specific success or failure path

The adapter must not filter invalid signatures, stale nonces, insufficient margin, bad price bounds, or state-dependent failures. Lighter's circuit must prove these outcomes.

For `BAD_AEAD`, `BAD_ENCODING`, and `L1_CANCELLED`, Lighter’s ordered-input circuit must consume the derived item. It must prove a no-state-change terminal result and consume no Lighter API nonce. The admission ticket remains spent.

### 9.4 Failure-semantics table

For every supported transaction type, the protocol definition must include these fields:

| Field | Required definition |
|---|---|
| Canonical parser | exact bytes, bounds, and version |
| Authentication | key lookup and signature predicate |
| Nonce rule | increment, skip, consume-on-failure behavior |
| Stateful verification | Exact ordered predicates |
| Failure result | stable code and state delta |
| Retry rule | Whether a retry can use the same signed transaction |
| Internal expansion | instruction-stack cycles generated by one logical input |

Blind admission removes API-server stateful preverification. These outcome rules form part of the consensus relation.

---

## 10. Canonical derivation stream

### 10.1 Namespace projection

Continuum can carry multiple applications on one global tape. The envelope header exposes `namespace_id`. All economically sensitive Lighter fields remain encrypted.

For a global cursor range `(old_cursor, new_cursor]`, the sequence proof performs these actions:

1. Verifies the complete global receipt/tick transition.
2. Selects every entry with the pinned Lighter namespace.
3. Includes every Lighter entry in the range.
4. Preserves global `(tick, position)` order.
5. Resolves each selected entry exactly once.

Lighter stores the last consumed global cursor and the namespace-local item count. The item count prevents ambiguity in a global range with zero Lighter items.

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

The logical transaction bytes form a private witness where the protocol permits privacy. Their length and hash are public to the proof relation. The commitment always includes length.

The proof relation uses these canonical resolution codes and values:

| Resolution | `resolution` | Cleartext fields | Compact execution fields |
|---|---:|---|---|
| `CLEAR` | `0` | Exact byte length and pinned `H_L` hash, with `terminal_reason = 0` | Parsed nonzero `tx_type`, exact five-field Lighter `tx_hash`, proved outcome, and `terminal_noop = false` |
| `BAD_AEAD` | `1` | Zero length, zero hash, and `terminal_reason = 1` | Zero `tx_type`, zero `tx_hash`, `outcome_class = 1`, and `terminal_noop = true` |
| `BAD_ENCODING` | `2` | Exact recovered length and hash, with `terminal_reason = 2` | Zero `tx_type`, zero `tx_hash`, `outcome_class = 2`, and `terminal_noop = true` |
| `L1_CANCELLED` | `3` | Zero length, zero hash, and `terminal_reason = 3` | Zero `tx_type`, zero `tx_hash`, `outcome_class = 3`, and `terminal_noop = true` |

Zero hash means every field limb is zero. Every item uses its contiguous namespace-local index as `logical_index`.

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

The compact item folds directly into one accumulator preimage. A separately hashed execution leaf does not exist. Lighter reuses its existing five-field transaction hash. The per-item hot path adds one tagged Poseidon2 hash and one 64-bit index split. Receipt, envelope, opening, and cleartext metadata stay in the sequence proof.

The deployment must pin the exact `H_L` hash, field, width, round constants, and padding rule from Lighter's audited circuit fork. A shared vector suite must cover these values:

- Field modulus and canonical limb range
- Field-element ordering
- Byte-to-field packing
- Length binding
- Domain constants
- Output limb order
- Serialization to Ethereum `bytes32`

The `bytes32` serialization must use four canonical 64-bit output limbs in declared order. Lighter's circuit conventions define the order. Shared vectors must bind this choice.

The global Keccak receipt and log commitments remain available for other Continuum namespaces and Ethereum Virtual Machine (EVM) accountability. V3.1 must add the rich namespace accumulator and compact execution accumulator. The sequence proof must prove their one-to-one relationship.

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

The protected claim is strongest for user ordering. Other roots fix their merge with user flow before reveal. They also bind both proofs to the same complete frame inputs.

The protocol does not sequence internal instruction-stack continuation separately. It deterministically expands one logical Lighter transaction and remains inside Lighter’s execution relation.

Protocol-created operations must follow one of these rules:

- The execution circuit proves a unique deterministic trigger and placement.
- An external signed operation enters through Continuum like any other user transaction.

Protected mode forbids discretionary internal transactions.

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

The witness contains these items:

- The relevant receipt, tick-record, log, and segment data
- Fixed envelopes and their DA membership proofs
- Time-lock openings or aggregate opening proofs
- Frame plans and their pre-opening commitments
- Namespace membership and projection paths
- Recovered cleartext bytes
- Canonical parse results
- Priority and oracle commitment witnesses
- Persistent ticket/nullifier updates

### 11.4 Required verification

`π_seq` proves all of the following:

1. The old state equals the previously accepted application state.
2. Every tick and transcript transition follows the pinned PoSq verifier.
3. Receipt signatures, epoch, tick, position, envelope hash, and digest-chain links are valid.
4. Every envelope passes the canonical header, window, size, padding, module, and puzzle-validity predicates.
5. Every ticket has an authorized issuer, valid signature, permitted class, live expiry, unused nullifier, and available policy capacity.
6. Each batch leaf contains the exact canonical receipt reference.
7. Positions and list lengths are unambiguous. Merkle roots bind leaf count.
8. Every envelope hash, ticket nullifier, and receipt position is unique across spans.
9. The transition proves the DA commitment for every envelope before receipt finality.
10. Frame boundaries follow the fixed pre-decryption rule.
11. The transcript commits `FramePlanV3` before any opening for the frame.
12. Every Lighter namespace position in the consumed range resolves exactly once.
13. Every accepted opening passes the pinned decryption verifier.
14. `BAD_AEAD` and `BAD_ENCODING` satisfy their objective predicates.
15. Each `L1_CANCELLED` item belongs to one authorized complete-suffix recovery event under §9.2.
16. Availability or solver failure is not converted into a terminal item.
17. The Lighter namespace projection preserves lexicographic `(tick, position)` order.
18. Each rich item maps to exactly one compact item. The proof computes both stream roots, `receipt_vector_root`, and `opening_root`.
19. Priority, oracle, protocol-event, L1-origin, and policy commitments match the frame plan.
20. The new cursor and persistent roots are the unique transition outputs.
21. The proof computes `C_bind` from the exact public transition data in §13.

The normative verifier must cover receipt verification, chain equality, cross-span duplicate state, openings, and gap reasons. Differential vectors must match the Rust implementation.

### 11.5 Proof backend

The relation is normative. The deployment domain versions the backend.

The reference path uses a recursively aggregatable proof over the field family in Lighter’s pinned prover fork. An Ethereum-verifiable wrapper follows this proof. Better benchmark results and the same proven relation can justify a zkVM or STARK wrapper. Lighter does not import RSA or TLP verification into each matching circuit. The sequence proof amortizes transcript and delay verification over a complete settlement span.

Production Lighter settlement accepts only a `ZK_FINALIZED` sequence state. An `OPTIMISTIC` transition cannot authorize a protected batch.

---

## 12. Ethereum priority and force inclusion

### 12.1 One settlement-enforced inbox

Lighter already maintains an Ethereum priority queue and an Escape Hatch. Current public interfaces include withdrawal, cancel-all, pool-share exit, and key change. They also include related safety operations. The architecture describes reduce-only exits. The L1 queue orders these operations. The protocol must process them before their deadline or enter Desert/Escape mode.

V3 uses this queue as the single settlement-enforced force path. It does not create a second independent PoSq market-operation queue.

The deterministic merge for each frame is:

```text
1. frame pre-execution under the committed oracle/protocol roots
2. all eligible L1 priority operations, in priority-request ID order
3. the Continuum protected user items, in receipted order
4. deterministic post-execution and instruction-stack completion
```

The proof and contract advance one authoritative priority cursor.

### 12.2 Scope of force operations

The force-operation scope includes these risk-reducing and asset-safety operations:

- Cancel-all
- Supported reduce-only immediate-or-cancel (IOC) close
- Secure withdrawal or full exit
- Public-pool exit
- Emergency API-key change where Lighter supports it

V3 does not force-include arbitrary delayed maker or leveraged taker orders. An L1-delayed trading order is often stale. It creates a public front-running surface. Relayers and probes still detect pre-receipt censorship of ordinary market flow. This detection does not guarantee the original market position.

### 12.3 Receipt challenge

Post-receipt omission is objectively accountable.

Each frame publishes a length-bound `receipt_vector_root` over:

```text
(domain_hash, cursor, envelope_hash, receipt_digest, receipt_signature_hash)
```

The user receives a Merkle path after frame closure. Consider a signed receipt that names a cursor behind the settled head. A mismatch with the finalized vector root lets the user submit a host-contract challenge with a bond. The operator must provide the matching inclusion proof within the response window. A failure or mismatch proves equivocation or omission. It slashes the Continuum bond and moves protected settlement to `SEQUENCE_STALLED`.

The operator bond provides slashable deterrence and a service-security reserve. Encrypted admission hides exact transaction notional, so message limits alone cannot prove full loss coverage.

The contract must stop receipts before the posted bond falls below the public policy minimum. The protocol treats this bond as deterrence, not full-loss insurance.

After a service-level agreement (SLA) timeout, the protocol can promote a receipt ahead of the settled head as evidence of a stalled sequence. It cannot regain its original market position through L1. The safe response pauses protected settlement and preserves cancel and exit rights.

### 12.4 Pre-receipt censorship

A single client cannot prove that an unacknowledged packet reached the ingress boundary. The production service uses these controls:

- Several independently operated relayers
- Signed rejection and full-window outcomes
- Deterministic capacity commitments
- Observer probes through the same paths
- Public regional latency and miss-rate telemetry

These controls make censorship measurable. They do not turn network delivery into a cryptographic fact.

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

The sequence proof derives both roots from the validity-proven Continuum transcript. Lighter derives `execution_stream_root` and the count from its executed logical inputs. It carries the rich `ordered_item_root` as a public binding value and recomputes `C_bind`. The settlement join requires equality of both roots, count, and `C_bind`. Lighter cannot substitute the rich root without breaking the sequence proof or join.

### 13.2 Lighter circuit delta

The target Lighter circuit must add these elements:

1. The compact field-native `ExecutionItemV3` accumulator, which advances once per logical input
2. Terminal no-state-change handling for `BAD_AEAD`, `BAD_ENCODING`, and `L1_CANCELLED`
3. Explicit public roots for priority, oracle, and protocol-event inputs
4. `C_bind` in block, segment, batch, and wrapper aggregation
5. Exact continuity verification for the prior and new Continuum cursor

Internal matching cycles do not advance the accumulator. A taker that consumes ten makers remains one logical sequenced transaction. Deterministic instruction-stack work follows it.

The V3.1 preimage removes the second leaf hash and byte decomposition. Within this rolling accumulator, each logical item needs one Poseidon2 transition to change the root. Activation requires measurements of constraint count, latency, memory, aggregation cost, and recursion depth against Lighter's exact audited prover fork.

### 13.3 Blob integration

The audited Lighter blob is 126,976 bytes. Its first 34 bytes are currently constrained to zero:

```text
bytes [0..1]   version
bytes [2..33]  reserved
```

The target wrapper must use this existing surface:

```text
version        = CONTINUUM_BINDING_V3
reserved[32]   = canonical_bytes32(C_bind)
```

The wrapper circuit must bind the word to the execution proof’s `C_bind`. The sequence proof must expose the same canonical word. The Lighter batch commitment already binds its polynomial commitment and execution public data. The version bump adds ordering to that proved data without increasing blob size.

This design follows the EIP-4844 pattern. A ZK rollup uses its internal commitment with the blob commitment and proves their equivalence. EVM execution can access the blob only through its commitment.

### 13.4 Contract flow

The settlement design defines this V3 contract interface:

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

The contract advances these values only after both proofs verify:

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

The contract never performs a linear scan for an anchor. Hashes address proof certificates. Each certificate must extend the stored state exactly.

### 13.6 Recursive aggregation

Recursive aggregation can verify `π_seq` inside Lighter’s final aggregation. Lighter can then submit one wrapped proof to Ethereum. The public relation and `C_bind` remain unchanged.

Recursion is optional. It must reduce total cost and meet these design criteria:

- Sequence proving and execution proving remain parallel.
- Independent verifier upgrades remain possible.
- Settlement latency remains within the SLA.
- The wrapper pins every upstream proof library.
- Public inputs identify the verifier configuration that authorized the batch.

---

## 14. Sequence validity host

V3 uses a stateful validity host.

### 14.1 Finality modes

```text
UNVERIFIED
OPTIMISTIC
ZK_FINALIZED
```

`OPTIMISTIC` is a non-settling status. Lighter production settlement accepts only `ZK_FINALIZED` sequence transitions.

### 14.2 Required host properties

The V3 host must complete these actions:

- Verify or register the exact sequence proof and verifier ID.
- Enforce old-to-new transcript and cursor continuity.
- Bind proof certificates to domain, epoch, namespace, DA, openings, and configuration.
- Store receipt-vector roots and response deadlines.
- Tie fraud evidence to one accepted span.
- Keep the bond above the public policy minimum while admission remains active.
- Use canonical low-`s` ECDSA where ECDSA remains.
- Use identical Rust, circuit, and Solidity challenge derivation.
- Persist ticket/nullifier and capacity commitments.
- Reject duplicate-last Merkle ambiguity by binding leaf count.
- Pause protected settlement after proven receipt equivocation.

Permanent differential vectors must cover Wesolowski challenge derivation and all other cross-language cryptography.

### 14.3 Durable consumer APIs

Live broadcasts are insufficient for a settlement-critical adapter. Continuum adds these durable consumer APIs:

```text
GetTapeRange(start_cursor, end_cursor)
GetTickRange(first_tick, last_tick)
GetFramePlan(frame_id)
GetReceiptVector(frame_id)
GetOpeningRange(start_cursor, end_cursor)
SubscribeFrom(cursor)
GetSequenceCertificate(certificate_hash)
```

Every API response has a canonical hash. Anyone can reconstruct it independently from DA.

---

## 15. Data availability

### 15.1 Preserve Lighter’s hybrid DA

Lighter publishes compressed account deltas and market data in Ethereum blobs. This data supports its public account-state reconstruction and Escape Hatch. Lighter does not publish the complete high-frequency transaction and order-book state.

V3 preserves that model. The Lighter blob adds only the 32-byte `C_bind` word to its existing reserved header.

### 15.2 Continuum DA requirements

Continuum DA retains these items:

- Fixed envelopes
- Admission receipts and receipt vectors
- Tick records, segment data, and frame plans
- Opening witnesses and aggregate opening proofs
- Sequence proof inputs that independent reproduction requires
- Namespace projection paths
- Sequence proof certificates

DA-before-receipt is mandatory. A receipt reaches final status after the configured DA quorum makes the envelope retrievable. Its commitment must also appear in the signed tick record.

The protocol does not copy the full envelope stream into Ethereum blobs. Each envelope contains at least 1,024 bytes. At 20,000 envelopes/s, 1,024-byte envelopes produce 20.48 MB/s before erasure coding and proofs. This load requires a dedicated high-throughput DA network. The network needs replicated archival nodes and a retrieval SLA.

### 15.3 Withholding behavior

Withheld transcript data prevents production of `π_seq`. Lighter can continue to expose unfinalized soft state, but Ethereum does not advance. Safety holds. Liveness moves through the states in §17.

Users retain receipts and receipt paths locally. Watchers mirror the receipt-vector roots and sequence certificates. Retention must exceed each of these periods: the receipt challenge window, Lighter proof delay, Ethereum reorganization margin, and recovery period.

---

## 16. Decryption throughput

### 16.1 Per-item TLP capacity

The per-item TLP profile uses one fixed-base solve-only time-lock puzzle per ciphertext. It aggregates verification proofs, not the sequential work. These formulas use arrival rate `λ`, delay work `T`, solver redundancy `r`, and operating headroom `h`:

```text
required aggregate squaring rate = λ × T × r × h
live puzzle lanes                = λ × delay_seconds × r × h
```

At 10,000 envelopes/s and `T = 2.5 million` squarings, one solver copy requires 25 billion squarings/s. At 20,000 envelopes/s, two replicas and 1.5× headroom require 150 billion squarings/s. Proof aggregation does not reduce this requirement.

### 16.2 Decryption profiles

V3 defines the module interface and separates these decryption profiles:

1. **Per-item TLP profile:** Supports bounded volume below a measured adversarial solver ceiling.
2. **Transparent batch-wave profile:** Supports full-scale operation with a concrete construction, implementation, security proof, and hardware benchmark.
3. **Threshold-decryption profile:** Provides an optional weaker mode with committee collusion and availability assumptions. It uses a distinct `decryption_module_id` and product label.

The full-scale profile must use one delay-dominant solve per maturity wave. Proof aggregation without work aggregation does not meet this profile.

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

Every delay class must satisfy the protocol's no-look inequality:

$$T_k > \alpha \cdot R_{req} \cdot ((W + \Gamma) \cdot \Delta + \delta_{sub} + \sigma_{fence} + \xi + \mu)$$

`T_k` is sequential work. `α` bounds adversarial hardware advantage. `R_req` is the required squaring rate.

`W + Γ` covers the admission window and publication slack. `Δ` is one tick. `δ_sub` bounds submission and admission time.

`σ_fence` is fence slack. `ξ + μ` covers observation, verification, and implementation margins. Protected mode must remain disabled unless every delay class satisfies this bound.

### 16.4 Opening capacity guarantee

Full-scale protected service requires independent evidence for these properties:

- Peak and sustained opening throughput above Lighter’s target load
- Two independent solver implementations
- P99 opening within the protected-delay budget
- Proof generation within the settlement SLA
- Hardware-advantage margin with documented benchmark methodology
- No optional reveal dependency on the Continuum or Lighter sequencer

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

Normal user admission, execution, sequence proving, execution proving, and Ethereum settlement remain active.

### 17.2 Sequence stalled

The system stops new receipts. Lighter can finish proving a prefix that is fully open. No batch beyond the last proved Continuum cursor can settle. Users can cancel or reduce risk through the supported priority lane.

### 17.3 Priority only

The system disables normal trading. Lighter processes settlement-enforced safety operations in priority-request order. Assets remain under Lighter’s existing custody and proof rules.

### 17.4 Desert / Escape Hatch

Lighter’s existing trigger freezes the last verified state. It permits independent exits from Ethereum-posted public account data. Continuum adds no new custody or withdrawal dependency.

### 17.5 Recovery

Recovery starts from the last jointly verified Lighter state, Continuum cursor, transcript root, and priority head. A new protected epoch must prove continuity from that state.

When all transcript data remains available, the deterministic frozen-suffix process in §9.2 can resolve missing openings. The sequence proof resolves every frozen position as `L1_CANCELLED`.

This recovery transition runs outside `PROTECTED` mode and leaves Lighter execution state unchanged. It consumes no Lighter nonce and cannot restore the lost market opportunity.

Transcript data loss cannot use this process. The system remains in `PRIORITY_ONLY` or enters the Escape Hatch.

Future protected service after transcript loss needs a new deployment domain from the last verified head. It cannot claim continuity across the unavailable suffix.

Governance cannot assert, select, reinterpret, or silently skip a receipted finalized position.

### 17.6 No automatic unprotected downgrade

Ordering fraud cannot trigger an automatic downgrade to “declared FCFS.” This rule prevents a faulting sequencer from benefiting through a protection downgrade.

Any future unprotected trading is a separate deployment or user-selected market state. It uses a new domain, new epoch, explicit UI, and no Continuum claim. It cannot share a mutable protected book during the same epoch.

---

## 18. Upgrade and governance safety

Lighter’s contracts and verifier stack support upgrades. Published audits identify privileged validator, governor, security-council, and upgrade-gatekeeper roles. V3 includes these roles in the trust surface.

Protected status requires these controls:

- Verifier and implementation hashes pinned in `DeploymentDomainV3`
- A nonzero public activation delay for normal upgrades
- Emergency authority limited to pausing, priority-only mode, and asset safety
- Verifier changes activated only at epoch boundaries
- Old and new verifier sets that jointly commit to the transition checkpoint
- No batch that spans an implementation, policy, encoding, or decryption change
- Two independent audits for changes to either proof relation
- Permanent public test vectors and reproducible verifier builds

Governance can make the protected guarantee conditional by bypassing the advertised notice period or installing arbitrary logic. The UI and risk documentation must state this condition directly.

---

## 19. End-to-end security properties

Let `B` be an Ethereum-finalized protected Lighter batch. Under the assumptions in §4.4:

### 19.1 Exact consumption

Every logical protected item that `B` consumes corresponds to exactly one Lighter namespace position in the proved Continuum range. No additional protected item can affect the state transition.

### 19.2 Order preservation

Consider protected Lighter items `a` and `b` in the consumed range. For receipted positions `p_a < p_b`, Lighter processes `a` before `b`.

### 19.3 No silent omission

Every Lighter namespace position in a protected range contributes one item. It is an executable transaction or an objectively proved terminal no-op.

Opening failure prevents protected finalization before the deterministic recovery freeze. Transcript data loss always prevents proof of the affected range.

### 19.4 Pre-content frame commitment

The system commits the frame boundary, priority range, oracle root, protocol-event root, and execution eligibility delay before the sequencer can use any payload.

### 19.5 Execution inheritance

Lighter’s proof executes the exact derived stream. Its existing price-time-priority, risk, liquidation, and state-transition guarantees apply to the Continuum-fixed order.

### 19.6 Receipt accountability

A valid signed receipt that conflicts with the finalized receipt vector provides objective slash evidence. It pauses later protected settlement.

### 19.7 Liveness containment

A Continuum or solver failure can stop new protected settlement. It cannot authorize a conflicting Lighter state. It also cannot block Lighter’s existing L1 safety and escape mechanisms.

---

## 20. Performance activation conditions

Lighter advertises tens of thousands of operations per second and millisecond engine latency. The integration must preserve that execution profile. It adds a proof and opening pipeline outside the matching hot path.

Activation requires published measurements for these criteria:

| Metric | Per-item profile | Full-scale profile |
|---|---:|---:|
| Sustained protected ingress | Target workload + 2× headroom | Lighter peak + 2× headroom |
| Receipt p99 | ≤ regional network budget + sub-ms service budget | Same under target load |
| Frame closure jitter | Zero content-dependent cuts | Deterministic replay equality |
| Opening p99 | Within protected-delay budget | Within budget under adversarial withholding |
| Sequence proof latency | Below Lighter settlement interval | Below interval at peak load |
| Execution-proof overhead | Measured against the unmodified prover | Pinned maximum regression |
| Ethereum verification | One sequence and one execution verification per settlement batch | Optional recursion under §13.6 |
| DA retrieval p99 | Sufficient for independent proof generation | Multi-provider SLA |

Ethereum does not verify a per-segment proof directly at a 25.6 ms cadence. Sequence proofs recursively aggregate many ticks and segments into one Lighter settlement certificate.

---

## 21. Public conformance guarantees

V3.1 conformance requires every property in this section.

### 21.1 Encoding and cryptography

- Canonical bytes cover every object and transaction type.
- Domain separation binds chain, contracts, implementation, verifier, epoch, namespace, policy, and decryption module.
- Merkle roots bind list length.
- ECDSA uses canonical low-`s`.
- Rust, Solidity, and circuit signature behavior matches.
- VDF/TLP challenge derivation and primality verification are identical across implementations.
- Hash-to-field and `C_bind` serialization vectors cover boundary values.

### 21.2 Ordering

- The system allocates receipt position atomically with ticket consumption.
- Independent adapters replay byte-identical frame plans and streams.
- Frame cuts remain identical under different plaintexts and execution costs.
- Every Lighter namespace position in a range appears exactly once.
- Cross-span duplicate and omitted receipt tests fail the proof.
- Batch submission creates separate receipts unless the Lighter transaction is protocol-atomic.

### 21.3 Opening

- Solve begins at receipt.
- Order and frame close before maturity.
- No unavailable opening can become a finalizable no-op in `PROTECTED` mode.
- A recovery cancellation covers only the deterministic frozen suffix from §9.2.
- Bad AEAD and bad encoding have unique objective predicates.
- Adversarial withholding load fits the measured solver cap.
- Two independent solver implementations agree on every vector.

### 21.4 Lighter execution

- The transaction relation covers every supported transaction type.
- Signature, nonce, expiry, and stateful failure semantics match protocol behavior or an explicitly versioned migration.
- The logical transaction accumulator advances exactly once despite multi-cycle matching.
- Rich and compact stream roots have a proved one-to-one mapping and identical declared count.
- Priority operations advance in Ethereum request-ID order.
- The protocol removes or sequences discretionary protocol-created transaction paths.
- The protocol commits oracle and frame roots before opening.

### 21.5 Proof join and settlement

- Proof `π_seq` alone cannot change Lighter state.
- Proof `π_exec` alone cannot settle a protected batch.
- Both proofs agree on `execution_stream_root`, item count, and `C_bind`.
- The sequence proof also binds `ordered_item_root` into `C_bind`.
- Cursor, transcript root, priority head, and state root advance atomically.
- The verifier rejects overlapping, skipping, or cross-epoch certificates.
- Settlement accepts only `ZK_FINALIZED` sequence state.

### 21.6 Faults and recovery

- An orphan-receipt challenge slashes and pauses.
- DA withholding stalls without corrupting state.
- A Continuum outage preserves priority cancel and exit operations.
- No automatic unprotected downgrade exists.
- Recovery begins at the last jointly verified head.
- The Escape Hatch works from the last finalized Lighter root with Continuum fully offline.

---

## 22. Sources

### Continuum primary sources

- [Continuum summary](https://docs.fermilabs.xyz/continuum/summary)
- [Continuum architecture](https://docs.fermilabs.xyz/continuum/architecture)

### Lighter primary sources

- [Technical Architecture: Lighter Core](https://docs.lighter.xyz/about-lighter/technical-architecture-lighter-core)
- [Lighter Protocol whitepaper, October 2025](https://assets.lighter.xyz/whitepaper.pdf)
- [Order Types & Matching](https://docs.lighter.xyz/perpetual-futures/orders-and-matching)
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

### Cryptographic and Ethereum references

- Boneh, Bonneau, Bünz, and Fisch, [Verifiable Delay Functions](https://eprint.iacr.org/2018/601)
- Wesolowski, [Efficient Verifiable Delay Functions](https://eprint.iacr.org/2018/623)
- [EIP-4844: Shard Blob Transactions](https://eips.ethereum.org/EIPS/eip-4844)

---

## 23. Final specification statement

A protected Lighter state transition is valid only under these conditions:

```text
1. Continuum validity-proves the canonical encrypted admission transcript;
2. the sequence proof derives one exact ordered Lighter input stream;
3. Lighter validity-proves execution over that exact stream;
4. both proofs and the Ethereum blob expose the same C_bind;
5. Ethereum advances the Lighter state and Continuum cursor atomically.
```

This design turns Lighter’s remaining sequencer discretion into a validity-proven input. It completes the end-to-end settlement statement.
