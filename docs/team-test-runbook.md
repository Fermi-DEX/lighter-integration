# Lighter team test runbook

This runbook separates what can be executed today from the final upstream
prover wiring. A green run means the extracted demo, V3.1 host relation,
Poseidon2 preimages, pinned accumulator gadget, and atomic settlement join are
internally consistent. It does not mean the production Lighter wrapper already
enforces Continuum validity.

## 1. Prerequisites

- Rust stable for the demo and source-independent host tests.
- Rust `nightly-2025-12-06` for Lighter's pinned Plonky2 fork.
- Foundry for the demo and production-reference Solidity tests.
- Git for applying the pinned upstream overlay.

The GitHub Actions workflow installs these toolchains and runs every command
below on a clean runner.

## 2. Host and demo tests

From the repository root:

```bash
cargo test -p continuum-lighter-plugin
cargo test -p demo-gateway -p sequencer -p vdf
```

The plugin tests cover canonical ordering, exact logical counts, terminal
no-ops, receipt and envelope duplication, cursor/config continuity, both
stream roots, `C_bind`, and an end-to-end sequence-to-settlement mutation.

## 3. Exact Lighter Poseidon2 adapter

```bash
cargo +nightly-2025-12-06 test \
  -p continuum-lighter-plugin \
  --features lighter-poseidon2
```

This feature pins Lighter's Plonky2 fork at
`e1c2d35450948b88fca6a7e69e2643c3ecad3caa`. The compact stream is
field-native: the previous four-field digest and existing five-field Lighter
transaction hash enter Poseidon2 directly. Each item uses one tagged hash
step; only the 64-bit logical index is split into two 32-bit limbs.

## 4. Atomic settlement tests

```bash
(cd demo/contracts && forge test)
(cd contracts/production && forge test)
```

The production-reference contract calls two pinned-verifier interfaces before
changing any head. Tests show that an invalid proof, unequal `C_bind`, either
root mismatch, count mismatch, or continuity failure reverts the whole update
and leaves the pending batch available.

## 5. Apply the Lighter overlay

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

The overlay adds the exact native and circuit gadget plus an order/terminal
mutation test. It retains Lighter's BUSL-1.1 header and copies no unrelated
upstream source.

## 6. Joint branch test

The first Lighter-owned branch should wire the included gadget through the
touch points in
[`upstream/lighter-prover-integration-map.md`](../upstream/lighter-prover-integration-map.md):

1. derive one compact item from each real transaction path;
2. add the terminal no-op selector for `BAD_AEAD` and `BAD_ENCODING`;
3. carry old/new root and logical count through heavy/light `JumpState`;
4. stitch those claims in the block circuit and recursion layers;
5. expose root/count/`C_bind` through batch and wrapper public inputs; and
6. version blob bytes 0..33 and constrain the reserved word to `C_bind`.

The branch must test mixed heavy/light streams ending in heavy, light,
terminal, and empty boundaries. Insert, delete, duplicate, reorder, index-skip,
outcome mutation, and padding-as-input cases must all fail.

## 7. Recursion decision

Use recursive segment aggregation inside the sequence prover from the first
scalable implementation. Keep the sequence and execution proofs independent
at settlement until benchmarks exist. Recursing the final sequence proof into
Lighter's wrapper is optional and should be adopted only if it lowers total
gas/calldata without materially increasing p95 latency or memory.

The acceptance metrics and honest pitch boundary are in
[`lit_improvement_roadmap.md`](../lit_improvement_roadmap.md).
