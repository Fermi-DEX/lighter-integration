# Security, verifiability, and economic guarantees

## How to read this document

This document separates a target guarantee from current enforcement. A tested reference relation is not a production validity proof.

Every cryptographic claim depends on its stated assumptions. Every economic claim depends on adoption, capacity, and the protected market surface.

## Guarantee levels

| Level | Meaning |
|---|---|
| Implemented | Runnable code enforces the property in its stated scope |
| Tested reference | Tests verify a host or contract relation with mock proof boundaries |
| Target | The V3.1 specification requires the property, but production code does not enforce it yet |

## Target settlement property

Consider one protected batch with two sound proofs. The sequence proof derives a complete ordered stream from a contiguous Continuum transition.

The Lighter proof executes the same stream. Both proofs and the versioned blob expose the same `C_bind`.

The atomic join then prevents insertion, deletion, duplication, or reordering of the protected stream. It advances both state heads together.

This claim assumes sound proofs, collision-resistant pinned hashes, and one canonical encoding for every committed value.

The current repository tests this relation at host and scaffold level. It does not enforce the relation against a real Lighter wrapper proof.

## Security claims and limits

| Claim | Current evidence | Production requirement |
|---|---|---|
| Contiguous order | Host tests reject skipped cursors, duplicates, missing resolutions, and root changes | A sound sequence SNARK must enforce every predicate |
| Exact execution binding | Rust and Solidity joins compare roots, counts, heads, and `C_bind` | Lighter's real wrapper and blob must expose the same values |
| Atomic state advance | The reference contract changes heads only after both mock verifier calls pass | Lighter's settlement lifecycle must contain the join |
| Replay resistance | Outer public inputs carry verifier identifiers and continuity values. The join compares them with the stored head | Governance must pin every domain value and control upgrades |
| Typed terminal outcomes | Host tests prevent a terminal item from hiding a transaction hash | Lighter needs a dedicated constrained terminal selector |
| Fail-closed availability | V3.1 defines missing data and solver failure as stalls | The production host and proof queue must enforce this state machine |
| Receipt accountability | The sequencer signs linked receipts and fraud data | Production needs durable receipt data, slashing rules, and sufficient bonds |

### VDF guarantee

The implemented verifier accepts a repeated-squaring relation through a Wesolowski proof. Domain separation binds the proof to its use and delay.

Production review must establish proof soundness for the selected RSA unknown-order group and Fiat-Shamir model.

The timelock delay also depends on unknown group order, calibrated sequentiality, and a bounded hardware advantage. It does not prove an absolute wall-clock delay.

The default RSA-2048 challenge modulus is a provenance placeholder. A production deployment must pin a ceremony output and its transcript.

### Timelock confidentiality

The solve-only KEM derives the AEAD key from the delayed group result. ChaCha20-Poly1305 protects payload integrity and binds associated data.

Confidentiality depends on delayed-result hardness, ChaCha20-Poly1305 security, and fresh high-entropy encryption input. A user leak or reused entropy defeats this protection.

The sequencer must not gain a private decryption shortcut. Security also requires that no party knows the production RSA modulus factors.

### Batch solve guarantee

The wave solver uses independent sequential chains. It combines their outputs and verifies one aggregated proof.

Tests reject altered witnesses and wave bindings. Production review must establish aggregate soundness for the 128-bit Fiat-Shamir weighting construction.

The aggregation reduces proof count, not sequential work.

Production capacity must cover the largest admitted wave within the exposure bound. The current repository has no adversarial capacity proof.

### Zero knowledge and recursion

The ordering statement requires succinct validity. It does not require zero knowledge for every public field.

Internal sequence recursion is part of the scalable target. It gives fixed proof size across many segments and enforces cross-segment continuity.

Cross-system recursion can combine the final sequence proof with Lighter's wrapper. This option changes cost and coupling, not the security statement.

No recursive sequence proof exists in this repository today.

## Verifiability

### What reviewers can verify now

Reviewers can run the VDF, timelock, sequencer, plugin, demo, and Solidity tests. They can reproduce the pinned Poseidon2 preimages.

Reviewers can apply the accumulator overlay to the exact pinned Lighter prover. They can compile and run its native mutation test.

Reviewers can alter roots, counts, cursors, terminal outcomes, or `C_bind`. The reference joins reject those changes.

The browser demo can re-derive signed records and commitment links. This feature verifies demo data, not real Lighter proof validity.

### What reviewers cannot verify yet

No reviewer can generate a real recursive sequence proof from this repository. No real Lighter wrapper proof exposes the final protected stream root.

The Solidity reference uses mock verifier responses. It does not verify live sequence or Lighter proofs.

The repository has no full mixed heavy and light Lighter witness test. It also has no measured execution-prover overhead or recursive-join benchmark.

The local data store is not a multi-provider availability system. Historical Sepolia metadata does not prove a current live deployment.

## Economic guarantees

### Protected ordering effect

Under the target assumptions, the protected sequencer cannot reorder receipted positions based on plaintext. This can reduce front-running and selective last-look extraction.

The result depends on timelock secrecy, full stream binding, and enough protected flow. Shared unprotected state can restore an ordering advantage.

A signed receipt supports evidence for orphan receipts, omission, or equivocation. Effective deterrence still depends on data availability, challenge rules, penalties, and bonds.

### Claims the design does not make

The design does not eliminate all MEV. It does not remove adverse selection, loss-versus-rebalancing, oracle effects, liquidations, or cross-venue strategies.

It does not guarantee trader profit, execution quality, or equal network access. It does not stop censorship before the sequencer issues a receipt.

It does not prove solver capacity or a sustainable solver market. Production needs explicit fees, admission limits, fallback capacity, and monitored service levels.

The demo slashing contract does not prove adequate production economics. Its bond floor and bounty rules are demonstration parameters.

### Asset safety and liveness

The target design preserves Lighter's priority queue and Escape Hatch. The queue supports forced operations. The Escape Hatch supports asset recovery.

The current settlement reference does not wire those paths. Production must verify priority continuity and test every operating-mode transition.

A stalled protected sequence must not become an operator-selected stream. The system must enter priority-only operation before using the existing escape process.

## Production evidence gate

The teams need the following evidence before they describe the target guarantees as production enforcement:

1. A sound recursive sequence prover and verifier artifact.
2. Full Lighter accumulator wiring across heavy, light, block, recursion, wrapper, and blob layers.
3. Real two-proof settlement with continuity, priority, governance, and escape-path tests.
4. A ceremony-derived modulus and reviewed timelock parameters.
5. Multi-provider data availability and tested restart recovery.
6. Real Lighter witness vectors and cross-language boundary vectors.
7. Measured prover latency, memory, proof size, gas, and solver capacity.
8. Reviewed admission fees, solver incentives, bond sizing, and failure budgets.
