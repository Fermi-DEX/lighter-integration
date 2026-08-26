# Design goals

## 1. Bind order to execution

Continuum must fix each protected position before any solver exposes its plaintext. The sequence proof must derive every protected Lighter input from one contiguous transcript range.

The Lighter execution proof must commit to the same ordered input stream. Ethereum settlement must compare both commitments before either state head advances.

This rule prevents a valid execution proof from approving a different protected order.

## 2. Keep Lighter as the execution authority

Continuum orders encrypted, already-signed Lighter transactions. It does not replace Lighter signatures, nonces, margin rules, matching rules, liquidations, or state transitions.

Lighter decides whether each clear transaction executes. Continuum proves where that transaction entered the protected stream.

## 3. Protect the Lighter prover hot path

The Lighter transaction circuit must not verify RSA, VDF, timelock, receipt, or data-availability relations. The separate sequence proof verifies those relations.

The execution circuit adds one field-native Poseidon2 transition for each logical input. It reuses Lighter's existing five-field transaction hash.

This split preserves heavy and light transaction-chain parallelism. It also lets both proofs run at the same time.

## 4. Bind rich evidence to compact execution data

The sequence proof computes two ordered roots. The rich root includes receipt data and opening-derived results. The compact root includes only execution fields.

The sequence proof separately verifies each opening commitment.

The sequence proof proves a one-to-one projection between these roots. `C_bind` commits to both roots, their count, continuity values, and batch context.

The Lighter proof computes only the compact root. This design avoids a second full transaction pass inside Lighter.

## 5. Fail closed

A protected batch without both valid proofs must not advance the settled Lighter state. A missing opening or unavailable receipt data must stall protected finality.

Only proved terminal outcomes can consume a protected position without a Lighter transaction. The current design defines `BAD_AEAD`, `BAD_ENCODING`, and `L1_CANCELLED` outcomes.

Solver failure and data unavailability are not terminal outcomes. An operator cannot convert either fault into a silent gap.

## 6. Use recursion at the correct layer

The scalable sequence prover must use recursive segment proofs. Each segment carries fixed public state and proves exact continuity with the next segment.

The first production settlement design keeps the final sequence proof and Lighter proof independent. Equality of `C_bind` forms the security boundary.

Cross-system recursion is an optional cost optimization. Adopt it only after measurements show lower total cost without harmful latency or upgrade coupling.

Recursion fixes external proof size across many segments. Its total verification cost needs measurement. It does not reduce the timelock's sequential work.

## 7. Make every trust root explicit

The deployment domain must pin verifier identifiers, implementation hashes, encodings, policies, decryption parameters, and upstream revisions. A configuration change starts a new epoch.

Canonical encodings must define lengths, field ranges, list bounds, and byte order. Cross-language vectors must cover every boundary value.

## 8. Preserve recovery paths

The target integration must retain Lighter's priority queue and Escape Hatch. A Continuum outage must not create a hidden unprotected sequencer path.

The operating mode can move from protected service to priority-only service. Asset recovery remains independent of Continuum liveness.

## 9. Support an honest team test

The repository must give both teams a small, reproducible review surface. Every upstream dependency must use a pinned revision.

The demo must remain available because it explains the user experience and ordering effect. Its simulated Lighter execution must stay clearly labeled.

The team-test package must separate runnable code, tested reference relations, pinned overlays, and target work.

## Non-goals

This design does not guarantee equal network latency or receipt issuance. It does not stop censorship that happens before a receipt exists.

It does not prove oracle correctness, trader profit, or complete MEV removal. It does not protect an unprotected lane that shares mutable Lighter state.

It does not give an absolute wall-clock delay. The delay claim depends on calibrated sequential work and a bounded hardware advantage.

It does not make the V1 demo contract a production settlement contract. The production integration must use real Lighter proofs and verifier pins.
