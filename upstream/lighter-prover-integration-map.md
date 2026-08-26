# Lighter prover integration map

This map is pinned to
[`elliottech/lighter-prover@8c01ea0`](https://github.com/elliottech/lighter-prover/tree/8c01ea010d6fd46bdb77ef2f93a79278d1adf0df).
It describes the smallest exact-order change that preserves Lighter's existing
heavy/light proving parallelism.

## Key observation

The prover already gives every active transaction a global `tx_index`.
Heavy and light transactions are proven in separate
`BlockTxChainCircuit` chains. `JumpState` then commits each chain's
continuous runs, gaps, and state-root boundary claims. `BlockCircuit` checks
that those claims cover one coherent global transition.

That machinery can carry the Continuum ordered-item accumulator as another
piece of transition state. This is better than introducing a separate global
serial hash pass.

## Required changes

| Upstream file | Change |
|---|---|
| `circuit/src/tx.rs` | Add the compact `ExecutionItemV3` witness and terminal outcome. |
| `circuit/src/tx_constraints.rs` | For heavy transactions, derive one `ExecutionItemV3` leaf from the existing `tx_hash` and constrain one accumulator transition. |
| `circuit/src/light_tx_constraints.rs` | Apply the identical transition for light transactions. |
| `circuit/src/block_tx.rs` | Extend `JumpState` with prior/new Continuum accumulator state and logical count. |
| `circuit/src/block_tx_constraints.rs` | Include the Continuum state in continuous-run checks and in coverage/claim hashes. Padding leaves it unchanged. |
| `circuit/src/block_constraints.rs` | Stitch heavy/light accumulator boundary claims exactly as existing state and delta roots are stitched. Expose old/new accumulator and count. |
| `circuit/src/recursion/cyclic_circuit.rs` | Enforce accumulator and cursor continuity across blocks/segments. |
| `circuit/src/recursion/batch.rs` | Carry the binding fields into the batch public target. |
| `circuit/src/recursion/wrapper_circuit.rs` | Carry `C_bind`, bind it to blob bytes 2..33, and optionally verify one recursive sequence proof. |
| `circuit/src/blob/constants.rs` | Keep the existing 32-byte reserved area; assign it under a version bump. |
| `circuit/src/blob/blob_constraints.rs` | Bind the version and `C_bind` word into the proved blob data. |
| `circuit/src/recursion/wrapper_circuit.rs::verify_version_and_reserved_data` | Replace the current all-zero assertion with an exact version check and equality to serialized `C_bind`. |

## Hot-path relation

For each active logical input at global index `i`:

```text
E_0 = Poseidon2(INIT_TAG, domain_hash[8×u32], cursor[2×u32], count[2×u32])
E_{i+1} = Poseidon2(
  STEP_TAG,
  E_i[4×Goldilocks],
  i[2×u32],
  tx_type,
  existing_tx_hash[5×Goldilocks],
  outcome_class,
  terminal_noop
)
logical_count_{i+1} = logical_count_i + 1
```

There is intentionally no separately hashed execution leaf. Folding the leaf
fields directly into the accumulator step removes one Poseidon2 invocation per
item. The only decomposition in the per-item hot path is the logical index into
two range-checked 32-bit limbs; Lighter's existing five-field transaction hash
is reused directly.

The sequence proof separately computes the richer `DerivedItemV3` root and
proves a one-to-one projection into these compact execution leaves. `C_bind`
contains both roots. This avoids making Lighter absorb receipt, envelope, and
opening metadata that its transaction circuit does not otherwise use.

The circuit performs the `E` transition once for the logical input, not once
per internal matching cycle. The old/new `E` values travel with the
same jump boundaries as old/new Lighter state roots.

A `BAD_AEAD` or `BAD_ENCODING` item uses a dedicated terminal transaction
selector. It advances `D` and the logical count, consumes the global
`tx_index`, leaves Lighter state and API nonce unchanged, and proves the
typed reason. Availability and solver failures have no selector: they stall
the sequence proof and cannot become no-ops.

## Why this is the preferred construction

- It is deterministic and collision-resistant under the same pinned Poseidon2
  assumption as the rest of the execution proof.
- It preserves heavy/light transaction-chain parallelism.
- It reuses the prover's existing global-index gap and boundary machinery.
- It adds one logical-input hash transition plus a small number of connections
  and counters. It does not add RSA, VDF, TLP, Keccak transcript replay, or a
  second full transaction pass to the execution hot path.
- It makes insertion, deletion, duplication, and reordering change the final
  accumulator, while existing global-index coverage prevents hidden gaps.

## Alternatives not selected

A single accumulator threaded directly through the heavy and light chains
would serialize them. A separate merge circuit is exact but re-hashes every
item after the two chain proofs. A two-point grand-product fingerprint is
cheap, but coordinating non-adaptively random challenges between two
independent proofs is a new soundness surface. The JumpState extension is both
exact and cheaper in this architecture.

## Recursive join

Start with independent `pi_seq` and `pi_exec` proofs joined by equal
`C_bind` on Ethereum. Both provers run in parallel.

Only after measurements, add `pi_seq` as one recursive proof target in
`WrapperInnerCircuit`. The wrapper already verifies eight chain proofs, one
delta-chain proof, and one blob-evaluation proof. The recursive variant must
connect the sequence proof's public `C_bind`, cursor, transcript root, and
verifier ID to the execution batch target and blob word. The sequence circuit
and its verifier data remain independently versioned.

If the sequence backend is not directly compatible with the pinned Plonky2
common data, use a narrow adapter proof. Do not import transcript or TLP
verification into every transaction circuit.

## Apply-ready first overlay

[`patches/lighter-prover/0001-continuum-execution-accumulator.patch`](../patches/lighter-prover/0001-continuum-execution-accumulator.patch)
adds the exact field-native gadget and native mutation test to the pinned
prover. CI checks that the patch still applies and compiles at the pinned
revision. The patch is the common review surface for hash semantics; the
upstream-owned `JumpState`, transaction selector, batch-public-input, and blob
wiring in the table above remain the joint integration step with Lighter.
