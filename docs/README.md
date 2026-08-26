# Documentation

An ordinary sequencer can read a transaction before it fixes that transaction's
position. This information lets the sequencer choose among different valid
orders.

Continuum is an ordering layer for encrypted transactions. Lighter is an
exchange that proves the correctness of its state changes.

The proof-backed design fixes an encrypted Lighter transaction before reveal.
A sequence proof then binds that protected order to Lighter's execution proof
at settlement.

The browser demonstration shows encrypted admission, opening, and optimistic
span settlement. Separate proof tests exercise the atomic proof join.

Start with [Fair ordering basics](./fair-ordering-basics.md). It uses two
competing orders to explain the complete path without proof-system background.

## System in one view

The proof-backed design uses these steps:

1. A client signs a normal Lighter transaction.
2. The client submits that transaction inside a fixed-size encrypted envelope.
3. Continuum assigns a transcript position and returns a signed receipt.
4. Sequential work opens the envelope after the position becomes fixed.
5. A sequence proof derives the exact ordered Lighter stream.
6. Lighter executes that stream and proves its state transition.
7. Both proofs must expose equal `C_bind` values before settlement accepts them.

The sequence proof establishes protected order. Lighter remains the authority
for signatures, nonces, risk rules, matching, liquidations, and state changes.

## Core concepts

| Concept | Meaning |
|---|---|
| Fair ordering | The system fixes protected positions before transaction reveal |
| Envelope | A fixed-size encrypted container for one signed Lighter transaction |
| Receipt | Continuum's signed commitment to an envelope position |
| Timelock | Delayed decryption based on sequential work |
| VDF | Verifiable sequential progress for the Continuum transcript |
| Sequence proof | Proof that one contiguous transcript range derives an exact ordered stream |
| Execution proof | Lighter's proof that it executed the chosen stream correctly |
| `C_bind` | One commitment that links the two proof statements and their batch context |
| Atomic settlement | A state update that advances both linked heads or neither head |
| Recovery lane | Lighter priority operations and asset recovery during protected-service failure |

## Read in this order

| Document | What it explains |
|---|---|
| [Fair ordering basics](./fair-ordering-basics.md) | The problem, a two-order example, user flow, proof roles, and glossary |
| [Design goals](./design-goals.md) | The security boundary and the reasons for each design choice |
| [How the integration works](./functionality.md) | Encrypted admission, timed opening, both proofs, atomic settlement, and recovery |
| [Security, verifiability, and economic guarantees](./security-verifiability-and-economic-guarantees.md) | Exact guarantees, assumptions, failure behavior, and economic limits |
| [Lighter integration specification](./lighter-integration-spec-v3.md) | Canonical data, proof relations, public inputs, state transitions, and settlement rules |
| [Verification guide](./team-test-runbook.md) | Exact commands and the meaning of each green test |

## How the pieces fit

Continuum owns the ordering statement. Its proof derives one compact execution
stream from verified receipts, timelock openings, and transcript continuity.

Lighter owns the execution statement. Its proof applies exchange rules to that
same compact stream and computes the matching ordered root.

`C_bind` links the two statements to one count, state transition, and batch
context. Settlement verifies both proofs and advances both state heads
atomically.

During a protected-service failure, priority operations and the Escape Hatch
provide the recovery path.
