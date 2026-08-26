# Demo contracts — PoSq × Lighter

Foundry project backing the demo's Ethereum edge.

| Contract | Purpose |
|---|---|
| `src/PoSqHost.sol` | anchors, **native Wesolowski segment-proof verification** (~452k gas), fraud proofs (equivocation / reorder / omission / invalid-log), forced inclusion, bond slashing |
| `src/BigMulMod.sol` | 2048-bit modular multiplication in Yul limb arithmetic (`IBigMulMod` for the host — no EVM precompile exists for wide mulmod) |
| `src/LighterBridgeDemo.sol` | `proposeSpan`: binds a Lighter batch to a PoSqHost anchor span + stream commitment, with a challenge window |

## Build & test

```bash
~/.foundry/bin/forge build
~/.foundry/bin/forge test   # 44 tests, 5 suites
```

Test highlights:

- `PoSqHostSegmentProof.t.sol` — a **real** Wesolowski proof
  (`vectors/segment.json`, reproducible via `vectors/gen_segment_vector.py`)
  verifies on-chain; corrupt-proof / wrong-T / wrong-output negatives.
- `BigMulModDiff.t.sol` — 66 differential vectors against a Python big-int
  reference (`vectors/gen_mulmod_vectors.py`).
- `PoSqHostFraud.t.sol` — fraud paths driven with genuinely signed records
  (test key), incl. `proveReorder` slash + bounty and the forced-inclusion
  lifecycle.
- `LighterBridgeDemo.t.sol` — full branch coverage against a real host with a
  signed anchor.

**Challenge-derivation note.** The Wesolowski challenge candidate is the
*double* hash `sha256(sha256(preimage))`: Rust `challenge_prime` hashes the
preimage, then `hash_to_prime` hashes that digest again before the
increment-to-prime walk. The contract and the vector generator both reproduce
this exactly — a live gateway proof from `/api/segments` verifies on-chain
unmodified.

## Deploy (Sepolia)

Deploys `BigMulMod` → main `PoSqHost` (honest sequencer, funded bond) →
**sacrificial** `PoSqHost` (malicious sequencer — its bond gets slashed in the
live demo) → `LighterBridgeDemo`. Addresses are logged and written to
`deployments/sepolia.json`.

```bash
SEQ_ADDR=0x…        # gateway sequencer address (/api/params sequencer_address)
MALICIOUS_ADDR=0x…  # demo malicious key (fraud-reorder scenario malicious_sequencer)
BOND_WEI=10000000000000000 SAC_BOND_WEI=5000000000000000 EPOCH_ID=0 \
~/.foundry/bin/forge script script/Deploy.s.sol:Deploy \
  --rpc-url https://ethereum-sepolia-rpc.publicnode.com \
  --private-key $(cat ../.sepolia-key) \
  --broadcast
```

Drop `--broadcast` for a dry run. The deployer key funds both bonds, so it
must hold ≥ `BOND_WEI + SAC_BOND_WEI + gas`.

Constraints to keep straight:

- `EPOCH_ID` must equal the epoch the live gateway signs anchors with
  (`submitAnchor` recovers it from the signature); a fresh gateway boots into
  epoch 0.
- The host is deployed with `q=3600`, `segmentTicks=256`; run the live
  gateway with `DEMO_Q=3600 DEMO_SEGMENT_TICKS=256` so its proofs pass the
  host's `t == q · segmentTicks` check.
