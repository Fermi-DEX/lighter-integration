# Continuum × Lighter integration

The Continuum × Lighter protocol is designed to give Lighter a protected order
that users and contracts can verify. It fixes encrypted Lighter transactions
before reveal and binds that order to Lighter's validity-proven execution.

Start with [Fair ordering basics](./docs/fair-ordering-basics.md) for one
concrete example and the full user-to-settlement flow.

## Why this integration exists

An ordinary sequencer can read a signed transaction before it fixes that
transaction's position. It can then use the trader, price, size, or order type
to choose a different valid order.

Lighter proves that its exchange executes a chosen stream correctly. Its proof
covers signatures, nonces, risk rules, matching, and state changes. That proof
alone does not prove fair selection of the input stream.

Continuum accepts each protected transaction as a fixed-size encrypted
envelope. A signed receipt fixes the envelope's transcript position before the
timelock reveals its contents.

A sequence proof derives the exact ordered Lighter stream from the Continuum
transcript. The Lighter execution proof commits to the same stream. Both
proofs must expose the same `C_bind` before Ethereum settlement accepts the
batch.

## Proof-backed transaction flow

The proof-backed design uses this flow:

1. The client signs an ordinary Lighter transaction.
2. The client pads and encrypts the transaction into a Continuum envelope.
3. Continuum fixes the hidden envelope's position and returns a signed receipt.
4. Public solvers complete the timelock work and reveal the signed transaction.
5. The sequence proof derives the exact protected input stream.
6. Lighter executes that stream and proves the resulting state transition.
7. Settlement verifies both proofs, compares `C_bind`, and advances both state
   heads atomically.

Lighter remains the execution authority. Continuum proves protected order, but
Lighter still applies every signature, nonce, margin, matching, and liquidation
rule.

## How the two proofs meet

In the protocol design, the sequence proof reads one contiguous Continuum
transcript range. It verifies receipts, timelock openings, terminal outcomes,
counts, and continuity.

That proof computes a rich evidence root and a compact execution root. It
proves that each compact item comes from exactly one verified transcript
position.

The Lighter proof executes the compact stream. It computes the same root while
it applies Lighter's existing exchange rules.

`C_bind` commits to both roots, the item count, state continuity, and batch
context. Settlement compares `C_bind` from the sequence proof, Lighter proof,
and versioned blob.

Both state heads advance only after both proofs pass. Any root, count, cursor,
or `C_bind` mismatch rejects the entire update.

## Why the Lighter hot path stays small

The design assigns receipts, RSA relations, VDF progress, timelock openings,
transcript continuity, and data commitments to the separate sequence proof.

The Lighter circuit adds one field-native Poseidon2 accumulator step for each
logical input. It reuses Lighter's existing five-field transaction hash.

This split keeps non-native cryptography outside Lighter's transaction circuit.
It also preserves parallel proving for heavy transactions, light transactions,
and the sequence relation.

## How recovery works

The protocol requires both valid proofs for a protected batch. Missing opening
data or failed solver work stalls protected finality instead of creating a
silent gap.

`BAD_AEAD` and `BAD_ENCODING` are protected terminal outcomes. A deterministic
`L1_CANCELLED` no-op resolves a complete frozen suffix during recovery.

Settlement fixes the suffix at a public fault deadline. Cancellation cannot
select individual positions, and it activates the fault penalty.

Lighter's priority queue remains available for forced operations. During a
Continuum outage, priority-only operation and the Escape Hatch preserve the
asset-recovery path.

The protected guarantee starts after receipt issuance. It does not guarantee
equal network latency or prevent censorship before a receipt exists.

## Learn the system

1. Read [Fair ordering basics](./docs/fair-ordering-basics.md).
2. Review the [design goals](./docs/design-goals.md).
3. Read [how the integration works](./docs/functionality.md).
4. Read the [security, verifiability, and economic guarantees](./docs/security-verifiability-and-economic-guarantees.md).
5. Use the [technical specification](./docs/lighter-integration-spec-v3.md) for
   exact proof relations.
6. Run the [verification guide](./docs/team-test-runbook.md).

## System components

| Path | Purpose |
|---|---|
| `crates/continuum-lighter-plugin` | Standalone V3 binding and proof-join reference API |
| `crates/sequencer`, `crates/vdf` | Embedded Continuum V1 runtime for the interactive demo. Production deployments use the current Continuum kernel |
| `demo` | Browser journey, simulated Lighter order book, bots, scenarios, and Solidity tests |
| `contracts/production` | Reference atomic settlement join |
| `patches/lighter-prover` | Apply-ready accumulator overlay for the pinned Lighter prover |
| `docs` | Newcomer guide, proof design, guarantees, specification, and verification guide |
| `upstream` | Pinned upstream revisions and precise Lighter prover integration map |

## Local verification

Run these commands from the repository root:

```bash
cargo test -p continuum-lighter-plugin
cargo +nightly-2025-12-06 test -p continuum-lighter-plugin --features lighter-poseidon2
cargo test -p demo-gateway
cd demo/contracts && forge test
cd ../../contracts/production && forge test
```

A green run verifies the extracted Rust relations, exact pinned Poseidon2
adapter, demo gateway, and Solidity reference contracts.

The plugin uses `Sha256ReferenceHasher` only for deterministic host tests. The
`lighter-poseidon2` feature uses the exact pinned Lighter Plonky2 fork and
field-native execution preimages.

## License

Fermi-owned material without a different notice uses the
[Business Source License 1.1](./licence.md). The VDF crate, MIT-tagged Solidity,
Lighter overlay, dependencies, and vendored material retain their stated
licenses.
