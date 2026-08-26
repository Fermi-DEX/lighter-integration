# Design goals

## The user problem

A sequencer can see a clear transaction before its execution position becomes binding. This knowledge can support reordering, selective rejection, or last-look behavior.

Continuum adds fair ordering to signed Lighter transactions. It fixes each protected position while the transaction remains encrypted.

Fair ordering means that a receipt fixes the position before the sequencer or public learns the plaintext.

The protocol later proves that Lighter executed the same protected stream. Lighter remains the authority for transaction validity and state transitions.

## Protocol flow

The integration uses six linked stages:

1. A user signs a Lighter transaction and encrypts it in a timelock envelope.
2. Continuum admits the envelope and signs a receipt that fixes its stream position.
3. Public sequential work produces a timed opening for the envelope.
4. A sequence proof derives the complete protected stream from linked receipts and openings.
5. The Lighter proof executes that stream and commits to the same `C_bind` value.
6. Atomic settlement advances both state heads only after both proofs agree.

## 1. Bind order to execution

Continuum must fix each protected position before a solver exposes its plaintext. The sequence proof must derive every protected Lighter input from one contiguous transcript range.

The Lighter execution proof must commit to the same ordered input stream. Ethereum settlement must compare both commitments before either state head advances.

This rule prevents a valid execution proof from approving a different protected order.

## 2. Keep Lighter as the execution authority

Continuum orders Lighter transactions that users sign before encryption. It does not replace Lighter signatures, nonces, margin rules, matching rules, liquidations, or state transitions.

Lighter decides whether each clear transaction executes. Continuum proves where that transaction entered the protected stream.

## 3. Protect the hot path of the Lighter prover

The Lighter transaction circuit must not verify RSA, VDF, timelock, receipt, or data-availability relations. A separate sequence proof must verify these relations.

The execution circuit must add one field-native Poseidon2 transition for each logical input. It must use Lighter's existing five-field transaction hash.

This split preserves parallelism between heavy and light transaction chains. Both proof generators can operate at the same time.

## 4. Bind rich evidence to compact execution data

The sequence proof must compute two ordered roots. The rich root includes receipt data and opening-derived results. The compact root includes only execution fields.

The sequence proof must verify each opening commitment separately. It must also prove a one-to-one projection between the rich and compact roots.

`C_bind` must commit to both roots, their count, continuity values, and batch context. The Lighter proof must compute only the compact root.

This design does not require a second full transaction pass inside Lighter.

## 5. Fail closed

A protected batch without both valid proofs cannot advance the settled Lighter state. A missing opening or unavailable receipt data stalls protected finality.

Only proved terminal outcomes can consume a protected position without a Lighter transaction. `BAD_AEAD` and `BAD_ENCODING` are protected terminal outcomes.

A deterministic `L1_CANCELLED` no-op can resolve only a complete suffix frozen during priority-only recovery.

Solver failure and data unavailability are not terminal outcomes. An operator cannot convert either fault into a silent gap.

## 6. Use recursion at the sequence layer

The sequence prover must use recursive segment proofs across long transcript ranges. Each segment must prove exact continuity with the next segment.

The final sequence proof and the Lighter proof can remain independent. Equality of `C_bind` forms their security boundary.

Cross-system recursion can combine both proofs. Measurements must show lower total cost without harmful latency or upgrade coupling before its use.

Recursion does not decrease sequential work for the timelock.

## 7. Make every trust root explicit

The deployment domain must pin verifier identifiers, implementation hashes, encodings, policies, decryption parameters, and upstream revisions. A configuration change starts a new epoch.

Canonical encodings define lengths, field ranges, list bounds, and byte order. Cross-language vectors cover each boundary value.

## 8. Preserve recovery paths

The integration must retain Lighter's priority queue and Escape Hatch. A Continuum outage cannot create a hidden unprotected sequencer path.

The operating mode can move from protected service to priority-only service. Asset recovery remains independent of Continuum liveness.

## Guarantee scope

The design does not guarantee equal network latency or receipt issuance. It does not stop censorship before a receipt exists.

It does not prove oracle correctness, trader profit, or complete maximal extractable value (MEV) removal. It does not protect an unprotected lane that shares mutable Lighter state.

The design does not give an absolute wall-clock delay. The delay claim depends on calibrated sequential work and a bounded hardware advantage.
