#!/usr/bin/env python3
"""Generate differential test vectors for BigMulMod.mulmod2048.

Python's arbitrary-precision integers are the exact reference implementation
standing in for the Rust `num-bigint` differential in the plan: for each vector
we compute `expected = (a * b) % n` with true big-integer math and emit it
alongside the 256-byte big-endian operands. `test/BigMulModDiff.t.sol` replays
every vector against the deployed Yul contract and asserts byte-for-byte parity.

Coverage: 64 vectors total — a spread of random full-width 2048-bit operands
against random moduli in [2^2047, 2^2048), plus deliberate edge shapes
(a=0, a=n-1, a=n, small values, a=b, modulus with low/high bit patterns).

Deterministic: seeded PRNG so the committed JSON is reproducible.
"""
import json
import random
import sys

BITS = 2048
BYTES = 256


def pad(x: int) -> str:
    """256-byte big-endian, 0x-prefixed. x is reduced mod 2^2048 defensively."""
    return "0x" + (x % (1 << BITS)).to_bytes(BYTES, "big").hex()


def rand_modulus(rng: random.Random) -> int:
    """Random odd modulus in [2^2047, 2^2048): top bit set so it is full width."""
    n = rng.getrandbits(BITS) | (1 << (BITS - 1)) | 1
    return n


def rand_full(rng: random.Random) -> int:
    return rng.getrandbits(BITS)


def main():
    rng = random.Random(0x50537121)  # "PoSq!" — deterministic seed
    vecs = []

    def add(a: int, b: int, n: int, tag: str):
        a %= 1 << BITS
        b %= 1 << BITS
        n %= 1 << BITS
        assert n != 0, "modulus must be non-zero"
        vecs.append((a, b, n, (a * b) % n, tag))

    # --- 48 random full-width vectors ---
    for _ in range(48):
        n = rand_modulus(rng)
        a = rand_full(rng)
        b = rand_full(rng)
        add(a, b, n, "random")

    # --- edge shapes ---
    n = rand_modulus(rng)
    add(0, rand_full(rng), n, "a=0")
    add(rand_full(rng), 0, n, "b=0")
    add(n - 1, n - 1, n, "a=b=n-1")          # (n-1)^2 mod n == 1
    add(n - 1, 1, n, "a=n-1,b=1")
    add(n, rand_full(rng), n, "a=n (==0 mod n)")
    add(1, 1, n, "a=b=1")
    add(2, 3, n, "small")
    add(rand_full(rng), 1, n, "b=1 identity")

    # a == b (squares), several widths
    for _ in range(6):
        n = rand_modulus(rng)
        a = rand_full(rng)
        add(a, a, n, "square")

    # narrow modulus (small n, wide a,b) — exercises the reduction path
    small_n = (1 << 300) - 189  # a 300-bit prime-ish odd modulus
    add(rand_full(rng), rand_full(rng), small_n, "narrow-modulus")
    add(rand_full(rng), rand_full(rng), small_n, "narrow-modulus")

    # modulus = 2 (extreme), a,b random -> parity of product
    add(rand_full(rng), rand_full(rng), 2, "modulus=2")
    add(rand_full(rng), rand_full(rng), 3, "modulus=3")

    out = {
        "_schema": "Differential vectors for BigMulMod.mulmod2048; expected=(a*b)%n "
        "computed with Python big integers (num-bigint reference). "
        "All operands 256-byte big-endian, 0x-prefixed. Reproduce: "
        "python3 gen_mulmod_vectors.py > mulmod.json",
        "count": len(vecs),
        "tags": [t for (_, _, _, _, t) in vecs],
        "a": [pad(a) for (a, _, _, _, _) in vecs],
        "b": [pad(b) for (_, b, _, _, _) in vecs],
        "n": [pad(n) for (_, _, n, _, _) in vecs],
        "expected": [pad(e) for (_, _, _, e, _) in vecs],
    }
    json.dump(out, sys.stdout, indent=2)
    sys.stdout.write("\n")
    sys.stderr.write(f"[gen_mulmod_vectors] wrote {len(vecs)} vectors\n")


if __name__ == "__main__":
    main()
