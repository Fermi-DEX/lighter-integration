// Live-data test: pull real payloads and re-derive everything with crypto.js.
const C = require("./crypto.js");
const BASE = "http://127.0.0.1:8087";
const get = (p) => fetch(BASE + p).then((r) => r.json());
let fail = 0;
function check(name, ok, extra) {
  console.log((ok ? "PASS" : "FAIL") + "  " + name + (extra ? "  " + extra : ""));
  if (!ok) fail++;
}

(async () => {
  const params = await get("/api/params");
  const SEQ = params.sequencer_address;
  console.log("sequencer:", SEQ, "\n");

  // Receipts + tick records + merkle roots from /api/ticks
  const ticks = await get("/api/ticks?limit=40");
  let multi = ticks.find((t) => t.entries.length > 1) || ticks[0];
  for (const t of ticks.slice(0, 5)) {
    const rv = C.verifyTickRecord(t.record, SEQ);
    check("tick-record sig tick=" + t.record.tick, rv.sigMatches, "-> " + rv.recovered);
    const br = C.verifyBatchRoot(t.entries, t.record.batch_root);
    check("batch_root merkle tick=" + t.record.tick + " (" + t.entries.length + " entries)", br.matches);
    for (const r of t.receipts) {
      const vr = C.verifyReceipt(r, SEQ);
      check("  receipt (" + r.tick + "," + r.pos + ") sig+digest", vr.sigMatches && vr.digestMatches);
    }
  }
  console.log("multi-entry tick used:", multi.record.tick, "entries:", multi.entries.length);
  const mbr = C.verifyBatchRoot(multi.entries, multi.record.batch_root);
  check("multi-entry batch_root", mbr.matches, mbr.root);

  // Stream commitment from a block with txs, anchored on previous block commitment
  const blocks = await get("/api/blocks?limit=60");
  blocks.sort((a, b) => a.number - b.number);
  const byNum = new Map(blocks.map((b) => [b.number, b]));
  const withTx = blocks.filter((b) => b.txs && b.txs.length && byNum.has(b.number - 1) && byNum.get(b.number - 1).epoch === b.epoch);
  console.log("\nblocks:", blocks.length, "with txs (+prev):", withTx.length);
  for (const b of withTx.slice(-4)) {
    const prev = byNum.get(b.number - 1);
    const sv = C.verifyStream(b, prev.stream_commitment);
    check("stream block #" + b.number + " (" + b.txs.length + " tx)", sv.matches && sv.stepsAllOk,
      sv.final.slice(2, 18) + " vs " + sv.expected.slice(0, 16));
  }

  // Signed rejection via scenario
  const acc = await (await fetch(BASE + "/api/scenario/accountable", { method: "POST" })).json();
  const rej = acc.result.rejection;
  const rv = C.verifyRejection(rej, SEQ);
  check("scenario rejection sig", rv.sigMatches, "-> " + rv.recovered);

  console.log("\n" + (fail ? fail + " FAILURES" : "ALL LIVE CHECKS PASSED"));
  process.exit(fail ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(2); });
