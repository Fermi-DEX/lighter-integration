// Self-test for crypto.js — run with node. Asserts the known-answer vector.
const C = require("./crypto.js");

let pass = 0, fail = 0;
function check(name, got, want) {
  const ok = String(got).toLowerCase() === String(want).toLowerCase();
  console.log((ok ? "PASS" : "FAIL") + "  " + name);
  if (!ok) { console.log("   got:  " + got); console.log("   want: " + want); fail++; } else pass++;
}

// 1. keccak256("") known answer
check("keccak256(\"\")",
  C.keccak256Hex(new Uint8Array(0)),
  "0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");

// 2. keccak256("abc") sanity (known keccak-256 value)
check("keccak256(\"abc\")",
  C.keccak256Hex(C.ascii("abc")),
  "0x4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45");

// 3. THE KNOWN-ANSWER VECTOR — receipt recovers to sequencer address
const receipt = {
  epoch: 0, tick: 5724, pos: 0,
  h: "0xf6d8affd24f2b4745d4a9c18a7f222152a71c437e089e153b14f671d5afd2f28",
  bucket: 0, window_start: 5722, window_len: 10,
  ticket_id: "0x00000000000000c58aab2b9ab58754cc",
  x_prev_hash: "0x9fb8d8d1894486b47c28bd7bad898be2849414c624b01609671999a05bca7e95",
  c_prev: "0xc595e0bb0186ad0cb4b5a73c33c64feb63c767560a1fae589eb32845e8c838c4",
  d_prev: "0xbcd65dc88c3f87048c2f869c1388cf5427da29925770600d98e4450f763d1275",
  d: "0xe0f92ca061fffade2ef3d7c442ca228b39359e78304485bd39352dbd6dd96e99",
  sig: "0x1f50bae32faa14efd955dec9ef5822d7fb7506b374473e1e86a82d3a432af0473db501ca421450f173fa2b7b5ee6440370ea9c2acc8106e690e04f7cb0cef92f1c",
};
const EXPECT = "0x1220d3518a79895965d5fab43d5a098cf3f91d82";
const vr = C.verifyReceipt(receipt, EXPECT);
console.log("\n-- receipt verification --");
console.log("preimage keccak (msgHash): " + vr.hash);
console.log("recovered sequencer:       " + vr.recovered);
console.log("expected  sequencer:       " + EXPECT);
check("RECOVER == sequencer address", vr.recovered, EXPECT);
check("sigMatches flag", vr.sigMatches, true);

// 4. digest hash-link check (d == keccak256(...))
console.log("\n-- digest hash-link --");
console.log("computed d: " + vr.digestComputed);
console.log("signed   d: " + receipt.d);
check("digest link d matches", vr.digestMatches, true);

console.log("\n" + pass + " passed, " + fail + " failed");
process.exit(fail ? 1 : 0);
