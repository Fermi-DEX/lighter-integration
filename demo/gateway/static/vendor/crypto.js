/*
 * crypto.js — dependency-free keccak256 + secp256k1 public-key recovery,
 * plus the PoSq / Lighter message layouts, so the dashboard can RE-DERIVE
 * every signed object client-side. No CDN, no build step. Works as a classic
 * <script> (attaches globalThis.PosqCrypto) and under node (module.exports).
 *
 * Everything here is the single source of truth: the node self-test and the
 * browser UI call the exact same functions.
 */
(function () {
  "use strict";

  // ------------------------------------------------------------------ bytes
  function hexToBytes(hex) {
    if (hex == null) return new Uint8Array(0);
    hex = String(hex).trim();
    if (hex.slice(0, 2) === "0x" || hex.slice(0, 2) === "0X") hex = hex.slice(2);
    if (hex.length % 2 !== 0) hex = "0" + hex;
    const out = new Uint8Array(hex.length / 2);
    for (let i = 0; i < out.length; i++) {
      out[i] = parseInt(hex.substr(i * 2, 2), 16);
    }
    return out;
  }
  function bytesToHex(bytes) {
    let s = "0x";
    for (let i = 0; i < bytes.length; i++) s += bytes[i].toString(16).padStart(2, "0");
    return s;
  }
  function ascii(str) {
    const out = new Uint8Array(str.length);
    for (let i = 0; i < str.length; i++) out[i] = str.charCodeAt(i) & 0xff;
    return out;
  }
  function concatBytes() {
    let len = 0;
    for (let i = 0; i < arguments.length; i++) len += arguments[i].length;
    const out = new Uint8Array(len);
    let off = 0;
    for (let i = 0; i < arguments.length; i++) {
      out.set(arguments[i], off);
      off += arguments[i].length;
    }
    return out;
  }
  // big-endian integer encoders (accept Number or BigInt)
  function uBE(value, nbytes) {
    let v = typeof value === "bigint" ? value : BigInt(value);
    const out = new Uint8Array(nbytes);
    for (let i = nbytes - 1; i >= 0; i--) {
      out[i] = Number(v & 0xffn);
      v >>= 8n;
    }
    return out;
  }
  const u8 = (v) => uBE(v, 1);
  const u16 = (v) => uBE(v, 2);
  const u32 = (v) => uBE(v, 4);
  const u64 = (v) => uBE(v, 8);
  // 32-byte field, from a hex string (throws-lenient: pads/truncates to 32)
  function b32(hex) {
    const raw = hexToBytes(hex);
    if (raw.length === 32) return raw;
    const out = new Uint8Array(32);
    out.set(raw.slice(0, 32), Math.max(0, 32 - raw.length));
    return out;
  }
  function b16(hex) {
    const raw = hexToBytes(hex);
    if (raw.length === 16) return raw;
    const out = new Uint8Array(16);
    out.set(raw.slice(0, 16), Math.max(0, 16 - raw.length));
    return out;
  }

  // ------------------------------------------------------------------ keccak256
  // Keccak-f[1600], rate 1088 bits (136 bytes), Keccak padding (0x01 .. 0x80).
  const MASK64 = (1n << 64n) - 1n;
  const KECCAK_RC = [
    0x0000000000000001n, 0x0000000000008082n, 0x800000000000808an, 0x8000000080008000n,
    0x000000000000808bn, 0x0000000080000001n, 0x8000000080008081n, 0x8000000000008009n,
    0x000000000000008an, 0x0000000000000088n, 0x0000000080008009n, 0x000000008000000an,
    0x000000008000808bn, 0x800000000000008bn, 0x8000000000008089n, 0x8000000000008003n,
    0x8000000000008002n, 0x8000000000000080n, 0x000000000000800an, 0x800000008000000an,
    0x8000000080008081n, 0x8000000000008080n, 0x0000000080000001n, 0x8000000080008008n,
  ];
  // rotation offsets r[x][y]
  const KECCAK_R = [
    [0n, 36n, 3n, 41n, 18n],
    [1n, 44n, 10n, 45n, 2n],
    [62n, 6n, 43n, 15n, 61n],
    [28n, 55n, 25n, 21n, 56n],
    [27n, 20n, 39n, 8n, 14n],
  ];
  function rotl64(x, n) {
    if (n === 0n) return x & MASK64;
    return ((x << n) | (x >> (64n - n))) & MASK64;
  }
  function keccakF(s) {
    for (let round = 0; round < 24; round++) {
      // theta
      const C = new Array(5);
      for (let x = 0; x < 5; x++) C[x] = s[x] ^ s[x + 5] ^ s[x + 10] ^ s[x + 15] ^ s[x + 20];
      const D = new Array(5);
      for (let x = 0; x < 5; x++) D[x] = C[(x + 4) % 5] ^ rotl64(C[(x + 1) % 5], 1n);
      for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++) s[x + 5 * y] ^= D[x];
      // rho + pi
      const B = new Array(25).fill(0n);
      for (let x = 0; x < 5; x++)
        for (let y = 0; y < 5; y++)
          B[y + 5 * ((2 * x + 3 * y) % 5)] = rotl64(s[x + 5 * y], KECCAK_R[x][y]);
      // chi
      for (let x = 0; x < 5; x++)
        for (let y = 0; y < 5; y++)
          s[x + 5 * y] = B[x + 5 * y] ^ ((B[(x + 1) % 5 + 5 * y] ^ MASK64) & B[(x + 2) % 5 + 5 * y]);
      // iota
      s[0] ^= KECCAK_RC[round];
    }
  }
  function keccak256(bytesLike) {
    const msg = bytesLike instanceof Uint8Array ? bytesLike : hexToBytes(bytesLike);
    const rate = 136;
    const s = new Array(25).fill(0n);
    // absorb full blocks + padded final
    const padLen = rate - (msg.length % rate);
    const padded = new Uint8Array(msg.length + padLen);
    padded.set(msg, 0);
    padded[msg.length] ^= 0x01; // keccak domain
    padded[padded.length - 1] ^= 0x80;
    for (let off = 0; off < padded.length; off += rate) {
      for (let i = 0; i < rate / 8; i++) {
        let lane = 0n;
        for (let j = 7; j >= 0; j--) lane = (lane << 8n) | BigInt(padded[off + i * 8 + j]);
        s[i] ^= lane;
      }
      keccakF(s);
    }
    // squeeze first 32 bytes (lanes 0..3, little-endian)
    const out = new Uint8Array(32);
    for (let i = 0; i < 4; i++) {
      let lane = s[i];
      for (let j = 0; j < 8; j++) {
        out[i * 8 + j] = Number(lane & 0xffn);
        lane >>= 8n;
      }
    }
    return out;
  }
  const keccak256Hex = (b) => bytesToHex(keccak256(b));

  // ------------------------------------------------------------------ secp256k1
  const P = 0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2fn;
  const N = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;
  const GX = 0x79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798n;
  const GY = 0x483ada7726a3c4655da4fbfc0e1108a8fd17b448a68554199c47d08ffb10d4b8n;
  const P14 = (P + 1n) >> 2n; // (p+1)/4, since p % 4 == 3

  function mod(a, m) {
    const r = a % m;
    return r >= 0n ? r : r + m;
  }
  function modPow(base, exp, m) {
    base = mod(base, m);
    let result = 1n;
    while (exp > 0n) {
      if (exp & 1n) result = (result * base) % m;
      base = (base * base) % m;
      exp >>= 1n;
    }
    return result;
  }
  const modInv = (a, m) => modPow(mod(a, m), m - 2n, m); // m prime

  // Jacobian point ops on y^2 = x^3 + 7 (a = 0). Identity = Z==0.
  function jDouble(X1, Y1, Z1) {
    if (Y1 === 0n || Z1 === 0n) return [0n, 1n, 0n];
    const A = (X1 * X1) % P;
    const B = (Y1 * Y1) % P;
    const C = (B * B) % P;
    let D = ((X1 + B) * (X1 + B) - A - C) % P;
    D = (2n * mod(D, P)) % P;
    const E = (3n * A) % P;
    const F = (E * E) % P;
    const X3 = mod(F - 2n * D, P);
    const Y3 = mod(E * mod(D - X3, P) - 8n * C, P);
    const Z3 = mod(2n * Y1 * Z1, P);
    return [X3, Y3, Z3];
  }
  function jAdd(X1, Y1, Z1, X2, Y2, Z2) {
    if (Z1 === 0n) return [X2, Y2, Z2];
    if (Z2 === 0n) return [X1, Y1, Z1];
    const Z1Z1 = (Z1 * Z1) % P;
    const Z2Z2 = (Z2 * Z2) % P;
    const U1 = (X1 * Z2Z2) % P;
    const U2 = (X2 * Z1Z1) % P;
    const S1 = (((Y1 * Z2) % P) * Z2Z2) % P;
    const S2 = (((Y2 * Z1) % P) * Z1Z1) % P;
    const H = mod(U2 - U1, P);
    const r = mod(2n * (S2 - S1), P);
    if (H === 0n) {
      if (r === 0n) return jDouble(X1, Y1, Z1);
      return [0n, 1n, 0n]; // infinity
    }
    const HH = (H * H) % P;
    const I = (4n * HH) % P;
    const J = (H * I) % P;
    const V = (U1 * I) % P;
    const X3 = mod(r * r - J - 2n * V, P);
    const Y3 = mod(r * mod(V - X3, P) - 2n * S1 * J, P);
    const Z3 = mod((mod((Z1 + Z2) * (Z1 + Z2), P) - Z1Z1 - Z2Z2) * H, P);
    return [X3, Y3, Z3];
  }
  function jMul(k, X, Y) {
    let RX = 0n, RY = 1n, RZ = 0n; // identity
    let AX = X, AY = Y, AZ = 1n;
    while (k > 0n) {
      if (k & 1n) [RX, RY, RZ] = jAdd(RX, RY, RZ, AX, AY, AZ);
      [AX, AY, AZ] = jDouble(AX, AY, AZ);
      k >>= 1n;
    }
    return [RX, RY, RZ];
  }
  function jToAffine(X, Y, Z) {
    if (Z === 0n) return null;
    const zi = modInv(Z, P);
    const zi2 = (zi * zi) % P;
    const zi3 = (zi2 * zi) % P;
    return [(X * zi2) % P, (Y * zi3) % P];
  }

  // Recover uncompressed pubkey (64-byte X||Y) from (hash, r, s, recid).
  function recoverPubkey(hashBytes, r, s, recid) {
    if (typeof r !== "bigint") r = BigInt(r);
    if (typeof s !== "bigint") s = BigInt(s);
    if (r <= 0n || r >= N || s <= 0n || s >= N) throw new Error("bad r/s range");
    const z = bytesToBig(hashBytes);
    // x = r + (recid>>1)*n
    let x = r + (BigInt(recid >> 1) * N);
    if (x >= P) throw new Error("x >= p");
    // y from curve, matching parity = recid&1
    const alpha = mod(x * x % P * x + 7n, P);
    let y = modPow(alpha, P14, P);
    if ((y & 1n) !== BigInt(recid & 1)) y = P - y;
    // verify point is on curve
    if (mod(y * y - alpha, P) !== 0n) throw new Error("point not on curve");
    const rInv = modInv(r, N);
    const u1 = mod(-z * rInv, N);
    const u2 = mod(s * rInv, N);
    const [g1x, g1y, g1z] = jMul(u1, GX, GY);
    const [g2x, g2y, g2z] = jMul(u2, x, y);
    const [qx, qy, qz] = jAdd(g1x, g1y, g1z, g2x, g2y, g2z);
    const aff = jToAffine(qx, qy, qz);
    if (!aff) throw new Error("recovered point at infinity");
    return concatBytes(uBE(aff[0], 32), uBE(aff[1], 32));
  }
  function bytesToBig(bytes) {
    let v = 0n;
    for (let i = 0; i < bytes.length; i++) v = (v << 8n) | BigInt(bytes[i]);
    return v;
  }
  function pubkeyToAddress(pub64) {
    const h = keccak256(pub64);
    return bytesToHex(h.slice(12, 32));
  }
  // sig65 = r(32)||s(32)||v(1), v in {27,28}. Returns 0x-address (lowercase).
  function recoverAddress(hashBytes, sig65) {
    const sig = sig65 instanceof Uint8Array ? sig65 : hexToBytes(sig65);
    if (sig.length !== 65) throw new Error("sig must be 65 bytes, got " + sig.length);
    const r = bytesToBig(sig.slice(0, 32));
    const s = bytesToBig(sig.slice(32, 64));
    let v = sig[64];
    let recid = v >= 27 ? v - 27 : v;
    const pub = recoverPubkey(hashBytes, r, s, recid);
    return pubkeyToAddress(pub);
  }

  // ------------------------------------------------------------------ PoSq layouts
  const T = {
    receipt: ascii("posq-receipt-v1"),
    digest: ascii("posq-digest-v1"),
    tick: ascii("posq-tick-record-v1"),
    rejection: ascii("posq-rejection-v1"),
    fullWindow: ascii("posq-full-window-v1"),
    leaf: ascii("posq-leaf-v1"),
    emptyBatch: ascii("posq-empty-batch-v1"),
    streamGenesis: ascii("lighter-stream-genesis-v1"),
    stream: ascii("lighter-stream-v1"),
  };

  function receiptPreimage(r) {
    return concatBytes(
      u8(0x02), T.receipt,
      u64(r.epoch), u64(r.tick), u32(r.pos), b32(r.h), u8(r.bucket),
      u64(r.window_start), u16(r.window_len), b16(r.ticket_id),
      b32(r.x_prev_hash), b32(r.c_prev), b32(r.d_prev), b32(r.d)
    );
  }
  // d == keccak256("posq-digest-v1" || d_prev || tick || pos || h || bucket || ticket_id)
  function receiptDigest(r) {
    return keccak256(concatBytes(
      T.digest, b32(r.d_prev), u64(r.tick), u32(r.pos), b32(r.h), u8(r.bucket), b16(r.ticket_id)
    ));
  }
  function verifyReceipt(r, expectedAddr) {
    const hash = keccak256(receiptPreimage(r));
    let recovered = null, err = null;
    try { recovered = recoverAddress(hash, r.sig); } catch (e) { err = e.message; }
    const dComputed = bytesToHex(receiptDigest(r));
    const digestMatches = dComputed.toLowerCase() === String(r.d).toLowerCase();
    const sigMatches = recovered && expectedAddr &&
      recovered.toLowerCase() === String(expectedAddr).toLowerCase();
    return { hash: bytesToHex(hash), recovered, sigMatches, digestComputed: dComputed, digestMatches, error: err };
  }

  function tickRecordPreimage(rec) {
    return concatBytes(
      u8(0x01), T.tick,
      u64(rec.epoch), u64(rec.segment), u64(rec.tick),
      b32(rec.x_hash), b32(rec.c_prev), b32(rec.c_t), b32(rec.batch_root), b32(rec.da_ref)
    );
  }
  function verifyTickRecord(rec, expectedAddr) {
    const hash = keccak256(tickRecordPreimage(rec));
    let recovered = null, err = null;
    try { recovered = recoverAddress(hash, rec.sig); } catch (e) { err = e.message; }
    const sigMatches = recovered && expectedAddr &&
      recovered.toLowerCase() === String(expectedAddr).toLowerCase();
    return { hash: bytesToHex(hash), recovered, sigMatches, error: err };
  }

  function rejectionPreimage(r) {
    return concatBytes(
      u8(0x03), T.rejection,
      u64(r.epoch), b32(r.h), u8(r.bucket), u64(r.window_start), u16(r.window_len),
      u8(r.reason_code != null ? r.reason_code : r.reason), b32(r.c_latest)
    );
  }
  function verifyRejection(r, expectedAddr) {
    const hash = keccak256(rejectionPreimage(r));
    let recovered = null, err = null;
    try { recovered = recoverAddress(hash, r.sig); } catch (e) { err = e.message; }
    const sigMatches = recovered && expectedAddr &&
      recovered.toLowerCase() === String(expectedAddr).toLowerCase();
    return { hash: bytesToHex(hash), recovered, sigMatches, error: err };
  }

  function fullWindowPreimage(r) {
    return concatBytes(
      u8(0x04), T.fullWindow,
      u64(r.epoch), u8(r.bucket), u64(r.window_start), u16(r.window_len),
      u32(r.capacity), b32(r.c_latest)
    );
  }
  function verifyFullWindow(r, expectedAddr) {
    const hash = keccak256(fullWindowPreimage(r));
    let recovered = null, err = null;
    try { recovered = recoverAddress(hash, r.sig); } catch (e) { err = e.message; }
    const sigMatches = recovered && expectedAddr &&
      recovered.toLowerCase() === String(expectedAddr).toLowerCase();
    return { hash: bytesToHex(hash), recovered, sigMatches, error: err };
  }

  // Generic: dispatch on object "type" field.
  function verifySigned(obj, expectedAddr) {
    switch (obj.type) {
      case "receipt": return Object.assign({ kind: "receipt" }, verifyReceipt(obj, expectedAddr));
      case "rejection": return Object.assign({ kind: "rejection" }, verifyRejection(obj, expectedAddr));
      case "full_window": return Object.assign({ kind: "full_window" }, verifyFullWindow(obj, expectedAddr));
      default:
        if (obj.d != null && obj.d_prev != null)
          return Object.assign({ kind: "receipt" }, verifyReceipt(obj, expectedAddr));
        if (obj.reason != null || obj.reason_code != null)
          return Object.assign({ kind: "rejection" }, verifyRejection(obj, expectedAddr));
        if (obj.capacity != null)
          return Object.assign({ kind: "full_window" }, verifyFullWindow(obj, expectedAddr));
        return { kind: "unknown", recovered: null, sigMatches: false, error: "unknown signed type" };
    }
  }

  // ------------------------------------------------------------------ merkle
  // leaf = keccak256("posq-leaf-v1" || tick || pos || h || bucket || ticket_id || keccak256(sig[65]))
  function leafHash(entry) {
    const sigHash = keccak256(hexToBytes(entry.receipt_sig));
    return keccak256(concatBytes(
      T.leaf, u64(entry.tick), u32(entry.pos), b32(entry.h), u8(entry.bucket), b16(entry.ticket_id), sigHash
    ));
  }
  function merkleParent(left, right) {
    return keccak256(concatBytes(u8(0x01), left, right));
  }
  function merkleRoot(entries) {
    if (!entries || entries.length === 0) return keccak256(T.emptyBatch);
    let level = entries.map(leafHash);
    while (level.length > 1) {
      const next = [];
      for (let i = 0; i < level.length; i += 2) {
        const l = level[i];
        const r = i + 1 < level.length ? level[i + 1] : level[i]; // odd duplicates last
        next.push(merkleParent(l, r));
      }
      level = next;
    }
    return level[0];
  }
  function verifyBatchRoot(entries, expectedRoot) {
    const root = bytesToHex(merkleRoot(entries));
    return { root, matches: root.toLowerCase() === String(expectedRoot).toLowerCase() };
  }

  // ------------------------------------------------------------------ stream commitment
  // Genesis (first block of an epoch):
  //   s0 = keccak256("lighter-stream-genesis-v1" || epoch || first_tick)
  // Per-tx fold (verified against tx.stream_after live):
  //   s_i = keccak256("lighter-stream-v1" || s_{i-1} || tick || pos || keccak256(payload))
  // The chain is continuous across the epoch; a mid-stream block is anchored on the
  // PREVIOUS block's stream_commitment (startCommitment). Final == this block's commitment.
  function streamGenesis(epoch, firstTick) {
    return keccak256(concatBytes(T.streamGenesis, u64(epoch), u64(firstTick)));
  }
  function streamStep(sPrev, tx) {
    const pHash = keccak256(hexToBytes(tx.payload_hex));
    return keccak256(concatBytes(T.stream, sPrev, u64(tx.tick), u32(tx.pos), pHash));
  }
  // startCommitment: hex of previous block's commitment; if null, uses genesis.
  function streamCommitment(epoch, firstTick, txs, startCommitment) {
    let s, anchor;
    if (startCommitment) { s = b32(startCommitment); anchor = "prev-block"; }
    else { s = streamGenesis(epoch, firstTick); anchor = "genesis"; }
    const steps = [{ label: anchor === "prev-block" ? "s_start (prev block)" : "s0 (genesis)", s: bytesToHex(s), anchor: true }];
    for (const tx of txs) {
      s = streamStep(s, tx);
      const expected = tx.stream_after ? String(tx.stream_after).replace(/^0x/, "").toLowerCase() : null;
      steps.push({
        label: "(" + tx.tick + "," + tx.pos + ")",
        s: bytesToHex(s),
        expected: expected,
        stepMatches: expected ? bytesToHex(s).slice(2) === expected : null,
      });
    }
    return { final: bytesToHex(s), steps, anchor };
  }
  // prevCommitment optional; if omitted and block is not epoch-genesis, only per-step
  // (stream_after) checks are meaningful.
  function verifyStream(block, prevCommitment) {
    const res = streamCommitment(block.epoch, block.first_tick, block.txs || [], prevCommitment);
    const expected = String(block.stream_commitment || "").replace(/^0x/, "").toLowerCase();
    res.expected = expected;
    res.matches = res.final.slice(2) === expected;
    res.stepsAllOk = res.steps.every((st) => st.anchor || st.stepMatches !== false);
    return res;
  }

  // ------------------------------------------------------------------ export
  const api = {
    // primitives
    keccak256, keccak256Hex, recoverPubkey, recoverAddress, pubkeyToAddress,
    // utils
    hexToBytes, bytesToHex, concatBytes, ascii, u8, u16, u32, u64, uBE, b32, b16, bytesToBig,
    modPow, modInv, mod,
    // posq
    receiptPreimage, receiptDigest, verifyReceipt,
    tickRecordPreimage, verifyTickRecord,
    rejectionPreimage, verifyRejection,
    fullWindowPreimage, verifyFullWindow,
    verifySigned,
    leafHash, merkleParent, merkleRoot, verifyBatchRoot,
    streamGenesis, streamStep, streamCommitment, verifyStream,
    // constants
    SECP: { P, N, GX, GY },
  };
  if (typeof globalThis !== "undefined") globalThis.PosqCrypto = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})();
