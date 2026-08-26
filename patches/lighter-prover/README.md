# Pinned Lighter prover overlay

This patch targets exactly
`elliottech/lighter-prover@8c01ea010d6fd46bdb77ef2f93a79278d1adf0df`
and retains the upstream BUSL-1.1 header.

It adds the field-native Poseidon2 accumulator gadget and its native mutation
test. It does **not** claim to be the final transaction-dispatch integration.
The remaining upstream-owned wiring is deliberately isolated in
[`upstream/lighter-prover-integration-map.md`](../../upstream/lighter-prover-integration-map.md):
thread the old/new accumulator through `JumpState`, add the terminal selector,
and expose the final root/count through batch and wrapper public inputs.

Apply and test:

```bash
git clone https://github.com/elliottech/lighter-prover
cd lighter-prover
git checkout 8c01ea010d6fd46bdb77ef2f93a79278d1adf0df
git apply --check ../0001-continuum-execution-accumulator.patch
git apply ../0001-continuum-execution-accumulator.patch
cargo +nightly-2025-12-06 test -p circuit continuum --lib
```

The root crate's `lighter-poseidon2` feature uses the same pinned Plonky2 fork
and the same field preimages, so the overlay and plugin can share exact vectors.
