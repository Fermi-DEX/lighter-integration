#!/usr/bin/env python3
"""Generate a real PoSq Wesolowski segment-proof vector for PoSqHost.verifySegmentProof.

Pure Python re-implementation of the exact byte layouts in crates/vdf/src/posq.rs.
Only sha256 is needed (no keccak). Python's built-in pow(b, e, m) does the t
squarings in C, so t ~ 10^6 is fast.
"""
import hashlib, secrets, time, json, sys

N = int("25195908475657893494027183240048398571429282126204032027777137836043662020707595556264018525880784406918290641249515082189298559149176184502808489120072844992687392807287776735971418347270261896375014971824691165077613379859095700097330459748808428401797429100642458691817195118746121515172654632282216869987549182422433637259085141865462043576798423387184774447920739934236584823824281198163815010674810451660377306056201619676256133844143603833904414952634432190114657544454178424020924616515723350778707749817125772467962926386356373289912154831438167899885040445364023527381951378636564391212010397122822120720357")

def pad256(x: int) -> bytes:
    return x.to_bytes(256, "big")

def hash_to_group(domain: bytes, inp: bytes) -> int:
    """crate posq.rs Group::hash_to_group: 9x counter-mode sha256, mod N, square."""
    wide = b""
    for counter in range(9):
        h = hashlib.sha256()
        h.update(b"posq-hash-to-group")
        h.update(len(domain).to_bytes(8, "big"))
        h.update(domain)
        h.update(counter.to_bytes(4, "big"))
        h.update(inp)
        wide += h.digest()
    x = int.from_bytes(wide, "big") % N
    return (x * x) % N

def challenge_digest(domain: bytes, x: int, y: int, t: int) -> int:
    """PoSqHost / posq.rs challenge candidate (big-endian uint256).

    posq.rs `challenge_prime` sha256-hashes the preimage, then `hash_to_prime`
    sha256-hashes that digest AGAIN before the increment-to-prime walk — so the
    candidate is the double hash sha256(sha256(preimage)).
    """
    h = hashlib.sha256()
    h.update(b"posq-wesolowski-challenge")
    h.update(len(domain).to_bytes(8, "big"))
    h.update(domain)
    h.update(pad256(x))
    h.update(pad256(y))
    h.update(t.to_bytes(8, "big"))
    return int.from_bytes(hashlib.sha256(h.digest()).digest(), "big")

# Miller-Rabin: must produce a genuine prime so PoSqHost.isProbablePrime
# (fixed bases 2..37) accepts it. Use those bases plus random rounds.
_SMALL = [2,3,5,7,11,13,17,19,23,29,31,37]
def is_probable_prime(n: int, rounds: int = 30) -> bool:
    if n < 2:
        return False
    for p in _SMALL:
        if n % p == 0:
            return n == p
    d = n - 1
    r = 0
    while d % 2 == 0:
        d //= 2
        r += 1
    bases = list(_SMALL) + [secrets.randbelow(n - 3) + 2 for _ in range(rounds)]
    for a in bases:
        a %= n
        if a < 2:
            continue
        x = pow(a, d, n)
        if x == 1 or x == n - 1:
            continue
        for _ in range(r - 1):
            x = (x * x) % n
            if x == n - 1:
                break
        else:
            return False
    return True

def next_prime_from(c: int) -> int:
    cand = c if c % 2 == 1 else c + 1
    while True:
        if is_probable_prime(cand):
            return cand
        cand += 2

def main():
    domain = b"posq-segment-proof-v1"   # PoSqHost SEGMENT_DOMAIN
    q = 3600         # reference q_squarings
    seg = 256        # reference segment_ticks
    t = q * seg      # 921600 squarings (= PoSqHost.qSquarings * segmentTicks)

    # Base x = a QR_N element, derived like a real segment start-of-segment state.
    x = hash_to_group(domain, b"demo-segment-0")

    t0 = time.time()
    e = 1 << t                       # 2^t as a big int
    y = pow(x, e, N)                 # y = x^(2^t) mod N  (the segment output)
    t1 = time.time()

    c = challenge_digest(domain, x, y, t)
    l = next_prime_from(c)
    assert l >= c and (l - c) <= 65536, f"challenge gap too large: {l-c}"

    quotient = e // l                # floor(2^t / l)
    r = e % l                        # 2^t mod l
    pi = pow(x, quotient, N)         # pi = x^floor(2^t/l) mod N
    t2 = time.time()

    # Self-check the Wesolowski identity exactly as PoSqHost does:
    # pi^l * x^r == y  (mod N)
    lhs = (pow(pi, l, N) * pow(x, r, N)) % N
    assert lhs == y, "Wesolowski identity failed!"
    # cross-check r via modexp of 2
    assert r == pow(2, t, l)

    vec = {
        "domain": domain.decode(),
        "q_squarings": q,
        "segment_ticks": seg,
        "t": t,
        "N_hex": "0x" + pad256(N).hex(),
        "x_base_hex": "0x" + pad256(x).hex(),      # PoSqHost param `y`  (base)
        "xEnd_output_hex": "0x" + pad256(y).hex(), # PoSqHost param `xEnd` (output)
        "pi_hex": "0x" + pad256(pi).hex(),         # PoSqHost param `pi`
        "l_dec": str(l),                            # PoSqHost param `l` (uint256)
        "l_hex": hex(l),
        "challenge_c_dec": str(c),
        "l_minus_c": l - c,
        "r_2powt_mod_l_dec": str(r),
    }
    print(json.dumps(vec, indent=2))
    sys.stderr.write(f"[timing] y=x^(2^t): {t1-t0:.2f}s  pi: {t2-t1:.2f}s\n")

if __name__ == "__main__":
    main()
