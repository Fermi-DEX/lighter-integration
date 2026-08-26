# PoSq × Lighter: Verifiable Execution, Now With a Verifiable Order

> **Superseded pitch draft.** Keep this beside the demo, but do not send it as
> the production claim. In particular, B3 is not implemented, the current
> bridge does not bind a Lighter state root or execution proof, and “one
> Poseidon2 permutation per tx” was not benchmarked. The corrected completeness
> and pitch gate are in `../lit_improvement_roadmap.md`.

*A two-page brief for the Lighter team, accompanying a live demo.*

---

## The one-sentence thesis

Your whitepaper proves **execution** — price-time-priority matching,
liquidations, funding — in SNARKs on Ethereum, and explicitly names the one
thing it does not yet prove: **the order the sequencer chose** (§2.3's
"millisecond-scale reordering windows"; §8's future work on "transaction
encryption and pre-commitment schemes … fair sequencing"). Proof of Sequence
(PoSq) is exactly that missing half — a VDF-clocked, blindly-admitted,
receipted, on-chain-slashable order — and because Lighter is a ZK system, the
two compose into a **single validity proof of execution *and* order
adherence** that no consensus or optimistic chain can match.

## Why it fits Lighter specifically (not a generic perps chain)

In Lighter, time priority is not a wall-clock timestamp — it is the
per-market, per-side **order nonce**, assigned in execution order and baked
into the Order Book Tree leaf index `I = p·2^O + o` (whitepaper §3.7.1). Your
circuits already prove the whole state transition is a deterministic function
of the transaction sequence. So *every* fairness property collapses to one
binding:

> **the batch's transaction stream = the opened prefix of the PoSq tape.**

Enforce that single stream-equality — optimistically behind a fraud proof
first, then folded into the proof you already generate — and the circuits you
already ship transitively prove **PoSq-fair** price-time priority, end to end.
No VDF in-circuit, no timelock in-circuit: the VDF stays in PoSq's on-chain
accountability layer; your circuit proves conformance to a commitment it takes
as public input. Hash alignment is already there — you merkleize with
Poseidon2, and our chains are built to re-genesis onto Poseidon2 for a ZK
backend.

## What PoSq adds, property by property

| Property | Lighter today | With PoSq |
|---|---|---|
| Execution correctness | ✓ validity proof | unchanged — still yours |
| **Order fairness** | ✗ sequencer picks order | ✓ VDF-ticked blind FCFS tape |
| **No last-look / no front-run** | ✗ sequencer sees plaintext | ✓ fixed-base timelock KEM: the sequencer orders ciphertexts |
| **Cancel priority** | ✗ | ✓ *free* — cancels race trades through the same blind FIFO; stale-quote sniping is structurally removed |
| **Accountable admission** | ✗ soft promise | ✓ signed receipt / rejection / full-window — "my order vanished" becomes cryptographic evidence |
| **Ordering slashing** | ✗ | ✓ host-chain bond + native fraud proofs on Ethereum |
| Censorship resistance | priority queue + escape hatch | unified: forced inclusion becomes a *slashing* condition, escape hatch stays as the deeper backstop |

The invariant we hold throughout: **PoSq failure degrades Lighter to exactly
what Lighter is today — never below it.** The integration is strictly additive
in trust.

## What the demo shows (live, on Sepolia)

A real PoSq sequencer runs with a Lighter-style order book (real order nonces),
driven by blind encrypted order flow. The dashboard re-verifies everything **in
your browser** — signatures, hash chains, merkle roots, the stream commitment —
and anchors land on **Ethereum Sepolia** with native VDF proof verification:

- **Front-run fails**: the same flow under PoSq blind FIFO vs plaintext-FCFS;
  the victim's fill is strictly better under PoSq.
- **Cancel priority**: a maker's cancel wins its race through the blind queue;
  nothing to snipe.
- **Accountable admission**: forced rejections/full-windows arrive as signed
  objects; the page recovers the sequencer's address client-side.
- **On-chain fraud + slashing**: a malicious reorder is caught by
  `PoSqHost.proveReorder` on Sepolia — bond slashed, event emitted, Etherscan
  link. (Against a sacrificial contract; the main demo keeps running.)
- **Native Wesolowski verification**: Ethereum itself verifies a live VDF
  segment proof via the modexp precompile + a 2048-bit mulmod library.
- **Stream equality**: flip one transaction byte and the batch's stream
  commitment diverges — any batch ≠ tape is detectable.

## The path (each step subsumes the last)

- **B0 — attested pilot**: your sequencer defers ordering to the PoSq tape;
  receipts become the order-ack. No circuit or contract change. *Runnable
  today.*
- **B1 — co-anchored**: each Ethereum state-update proposal binds to a PoSq
  anchor span.
- **B2 — stream equality, optimistic**: withdrawals gate on a challenge window;
  any reorder/omission is an on-chain fraud proof against the PoSq bond.
- **B3 — validity-proven**: fold the stream-equality constraint (~one Poseidon2
  permutation per tx) plus a `TxFailed` cycle outcome into your block circuits;
  the single proof now attests execution **and** order. The fraud game retires.

The only genuinely new Lighter circuit work is at B3, and it is small next to a
matching cycle. Everything before it is adapter + contract glue on our side.

## What we would need from you to go past the demo

1. The internal structure of your block/batch transaction commitment (hash
   chain vs merkle) — the single disclosure the B2 challenge game needs.
2. A sizing pass: the stream constraint's constraint-count against your proving
   budget (far cheaper than VDF-in-SNARK, which we deliberately avoid).
3. A joint liveness/bond-sizing statement (single-sequencer on both sides
   today; forced inclusion + slashing as the escape, not consensus).

Full technical spec: `integrations-v2/lighter-integration.md`. The ordering
layer is formally specified and partially machine-checked in Lean (fraud-proof
soundness and completeness, admission FIFO) — available as a rigor appendix.
