# Security, verifiability, and economic guarantees

## Threat model

Continuum protects a signed Lighter transaction after the sequencer issues its receipt. The receipt fixes the transaction's encrypted envelope in a protected stream.

The sequencer can control admission, publication, and service availability. A solver can withhold an opening or submit invalid data.

The protocol prevents these parties from changing a receipted order after they learn its plaintext. It makes later omission or equivocation detectable.

The protocol does not prevent rejection or censorship before receipt issuance. It also does not guarantee equal network latency.

## Final verification

A protected production update becomes final only after the recursive sequence verifier and Lighter wrapper verifier accept both proofs. These verifiers enforce the cryptographic boundary.

## Core settlement guarantee

One protected batch contains a sequence proof and a Lighter execution proof. The sequence proof derives a complete stream from one contiguous Continuum transition.

The Lighter proof executes the same stream. Both proofs and the versioned blob expose the same `C_bind`.

The atomic join advances the Continuum head and Lighter state head together. A failed verifier call advances neither head.

The public inputs bind verifier identifiers and continuity values. The atomic join compares them with the stored head and rejects replayed or forked transitions.

Under the stated assumptions, the join prevents insertion, deletion, duplication, or reordering inside the protected stream.

## Security assumptions

| Assumption | Security role |
|---|---|
| Sequence-proof soundness | Every accepted stream obeys the receipt, opening, continuity, and resolution rules |
| Lighter-proof soundness | Every accepted state transition follows Lighter's execution rules |
| Collision-resistant pinned hashes | Different committed values cannot share an accepted root or `C_bind` |
| Canonical encoding | Every logical value has one committed byte or field representation |
| Unknown RSA group order | No party knows the factors of the RSA modulus or a private decryption shortcut |
| Bounded hardware advantage | The calibrated sequential delay remains within its stated exposure model |
| Fresh encryption input | Each timelock envelope uses unique and high-entropy caller input |
| Data availability | Verifiers can retrieve the receipts, openings, frames, and committed batch data |
| Pinned deployment domain | Verifier identifiers, code hashes, policies, parameters, and upstream revisions remain unambiguous |

## Admission secrecy

The user signs the Lighter transaction before encryption. The timelock key-encapsulation mechanism (KEM) derives its AEAD key from a delayed group result.

ChaCha20-Poly1305 protects payload integrity and binds associated data. Confidentiality also depends on fresh encryption input and delayed-result hardness.

A user leak or reused entropy defeats this protection. Knowledge of the RSA factors also defeats the intended delay.

The delay claim is computational, not an absolute wall-clock guarantee. It depends on calibrated sequential work and a bounded hardware advantage.

## Receipt accountability

The sequencer signs linked receipts that bind envelope commitments to protected positions. These signatures give public evidence of orphan receipts, omission, or equivocation.

Receipt evidence does not force the sequencer to issue a receipt. The protection starts only after issuance.

Economic deterrence depends on available receipt data, challenge rules, penalties, and sufficient bonds. Cryptographic evidence alone does not set an adequate penalty.

## VDF and timelock boundary

A verifiable delay function (VDF) proves that a defined amount of sequential computation occurred.

The VDF verifier accepts a repeated-squaring relation through a Wesolowski proof. Domain separation binds the proof to its use and delay.

VDF soundness depends on the selected RSA unknown-order group and the Fiat-Shamir model. A production deployment must pin a ceremony-generated modulus and its transcript.

The V1 demo uses a challenge modulus to illustrate the mechanism.

The wave solver uses independent sequential chains. It combines their outputs with 128-bit Fiat-Shamir weights and verifies one aggregated proof.

Aggregate security also depends on the soundness of this 128-bit Fiat-Shamir weighting construction.

Aggregation reduces proof count and verification work. It does not reduce the sequential work for each ciphertext.

Solver capacity must cover the largest admitted wave within the exposure bound. Insufficient capacity stalls protected finality.

## Sequence-proof boundary

The sequence proof verifies a contiguous range, cursor continuity, counts, roots, duplicates, openings, and typed terminal outcomes.

Each protected position resolves to a Lighter transaction or one proved terminal outcome. `BAD_AEAD` and `BAD_ENCODING` are the protected terminal types.

`L1_CANCELLED` is a deterministic recovery no-op for a complete frozen suffix. It cannot resolve selected positions or replace missing transcript data.

Missing data and solver failure are not terminal types. Neither condition can create a silent gap.

The ordering statement requires succinct validity. It does not require zero knowledge for every public field.

Recursive segment proofs keep proof size fixed across long ranges. They also enforce continuity between adjacent segments.

Settlement accepts the sequence statement only through the sequence-proof verifier. This verifier defines the validity boundary for the ordered stream.

## Execution-proof boundary

The Lighter proof verifies the exchange state transition under Lighter's rules. It also accumulates the ordered execution stream with Poseidon2.

The sequence proof and execution proof remain separate validity statements. Equality of `C_bind` binds them at settlement.

Cross-system recursion can combine both statements. This choice changes cost and upgrade coupling, but it does not change the settlement guarantee.

The settlement domain pins both verifier identifiers and their implementation hashes. Governance controls verifier upgrades. A configuration change starts a new epoch.

## Failure behavior and recovery

A missing opening, unavailable receipt data, or solver failure stalls the protected sequence. The operator cannot replace a stalled position with another transaction.

The system can move from protected service to priority-only operation. This transition cannot create a hidden unprotected path that shares the protected stream.

At the fixed fault deadline, settlement can freeze one complete suffix. With full transcript data, a recovery proof can cancel that entire suffix outside protected mode.

The recovery event activates slashing and leaves Lighter execution state unchanged. Without transcript data, the system must remain priority-only or enter the Escape Hatch.

Lighter's priority queue supports forced operations. The Escape Hatch supports asset recovery without Continuum liveness.

## Public verifiability

A verifier can verify receipt signatures, transcript links, cursor continuity, openings, roots, counts, terminal outcomes, and `C_bind`.

The verifier can also compare the sequence proof, Lighter proof, and versioned blob at the atomic join.

Browser-derived signatures and commitments provide independent data verification. The proof verifiers define cryptographic settlement validity.

Canonical encodings define lengths, field ranges, list bounds, and byte order. Cross-language vectors can verify each boundary value.

## Economic guarantees

### Protected ordering effect

The protected sequencer cannot reorder receipted positions based on plaintext under the stated assumptions. This property can reduce front-running and selective last-look extraction.

The result depends on timelock secrecy, complete stream binding, and sufficient protected flow. Shared unprotected state can restore an ordering advantage.

The guarantee concerns order integrity. It does not fix the market price or guarantee favorable execution.

### Limits

The design does not eliminate all maximal extractable value (MEV). It does not remove adverse selection, loss-versus-rebalancing, oracle effects, liquidations, or cross-venue strategies.

It does not guarantee trader profit, execution quality, or equal network access. It does not stop censorship before the sequencer issues a receipt.

It does not prove solver capacity or a sustainable solver market. A deployment defines fees, admission limits, fallback capacity, and service levels.

Economic accountability depends on solver incentives, bond sizes, challenge costs, and failure budgets. The public policy must state every amount and compensation rule.

Encrypted admission hides exact notional at receipt time. The operator bond provides deterrence, not guaranteed full-loss coverage.
