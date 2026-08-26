# Lighter team test runbook

This runbook verifies how protected Continuum order connects to Lighter
execution. The sequence relation and execution relation meet through equal
`C_bind` commitments.

A green run verifies five reference boundaries:

- Rust verifies ordered-stream construction, continuity, VDF, timelock, and
  demo behavior.
- The Poseidon2 adapter verifies exact field preimages against Lighter's pinned
  Plonky2 fork.
- The overlay applies to the pinned Lighter prover and passes its native
  mutation test.
- Solidity verifies atomic rejection for proof, root, count, cursor, and
  `C_bind` mismatches.
- Adversarial tests verify rejection of changed or reordered protected inputs.

These results verify reference relations and mock-verifier settlement. Live enforcement also requires deployed sequence-proof and Lighter-proof verifiers.

## 1. Prerequisites

- Install Rust stable for the demo and source-independent host tests.
- Install Rust `nightly-2025-12-06` for Lighter's pinned Plonky2 fork.
- Install Foundry for the demo and production-reference Solidity tests.
- Install Git to apply the pinned upstream overlay.

The GitHub Actions workflow installs these tools on clean runners. It leaves
the wall-clock benchmark disabled.

## 2. Host and demo tests

From the repository root, run:

```bash
cargo test -p continuum-lighter-plugin
cargo test -p demo-gateway -p sequencer -p vdf
```

The plugin tests cover canonical ordering, exact logical counts, terminal
no-ops, duplicate receipts, duplicate envelopes, and cursor and configuration
continuity. They also cover both stream roots, `C_bind`, and an end-to-end
sequence-to-settlement mutation.

They also cover canonical malformed-payload fields. `BAD_ENCODING` binds
recovered bytes. `BAD_AEAD` rejects every nonzero cleartext hash.

Generic CI ignores the inherited `test_performance_improvement` VDF wall-clock
test. Its 100 ms threshold depends on the runner class.

On the pinned benchmark host, run:

```bash
cargo test -p vdf test_performance_improvement -- --ignored
```

## 3. Exact Lighter Poseidon2 adapter

Run:

```bash
cargo +nightly-2025-12-06 test \
  -p continuum-lighter-plugin \
  --features lighter-poseidon2
```

This feature pins Lighter's Plonky2 fork at
`e1c2d35450948b88fca6a7e69e2643c3ecad3caa`. The compact stream uses native
field values. Poseidon2 receives the previous four-field digest and the
existing five-field Lighter transaction hash directly.

Each item uses one tagged hash step. Only the 64-bit logical index splits into
two 32-bit limbs.

## 4. Atomic settlement tests

Run:

```bash
(cd demo/contracts && \
  forge install foundry-rs/forge-std@v1.9.7 --no-git --shallow)
(cd demo/contracts && forge test)
(cd contracts/production && forge test)
```

The production-reference contract calls two pinned-verifier interfaces before
it changes any head. Tests verify atomic rejection for an invalid proof,
unequal `C_bind`, either root mismatch, count mismatch, or continuity failure.
The rejection leaves the pending batch available.

## 5. Apply the Lighter overlay

Run:

```bash
git clone https://github.com/elliottech/lighter-prover.git /tmp/lighter-prover
git -C /tmp/lighter-prover checkout \
  8c01ea010d6fd46bdb77ef2f93a79278d1adf0df
git -C /tmp/lighter-prover apply --check \
  "$PWD/patches/lighter-prover/0001-continuum-execution-accumulator.patch"
git -C /tmp/lighter-prover apply \
  "$PWD/patches/lighter-prover/0001-continuum-execution-accumulator.patch"
(cd /tmp/lighter-prover && \
  cargo +nightly-2025-12-06 test -p circuit continuum --lib)
```

The overlay adds the exact native gadget, circuit gadget, and order and
terminal mutation test. It retains Lighter's BUSL-1.1 header. It copies no
unrelated upstream source.
