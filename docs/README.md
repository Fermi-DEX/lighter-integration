# Documentation

This repository is primarily a Lighter integration package. It also includes a functional Continuum runtime snapshot for the self-contained demo.

The runtime includes the VDF, timelock KEM, sequencer, receipts, transcript, and demo anchoring path. It is not the latest production Continuum kernel.

## Repository boundary

| Area | Included now | Boundary |
|---|---|---|
| Lighter integration | Binding types, stream roots, `C_bind`, host transition verification, settlement join, and a pinned prover overlay | The full Lighter heavy and light circuit wiring remains joint work |
| VDF kernel | RSA-2048 repeated squaring and Wesolowski proof generation and verification | The default modulus is a challenge-modulus placeholder |
| Timelock kernel | Solve-only KEM, public opening, AEAD protection, and aggregated wave proofs | Each ciphertext still needs an independent sequential solve chain |
| Continuum sequencer | Admission, signed receipts, work-defined ticks, transcript records, local storage, anchors, and fraud evidence | This code is the V1 demo snapshot, not the latest production kernel |
| Demo bridge | Embedded sequencer, simulated Lighter order book, browser verification, scenarios, and optional Sepolia posting | The demo does not verify real Lighter execution proofs |
| Sequence validity | A tested host relation and public input model | The recursive `SequenceTransitionProof` SNARK does not exist yet |
| Settlement | A tested Rust join and Solidity reference contract | Real Lighter verifiers, custody, blob, governance, and escape-path wiring remain open |

The generic optimization and FPGA modules in `crates/vdf` are experiments. This documentation does not treat those modules as production guarantees.

## Start here

| Document | Purpose |
|---|---|
| [Design goals](./design-goals.md) | The intended system boundary and proof architecture |
| [Full functionality](./functionality.md) | Current code, test surfaces, run modes, and missing production work |
| [Security, verifiability, and economic guarantees](./security-verifiability-and-economic-guarantees.md) | Conditional claims, assumptions, limits, and failure behavior |
| [Lighter integration specification v3.1](./lighter-integration-spec-v3.md) | The detailed target protocol and normative proof relations |
| [Lighter team test runbook](./team-test-runbook.md) | Reproducible commands for the extracted repository and pinned overlays |
| [Improvement roadmap](../lit_improvement_roadmap.md) | Completion verdict, prover efficiency plan, recursion policy, and pitch gates |

## Version labels

`V1` identifies the preserved demo runtime and its optimistic bridge. `V3.1` identifies the target validity-enforced integration design.

The two versions coexist for a reason. V1 gives the teams a runnable bridge and visual review surface. V3.1 defines the production security boundary.

Files under `integrations-v2` and `demo/EXECUTIVE-BRIEF.md` record historical design work. They do not override the V3.1 specification or this status summary.

## Status terms

| Term | Meaning |
|---|---|
| Runnable | The repository contains an executable path with tests |
| Tested reference | Tests cover the relation, but no production verifier enforces it |
| Pinned overlay | A patch applies to one exact upstream revision |
| Target | The specification defines the behavior, but the implementation remains incomplete |
