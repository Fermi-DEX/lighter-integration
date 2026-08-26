// pagecheck.cjs — exercise the DASHBOARD's own verification layer (verify.js →
// PosqVerify, which wraps vendor/crypto.js) against the live gateway API. This
// is exactly what each panel's ✓ badge runs in the browser, so a pass here is a
// pass in the page. Run: node vendor/pagecheck.cjs   (gateway on :8080)
const V = require("../verify.js"); // PosqVerify — the page's re-derivation functions
const BASE = process.env.BASE || "http://127.0.0.1:8080";
const get = (p) => fetch(BASE + p).then((r) => r.json());
const post = (p) => fetch(BASE + p, { method: "POST" }).then((r) => r.json());

let fail = 0,
  pass = 0;
function check(name, ok, extra) {
  console.log((ok ? "PASS" : "FAIL") + "  " + name + (extra ? "  " + extra : ""));
  ok ? pass++ : fail++;
}

(async () => {
  const params = await get("/api/params");
  const SEQ = params.sequencer_address;
  console.log("gateway sequencer:", SEQ, "\n");

  // 1. Sealed ticks: PosqVerify.sealedTick = record sig + merkle batch_root.
  const ticks = await get("/api/ticks?limit=20");
  console.log("-- panel 3: sealed ticks (record sig + merkle batch_root) --");
  let tvOne = null;
  for (const t of ticks.slice(0, 6)) {
    const r = V.sealedTick(t, SEQ);
    if (!tvOne) tvOne = r;
    check("tick " + t.record.tick + " (" + t.entries.length + " entries)", r.ok, r.checks.map((c) => (c.ok ? "✓" : "✗") + c.label.split(" →")[0]).join(" · "));
  }

  // 2. Admission outcomes: PosqVerify.admissionOutcome = secp256k1 recover +
  //    digest-chain link recompute (this is panel 2's badge).
  console.log("\n-- panel 2: admission outcomes (recover sig + recompute digest link) --");
  const submits = await get("/api/submits?limit=12");
  let recoveredOne = null;
  for (const s of submits.slice(0, 6)) {
    const r = V.admissionOutcome(s.outcome, SEQ);
    if (!recoveredOne) recoveredOne = r;
    check(s.kind + " (" + s.tick + "," + s.pos + ")", r.ok, "recovered " + (r.recovered ? r.recovered.slice(0, 12) : "?"));
  }

  // 3. Stream commitment: PosqVerify.streamBlock recomputes s_n and matches the
  //    posted commitment, anchored on the previous block (panel 3).
  console.log("\n-- panel 3: opened-stream commitment recomputed & matched --");
  const blocks = await get("/api/blocks?limit=80");
  const byNum = new Map(blocks.map((b) => [b.number, b]));
  const cands = blocks.filter((b) => b.txs.length && byNum.has(b.number - 1) && byNum.get(b.number - 1).epoch === b.epoch).sort((a, b) => a.number - b.number);
  let streamOne = null;
  for (const b of cands.slice(-4)) {
    const prev = byNum.get(b.number - 1);
    const r = V.streamBlock(b, prev.stream_commitment);
    if (!streamOne) streamOne = r;
    check("stream block #" + b.number + " (" + b.txs.length + " tx)", r.ok, r.final.slice(0, 14) + "…");
  }
  if (!cands.length) console.log("   (no non-empty block with a known predecessor yet — let traffic run)");

  // 4. Signed rejection via the accountable scenario (panel 2 / 5).
  console.log("\n-- panel 5: accountable rejection (signed, browser-verified) --");
  const acc = await post("/api/scenario/accountable");
  const rr = V.admissionOutcome(acc.result.rejection, SEQ);
  check("scenario rejection sig recovers to sequencer", rr.ok, "recovered " + (rr.recovered ? rr.recovered.slice(0, 12) : "?"));

  // 5. Envelope binding: h = commitment_hash(epoch, envelope_bytes), from the
  //    REAL submitted bytes (panel 2's envelope inspector badge).
  console.log("\n-- panel 2: envelope bytes → commitment binding --");
  const withEnv = submits.find((s) => s.kind === "admitted" && s.envelope_hex);
  let bindOne = null;
  if (withEnv) {
    const ep = withEnv.outcome.epoch != null ? withEnv.outcome.epoch : params.epoch;
    bindOne = V.envelopeBinding(ep, withEnv.envelope_hex, withEnv.h);
    check("h = keccak(posq-commit-v1 ‖ epoch ‖ " + withEnv.envelope_hex.length / 2 + "B envelope)", bindOne.ok, bindOne.computed.slice(0, 16) + "…");
    const hdr = V.decodeEnvelopeHeader(withEnv.envelope_hex);
    check("39-byte AAD decodes (magic 0x5051, ns " + hdr.namespace + ")", hdr.magic === "0x5051" && hdr.version === 1 && hdr.namespace === params.namespace);
    // tampered envelope must NOT bind
    const flipped = withEnv.envelope_hex.slice(0, -2) + (withEnv.envelope_hex.slice(-2) === "00" ? "01" : "00");
    check("tampered envelope byte breaks the binding", !V.envelopeBinding(ep, flipped, withEnv.h).ok);
  } else {
    check("submit with envelope_hex present", false, "no admitted submit carried envelope_hex");
  }

  // 6. Full Wesolowski segment verification (panel 1's headline badge):
  //    challenge derivation (double sha256 → prime) + pi^l·y^r ≡ x_end mod N.
  console.log("\n-- panel 1: Wesolowski segment proof verified in-browser code --");
  const segs = await get("/api/segments?limit=3");
  let wesOne = null;
  for (const g of segs) {
    const t0 = Date.now();
    const w = V.wesolowski(g, params.modulus_n);
    if (!wesOne) wesOne = w;
    check("segment " + g.segment + " Wesolowski (t=" + g.t + " sq)", w.ok, w.checks.map((c) => (c.ok ? "✓" : "✗")).join("") + " in " + (Date.now() - t0) + "ms");
  }
  if (segs.length) {
    const g = segs[0];
    const badPi = Object.assign({}, g, { pi: g.pi.slice(0, -2) + (g.pi.slice(-2).toLowerCase() === "00" ? "01" : "00") });
    check("corrupted pi REJECTED", !V.wesolowski(badPi, params.modulus_n).ok);
    const badL = Object.assign({}, g, { l: (BigInt(g.l) + 2n).toString() });
    check("perturbed challenge l REJECTED", !V.wesolowski(badL, params.modulus_n).ok);
  } else {
    check("segments available", false, "empty /api/segments");
  }

  // 7. Anchor signature: full submitAnchor calldata recovers to the sequencer
  //    (panel 4's local-anchor badge).
  console.log("\n-- panel 4: anchor signature (submitAnchor calldata) --");
  const anchors = await get("/api/anchors?limit=3");
  let anchOne = null;
  for (const a of anchors) {
    const r = V.anchorSig(a, SEQ);
    if (!anchOne) anchOne = r;
    check("anchor segs [" + a.first_segment + "," + a.last_segment + "] sig", r.ok, "recovered " + (r.recovered ? r.recovered.slice(0, 12) : "?"));
  }
  if (anchors.length) {
    const a0 = Object.assign({}, anchors[0], { c_b: "0x" + "11".repeat(32) });
    check("tampered anchor field breaks recovery", !V.anchorSig(a0, SEQ).ok);
  }

  console.log("\n=== SUMMARY ===");
  if (tvOne) console.log("digest-chain / merkle:", tvOne.ok ? "OK" : "MISMATCH");
  if (recoveredOne) console.log("admission signature recovered:", recoveredOne.recovered, recoveredOne.ok ? "= sequencer ✓" : "✗");
  if (streamOne) console.log("stream commitment recompute:", streamOne.ok ? "matches posted ✓" : "MISMATCH");
  if (bindOne) console.log("envelope → h binding:", bindOne.ok ? "recomputed from raw bytes ✓" : "MISMATCH");
  if (wesOne) console.log("Wesolowski VDF proof:", wesOne.ok ? "verified in-browser code ✓" : "FAILED");
  if (anchOne) console.log("anchor signature:", anchOne.ok ? anchOne.recovered + " = sequencer ✓" : "FAILED");
  console.log("\n" + pass + " passed, " + fail + " failed");
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(2);
});
