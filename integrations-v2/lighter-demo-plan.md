# Lighter Integration Demo — Design & Build Plan

> **Historical V1 demo plan.** The demo remains useful and is preserved. Its
> proofs and contracts are not the production V3 settlement boundary.

**Purpose.** A live, self-verifying dashboard demonstrating the PoSq × Lighter
integration (spec: [`lighter-integration.md`](./lighter-integration.md)),
packaged for the Lighter team to evaluate. It shows the integration's key
properties on a real embedded PoSq sequencer with anchors and fraud proofs
landing on **Ethereum Sepolia**.

**Filename note.** Requested as `integrations_v2/lighter_demo_plan.md`; saved
here as `integrations-v2/lighter-demo-plan.md` to match the hyphenated
directory and sibling files (`lighter-integration.md`, `direct-spec.md`).

---

## 1. Design principle

The audience are ZK/exchange engineers; what convinces them is **verifiable
objects, not charts**. Every claim on the dashboard is backed by either (a) a
signed protocol object the browser re-verifies client-side (secp256k1 recover,
keccak hash-links, merkle roots, stream commitments), or (b) a Sepolia
transaction openable on Etherscan. Tagline: *"don't trust this page — it
re-derives everything in your browser."*

---

## 2. Topology

```
 traffic bots ──┐ envelopes (KEM-encrypted Lighter-style intents)
 (makers/takers/│
  cancels)      ▼
        ┌───────────────┐  in-proc  ┌──────────────────┐   SSE/REST   ┌───────────┐
        │ PoSq sequencer │─────────▶│  demo-gateway     │─────────────▶│ dashboard │
        │ (dev profile,  │  tape/    │  + lighter-sim    │              │ (static,  │
        │  calibrated q) │  ticks    │  (adapter R1–R7,  │              │  in-browser│
        └───────┬────────┘           │  toy CLOB, stream │              │  verify)  │
                │ anchors (HostClient)│  commitments)     │              └─────┬─────┘
                ▼                     └────────┬──────────┘                    │ JSON-RPC
        ┌──────────────────── Sepolia ─────────▼──────────────────┐           │ (read-only)
        │  PoSqHost.sol           LighterBridgeDemo.sol           │◀──────────┘
        │  (bond, anchors,        (proposeSpan: batch ↔ tick      │
        │   fraud proofs,          span ↔ anchor, stream commit)  │
        │   native Wesolowski)    BigMulMod.sol (Yul 2048-bit)    │
        └──────────────────────────────────────────────────────────┘
```

All new code lives under `demo/`. The sequencer, VDF, timelock KEM, records,
anchors, fraud proofs, and `PoSqHost.sol` are the **existing repo
implementation** — the demo embeds them, it does not reimplement them.

---

## 3. Components

### 3.1 `demo/gateway` (Rust, workspace crate) — DONE / in progress

- **Embedded node**: a real `SequencerNode` (dev profile, `q` calibrated at
  boot so a tick ≈ Δ on the host). Bounded per epoch; an epoch supervisor
  rotates (also bounds memory).
- **lighter-sim adapter** (`lighter_sim.rs`): consumes the tape strictly in
  `(t, j)` order (R3 head-blocking), assembles fixed tick-range blocks (R1),
  applies the validity split (R4: stateless-invalid ⇒ gap, stateful-fail ⇒
  recorded `Failed` tx), maintains the opened-stream commitment (spec §5/B2),
  and runs a toy price-time-priority CLOB whose **order nonces are the real
  Lighter priority mechanism** (whitepaper §3.7.1).
- **Traffic bots** (`bots.rs`): makers (place → cancel → replace, so cancels
  race through the blind FIFO) and takers, all via the KEM envelope path — the
  tape sees only ciphertexts.
- **HostClient poster** (`main.rs` + `eth.rs`): records every anchor locally
  and (when configured) forwards it to Sepolia via `PoSqHost.submitAnchor`,
  throttled to a policy cadence.
- **API** (`api.rs`): REST for params/status/tape/ticks/blocks/book/trades/
  submits/anchors + `/api/events` SSE. Byte-faithful hex encodings
  (`dto.rs`) so the page verifies client-side.
- **Scenarios** (`scenarios.rs`): the six scripted demos (§5 below).

### 3.2 `demo/contracts` (Solidity + Foundry) — Opus subagent

- **`BigMulMod.sol`**: the `IBigMulMod` implementation `PoSqHost` needs —
  2048-bit modular multiplication in Yul limb arithmetic. Enables **on-chain
  native Wesolowski verification** (`PoSqHost.verifySegmentProof`) — the
  single strongest artifact for a ZK audience: *Ethereum verifying a live VDF
  segment proof*.
- **`LighterBridgeDemo.sol`**: `proposeSpan(batchId, epoch, firstTick,
  lastTick, anchorId, streamCommitment)` binding a Lighter batch to a
  `PoSqHost` anchor span (spec §6, B1); stores the stream commitment for the
  challenge window.
- **Foundry**: unit tests incl. **differential tests** of `mulmod2048` against
  Rust `num-bigint`; a test that verifies a real segment proof emitted by the
  VDF crate; deploy scripts for Sepolia (main `PoSqHost`, a **sacrificial**
  `PoSqHost` whose configured sequencer is the demo's malicious key, and the
  bridge).

### 3.3 `demo/gateway/static` (single static page) — Opus subagent

Vendored `noble-secp256k1` + keccak (no CDN). Panels:

1. **Clock & tape** — tick/segment counters, cadence sparkline, segment seals
   with Wesolowski status; the raw tape scrolling `(t,j): pending → opened /
   gap:<reason>`.
2. **Blind admission** — envelope inspector (39-byte visible header vs opaque
   ciphertext, then what it opens to); receipt-latency histogram; each
   outcome as its signed object with **signature recovered in the browser**
   and the digest-chain link recomputed live.
3. **Tape → Lighter** — the order book with visible order nonces (Lighter's
   priority mechanism), block-by-block `(t,j) ↔ (block, txIndex)` map, gaps
   preserved, and the **stream commitment recomputed in-browser** and matched
   against the posted one.
4. **Ethereum** — `PoSqHost`/bridge state read directly from Sepolia by the
   browser (bond, anchors table, forced queue, fraud log, per-op gas), with
   Etherscan links.
5. **Scenarios** — the six buttons.
6. **Scorecard** — the spec's "who supplies what" table, each row badged with
   live evidence.

Load the `dataviz` skill before any chart work.

---

## 4. Ethereum footprint & decisions (approved)

- **Network**: Sepolia. Fresh keypair generated
  (`0x60eB5b83B1B46537C66B7f2Bb610468E73090C5b`); user funds via faucet;
  public Sepolia RPC.
- **On-chain Wesolowski verification**: YES — build the `BigMulMod` Yul lib in
  v1.
- **Hosting**: both — a live instance from this box **and** a self-contained
  `docker compose up` package.
- **Anchor cadence**: throttled (≈1 bundle/min) to keep faucet ETH sufficient.

---

## 5. Scripted scenarios (the demo's spine)

1. **Front-run fails** — same flow under PoSq blind FIFO vs plaintext-FCFS;
   victim's average fill is strictly better under PoSq.
2. **Cancel priority** — maker cancel races a taker through the blind FIFO;
   stale-quote sniping structurally dead (the CLOB headline).
3. **Accountable admission** — force a rejection/full-window; both are signed,
   browser-verifiable objects; "my order vanished" is impossible.
4. **Fraud + slashing, on-chain** — malicious-sequencer reorder → gateway
   builds `proveReorder` calldata → submitted to the **sacrificial** PoSqHost
   → bond slashed, `RescueMode` event, Etherscan link. Main host untouched.
5. **Forced inclusion** — `forceInclude(h)` → `dischargeForced` within
   `F_force`, else `proveForcedDefault` slashes.
6. **Stream mismatch** — flip one tx byte, show the stream commitment diverges;
   any batch ≠ tape is detectable (B2 challenge).

---

## 6. Package sent to Lighter

`demo/README.md` (run it yourself: `docker compose up`), the spec
(`lighter-integration.md`), a 2-page executive brief for their team (their
whitepaper §8 roadmap → delivered), the live dashboard URL + Etherscan links
(PoSqHost, bridge, a real slashing tx), and the formal-verification note
(Lean-proved fraud soundness/completeness, D001) as a rigor signal.

---

## 7. Build order & workflow

Opus subagents parallelize the two large independent chunks (contracts,
dashboard) while the main thread finishes the gateway and integrates.

| Phase | Deliverable | Owner |
|---|---|---|
| P1 | gateway crate: adapter, bots, API, scenarios, compiles + tests green | main |
| P2 | contracts: BigMulMod Yul + bridge + foundry tests + deploy scripts | Opus subagent |
| P3 | dashboard: all six panels, in-browser verification | Opus subagent |
| P4 | Sepolia deploy (fund → deploy → wire poster → verify a segment proof on-chain) | main + subagent output |
| P5 | package: README, executive brief, docker-compose, launch live instance | main |

**What's needed from the user:** fund the Sepolia address above; a hosting
port/domain to expose the live instance.

---

## 8. Status log

- [x] Spec reconciled and rewritten (`lighter-integration.md`).
- [x] Keypair generated (`0x60eB5b83B1B46537C66B7f2Bb610468E73090C5b`),
      stored at `demo/.sepolia-key` (gitignored); **funded ≈0.05 ETH**
      (checked 2026-07-06).
- [x] Foundry installed.
- [x] `demo/gateway` crate: adapter, bots, dto, api, scenarios, main, eth —
      **compiles, unit tests green, verified live** (70 admitted @ p50 215µs,
      64 opened via timelock solver, trades through the CLOB, 253 blocks,
      anchors with stream commitments, all six scenarios return correctly,
      Sepolia RPC reachable). No-look invariant enforced (delay menu derived
      from calibrated q).
- [x] Contracts: BigMulMod Yul + bridge + foundry tests + deploy scripts —
      **44 tests green** (BigMulMod KATs, 66 differential mulmod vectors vs a
      Python big-int reference, real Wesolowski segment proof verified
      on-chain-style at **~452k gas** incl. negatives, LighterBridgeDemo full
      branch coverage, fraud paths proveReorder/proveEquivocation/
      proveInvalidLog + forced-inclusion lifecycle with genuinely signed
      records, `script/Deploy.s.sol` main+sacrificial hosts + bridge, dry-run
      clean, writes `deployments/sepolia.json`).
- [x] Dashboard v1: six panels + in-browser secp256k1/keccak/merkle/stream
      verification (`static/index.html,app.js,verify.js,viz.js,eth.js,util.js,
      style.css`) — node checker `vendor/pagecheck.cjs` 17/17 against the live
      gateway. Follow-up pass (Opus subagent, running): wire the new
      verification surface below.
- [x] **Review fixes (main thread)**: (1) **real contract bug found & fixed** —
      the Wesolowski challenge candidate is the double hash
      `sha256(sha256(preimage))` (Rust `challenge_prime` → `hash_to_prime`
      hashes twice); both `PoSqHost.sol` copies + the vector generator used a
      single hash, so live proofs would have been rejected on-chain. Fixed in
      `contracts/PoSqHost.sol` + `demo/contracts/src/PoSqHost.sol`, vector
      regenerated, 44/44 green, and a **live** gateway proof from
      `/api/segments` verified against the fixed contract. (2) API gaps:
      `envelope_hex` on submits, `/api/segments` (full y/x_end/pi/t/l, contract
      encoding), `modulus_n` on params, full `submitAnchor` calldata + poster
      path in `eth.rs`, `DEMO_Q` pin for on-chain parity, `stream-mismatch`
      scenario fixed (non-empty block + correct pre-state + self-check).
      `demo/contracts/README.md` written.
- [x] Package drafts: `demo/README.md`, `demo/EXECUTIVE-BRIEF.md`,
      `docker-compose.yml`, `Dockerfile`, `.gitignore`.
- [x] **Sepolia deployed & tested (2026-07-06)**: PoSqHost main
      `0xc9ec12bda232160fa9d6cadE81C37357dA6d0809` (bond 0.01 ETH, sequencer
      0x1220d3…1d82), sacrificial `0xf1cD3a61CF82C6D09eB3Aefa0d4F15b98BEe8714`
      (bond 0.005, malicious 0xd51164…3fbb), bridge
      `0xAD000Ff52DD15d38b6580d04a860e355e918EA5e`, BigMulMod
      `0xEBB70b5945D5112394323b7cb1d332Fc4b486480`; epoch 0. On-chain test:
      `verifySegmentProof` true for the real vector AND for a **live** segment
      from the running gateway; corrupted π → false; state reads correct.
- [x] **Live instance**: screen session `posq-lighter-demo`, gateway with
      `DEMO_Q=3600 DEMO_SEGMENT_TICKS=256` + deployed addresses, anchor poster
      every 1800s; nginx site `posq-lighter-demo` on **:8088** →
      127.0.0.1:8080 (SSE-safe), ufw 8088/tcp allowed. External:
      http://35.204.78.82:8088 (GCP-level firewall unverifiable from the box —
      instance tag `web-server`).
- [~] **Standalone repo `posq-lighter-demo`**: staged + committed at
      `~/stagin4/posq-lighter-demo` (contracts w/ vendored forge-std, 44 tests
      green in-place; frontend nginx docker package defaulting to the hosted
      gateway; spec + brief + README with Etherscan links). `gh repo create
      --push` blocked by the permission classifier — needs user approval.
- [ ] Launch live instance (needs a port/domain to expose).
