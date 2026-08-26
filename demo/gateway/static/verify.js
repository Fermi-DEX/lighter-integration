/*
 * verify.js — the page's client-side re-derivation layer.
 *
 * Thin, human-legible wrappers over vendor/crypto.js. Every function returns
 * { ok, checks:[{label, ok, detail}], ... } so a panel can render a badge that
 * spells out EXACTLY what was re-derived in the browser. The node checker
 * (vendor/pagecheck.cjs) calls these same functions against the live API, so
 * "verified in-browser" and "verified in CI" are literally the same code.
 *
 * Works as a classic <script> (attaches globalThis.PosqVerify) and under node
 * (module.exports), mirroring crypto.js.
 */
(function () {
  "use strict";
  const C =
    typeof module !== "undefined" && module.exports
      ? require("./vendor/crypto.js")
      : globalThis.PosqCrypto;
  if (!C) throw new Error("verify.js: PosqCrypto not loaded");

  const short = (h, n) => {
    if (h == null) return "";
    const s = String(h);
    if (s.length <= 2 + n * 2 + 4) return s;
    return s.slice(0, 2 + n) + "…" + s.slice(-n);
  };

  // ---------------------------------------------------------------- sha256
  // Compact pure-JS SHA-256 (FIPS 180-4). Needed for the Wesolowski challenge
  // derivation (candidate = sha256(sha256(preimage))); crypto.js only ships
  // keccak. KAT: sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad.
  const SHA_K = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ];
  function sha256(bytesLike) {
    const msg = bytesLike instanceof Uint8Array ? bytesLike : C.hexToBytes(bytesLike);
    const bitLen = msg.length * 8;
    const padded = new Uint8Array((((msg.length + 8) >> 6) + 1) << 6);
    padded.set(msg);
    padded[msg.length] = 0x80;
    const dv = new DataView(padded.buffer);
    dv.setUint32(padded.length - 8, Math.floor(bitLen / 0x100000000));
    dv.setUint32(padded.length - 4, bitLen >>> 0);
    let h0 = 0x6a09e667, h1 = 0xbb67ae85, h2 = 0x3c6ef372, h3 = 0xa54ff53a,
      h4 = 0x510e527f, h5 = 0x9b05688c, h6 = 0x1f83d9ab, h7 = 0x5be0cd19;
    const w = new Uint32Array(64);
    const rotr = (x, n) => (x >>> n) | (x << (32 - n));
    for (let off = 0; off < padded.length; off += 64) {
      for (let i = 0; i < 16; i++) w[i] = dv.getUint32(off + i * 4);
      for (let i = 16; i < 64; i++) {
        const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
        const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
        w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
      }
      let a = h0, b = h1, c = h2, d = h3, e = h4, f = h5, g = h6, h = h7;
      for (let i = 0; i < 64; i++) {
        const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        const ch = (e & f) ^ (~e & g);
        const t1 = (h + S1 + ch + SHA_K[i] + w[i]) >>> 0;
        const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        const maj = (a & b) ^ (a & c) ^ (b & c);
        const t2 = (S0 + maj) >>> 0;
        h = g; g = f; f = e; e = (d + t1) >>> 0;
        d = c; c = b; b = a; a = (t1 + t2) >>> 0;
      }
      h0 = (h0 + a) >>> 0; h1 = (h1 + b) >>> 0; h2 = (h2 + c) >>> 0; h3 = (h3 + d) >>> 0;
      h4 = (h4 + e) >>> 0; h5 = (h5 + f) >>> 0; h6 = (h6 + g) >>> 0; h7 = (h7 + h) >>> 0;
    }
    const out = new Uint8Array(32);
    const odv = new DataView(out.buffer);
    [h0, h1, h2, h3, h4, h5, h6, h7].forEach((v, i) => odv.setUint32(i * 4, v));
    return out;
  }

  // ------------------------------------------------- envelope decode + binding
  // Byte layout per crates/sequencer/src/envelope.rs (encode/decode):
  //   [0..2) magic  [2] version  [3] bucket  [4..12) namespace u64be
  //   [12..20) window.start u64be  [20..22) window.len u16be  [22] delay_class
  //   [23..39) client nonce η           — 39-byte AAD header prefix —
  //   [39..55) ticket.id  [55] denom  [56] ticket.bucket  [57..122) issuer_sig
  //   [122..124) u_len u16be  [124..380) u (right-aligned, 256B field)
  //   [380..382) body_len u16be  [382..) AEAD body + zero pad
  function decodeEnvelopeHeader(envelopeHex) {
    const b = C.hexToBytes(envelopeHex);
    if (b.length < 382) return { error: "envelope too short: " + b.length + " bytes" };
    const u64at = (o) => {
      let v = 0n;
      for (let i = 0; i < 8; i++) v = (v << 8n) | BigInt(b[o + i]);
      return v;
    };
    const bodyLen = (b[380] << 8) | b[381];
    return {
      totalBytes: b.length,
      magic: C.bytesToHex(b.slice(0, 2)),
      version: b[2],
      bucket: b[3],
      namespace: Number(u64at(4)),
      windowStart: Number(u64at(12)),
      windowLen: (b[20] << 8) | b[21],
      delayClass: b[22],
      nonce: C.bytesToHex(b.slice(23, 39)),
      headerAadHex: C.bytesToHex(b.slice(0, 39)),
      ticketId: C.bytesToHex(b.slice(39, 55)),
      ticketDenomination: b[55],
      ticketBucket: b[56],
      uLen: (b[122] << 8) | b[123],
      bodyLen,
      bodyHex: C.bytesToHex(b.slice(382, Math.min(b.length, 382 + bodyLen))),
    };
  }

  // h = keccak256("posq-commit-v1" ‖ u64be(epoch) ‖ envelope_bytes)
  // (envelope.rs commitment_hash). The binding the tape's `h` promises.
  function commitmentHash(epoch, envelopeHex) {
    const m = C.concatBytes(C.ascii("posq-commit-v1"), C.u64(epoch), C.hexToBytes(envelopeHex));
    return C.bytesToHex(C.keccak256(m));
  }

  function envelopeBinding(epoch, envelopeHex, expectedH) {
    const computed = commitmentHash(epoch, envelopeHex);
    const ok = computed.toLowerCase() === String(expectedH || "").toLowerCase();
    return {
      ok,
      computed,
      checks: [
        {
          label: "h = keccak(posq-commit-v1 ‖ epoch ‖ envelope bytes)",
          ok,
          detail: ok
            ? "recomputed from " + (envelopeHex.length / 2) + " raw bytes = receipt h " + short(expectedH, 8)
            : "computed " + short(computed, 8) + " ≠ " + short(expectedH, 8),
        },
      ],
    };
  }

  // ------------------------------------------------------------- wesolowski
  // Full Wesolowski verification of a segment seal, exactly PoSqHost
  // .verifySegmentProof / vdf::posq::verify_sequential:
  //   1. challenge: candidate = sha256(sha256("posq-wesolowski-challenge" ‖
  //      u64be(|domain|) ‖ "posq-segment-proof-v1" ‖ pad256(y) ‖ pad256(x_end)
  //      ‖ u64be(t))); require l ≥ candidate, l − candidate ≤ 65536, l prime.
  //   2. equation: r = 2^t mod l;  pi^l · y^r mod N == x_end.
  // ~512 modmuls of 2048-bit BigInts (~tens of ms) — call on-demand per seal.
  const WES_DOMAIN = "posq-segment-proof-v1";

  function millerRabin(n) {
    if (n < 2n) return false;
    const smallPrimes = [2n, 3n, 5n, 7n, 11n, 13n, 17n, 19n, 23n, 29n, 31n, 37n];
    for (const p of smallPrimes) {
      if (n === p) return true;
      if (n % p === 0n) return false;
    }
    let d = n - 1n, s = 0n;
    while ((d & 1n) === 0n) {
      d >>= 1n;
      s++;
    }
    for (const a of smallPrimes) {
      let x = C.modPow(a, d, n);
      if (x === 1n || x === n - 1n) continue;
      let composite = true;
      for (let i = 1n; i < s; i++) {
        x = (x * x) % n;
        if (x === n - 1n) {
          composite = false;
          break;
        }
      }
      if (composite) return false;
    }
    return true;
  }

  function pad256(bytes) {
    const out = new Uint8Array(256);
    out.set(bytes.slice(-256), 256 - Math.min(256, bytes.length));
    return out;
  }

  // seg: /api/segments item {y, x_end, pi, t, l, ...}; modulusHex from /api/params.
  function wesolowski(seg, modulusHex) {
    const N = C.bytesToBig(C.hexToBytes(modulusHex));
    const y = C.bytesToBig(C.hexToBytes(seg.y));
    const xEnd = C.bytesToBig(C.hexToBytes(seg.x_end));
    const pi = C.bytesToBig(C.hexToBytes(seg.pi));
    const l = BigInt(seg.l);
    const t = BigInt(seg.t);
    const checks = [];
    // 1. Fiat-Shamir challenge derivation (double sha256, then range + prime).
    const preimage = C.concatBytes(
      C.ascii("posq-wesolowski-challenge"),
      C.u64(BigInt(WES_DOMAIN.length)),
      C.ascii(WES_DOMAIN),
      pad256(C.hexToBytes(seg.y)),
      pad256(C.hexToBytes(seg.x_end)),
      C.u64(t)
    );
    const candidate = C.bytesToBig(sha256(sha256(preimage)));
    const inRange = l >= candidate && l - candidate <= 65536n;
    checks.push({
      label: "challenge l = HashToPrime(domain ‖ y ‖ x_end ‖ t)",
      ok: inRange,
      detail: inRange
        ? "l − sha256² candidate = " + (l - candidate) + " (≤ 65536)"
        : "l out of range of the recomputed candidate (Δ = " + (l - candidate) + ")",
    });
    const prime = millerRabin(l);
    checks.push({
      label: "l probable-prime (Miller-Rabin, 12 bases)",
      ok: prime,
      detail: prime ? "l ≈ 2^" + l.toString(2).length : "l is composite",
    });
    // 2. The verification equation.
    const r = C.modPow(2n, t, l);
    const lhs = (C.modPow(pi, l, N) * C.modPow(y, r, N)) % N;
    const eqOk = lhs === xEnd;
    checks.push({
      label: "pi^l · y^r ≡ x_end (mod N),  r = 2^t mod l",
      ok: eqOk,
      detail: eqOk
        ? t + " squarings attested by one 2048-bit equation"
        : "lhs ≠ x_end — proof invalid",
    });
    return { ok: checks.every((c) => c.ok), checks, candidate: "0x" + candidate.toString(16), l: seg.l };
  }

  // ------------------------------------------------------------- anchor sig
  // signing_message (anchor.rs): 0x06 ‖ "posq-anchor-v1" ‖ u64be epoch ‖
  // u64be first_segment ‖ u64be last_segment ‖ u64be first_tick ‖ u64be
  // last_tick ‖ x_b_hash ‖ c_b ‖ segment_x_end_hashes… ‖ receipts_root ‖
  // da_attestation ‖ transcript_commitment — keccak256, secp256k1 recover.
  function anchorSig(a, seqAddr) {
    const parts = [
      C.u8(0x06),
      C.ascii("posq-anchor-v1"),
      C.u64(a.epoch),
      C.u64(a.first_segment),
      C.u64(a.last_segment),
      C.u64(a.first_tick),
      C.u64(a.last_tick),
      C.b32(a.x_b_hash),
      C.b32(a.c_b),
    ];
    for (const h of a.segment_x_end_hashes || []) parts.push(C.b32(h));
    parts.push(C.b32(a.receipts_root), C.b32(a.da_attestation), C.b32(a.transcript_commitment));
    const hash = C.keccak256(C.concatBytes.apply(null, parts));
    let recovered = null, err = null;
    try {
      recovered = C.recoverAddress(hash, a.sig);
    } catch (e) {
      err = e.message;
    }
    const ok = !!(recovered && seqAddr && recovered.toLowerCase() === String(seqAddr).toLowerCase());
    return {
      ok,
      recovered,
      checks: [
        {
          label: "anchor sig (submitAnchor calldata) → sequencer key",
          ok,
          detail: recovered
            ? short(recovered, 10) + (ok ? " = sequencer" : " ≠ " + short(seqAddr, 10))
            : "recover failed: " + (err || "?"),
        },
      ],
    };
  }

  // A signed admission outcome (receipt | rejection | full_window) — recover
  // the secp256k1 signature and (for receipts) recompute the digest-chain link.
  function admissionOutcome(obj, seqAddr) {
    const kind = obj.type || (obj.d != null ? "receipt" : obj.reason_code != null ? "rejection" : "full_window");
    const res = C.verifySigned(obj, seqAddr);
    const checks = [];
    checks.push({
      label: "secp256k1 recover → sequencer key",
      ok: !!res.sigMatches,
      detail: res.recovered
        ? short(res.recovered, 10) + (res.sigMatches ? " = sequencer" : " ≠ " + short(seqAddr, 10))
        : "recover failed: " + (res.error || "?"),
    });
    if (kind === "receipt") {
      checks.push({
        label: "digest-chain link d = keccak(d_prev‖…)",
        ok: !!res.digestMatches,
        detail: res.digestMatches
          ? "recomputed " + short(res.digestComputed, 8) + " = signed d"
          : "computed " + short(res.digestComputed, 8) + " ≠ signed " + short(obj.d, 8),
      });
    }
    return { kind, ok: checks.every((c) => c.ok), recovered: res.recovered, checks, raw: res };
  }

  // A sealed tick: tick-record signature + merkle batch_root over its entries.
  function sealedTick(tick, seqAddr) {
    const rv = C.verifyTickRecord(tick.record, seqAddr);
    const br = C.verifyBatchRoot(tick.entries, tick.record.batch_root);
    const checks = [
      {
        label: "tick-record sig → sequencer key",
        ok: !!rv.sigMatches,
        detail: rv.recovered ? short(rv.recovered, 10) + (rv.sigMatches ? " = sequencer" : " ≠ seq") : "recover failed",
      },
      {
        label: "merkle batch_root over " + (tick.entries || []).length + " leaf/leaves",
        ok: !!br.matches,
        detail: br.matches ? "recomputed = signed root " + short(tick.record.batch_root, 8) : "recomputed " + short(br.root, 8) + " ≠ signed",
      },
    ];
    return { ok: checks.every((c) => c.ok), checks, recovered: rv.recovered, root: br.root };
  }

  // A Lighter block's opened-stream commitment, anchored on the previous
  // block's commitment (or epoch genesis for the first block).
  function streamBlock(block, prevCommitment) {
    const sv = C.verifyStream(block, prevCommitment);
    const nTx = (block.txs || []).length;
    const checks = [
      {
        label: "fold " + nTx + " tx → stream commitment s_n",
        ok: !!sv.matches,
        detail: sv.matches
          ? "recomputed = posted " + short("0x" + sv.expected, 8)
          : "recomputed " + short(sv.final, 8) + " ≠ posted " + short("0x" + sv.expected, 8),
      },
      {
        label: "per-tx step chain (" + sv.anchor + " anchor)",
        ok: !!sv.stepsAllOk,
        detail: sv.stepsAllOk ? "every s_i = signed stream_after" : "a per-tx step diverged",
      },
    ];
    return { ok: checks.every((c) => c.ok), checks, steps: sv.steps, final: sv.final };
  }

  const api = {
    admissionOutcome, sealedTick, streamBlock, short, C,
    sha256, decodeEnvelopeHeader, commitmentHash, envelopeBinding,
    wesolowski, millerRabin, anchorSig,
  };
  if (typeof globalThis !== "undefined") globalThis.PosqVerify = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})();
