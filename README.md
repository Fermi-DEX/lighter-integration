# Continuum × Lighter integration

A self-contained extraction of the Lighter-specific Continuum work. This is
primarily a Lighter integration package. It also embeds the functional V1
Continuum VDF, timelock, and sequencer runtime required by the demo.

The embedded runtime is not the latest production Continuum kernel. The
runnable V1 bridge, production-facing Rust plugin, and proof design provide a
complete review and team-test surface.

## Status

- **Demo-complete:** the preserved V1 bridge embeds the Continuum sequencer,
  drives a Lighter-style order book, exposes a self-verifying browser UI, and
  includes Sepolia-oriented contracts and adversarial tests.
- **Team-test ready:** V3.1 types, the structural sequence transition,
  field-native Poseidon2 stream folding, dual-root `C_bind`, and the atomic
  two-proof join have standalone Rust and Solidity implementations with
  adversarial tests.
- **Pinned upstream overlay included:** an apply-ready patch adds the exact
  one-Poseidon-per-item gadget to Lighter's pinned prover and CI compiles its
  native mutation test.
- **Not production-complete:** the overlay still must be threaded through
  Lighter's heavy/light `JumpState`, terminal selector, batch/wrapper public
  inputs, and versioned blob word. The recursive sequence SNARK, production
  DA, live verifier pins, and measured prover deltas remain joint work.

The detailed verdict, efficient prover design, recursive option, test matrix,
and pitch gate are in [`lit_improvement_roadmap.md`](./lit_improvement_roadmap.md).
The normative protocol draft is in
[`docs/lighter-integration-spec-v3.md`](./docs/lighter-integration-spec-v3.md).
Start with the [`docs` index](./docs/README.md) for the scope, design goals,
full functionality, and conditional guarantees.

## Repository layout

| Path | Purpose |
|---|---|
| `crates/continuum-lighter-plugin` | Standalone V3 binding and proof-join reference API |
| `crates/sequencer`, `crates/vdf` | Exact runtime support required by the preserved V1 demo |
| `demo` | Runnable V1 bridge, dashboard, bots, scenarios, and Solidity tests |
| `contracts/production` | Reference atomic settlement join; not a deployed Lighter patch |
| `patches/lighter-prover` | Apply-ready accumulator overlay for the pinned Lighter prover |
| `integrations-v2` | Historical V1/V2 design material |
| `docs/README.md` | Documentation map and repository boundary |
| `docs/design-goals.md` | Design goals and non-goals |
| `docs/functionality.md` | Full current functionality and production gaps |
| `docs/security-verifiability-and-economic-guarantees.md` | Security assumptions and conditional guarantees |
| `docs/lighter-integration-spec-v3.md` | Current production design |
| `docs/team-test-runbook.md` | Reproducible review and test sequence for both teams |
| `upstream` | Pinned upstream revisions and precise Lighter prover integration map |

## Local checks

```bash
cargo test -p continuum-lighter-plugin
cargo +nightly-2025-12-06 test -p continuum-lighter-plugin --features lighter-poseidon2
cargo test -p demo-gateway
cd demo/contracts && forge test
cd ../../contracts/production && forge test
```

The plugin's `Sha256ReferenceHasher` exists only for deterministic host tests.
The `lighter-poseidon2` feature uses the exact pinned Lighter Plonky2 fork and
field-native execution preimages; it is the review/reference implementation
for production vectors.

## License

Fermi-owned material without a different notice uses the
[Business Source License 1.1](./licence.md). The VDF crate, MIT-tagged Solidity,
the Lighter overlay, dependencies, and vendored material keep their stated
licenses.
