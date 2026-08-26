# PoSq × Lighter Integration Demo

> **Historical V1 demo.** This remains the runnable bridge and pitch
> visualization. It demonstrates ordering observability, browser
> self-verification, and optimistic fraud/slashing paths. It does **not** bind
> a production Lighter state transition to a validity-proven Continuum
> transcript. See `../lit_improvement_roadmap.md` for the production boundary.

A live, self-verifying demonstration that binds **Lighter's verifiable
execution** to **Proof of Sequence's verifiable order**. A real PoSq sequencer
runs a Lighter-style order book (with real per-side order nonces — Lighter's
actual price-time-priority mechanism), driven by blind, timelock-encrypted
order flow. A browser dashboard re-derives every signed object client-side, and
anchors + fraud proofs land on **Ethereum Sepolia** with native VDF proof
verification.

- **What it argues:** [`EXECUTIVE-BRIEF.md`](./EXECUTIVE-BRIEF.md) (for the
  Lighter team).
- **How it works:** [`../integrations-v2/lighter-integration.md`](../integrations-v2/lighter-integration.md)
  (the technical spec) and [`../integrations-v2/lighter-demo-plan.md`](../integrations-v2/lighter-demo-plan.md)
  (this demo's design).

> **Design principle:** don't trust the dashboard — it re-derives everything in
> your browser (secp256k1 recover over keccak256, hash-chain links, merkle
> roots, the batch↔tape stream commitment) and links every on-chain claim to
> Etherscan.

---

## Quick start (local, no Ethereum)

```bash
# from the repo root
cargo run -p demo-gateway --release
# → dashboard on http://0.0.0.0:8080
```

That's it. The gateway embeds a real `SequencerNode` (dev profile, `q`
calibrated to your CPU so a tick ≈ Δ), runs the lighter-sim ingest adapter over
its tape, drives blind maker/taker/cancel bots, and serves the dashboard +
REST/SSE API. In local mode the Ethereum panel shows "local-only"; everything
else is fully live.

### With Docker

```bash
cd demo
docker compose up
# → dashboard on http://localhost:8080
```

---

## Enabling the Sepolia edge (anchors + on-chain fraud proofs)

1. **Deploy the contracts** (see [`contracts/README.md`](./contracts/README.md)):
   `BigMulMod` (2048-bit modular multiply, for native Wesolowski verification),
   a main `PoSqHost`, a **sacrificial** `PoSqHost` (for the slashing demo), and
   `LighterBridgeDemo`.
2. **Fund the poster key.** The demo uses a throwaway Sepolia key in
   `demo/.sepolia-key` (gitignored). Its address is printed at boot; fund it
   from any Sepolia faucet.
3. **Point the gateway at the deployment** via environment variables:

```bash
export DEMO_ETH_RPC="https://ethereum-sepolia-rpc.publicnode.com"
export DEMO_ETH_KEY_FILE="demo/.sepolia-key"
export DEMO_POSQ_HOST="0x…"              # main host
export DEMO_POSQ_HOST_SACRIFICIAL="0x…"  # slashing-demo host
export DEMO_BRIDGE="0x…"                  # LighterBridgeDemo
# On-chain parity: PoSqHost checks t == q · segmentTicks, so the live
# instance must run the exact (q, segmentTicks) the host was deployed with.
export DEMO_Q=3600
export DEMO_SEGMENT_TICKS=256
cargo run -p demo-gateway --release
```

The Ethereum panel then shows chain id, poster balance, live anchor/span
postings, and Etherscan links.

---

## Configuration (environment)

| Var | Default | Meaning |
|---|---|---|
| `DEMO_HTTP_ADDR` | `0.0.0.0:8080` | dashboard bind address |
| `DEMO_DELTA_US` | `10000` | target tick duration µs (Δ); `q` is calibrated to it |
| `DEMO_Q` | unset | pin `q` instead of calibrating (required for on-chain segment-proof parity; the deployed host was configured with `q=3600`, `segmentTicks=256`) |
| `DEMO_SEGMENT_TICKS` | `128` | ticks per segment |
| `DEMO_BLOCK_TICKS` | `8` | PoSq ticks per Lighter block |
| `DEMO_ANCHOR_EVERY` | `4` | segments per anchor |
| `DEMO_WINDOW_TICKS` | `12` | inclusion window length |
| `DEMO_EPOCH_SEGMENTS` | `2800` | segments before epoch rotation (bounds memory) |
| `DEMO_ETH_RPC` | publicnode Sepolia | JSON-RPC endpoint |
| `DEMO_POSQ_HOST` / `_SACRIFICIAL` / `DEMO_BRIDGE` | unset | deployed addresses (enables the Ethereum edge) |
| `DEMO_ANCHOR_POST_SECS` | `60` | anchor/span posting throttle |

---

## What's in the box

```
demo/
  gateway/          Rust: embedded sequencer + lighter-sim adapter + bots + API
    src/
      lighter_sim.rs  the ingest adapter (spec §4 R1–R7) + toy price-time CLOB
      bots.rs         blind maker/taker/cancel traffic
      scenarios.rs    the six scripted demos (incl. on-chain fraud calldata)
      api.rs          REST + SSE
      dto.rs          byte-faithful hex encodings for in-browser verification
      eth.rs          Sepolia anchor/span poster (via foundry `cast`)
      main.rs         epoch supervisor wiring it together
    static/         the dashboard (self-contained, offline-capable)
  contracts/        Solidity + Foundry: BigMulMod, LighterBridgeDemo, deploy
  EXECUTIVE-BRIEF.md
  README.md
  docker-compose.yml
```

The sequencer, VDF, timelock KEM, records, anchors, fraud proofs, and
`PoSqHost.sol` are the **existing repository implementation**. The demo embeds
them. The Lighter execution path is a simulated price-time order book, and the
dashboard labels it as a demonstration surface.

---

## The six scenarios

Click them in the dashboard, or `curl -X POST localhost:8080/api/scenario/<name>`:

| Name | Shows |
|---|---|
| `frontrun` | front-run fails under blind admission (vs plaintext-FCFS) |
| `cancel-priority` | stale-quote sniping structurally removed |
| `accountable` | signed rejection, verified in-browser (no silent outcomes) |
| `fraud-reorder` | `proveReorder` calldata → slash the sacrificial host on Sepolia |
| `forced-inclusion` | censorship resistance as a slashing condition |
| `stream-mismatch` | any batch ≠ tape changes the stream commitment |

---

## Tests

```bash
cargo test -p demo-gateway --bin demo-gateway   # adapter/CLOB/stream tests
cd contracts && forge test                        # mulmod + segment-proof + bridge
```
