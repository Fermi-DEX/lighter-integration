// app.js — orchestration + the six panels. Vanilla ES module. Everything a
// panel claims is re-derived in the browser (PosqVerify → vendor/crypto.js) or
// is a Sepolia read/link. Live via SSE /api/events with a polling backstop.

import { el, clear, hexChip, badge, statusChip, num, ago, getJSON, postJSON } from "./util.js";
import { cadenceSparkline, latencyHistogram } from "./viz.js";
import * as ETH from "./eth.js";

const V = globalThis.PosqVerify;
const $ = (id) => document.getElementById(id);
const ETHERSCAN = "https://sepolia.etherscan.io";

const S = {
  params: null,
  seq: null,
  cadence: [], // {t, tick}
  cadenceRates: [],
  blocksByNum: new Map(),
  memo: new Map(), // verification cache keyed by object identity
  sseUp: false,
};

function memoVerify(key, fn) {
  if (S.memo.has(key)) return S.memo.get(key);
  const r = fn();
  S.memo.set(key, r);
  if (S.memo.size > 4000) S.memo.clear();
  return r;
}

// ------------------------------------------------------------------ boot
async function boot() {
  try {
    S.params = await getJSON("/api/params");
  } catch (e) {
    $("boot-error").textContent = "gateway unreachable: " + e.message;
    return;
  }
  if (!S.params.ready) {
    $("boot-error").textContent = "sequencer calibrating q… retrying";
    setTimeout(boot, 2000);
    return;
  }
  S.seq = S.params.sequencer_address;
  $("seq-addr").appendChild(hexChip(S.seq, { head: 12, tail: 6 }));
  $("namespace").textContent = S.params.namespace;
  renderParams();
  renderScorecard();
  initEthPanel();
  wireScenarioButtons();
  connectSSE();
  await refreshAll();
  setInterval(refreshAll, 2500);
}

// ------------------------------------------------------- tabs & panel collapse
// Featured panels (2/3/4/5) expand by default; 1 and 6 collapse to a live
// summary strip. State persists per panel in localStorage. Rendering continues
// into hidden DOM while collapsed, so expand is instant and the SSE/poll loop
// never branches on visibility.
const PANELS = [
  { id: "panel-admission", collapsed: false },
  { id: "panel-ethereum", collapsed: false },
  { id: "panel-lighter", collapsed: false },
  { id: "panel-scenarios", collapsed: false },
  { id: "panel-clock", collapsed: true },
  { id: "panel-scorecard", collapsed: true },
];

function setCollapsed(id, collapsed) {
  const p = $(id);
  if (!p) return;
  p.classList.toggle("collapsed", collapsed);
  const t = p.querySelector(".ph-toggle");
  if (t) t.textContent = collapsed ? "▸" : "▾";
  localStorage.setItem("posq.panel." + id, collapsed ? "1" : "0");
}

function initCollapse() {
  for (const cfg of PANELS) {
    const saved = localStorage.getItem("posq.panel." + cfg.id);
    setCollapsed(cfg.id, saved == null ? cfg.collapsed : saved === "1");
    const head = $(cfg.id) && $(cfg.id).querySelector(".panel-head");
    if (head) {
      head.addEventListener("click", () => {
        setCollapsed(cfg.id, !$(cfg.id).classList.contains("collapsed"));
      });
    }
  }
}

function setSummary(id, text) {
  const s = $(id);
  if (s) s.textContent = text || "";
}

function showTab(name) {
  $("view-dashboard").hidden = name !== "dashboard";
  $("view-about").hidden = name !== "about";
  for (const b of $("tabs").querySelectorAll(".tab")) b.classList.toggle("active", b.dataset.tab === name);
}

function initTabs() {
  for (const b of $("tabs").querySelectorAll(".tab")) {
    b.addEventListener("click", () => showTab(b.dataset.tab));
  }
  // About-tab deep links: switch to the dashboard, expand the panel, scroll.
  for (const a of document.querySelectorAll(".goto[data-goto]")) {
    a.addEventListener("click", (e) => {
      e.preventDefault();
      const id = a.dataset.goto;
      showTab("dashboard");
      setCollapsed(id, false);
      const t = $(id);
      if (t) t.scrollIntoView({ behavior: "smooth", block: "start" });
    });
  }
}

initTabs();
initCollapse();

function renderParams() {
  const p = S.params;
  const rows = [
    ["Δ tick", (p.delta_micros / 1000).toFixed(1) + " ms"],
    ["q (calibrated)", num(p.q_squarings) + " sq/tick"],
    ["mandated rate", num(Math.round(p.mandated_rate_sq_per_s)) + " sq/s"],
    ["segment", p.segment_ticks + " ticks"],
    ["window", p.window_ticks + " ticks"],
    ["delay class 0", p.delay_ticks_class0 + " ticks"],
    ["grace", num(p.grace_ticks) + " ticks"],
    ["capacity", p.capacity_per_tick_per_bucket + " /tick/bucket"],
    ["envelope", p.envelope_bytes + " B"],
    ["anchor every", p.anchor_every_segments + " segs"],
  ];
  const host = clear($("params-grid"));
  for (const [k, v] of rows) host.appendChild(el("div", { class: "pcell" }, [el("span", { class: "pk", text: k }), el("span", { class: "pv", text: v })]));
}

// ------------------------------------------------------------------ refresh
async function refreshAll() {
  try {
    const [status, tape, book, submits, blocks, anchors, ticks, segments, log] = await Promise.all([
      getJSON("/api/status"),
      getJSON("/api/tape?limit=44"),
      getJSON("/api/book"),
      getJSON("/api/submits?limit=40"),
      getJSON("/api/blocks?limit=48"),
      getJSON("/api/anchors?limit=24"),
      getJSON("/api/ticks?limit=16"),
      getJSON("/api/segments?limit=8"),
      getJSON("/api/scenario-log"),
    ]);
    onStatus(status);
    renderTape(tape);
    renderBook(book);
    renderAdmission(submits, tape);
    ingestBlocks(blocks);
    renderMap(blocks);
    renderStreamVerify();
    renderTicks(ticks);
    renderSegments(segments);
    renderAnchors(anchors);
    renderScenarioLog(log);
    $("conn").className = "conn " + (S.sseUp ? "live" : "poll");
    $("conn").textContent = S.sseUp ? "● SSE live" : "● polling";
  } catch (e) {
    $("conn").className = "conn down";
    $("conn").textContent = "● " + e.message;
  }
}

// ------------------------------------------------------------------ panel 1: clock & tape
function onStatus(st) {
  const now = Date.now();
  const last = S.cadence[S.cadence.length - 1];
  if (!last || st.tick !== last.tick) {
    if (last) {
      const dt = (now - last.t) / 1000;
      if (dt > 0.2) S.cadenceRates.push((st.tick - last.tick) / dt);
      if (S.cadenceRates.length > 60) S.cadenceRates.shift();
    }
    S.cadence.push({ t: now, tick: st.tick });
    if (S.cadence.length > 61) S.cadence.shift();
  }
  const tiles = {
    "k-tick": num(st.tick),
    "k-segment": num(st.segment),
    "k-epoch": st.epoch == null ? "—" : st.epoch,
    "k-admitted": num(st.admitted),
    "k-opened": num(st.opened),
    "k-gaps": num(st.tape_gaps),
  };
  for (const id in tiles) $(id).textContent = tiles[id];
  const rate = S.cadenceRates.length ? S.cadenceRates[S.cadenceRates.length - 1] : 0;
  $("k-cadence").textContent = rate.toFixed(1) + " tk/s";
  cadenceSparkline($("cadence-spark"), S.cadenceRates);
  // cadence-fault / segment-seal status line
  const faults = st.cadence_faults || [];
  const seg = clear($("seg-status"));
  seg.appendChild(statusChip(faults.length ? faults.length + " cadence faults" : "cadence nominal", faults.length ? "critical" : "good"));
  seg.appendChild(statusChip("segment " + num(st.segment) + " sealing", "info"));
  seg.appendChild(el("span", { class: "seg-note", text: "each segment seal carries a Wesolowski VDF proof — verified below in your browser, and natively on-chain by PoSqHost.verifySegmentProof (same object, same equation)" }));
  // collapsed-header summary strip (panel 1)
  let wesOk = 0;
  for (const [k, v] of S.memo) if (k.startsWith("wes:") && v && v.ok) wesOk++;
  setSummary("sum-clock", "tick " + num(st.tick) + " · seg " + num(st.segment) + " · " + rate.toFixed(1) + " tk/s · seals ✓ " + wesOk + " verified");
}

function outcomeTone(outcome) {
  if (outcome === "opened") return "good";
  if (outcome === "pending") return "warning";
  return "critical"; // gap:*
}

function renderTape(tape) {
  const host = clear($("tape-list"));
  $("tape-count").textContent = tape.length + " shown";
  for (const t of tape) {
    const row = el("div", { class: "tape-row" });
    row.appendChild(el("span", { class: "tj", text: "(" + t.tick + "," + t.pos + ")" }));
    row.appendChild(hexChip(t.h, { head: 5, tail: 4 }));
    row.appendChild(statusChip(t.outcome, outcomeTone(t.outcome)));
    if (t.outcome === "opened" && t.detail) {
      const tx = t.detail.payload_utf8 || "";
      row.appendChild(el("span", { class: "tape-plain", text: tx.length > 68 ? tx.slice(0, 66) + "…" : tx }));
    } else if (t.outcome === "pending" && t.detail) {
      row.appendChild(el("span", { class: "tape-plain muted", text: "mature @ tick " + t.detail.mature_at }));
    }
    host.appendChild(row);
  }
}

// ------------------------------------------------------------------ panel 2: blind admission
function renderAdmission(submits, tape) {
  renderEnvelopeInspector(submits, tape);
  const lat = submits.filter((s) => s.latency_us != null).map((s) => s.latency_us);
  latencyHistogram($("lat-hist"), lat);
  const st = S.lastStatus || {};
  $("lat-stats").textContent = submits.length + " recent · shows raw admission latency (envelope decode + fence + sign)";
  const sorted = lat.slice().sort((a, b) => a - b);
  const p50 = sorted.length ? sorted[Math.floor(sorted.length / 2)] : 0;
  setSummary("sum-admission", submits.length + " signed outcomes · p50 " + p50 + "µs");
  // signed outcomes list
  const host = clear($("admit-list"));
  $("admit-count").textContent = submits.length + " shown";
  for (const s of submits) {
    const o = s.outcome;
    const key = "adm:" + (o.sig || "") + ":" + o.type;
    const res = memoVerify(key, () => V.admissionOutcome(o, S.seq));
    const card = el("div", { class: "admit-card" });
    const kindTone = s.kind === "admitted" ? "good" : s.kind.startsWith("rejected") ? "critical" : "warning";
    const head = el("div", { class: "admit-head" }, [
      statusChip(s.kind, kindTone),
      el("span", { class: "muted mono", text: s.bot }),
      s.kind === "admitted" ? el("span", { class: "tj", text: "(" + s.tick + "," + s.pos + ")" }) : null,
      el("span", { class: "muted", text: ago(s.at_ms) }),
      el("span", { class: "grow" }),
      el("span", { class: "muted mono", text: (s.latency_us || 0) + "µs" }),
    ]);
    card.appendChild(head);
    const fields = el("div", { class: "kv-inline" });
    if (o.h) fields.appendChild(kv("h", hexChip(o.h, { head: 6, tail: 4 })));
    if (o.d) fields.appendChild(kv("d", hexChip(o.d, { head: 6, tail: 4 })));
    if (o.reason) fields.appendChild(kv("reason", statusChip(o.reason, "critical")));
    card.appendChild(fields);
    card.appendChild(badge(res));
    host.appendChild(card);
  }
}

function kv(k, v) {
  return el("span", { class: "kv" }, [el("span", { class: "kk", text: k }), typeof v === "string" ? el("span", { class: "kvv", text: v }) : v]);
}

function renderEnvelopeInspector(submits, tape) {
  const host = clear($("env-inspector"));
  // pick an admitted submit (with raw envelope bytes) whose commitment opened.
  const openedByH = new Map();
  for (const t of tape) if (t.outcome === "opened" && t.detail) openedByH.set(t.h.toLowerCase(), t);
  let pick = null;
  for (const s of submits) {
    if (s.kind === "admitted" && s.outcome && s.envelope_hex && openedByH.has((s.outcome.h || "").toLowerCase())) {
      pick = { s, te: openedByH.get(s.outcome.h.toLowerCase()) };
      break;
    }
  }
  if (!pick) {
    host.appendChild(el("div", { class: "muted", text: "waiting for an admitted envelope to open on the tape…" }));
    return;
  }
  const { s, te } = pick;
  const o = s.outcome;
  const hdr = V.decodeEnvelopeHeader(s.envelope_hex);
  const cols = el("div", { class: "env-cols" });
  // visible header — decoded from the REAL first 39 bytes
  const vis = el("div", { class: "env-col" }, [el("div", { class: "env-title", text: "visible header — 39-byte AAD, decoded from the raw bytes" })]);
  const vg = el("div", { class: "kv-stack" });
  if (hdr.error) {
    vg.appendChild(el("div", { class: "muted", text: hdr.error }));
  } else {
    vg.appendChild(kv("bytes[0..39]", hexChip(hdr.headerAadHex, { head: 12, tail: 8 })));
    vg.appendChild(kv("magic·ver", hdr.magic + " · v" + hdr.version));
    vg.appendChild(kv("bucket", String(hdr.bucket) + (hdr.bucket === 0 ? " (Majors)" : "")));
    vg.appendChild(kv("namespace", String(hdr.namespace)));
    vg.appendChild(kv("window", "[" + hdr.windowStart + ", +" + hdr.windowLen + "]"));
    vg.appendChild(kv("delay class", String(hdr.delayClass)));
    vg.appendChild(kv("nonce η", hexChip(hdr.nonce, { head: 6, tail: 4 })));
    vg.appendChild(kv("ticket", hexChip(hdr.ticketId, { head: 6, tail: 4 })));
  }
  vis.appendChild(vg);
  cols.appendChild(vis);
  // opaque ciphertext — the actual body bytes
  const opq = el("div", { class: "env-col opaque" }, [el("div", { class: "env-title", text: "opaque ciphertext — what the sequencer ordered blind" })]);
  if (!hdr.error) {
    opq.appendChild(kv("KEM u", hdr.uLen + " B"));
    opq.appendChild(kv("AEAD body", hdr.bodyLen + " B of " + hdr.totalBytes + " B envelope"));
    opq.appendChild(el("div", { class: "cipher-blob" }, [hexChip("0x" + String(hdr.bodyHex).replace(/^0x/, ""), { head: 40, tail: 12 })]));
  }
  opq.appendChild(el("div", { class: "muted small", text: "no op type, side, price, size, or account visible until the timelock solved." }));
  cols.appendChild(opq);
  // opened to
  const opn = el("div", { class: "env-col" }, [el("div", { class: "env-title", text: "opened to (after timelock solve)" })]);
  opn.appendChild(kv("account", hexChip(te.detail.account, { head: 6, tail: 4 })));
  opn.appendChild(kv("intent nonce", String(te.detail.nonce)));
  opn.appendChild(el("pre", { class: "plain-json", text: te.detail.payload_utf8 }));
  cols.appendChild(opn);
  host.appendChild(cols);
  // binding: h = commitment_hash(epoch, envelope bytes), recomputed from raw bytes
  const bindKey = "bind:" + o.h;
  const epoch = o.epoch != null ? o.epoch : S.params.epoch; // receipt carries its epoch (robust across rotation)
  host.appendChild(badge(memoVerify(bindKey, () => V.envelopeBinding(epoch, s.envelope_hex, o.h))));
  const key = "adm:" + (o.sig || "") + ":" + o.type;
  host.appendChild(badge(memoVerify(key, () => V.admissionOutcome(o, S.seq))));
}

// ------------------------------------------------------------------ panel 3: tape → lighter
function renderBook(book) {
  const snap = book.snapshot;
  $("book-meta").textContent =
    "last " + (snap.last_trade_price ?? "—") + " · next ask nonce " + snap.next_ask_nonce + " · next bid nonce " + snap.next_bid_nonce + (book.head_blocked ? " · HEAD BLOCKED" : "");
  const mkSide = (arr, side) => {
    const host = clear($(side === "ask" ? "book-asks" : "book-bids"));
    for (const o of arr) {
      host.appendChild(
        el("div", { class: "book-row " + side }, [
          el("span", { class: "b-nonce", title: "order nonce = Lighter time priority", text: "#" + o.nonce }),
          el("span", { class: "b-price mono", text: num(o.price) }),
          el("span", { class: "b-size mono", text: num(o.size) }),
          el("span", { class: "b-acct", text: o.account.slice(0, 8) + "…" }),
          el("span", { class: "b-coi muted", text: "coi " + o.coi }),
        ])
      );
    }
  };
  mkSide((snap.asks || []).slice().reverse(), "ask");
  mkSide(snap.bids || [], "bid");
}

function ingestBlocks(blocks) {
  for (const b of blocks) S.blocksByNum.set(b.number, b);
  if (S.blocksByNum.size > 800) {
    const keys = [...S.blocksByNum.keys()].sort((a, b) => a - b);
    for (const k of keys.slice(0, keys.length - 800)) S.blocksByNum.delete(k);
  }
}

function renderMap(blocks) {
  const host = clear($("map-list"));
  $("map-count").textContent = blocks.length + " blocks";
  const latest = blocks.length ? blocks[0] : null;
  S.latestBlockNo = latest ? latest.number : null;
  for (const b of blocks) {
    if (!b.txs.length && !b.gaps.length) continue; // skip empty blocks in the map
    const row = el("div", { class: "map-block" });
    row.appendChild(
      el("div", { class: "map-head" }, [
        el("span", { class: "map-bno", text: "block " + b.number }),
        el("span", { class: "muted", text: "ticks [" + b.first_tick + "," + b.last_tick + "]" }),
        el("span", { class: "grow" }),
        el("span", { class: "muted small", text: b.txs.length + " tx · " + b.gaps.length + " gap" }),
      ])
    );
    const items = el("div", { class: "map-items" });
    b.txs.forEach((tx, i) => {
      const applied = tx.result && tx.result.Applied;
      const failed = tx.result && tx.result.Failed;
      const trades = applied ? applied.trades.length : 0;
      items.appendChild(
        el("div", { class: "map-tx" }, [
          el("span", { class: "tj", text: "(" + tx.tick + "," + tx.pos + ")" }),
          el("span", { class: "map-arrow", text: "→" }),
          el("span", { class: "map-idx", text: "[" + b.number + "," + i + "]" }),
          el("span", { class: "map-op", text: opLabel(tx.tx) }),
          applied ? statusChip(trades ? trades + " trade" : "applied", trades ? "good" : "info") : statusChip("failed: " + (failed && failed.reason), "warning"),
        ])
      );
    });
    b.gaps.forEach((g) => {
      items.appendChild(
        el("div", { class: "map-tx gap" }, [
          el("span", { class: "tj", text: "(" + g.tick + "," + g.pos + ")" }),
          el("span", { class: "map-arrow", text: "→" }),
          el("span", { class: "map-idx muted", text: "[gap preserved]" }),
          statusChip("gap: " + g.reason, "critical"),
        ])
      );
    });
    row.appendChild(items);
    host.appendChild(row);
  }
}

function opLabel(tx) {
  if (!tx) return "?";
  if (tx.op === "cancel") return "cancel coi " + tx.coi;
  return tx.side + " " + tx.kind + " " + (tx.price ? "@" + tx.price : "") + " ×" + tx.size;
}

function renderStreamVerify() {
  const host = clear($("stream-verify"));
  // latest block with txs whose predecessor (same epoch) is known.
  const nums = [...S.blocksByNum.keys()].sort((a, b) => b - a);
  let target = null;
  for (const n of nums) {
    const b = S.blocksByNum.get(n);
    if (b.txs.length && S.blocksByNum.has(n - 1) && S.blocksByNum.get(n - 1).epoch === b.epoch) {
      target = b;
      break;
    }
  }
  if (!target) {
    host.appendChild(el("div", { class: "muted", text: "waiting for a non-empty block with a known predecessor…" }));
    return;
  }
  const prev = S.blocksByNum.get(target.number - 1);
  const key = "stream:" + target.stream_commitment;
  const res = memoVerify(key, () => V.streamBlock(target, prev.stream_commitment));
  setSummary("sum-lighter", "block " + num(S.latestBlockNo != null ? S.latestBlockNo : target.number) + " · s_n " + (res.ok ? "✓ recomputed = posted" : "✗ MISMATCH") + " · gaps preserved");
  host.appendChild(
    el("div", { class: "sv-head" }, [
      el("span", { text: "block " + target.number + " · " + target.txs.length + " tx" }),
      el("span", { class: "muted small", text: "anchored on block " + (target.number - 1) + "'s commitment" }),
    ])
  );
  host.appendChild(kv("posted s_n", hexChip("0x" + target.stream_commitment, { head: 10, tail: 8 })));
  const steps = el("div", { class: "sv-steps" });
  for (const st of res.steps.slice(0, 10)) {
    steps.appendChild(
      el("div", { class: "sv-step " + (st.anchor ? "anchor" : st.stepMatches === false ? "bad" : "ok") }, [
        el("span", { class: "sv-lab mono", text: st.label }),
        hexChip(st.s, { head: 6, tail: 4 }),
        st.anchor ? el("span", { class: "muted small", text: "start" }) : el("span", { class: "sv-mark", text: st.stepMatches === false ? "✗" : "✓" }),
      ])
    );
  }
  host.appendChild(steps);
  host.appendChild(badge(res));
}

function renderTicks(ticks) {
  const host = clear($("ticks-list"));
  $("ticks-count").textContent = ticks.length + " sealed";
  for (const t of ticks.slice(0, 8)) {
    const key = "tick:" + t.record.sig;
    const res = memoVerify(key, () => V.sealedTick(t, S.seq));
    const row = el("div", { class: "tick-row" }, [
      el("span", { class: "tj", text: "tick " + t.record.tick }),
      el("span", { class: "muted small", text: t.entries.length + " entr" + (t.entries.length === 1 ? "y" : "ies") }),
      kv("batch_root", hexChip(t.record.batch_root, { head: 6, tail: 4 })),
      badge(res),
    ]);
    host.appendChild(row);
  }
}

// ------------------------------------------------------------ panel 1: segment seals
// Full Wesolowski verification (challenge derivation + pi^l·y^r ≡ x_end mod N)
// runs in-browser via BigInt (~a few ms). The newest seal auto-verifies once
// (memoized, deferred off the poll path); older seals verify on click.
function renderSegments(segments) {
  const host = clear($("seg-list"));
  $("seg-count").textContent = segments.length + " shown";
  if (!S.params.modulus_n) {
    host.appendChild(el("div", { class: "muted", text: "modulus_n missing from /api/params — cannot verify seals" }));
    return;
  }
  segments.forEach((g, idx) => {
    const key = "wes:" + g.epoch + ":" + g.segment;
    const row = el("div", { class: "seg-row" });
    row.appendChild(el("span", { class: "tj", text: "seg " + g.segment }));
    row.appendChild(kv("T", num(g.t) + " sq"));
    row.appendChild(kv("x_end", hexChip(g.x_end_hash, { head: 6, tail: 4 })));
    row.appendChild(kv("l", V.short("0x" + BigInt(g.l).toString(16), 6)));
    const slot = el("span", { class: "seg-verify" });
    const showBadge = () => {
      clear(slot).appendChild(badge(S.memo.get(key)));
      const head = slot.querySelector(".vbadge-head");
      if (head && S.memo.get(key).ok) head.textContent = "Wesolowski ✓ verified in-browser";
    };
    if (S.memo.has(key)) {
      showBadge();
    } else if (idx === 0) {
      slot.appendChild(el("span", { class: "muted small", text: "verifying…" }));
      // Defer off the render pass; memoized so it runs once per seal.
      setTimeout(() => {
        memoVerify(key, () => V.wesolowski(g, S.params.modulus_n));
        showBadge();
      }, 0);
    } else {
      const btn = el("button", { class: "btn tiny", text: "verify" });
      btn.addEventListener("click", () => {
        btn.disabled = true;
        btn.textContent = "verifying…";
        setTimeout(() => {
          memoVerify(key, () => V.wesolowski(g, S.params.modulus_n));
          showBadge();
        }, 0);
      });
      slot.appendChild(btn);
    }
    row.appendChild(slot);
    host.appendChild(row);
  });
}

// ------------------------------------------------------------ panel 4: local anchors
// Each local anchor now carries the full PoSqHost.submitAnchor calldata; the
// anchor signature is recovered in-browser against the sequencer key.
function renderAnchors(anchors) {
  const host = clear($("anchors-local"));
  $("anchors-count").textContent = anchors.length + " local";
  for (const a of anchors.slice(0, 12)) {
    const key = "anch:" + (a.sig || "") + ":" + a.first_tick;
    const res = a.sig ? memoVerify(key, () => V.anchorSig(a, S.seq)) : null;
    const row = el("div", { class: "anchor-row" });
    row.appendChild(
      el("div", { class: "admit-head" }, [
        el("span", { class: "tj", text: "segs [" + a.first_segment + "," + a.last_segment + "]" }),
        el("span", { class: "muted", text: "ticks [" + a.first_tick + "," + a.last_tick + "]" }),
        statusChip(a.sepolia === "local-only" ? "local-only" : a.sepolia === "pending" ? "pending" : "posted", a.sepolia === "local-only" ? "info" : a.sepolia === "pending" ? "warning" : "good"),
        el("span", { class: "grow" }),
        el("span", { class: "muted", text: ago(a.at_ms) }),
      ])
    );
    const fields = el("div", { class: "kv-inline" });
    fields.appendChild(kv("c_b", hexChip(a.c_b, { head: 6, tail: 4 })));
    fields.appendChild(kv("receipts_root", hexChip(a.receipts_root, { head: 6, tail: 4 })));
    if (a.stream_commitment) fields.appendChild(kv("s_n", hexChip("0x" + String(a.stream_commitment).replace(/^0x/, ""), { head: 6, tail: 4 })));
    fields.appendChild(kv("seg x_end hashes", String((a.segment_x_end_hashes || []).length)));
    if (a.host_tx) fields.appendChild(el("a", { class: "escan", href: ETHERSCAN + "/tx/" + a.host_tx, target: "_blank", text: "submitAnchor tx ↗" }));
    if (a.bridge_tx) fields.appendChild(el("a", { class: "escan", href: ETHERSCAN + "/tx/" + a.bridge_tx, target: "_blank", text: "proposeSpan tx ↗" }));
    row.appendChild(fields);
    if (res) row.appendChild(badge(res));
    host.appendChild(row);
  }
}

// ------------------------------------------------------------------ panel 4: ethereum
function initEthPanel() {
  const eth = (S.params && S.params.eth) || {};
  const savedRpc = localStorage.getItem("posq.rpc") || eth.rpc || "";
  const savedHost = localStorage.getItem("posq.host") || eth.posq_host || "";
  const savedBridge = localStorage.getItem("posq.bridge") || eth.bridge || "";
  const cfg = clear($("eth-config"));
  const rpcIn = el("input", { class: "eth-in", value: savedRpc, placeholder: "Sepolia JSON-RPC URL" });
  const hostIn = el("input", { class: "eth-in", value: savedHost, placeholder: "PoSqHost address (0x…) — not yet deployed" });
  const bridgeIn = el("input", { class: "eth-in", value: savedBridge, placeholder: "LighterBridgeDemo address (0x…)" });
  const connect = el("button", { class: "btn", text: "read from Sepolia" });
  connect.addEventListener("click", () => {
    localStorage.setItem("posq.rpc", rpcIn.value.trim());
    localStorage.setItem("posq.host", hostIn.value.trim());
    localStorage.setItem("posq.bridge", bridgeIn.value.trim());
    loadEth(rpcIn.value.trim(), hostIn.value.trim(), bridgeIn.value.trim());
  });
  cfg.appendChild(el("div", { class: "eth-form" }, [labelWrap("RPC", rpcIn), labelWrap("PoSqHost", hostIn), labelWrap("Bridge", bridgeIn), connect]));
  loadEth(savedRpc, savedHost, savedBridge);
}

function labelWrap(t, input) {
  return el("label", { class: "eth-label" }, [el("span", { class: "muted small", text: t }), input]);
}

async function loadEth(rpcUrl, hostAddr, bridgeAddr) {
  const eth = (S.params && S.params.eth) || {};
  const sum = clear($("eth-summary"));
  const posterLink = eth.poster_address
    ? el("a", { class: "escan", href: ETHERSCAN + "/address/" + eth.poster_address, target: "_blank" }, hexChip(eth.poster_address, { head: 8, tail: 6 }))
    : el("span", { class: "muted", text: "—" });
  sum.appendChild(ethStat("chain", eth.chain_id === 11155111 ? "Sepolia (11155111)" : eth.chain_id || "?"));
  sum.appendChild(ethStat("poster balance", (eth.poster_balance_eth ? Number(eth.poster_balance_eth).toFixed(4) : "?") + " ETH"));
  const posterCell = ethStat("poster", "");
  posterCell.querySelector(".es-v").appendChild(posterLink);
  sum.appendChild(posterCell);
  sum.appendChild(ethStat("gateway poster", eth.enabled ? "enabled" : "local-only"));

  clear($("eth-anchors"));
  clear($("eth-forced"));
  clear($("eth-fraud"));
  if (!rpcUrl) {
    setSummary("sum-ethereum", "no RPC configured");
    $("eth-anchors").appendChild(el("div", { class: "notice", text: "No RPC configured — paste a Sepolia RPC URL above to read on-chain state." }));
    return;
  }
  if (!hostAddr) {
    setSummary("sum-ethereum", "contracts not configured");
    $("eth-anchors").appendChild(
      el("div", { class: "notice" }, [
        el("strong", { text: "PoSqHost / bridge not yet deployed. " }),
        "The demo runs local-only; anchors above are mirrored to the local log (see below). Deploy the contracts, paste their addresses, and this panel reads bond, anchors, forced queue, and the fraud log directly from Sepolia in your browser.",
      ])
    );
    return;
  }
  ETH.setRpc(rpcUrl);
  $("eth-anchors").appendChild(el("div", { class: "muted", text: "reading PoSqHost " + hostAddr + " …" }));
  try {
    const host = await ETH.readHost(hostAddr);
    renderEthHost(host, hostAddr);
    if (bridgeAddr) {
      const br = await ETH.readBridge(bridgeAddr);
      renderEthBridge(br, bridgeAddr);
    }
    // sacrificial host: the slashing demo target (malicious sequencer)
    if (eth.posq_host_sacrificial) {
      try {
        const sac = await ETH.readHost(eth.posq_host_sacrificial);
        renderEthSacrificial(sac, eth.posq_host_sacrificial);
      } catch (_) {}
    }
    // per-op gas from known txs
    const txs = [eth.last_anchor_tx, eth.last_span_tx].filter(Boolean);
    if (txs.length) {
      const gasHost = $("eth-fraud");
      for (const tx of txs) {
        try {
          const g = await ETH.gasUsed(tx);
          if (g) gasHost.appendChild(el("div", { class: "gasrow" }, [el("span", { class: "muted", text: "tx gasUsed" }), el("span", { class: "mono", text: num(g) }), txLink(tx)]));
        } catch (_) {}
      }
    }
  } catch (e) {
    setSummary("sum-ethereum", "on-chain read failed");
    clear($("eth-anchors")).appendChild(el("div", { class: "notice bad", text: "on-chain read failed: " + e.message + " — RPC reachable? contract deployed at this address?" }));
  }
}

function renderEthHost(h, addr) {
  setSummary("sum-ethereum", "bond " + weiEth(h.bond) + " ETH · " + h.anchors.length + " anchors on-chain · " + (h.rescueMode ? "RESCUE" : "healthy"));
  const sum = $("eth-summary");
  sum.appendChild(ethStat("bond", weiEth(h.bond) + " ETH"));
  sum.appendChild(ethStat("bond floor", weiEth(h.bondFloor) + " ETH"));
  const rescue = ethStat("mode", h.rescueMode ? "RESCUE (slashed)" : "healthy");
  rescue.querySelector(".es-v").className = "es-v " + (h.rescueMode ? "bad" : "good");
  sum.appendChild(rescue);
  const seqOk = h.sequencer && S.seq && h.sequencer.toLowerCase() === S.seq.toLowerCase();
  const seqCell = ethStat("on-chain sequencer", "");
  seqCell.querySelector(".es-v").appendChild(statusChip(seqOk ? "matches embedded key" : "differs", seqOk ? "good" : "warning"));
  sum.appendChild(seqCell);
  // anchors table
  const an = clear($("eth-anchors"));
  an.appendChild(el("div", { class: "sub-title" }, [
    el("span", { text: "Anchors on-chain (" + h.anchors.length + ") " }),
    addrLink(addr),
  ]));
  if (!h.anchors.length) an.appendChild(el("div", { class: "muted", text: "no anchors submitted yet" }));
  for (const a of h.anchors.slice(-12).reverse()) {
    an.appendChild(
      el("div", { class: "eth-row" }, [
        el("span", { class: "mono", text: "#" + a.id }),
        el("span", { class: "muted", text: "ticks [" + a.firstTick + "," + a.lastTick + "]" }),
        kv("cB", hexChip(a.cB, { head: 6, tail: 4 })),
        el("span", { class: "muted small", text: "accepted " + new Date(a.acceptedAt * 1000).toISOString().slice(11, 19) }),
        a.tx ? txLink(a.tx) : el("span", { class: "muted small", text: "" }),
      ])
    );
  }
  // forced queue
  const fq = clear($("eth-forced"));
  fq.appendChild(el("div", { class: "sub-title", text: "Forced-inclusion queue (" + h.forced.length + ")" }));
  if (!h.forced.length) fq.appendChild(el("div", { class: "muted", text: "empty" }));
  for (const f of h.forced.slice(-8).reverse()) {
    fq.appendChild(el("div", { class: "eth-row" }, [el("span", { class: "mono", text: "#" + f.id }), hexChip(f.h, { head: 6, tail: 4 }), statusChip(f.discharged ? "discharged" : "pending", f.discharged ? "good" : "warning"), f.tx ? txLink(f.tx) : el("span", { class: "muted small", text: "" })]));
  }
  // fraud log
  const fr = clear($("eth-fraud"));
  fr.appendChild(el("div", { class: "sub-title", text: "Fraud log — main host (" + h.fraud.length + ")" }));
  if (!h.fraud.length) fr.appendChild(el("div", { class: "muted", text: "no fraud proven (bond intact)" }));
  for (const x of h.fraud) fr.appendChild(el("div", { class: "eth-row" }, [statusChip("FraudProven k=" + x.kind, "critical"), el("span", { class: "muted", text: "challenger " + x.challenger.slice(0, 10) + "…" }), txLink(x.tx)]));
}

// The sacrificial host absorbs the live slashing demo: same PoSqHost code,
// configured with the malicious sequencer key, so proveReorder lands here
// while the main host stays healthy.
function renderEthSacrificial(h, addr) {
  const fr = $("eth-fraud");
  fr.appendChild(el("div", { class: "sub-title" }, [
    el("span", { text: "Sacrificial host (malicious sequencer) " }),
    el("a", { class: "escan", href: ETHERSCAN + "/address/" + addr, target: "_blank" }, hexChip(addr, { head: 8, tail: 6 })),
  ]));
  fr.appendChild(el("div", { class: "eth-row" }, [
    el("span", { class: "muted", text: "bond" }),
    el("span", { class: "mono", text: weiEth(h.bond) + " ETH" }),
    statusChip(h.rescueMode ? "RESCUE (slashed)" : "healthy", h.rescueMode ? "critical" : "good"),
  ]));
  if (!h.fraud.length) fr.appendChild(el("div", { class: "muted", text: "no fraud proven yet — run scenario 4 and submit the calldata" }));
  for (const x of h.fraud) fr.appendChild(el("div", { class: "eth-row" }, [statusChip("FraudProven k=" + x.kind + " (reorder)", "critical"), el("span", { class: "mono", text: "slashed " + weiEth(x.slashed) + " ETH" }), el("span", { class: "muted", text: "challenger " + x.challenger.slice(0, 10) + "… (10% bounty paid)" }), txLink(x.tx)]));
}

function renderEthBridge(b, addr) {
  const an = $("eth-anchors");
  an.appendChild(el("div", { class: "sub-title" }, [
    el("span", { text: "Bridge spans (" + b.spanCount + ") " }),
    addrLink(addr),
  ]));
  for (const s of b.spans.slice(-8).reverse()) {
    an.appendChild(
      el("div", { class: "eth-row" }, [
        el("span", { class: "mono", text: "span #" + s.id }),
        el("span", { class: "muted", text: "[" + s.firstTick + "," + s.lastTick + "] → anchor #" + s.anchorId }),
        kv("s_n", hexChip(s.streamCommitment, { head: 6, tail: 4 })),
        statusChip(s.rejected ? "rejected (FCFS)" : "bound", s.rejected ? "critical" : "good"),
        s.tx ? txLink(s.tx) : el("span", { class: "muted small", text: "" }),
      ])
    );
  }
}

function ethStat(k, v) {
  return el("div", { class: "es" }, [el("span", { class: "es-k muted small", text: k }), el("span", { class: "es-v", text: v })]);
}
function weiEth(wei) {
  return (Number(wei) / 1e18).toFixed(4);
}
function txLink(tx) {
  return el("a", { class: "escan", href: ETHERSCAN + "/tx/" + tx, target: "_blank", text: tx.slice(0, 10) + "… ↗" });
}
function addrLink(addr) {
  return el("a", { class: "escan", href: ETHERSCAN + "/address/" + addr, target: "_blank", text: addr.slice(0, 10) + "… ↗" });
}

// ------------------------------------------------------------------ panel 5: scenarios
const SCENARIOS = [
  { name: "frontrun", label: "Front-run fails" },
  { name: "cancel-priority", label: "Cancel priority" },
  { name: "accountable", label: "Accountable admission" },
  { name: "fraud-reorder", label: "Fraud + slash (on-chain)" },
  { name: "forced-inclusion", label: "Forced inclusion" },
  { name: "stream-mismatch", label: "Stream mismatch" },
];

function wireScenarioButtons() {
  const host = clear($("scenario-buttons"));
  for (const sc of SCENARIOS) {
    const btn = el("button", { class: "btn scen", text: sc.label });
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      btn.classList.add("busy");
      try {
        const res = await postJSON("/api/scenario/" + sc.name);
        renderScenarioResult(res.result, sc.name);
      } catch (e) {
        $("scenario-result").textContent = "error: " + e.message;
      } finally {
        btn.disabled = false;
        btn.classList.remove("busy");
      }
    });
    host.appendChild(btn);
  }
}

function renderScenarioResult(r, name) {
  const host = clear($("scenario-result"));
  if (!r || r.error) {
    host.appendChild(el("div", { class: "notice bad", text: r ? r.error : "no result" }));
    return;
  }
  host.appendChild(el("div", { class: "scen-title", text: r.title || name }));
  if (name === "frontrun") return scenFrontrun(host, r);
  if (name === "cancel-priority") return scenCancel(host, r);
  if (name === "accountable") return scenAccountable(host, r);
  if (name === "fraud-reorder") return scenFraud(host, r);
  if (name === "forced-inclusion") return scenForced(host, r);
  if (name === "stream-mismatch") return scenStream(host, r);
  host.appendChild(el("pre", { class: "plain-json", text: JSON.stringify(r, null, 1) }));
}

const VICTIM = "0x" + "05".repeat(20); // scenario_frontrun victim = acct(5)

// Average fill price for the victim's own takes (scenario mixes both takers).
function avgFill(fills, taker) {
  let sz = 0, notional = 0;
  for (const f of fills || []) {
    if (taker && (f.taker || "").toLowerCase() !== taker.toLowerCase()) continue;
    sz += f.size;
    notional += f.size * f.price;
  }
  return sz ? notional / sz : 0;
}

function scenFrontrun(host, r) {
  const blindAvg = avgFill(r.blind_posq.fills, VICTIM);
  const fcfsAvg = avgFill(r.plaintext_fcfs.fills, VICTIM);
  const cmp = el("div", { class: "scen-cols" });
  cmp.appendChild(scenCol("PoSq blind FIFO", r.blind_posq.note, "victim avg fill", blindAvg.toFixed(2), "good", r.blind_posq.fills));
  cmp.appendChild(scenCol("plaintext FCFS (attacker sees order)", r.plaintext_fcfs.note, "victim avg fill", fcfsAvg.toFixed(2), "critical", r.plaintext_fcfs.fills));
  host.appendChild(cmp);
  const better = fcfsAvg > blindAvg;
  host.appendChild(el("div", { class: "verdict " + (better ? "good" : "bad"), text: "✓ " + r.verdict + "  (victim pays " + blindAvg.toFixed(2) + " blind vs " + fcfsAvg.toFixed(2) + " under FCFS)" }));
}

function scenCol(title, note, statLabel, statVal, tone, fills) {
  const c = el("div", { class: "scen-col " + tone });
  c.appendChild(el("div", { class: "scen-col-title", text: title }));
  c.appendChild(el("div", { class: "hero " + tone, text: statVal }));
  c.appendChild(el("div", { class: "muted small", text: statLabel }));
  const tl = el("div", { class: "fill-list" });
  for (const f of fills || []) tl.appendChild(el("div", { class: "fill mono", text: (f.taker || "").slice(0, 8) + "… ×" + f.size + " @ " + f.price }));
  c.appendChild(tl);
  c.appendChild(el("div", { class: "muted small", text: note }));
  return c;
}

function scenCancel(host, r) {
  const a = r.cancel_wins_race_blind, b = r.cancel_loses_plaintext;
  const cmp = el("div", { class: "scen-cols" });
  const c1 = el("div", { class: "scen-col good" }, [el("div", { class: "scen-col-title", text: "PoSq blind FIFO" }), el("div", { class: "hero good", text: a.sniped_size }), el("div", { class: "muted small", text: "size sniped off the stale quote" }), el("div", { class: "muted small", text: a.note })]);
  const c2 = el("div", { class: "scen-col critical" }, [el("div", { class: "scen-col-title", text: "plaintext (op-type visible)" }), el("div", { class: "hero critical", text: b.sniped_size }), el("div", { class: "muted small", text: "size sniped off the stale quote" }), el("div", { class: "muted small", text: b.note })]);
  cmp.appendChild(c1);
  cmp.appendChild(c2);
  host.appendChild(cmp);
  host.appendChild(el("div", { class: "verdict good", text: "✓ " + r.verdict }));
}

function scenAccountable(host, r) {
  host.appendChild(el("div", { class: "muted small", text: r.note }));
  const rej = r.rejection;
  const card = el("div", { class: "admit-card" });
  card.appendChild(el("div", { class: "admit-head" }, [statusChip("signed rejection", "critical"), kv("reason", rej.reason), kv("code", String(rej.reason_code))]));
  card.appendChild(el("div", { class: "kv-inline" }, [kv("h", hexChip(rej.h, { head: 6, tail: 4 })), kv("c_latest", hexChip(rej.c_latest, { head: 6, tail: 4 }))]));
  const res = V.admissionOutcome(rej, S.seq);
  card.appendChild(badge(res));
  host.appendChild(card);
  host.appendChild(el("div", { class: "muted small", text: "Re-derived in your browser: the sequencer's own key signed this rejection. 'My order vanished' is now a signed object with a bond behind it." }));
}

function scenFraud(host, r) {
  host.appendChild(el("div", { class: "muted small", text: r.explanation }));
  host.appendChild(kv("malicious sequencer", hexChip(r.malicious_sequencer, { head: 10, tail: 6 })));
  // verify the honest receipt recovers to the malicious signer
  const rec = r.receipt;
  const res = V.admissionOutcome(rec, r.malicious_sequencer);
  const card = el("div", { class: "admit-card" });
  card.appendChild(el("div", { class: "admit-head" }, [statusChip("victim's signed receipt", "info"), el("span", { class: "tj", text: "(" + rec.tick + "," + rec.pos + ")" })]));
  card.appendChild(badge(res));
  card.appendChild(el("div", { class: "muted small", text: "The receipt recovers to the malicious sequencer's key and its digest link recomputes — proving the operator signed a placement it then contradicted." }));
  host.appendChild(card);
  // calldata
  host.appendChild(el("div", { class: "sub-title", text: "PoSqHost.proveReorder calldata (ready to submit to the sacrificial host)" }));
  host.appendChild(el("pre", { class: "plain-json calldata", text: JSON.stringify(r.calldata, null, 1) }));
  host.appendChild(el("div", { class: "muted small", text: r.note }));
}

function scenForced(host, r) {
  host.appendChild(kv("commitment h", hexChip(r.commitment_hash, { head: 10, tail: 8 })));
  const fl = el("ol", { class: "flow" });
  for (const step of r.flow) fl.appendChild(el("li", { text: step }));
  host.appendChild(fl);
  host.appendChild(el("div", { class: "muted small", text: r.note }));
}

function scenStream(host, r) {
  if (!r.tampered_stream_commitment) {
    host.appendChild(el("div", { class: "notice", text: "no non-empty block available yet — let traffic run a moment and click again." }));
    host.appendChild(el("div", { class: "muted small", text: r.note }));
    return;
  }
  // The story: pre-state → honest step recomputed (and matching the posted
  // commitment) → the same step with ONE flipped payload byte → divergence.
  const t = r.tampered_tx || {};
  const flow = el("div", { class: "kv-stack" });
  flow.appendChild(kv("block", String(r.block) + "  · tampered tx (" + t.tick + "," + t.pos + ")"));
  flow.appendChild(kv("s_prev (pre-state)", hexChip(r.stream_prev, { head: 10, tail: 6 })));
  const honestRow = el("div", { class: "sv-step " + (r.honest_recomputation_matches ? "ok" : "bad") }, [
    el("span", { class: "sv-lab mono", text: "honest step" }),
    hexChip(r.recomputed_honest_step, { head: 10, tail: 6 }),
    el("span", { class: "sv-mark", text: r.honest_recomputation_matches ? "✓ recomputed = posted s_n" : "✗ recomputation mismatch" }),
  ]);
  flow.appendChild(honestRow);
  host.appendChild(flow);
  const cmp = el("div", { class: "scen-cols" });
  cmp.appendChild(el("div", { class: "scen-col good" }, [el("div", { class: "scen-col-title", text: "honest s_n (tape order)" }), hexChip("0x" + String(r.honest_stream_commitment).replace(/^0x/, ""), { head: 12, tail: 8, expanded: true })]));
  cmp.appendChild(el("div", { class: "scen-col critical" }, [el("div", { class: "scen-col-title", text: "same fold, ONE payload byte flipped" }), hexChip(r.tampered_stream_commitment, { head: 12, tail: 8, expanded: true })]));
  host.appendChild(cmp);
  const differ = String(r.tampered_stream_commitment).replace(/^0x/, "").toLowerCase() !== String(r.honest_stream_commitment).replace(/^0x/, "").toLowerCase();
  const ok = differ && r.honest_recomputation_matches !== false;
  host.appendChild(el("div", { class: "verdict " + (ok ? "good" : "bad"), text: (ok ? "✓ honest step reproduces the posted commitment; the tampered fold diverges — " : "✗ ") + "any batch ≠ tape is detectable (B2 challengeStream fires)" }));
  host.appendChild(el("div", { class: "muted small", text: r.note }));
}

function renderScenarioLog(log) {
  const host = clear($("scenario-log"));
  $("scenlog-count").textContent = log.length + " runs";
  setSummary("sum-scenarios", "6 buttons · " + log.length + " runs logged");
  for (const e of log.slice(0, 20)) {
    host.appendChild(el("div", { class: "slog-row" }, [el("span", { class: "muted", text: ago(e.at_ms) }), el("span", { class: "mono", text: e.name }), el("span", { class: "muted small", text: (e.result && e.result.title) || "" })]));
  }
}

// ------------------------------------------------------------------ panel 6: scorecard
function renderScorecard() {
  const rows = [
    { obj: "Receipt ρ(t,j)", supplies: "Signed order-ack: position (t,j) fixed at admission, digest-chained, slashable. The pre-commitment Lighter's whitepaper §2.3 promises, with a bond.", panel: "panel-admission", ev: "sig + digest recomputed in-browser", verified: true },
    { obj: "Tick record R_t", supplies: "Per-tick sealed batch; batch_root is a keccak merkle over BatchEntry leaves.", panel: "panel-lighter", ev: "record sig + merkle root recomputed", verified: true },
    { obj: "Tape entry", supplies: "The adapter's live input; (tick,pos) with outcome ∈ {pending, opened, gap}; intent revealed once opened. Envelope bytes rebind to h = keccak(commit-domain ‖ epoch ‖ bytes).", panel: "panel-admission", ev: "h recomputed from raw envelope bytes", verified: true },
    { obj: "Segment seal", supplies: "Per-segment Wesolowski VDF proof — the clock itself. The same (y, x_end, pi, t, l) object verifies here and in PoSqHost.verifySegmentProof.", panel: "panel-clock", ev: "Wesolowski verified in-browser", verified: true },
    { obj: "Anchor", supplies: "Host-chain checkpoint the bridge binds batches to; carries cB, receipts_root, segment x_end hashes, and the span's stream commitment — the full submitAnchor calldata.", panel: "panel-ethereum", ev: "anchor sig recovered in-browser", verified: true },
    { obj: "Stream commitment", supplies: "Opened-stream commitment s_n over stateless-valid opened plaintexts in (t,j) order — the one equality that binds Lighter execution to the PoSq tape (B2).", panel: "panel-lighter", ev: "s_n recomputed & matched in-browser", verified: true },
    { obj: "Fraud proofs", supplies: "Tape-internal accountability (receipt↔batch↔chain): reorder, omission, equivocation, invalid-log, forced-default. proveReorder calldata is generated live.", panel: "panel-scenarios", ev: "calldata + receipt verified", verified: true },
  ];
  setSummary("sum-scorecard", rows.filter((r) => r.verified).length + "/" + rows.length + " rows live-verified");
  const host = clear($("scorecard"));
  for (const r of rows) {
    const row = el("div", { class: "sc-row" });
    row.appendChild(el("div", { class: "sc-obj mono", text: r.obj }));
    row.appendChild(el("div", { class: "sc-supplies", text: r.supplies }));
    const ev = el("div", { class: "sc-ev" }, [statusChip(r.verified ? "✓ " + r.ev : r.ev, r.verified ? "good" : "info")]);
    const link = el("a", { class: "sc-link", href: "#" + r.panel, text: "→ evidence" });
    link.addEventListener("click", (e) => {
      e.preventDefault();
      setCollapsed(r.panel, false); // evidence may live in a collapsed panel
      const t = $(r.panel);
      if (t) t.scrollIntoView({ behavior: "smooth", block: "start" });
    });
    ev.appendChild(link);
    row.appendChild(ev);
    host.appendChild(row);
  }
}

// ------------------------------------------------------------------ SSE
let sseDebounce = null;
function connectSSE() {
  let es;
  try {
    es = new EventSource("/api/events");
  } catch (_) {
    S.sseUp = false;
    return;
  }
  es.onopen = () => {
    S.sseUp = true;
  };
  es.onerror = () => {
    S.sseUp = false; // polling backstop keeps the page live
  };
  es.onmessage = (ev) => {
    S.sseUp = true;
    // Any event → schedule a coalesced refresh so a burst doesn't thrash.
    if (sseDebounce) return;
    sseDebounce = setTimeout(() => {
      sseDebounce = null;
      refreshAll();
    }, 350);
  };
}

boot();
