// eth.js — read PoSqHost + LighterBridgeDemo state directly from Sepolia in the
// browser via JSON-RPC (eth_call / eth_getLogs / eth_getTransactionReceipt).
// No web3 dependency: function selectors are keccak256 (from vendor/crypto.js)
// and the ABI here is all static 32-byte words, so encode/decode is trivial.
// Degrades to a clear "not configured / not yet deployed" state.

const K = () => globalThis.PosqCrypto;

let RPC = null;
let reqId = 1;

export function setRpc(url) {
  RPC = url;
}

async function rpcTo(url, method, params) {
  const r = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: reqId++, method, params }),
  });
  const j = await r.json();
  if (j.error) throw new Error(method + ": " + (j.error.message || JSON.stringify(j.error)));
  return j.result;
}

async function rpc(method, params) {
  if (!RPC) throw new Error("no RPC configured");
  return rpcTo(RPC, method, params);
}

const selCache = {};
function selector(sig) {
  if (!selCache[sig]) selCache[sig] = K().bytesToHex(K().keccak256(K().ascii(sig))).slice(0, 10);
  return selCache[sig];
}
function topic(sig) {
  return K().bytesToHex(K().keccak256(K().ascii(sig)));
}
const word = (hexNo0x) => "0".repeat(64);
function padWord(n) {
  let h = (typeof n === "bigint" ? n : BigInt(n)).toString(16);
  return "0".repeat(64 - h.length) + h;
}
function call(to, sig, argWords) {
  const data = selector(sig) + (argWords || []).join("");
  return rpc("eth_call", [{ to, data }, "latest"]);
}
// slice a 0x return into 32-byte words
function words(ret) {
  const h = (ret || "0x").replace(/^0x/, "");
  const out = [];
  for (let i = 0; i < h.length; i += 64) out.push(h.slice(i, i + 64));
  return out;
}
const asUint = (w) => (w ? BigInt("0x" + w) : 0n);
const asAddr = (w) => "0x" + (w || word()).slice(24);
const asBytes32 = (w) => "0x" + (w || word());
const asBool = (w) => asUint(w) !== 0n;

export async function chainId() {
  return Number(asUint(words(await rpc("eth_chainId", []))[0]));
}
export async function balanceEth(addr) {
  const wei = asUint(words(await rpc("eth_getBalance", [addr, "latest"]))[0]);
  return (Number(wei) / 1e18).toFixed(6);
}

// Free RPCs cripple eth_getLogs (publicnode rejects it outright as "archive";
// drpc caps ranges at 10k blocks), so logs go to a getLogs-capable endpoint in
// ≤9500-block chunks from just before the demo suite's deployment, cached per
// (address, event) for the session. eth_call stays on the configured RPC.
const LOGS_RPC = "https://sepolia.drpc.org";
const DEPLOY_BLOCK = 11218000; // demo contracts deployed at ~11,218,148
const logsCache = new Map();

async function recentLogs(addr, signature) {
  const key = addr + "|" + signature;
  const hit = logsCache.get(key);
  if (hit && Date.now() - hit.at < 60_000) return hit.logs;
  const latest = parseInt(await rpcTo(LOGS_RPC, "eth_blockNumber", []), 16);
  const logs = [];
  for (let from = DEPLOY_BLOCK; from <= latest; from += 9500) {
    const to = Math.min(from + 9499, latest);
    const params = [{ address: addr, fromBlock: "0x" + from.toString(16), toBlock: "0x" + to.toString(16), topics: [topic(signature)] }];
    let chunk;
    try {
      chunk = await rpcTo(LOGS_RPC, "eth_getLogs", params);
    } catch (_) {
      // free-tier rate limit: back off once, then give up on this chunk
      await new Promise((r) => setTimeout(r, 800));
      chunk = await rpcTo(LOGS_RPC, "eth_getLogs", params);
    }
    for (const l of chunk || []) logs.push(l);
  }
  logsCache.set(key, { at: Date.now(), logs });
  return logs;
}

// Map "id (indexed topic1) -> transactionHash" for an event, so on-chain rows
// can link to the exact Etherscan tx that created them. Best-effort: returns
// {} if the RPC rejects the log query.
async function eventTxById(addr, signature) {
  try {
    const logs = await recentLogs(addr, signature);
    const map = {};
    for (const l of logs || []) map[Number(BigInt(l.topics[1]))] = l.transactionHash;
    return map;
  } catch (_) {
    return {};
  }
}

// Read the whole PoSqHost view surface. Throws on RPC failure.
export async function readHost(addr) {
  const out = { address: addr };
  out.sequencer = asAddr(words(await call(addr, "sequencer()"))[0]);
  out.epoch = Number(asUint(words(await call(addr, "epoch()"))[0]));
  out.bond = asUint(words(await call(addr, "bond()"))[0]);
  out.bondFloor = asUint(words(await call(addr, "bondFloor()"))[0]);
  out.rescueMode = asBool(words(await call(addr, "rescueMode()"))[0]);
  out.challengeWindow = Number(asUint(words(await call(addr, "challengeWindow()"))[0]));
  out.qSquarings = Number(asUint(words(await call(addr, "qSquarings()"))[0]));
  out.segmentTicks = Number(asUint(words(await call(addr, "segmentTicks()"))[0]));
  out.fForce = Number(asUint(words(await call(addr, "fForce()"))[0]));
  // anchors[] — iterate until the getter reverts (past end).
  out.anchors = [];
  for (let i = 0; i < 64; i++) {
    let w;
    try {
      w = words(await call(addr, "anchors(uint256)", [padWord(i)]));
    } catch (_) {
      break;
    }
    if (w.length < 6) break;
    out.anchors.push({
      id: i,
      firstTick: Number(asUint(w[0])),
      lastTick: Number(asUint(w[1])),
      cB: asBytes32(w[2]),
      receiptsRoot: asBytes32(w[3]),
      acceptedAt: Number(asUint(w[5])),
    });
  }
  // forcedQueue[]
  out.forced = [];
  for (let i = 0; i < 64; i++) {
    let w;
    try {
      w = words(await call(addr, "forcedQueue(uint256)", [padWord(i)]));
    } catch (_) {
      break;
    }
    if (w.length < 3) break;
    out.forced.push({ id: i, h: asBytes32(w[0]), enqueuedAt: Number(asUint(w[1])), discharged: asBool(w[2]) });
  }
  // Etherscan tx per row, recovered from the emitting events (skipped when
  // there are no rows — keeps the chunked log queries off free-tier limits).
  if (out.anchors.length) {
    const anchorTxs = await eventTxById(addr, "AnchorAccepted(uint256,uint64,uint64,bytes32)");
    for (const a of out.anchors) a.tx = anchorTxs[a.id] || null;
  }
  if (out.forced.length) {
    const forcedTxs = await eventTxById(addr, "ForcedEnqueued(uint256,bytes32)");
    for (const f of out.forced) f.tx = forcedTxs[f.id] || null;
  }
  // fraud log via events
  try {
    const logs = await recentLogs(addr, "FraudProven(uint8,address,uint256)");
    out.fraud = (logs || []).map((l) => {
      const d = words(l.data); // [kind, slashedWei]
      return {
        kind: Number(asUint(d[0])),
        slashed: asUint(d[1] || "0"),
        challenger: "0x" + (l.topics[1] || word()).slice(26),
        tx: l.transactionHash,
        block: parseInt(l.blockNumber, 16),
      };
    });
  } catch (_) {
    out.fraud = [];
  }
  return out;
}

export async function readBridge(addr) {
  const out = { address: addr };
  out.spanCount = Number(asUint(words(await call(addr, "spanCount()"))[0]));
  out.spans = [];
  for (let i = 0; i < out.spanCount && i < 64; i++) {
    const w = words(await call(addr, "spans(uint256)", [padWord(i)]));
    out.spans.push({
      id: i,
      epoch: Number(asUint(w[0])),
      firstTick: Number(asUint(w[1])),
      lastTick: Number(asUint(w[2])),
      cB: asBytes32(w[4]),
      streamCommitment: asBytes32(w[5]),
      anchorId: Number(asUint(w[6])),
      rejected: asBool(w[9]),
    });
  }
  const spanTxs = await eventTxById(addr, "SpanProposed(uint256,uint64,uint256,uint64,uint64,bytes32,bytes32,uint64)");
  for (const s of out.spans) s.tx = spanTxs[s.id] || null;
  return out;
}

// gasUsed for a known tx (per-op gas), when a tx hash is available.
export async function gasUsed(txHash) {
  const rcpt = await rpc("eth_getTransactionReceipt", [txHash]);
  if (!rcpt) return null;
  return Number(asUint((rcpt.gasUsed || "0x0").replace(/^0x/, "")));
}
