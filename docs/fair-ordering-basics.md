# Fair ordering basics

## The problem

An ordinary sequencer receives clear signed transactions. It can inspect each
transaction before it fixes the transaction order.

This power matters because different valid orders can produce different
trades. A valid execution proof alone does not prove that the sequencer chose
the order fairly.

Lighter proves that its matching engine executes a chosen input stream
correctly. Continuum adds proof that the protected input stream follows an
order fixed before transaction reveal.

## A two-order example

Assume an order book has one unit of resting sell liquidity at $3,000. Alice
and Bob each submit a marketable buy order for one unit.

Only the first buy order receives that liquidity at $3,000. The order of the
two buys changes the result, even though both orders are valid.

An ordinary sequencer can read both buys before it chooses their order. It can
place Bob first after it learns the traders, prices, sizes, or other contents.

With Continuum, Alice and Bob submit fixed-size encrypted envelopes. Assume
Continuum gives Alice position 101 and Bob position 102 through signed
receipts.

The envelopes open after those positions become fixed. The ordered-input
accumulator and `C_bind` join bind Alice's order before Bob's order.

Lighter's normal matching proof then applies its execution rules to that bound
stream.

This guarantee starts with receipt issuance. It does not prove who first sent
a packet. It does not stop censorship before Continuum issues a receipt.

## Seven steps from user to settlement

The proof-backed design uses this flow:

1. The client signs an ordinary Lighter transaction with the existing Lighter
   API key.
2. The client pads and encrypts that signed transaction into a fixed-size
   Continuum envelope.
3. Continuum admits the hidden envelope, fixes its transcript position, and
   returns a linked signed receipt.
4. Public solvers complete the required sequential work. The timelock reveals
   the transaction only after its position becomes fixed.
5. The sequence prover verifies the transcript, receipts, openings, and
   continuity. It derives the exact ordered Lighter input stream.
6. Lighter executes that stream under its existing rules. Its execution proof
   commits to the same compact ordered stream.
7. Ethereum settlement verifies both proofs and equal `C_bind` values. It then
   advances the Lighter state and Continuum cursor atomically.

## What each part does

### Timelock and VDF

The timelock hides each signed transaction until a solver completes required
sequential work. This design assumes that nobody knows the RSA modulus factors.
Under this assumption, no party gets a private shortcut.

A production deployment uses a ceremony-derived modulus with unknown factors.
The V1 demo uses a challenge modulus to illustrate the mechanism.

The VDF gives the Continuum transcript verifiable sequential progress. A short
proof lets another party verify a large amount of repeated-squaring work.

These tools fix order before reveal. They do not provide an absolute wall
clock. Their delay depends on calibrated work and a bounded hardware
advantage.

### Sequence proof

The sequence proof covers one contiguous Continuum transcript transition. It
verifies each protected position and derives one complete Lighter input
stream.

This proof rejects a missing, duplicate, inserted, or reordered protected
item. It also verifies defined terminal outcomes for positions without an
executable transaction.

The protocol names this proof `SequenceTransitionProof`.

### Lighter execution proof

The Lighter proof verifies signatures, nonces, risk rules, matching, and state
changes for the chosen stream. It remains the authority for transaction
execution.

Continuum does not replace Lighter's matching engine. It proves which protected
signed transaction enters that engine first.

### `C_bind`

The sequence proof commits to rich ordering evidence and a compact execution
stream. The Lighter proof computes the same compact stream commitment.

`C_bind` binds those commitments to their count, continuity values, and batch
context. Both proofs must expose the same `C_bind` before settlement accepts
the protected transition.

The proofs can remain separate. Their equality at settlement forms the
security boundary.

### Recovery lane

A missing valid sequence proof stalls protected finality. The operator cannot
replace the missing proof with a different unprotected user order.

Lighter's priority queue preserves forced operations. During a Continuum
outage, the system can use priority-only operation and the existing Escape
Hatch for asset recovery.

## Guarantee boundary

With sound proofs and collision-resistant hashes, the proof join prevents
insertion, deletion, duplication, or reordering inside the protected stream.

Timelock secrecy also requires fresh encryption input, unknown RSA factors,
and enough sequential work. Durable receipt and opening data must remain
available to the prover.

The design does not guarantee equal network latency, pre-receipt inclusion,
oracle correctness, trader profit, or removal of all maximal extractable value
(MEV). Unprotected flow that shares mutable state can weaken the protected
ordering effect.

## Glossary

| Term | Meaning |
|---|---|
| Envelope | A fixed-size encrypted container for one signed Lighter transaction |
| Receipt | Continuum's signed promise that binds an envelope to a transcript position |
| Transcript | The ordered record of envelopes, receipts, openings, and state continuity |
| Timelock | Encryption that needs sequential work before public decryption |
| VDF | A function with sequential evaluation and efficient public verification |
| Sequence proof | Proof that a contiguous Continuum transition derives one exact protected input stream |
| Execution proof | Lighter's proof that it applied its rules correctly to the chosen stream |
| `C_bind` | The commitment that links both proof statements to one batch |
| Atomic join | Settlement logic that accepts both linked proofs or advances neither state |
| Recovery lane | Priority operations and asset recovery used during protected-service failure |
