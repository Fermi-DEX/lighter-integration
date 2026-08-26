// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IBigMulMod} from "./PoSqHost.sol";

/// @title BigMulMod — 2048-bit modular multiplication for PoSqHost.
///
/// Implements `IBigMulMod.mulmod2048(a, b, n) = (a * b) mod n` over 256-byte
/// (2048-bit) big-endian operands. The EVM has no wide-mulmod precompile, so
/// `PoSqHost.verifySegmentProof` delegates the final Wesolowski multiplication
/// `pi^l · y^r mod N` here.
///
/// Method (correctness over micro-optimization):
///   1. Schoolbook multiply `a * b` into a 4096-bit (512-byte) product using
///      64-bit limbs in inline Yul. Each partial product of two 64-bit limbs
///      is < 2^128, so it never overflows a 256-bit EVM word, and a whole
///      column of ≤ 32 such products stays < 2^133 — the accumulation is
///      exact with no carry-loss subtleties.
///   2. Reduce the 512-byte product modulo the 256-byte `n` with the modexp
///      precompile (0x05) using exponent 1: `product^1 mod n = product mod n`.
///      EIP-198 modexp accepts a base wider than the modulus, so this is a
///      clean, precompile-backed long-division reduction.
///
/// Identity used in step 2: for any B, `B^1 mod M == B mod M`, and the modexp
/// precompile computes `B^E mod M` for arbitrary-length B — hence a 512-byte
/// base against a 256-byte modulus yields the 256-byte remainder directly.
contract BigMulMod is IBigMulMod {
    /// @notice (a * b) mod n for 256-byte big-endian a, b, n.
    /// @param a 256-byte big-endian multiplicand (may be ≥ or < n).
    /// @param b 256-byte big-endian multiplier (may be ≥ or < n).
    /// @param n 256-byte big-endian modulus (must be non-zero).
    /// @return 256-byte big-endian (a * b) mod n.
    function mulmod2048(bytes memory a, bytes memory b, bytes memory n)
        external
        view
        returns (bytes memory)
    {
        require(a.length == 256 && b.length == 256 && n.length == 256, "operand length");
        bytes memory product = _mul256x256(a, b); // 512-byte big-endian a*b
        return _reduce(product, n); // (a*b) mod n, 256-byte big-endian
    }

    /// @dev Schoolbook 2048x2048 -> 4096-bit multiply with 64-bit limbs.
    ///      Returns a fresh 512-byte big-endian buffer holding a*b exactly.
    function _mul256x256(bytes memory a, bytes memory b)
        internal
        pure
        returns (bytes memory product)
    {
        product = new bytes(512);
        assembly {
            let MASK := 0xffffffffffffffff // 2^64 - 1
            let aptr := add(a, 32) // 256 bytes of `a`, big-endian
            let bptr := add(b, 32)
            let pptr := add(product, 32) // 512-byte output, big-endian

            // Scratch: 32 a-limbs, 32 b-limbs, 64 column accumulators, all
            // little-endian (index 0 = least significant 64 bits). Memory past
            // the free pointer is zero-initialised by the EVM, so `col` starts
            // clean; we never publish the free pointer, so this scratch is
            // transient (the next Solidity allocation reuses it — fine, we are
            // done with it by then).
            let aL := mload(0x40)
            let bL := add(aL, 1024) // 32 words
            let col := add(aL, 2048) // + 32 words

            // --- split each 256-bit word into four 64-bit limbs ---
            // Little-endian word k (k=0 = least significant) sits at the
            // big-endian byte offset 32*(7-k).
            for { let k := 0 } lt(k, 8) { k := add(k, 1) } {
                let wa := mload(add(aptr, mul(32, sub(7, k))))
                let wb := mload(add(bptr, mul(32, sub(7, k))))
                let base := mul(32, mul(4, k))
                mstore(add(aL, base), and(wa, MASK))
                mstore(add(aL, add(base, 32)), and(shr(64, wa), MASK))
                mstore(add(aL, add(base, 64)), and(shr(128, wa), MASK))
                mstore(add(aL, add(base, 96)), and(shr(192, wa), MASK))
                mstore(add(bL, base), and(wb, MASK))
                mstore(add(bL, add(base, 32)), and(shr(64, wb), MASK))
                mstore(add(bL, add(base, 64)), and(shr(128, wb), MASK))
                mstore(add(bL, add(base, 96)), and(shr(192, wb), MASK))
            }

            // --- accumulate column sums: col[i+j] += aL[i] * bL[j] ---
            // Each product < 2^128; each column receives ≤ 32 of them, so the
            // running sum stays < 2^133 and never overflows a 256-bit word.
            for { let i := 0 } lt(i, 32) { i := add(i, 1) } {
                let ai := mload(add(aL, mul(32, i)))
                for { let j := 0 } lt(j, 32) { j := add(j, 1) } {
                    let cptr := add(col, mul(32, add(i, j)))
                    mstore(cptr, add(mload(cptr), mul(ai, mload(add(bL, mul(32, j))))))
                }
            }

            // --- carry-propagate the 64 column sums into 64 output limbs, then
            //     pack four little-endian limbs per 256-bit word and store
            //     big-endian (most significant word first). ---
            let carry := 0
            for { let m := 0 } lt(m, 16) { m := add(m, 1) } {
                let word := 0
                for { let s := 0 } lt(s, 4) { s := add(s, 1) } {
                    let kk := add(mul(4, m), s)
                    let v := add(mload(add(col, mul(32, kk))), carry)
                    word := or(word, shl(mul(64, s), and(v, MASK)))
                    carry := shr(64, v)
                }
                // Little-endian output word m -> big-endian position (15 - m).
                mstore(add(pptr, mul(32, sub(15, m))), word)
            }
            // carry is provably 0 here: a*b < 2^4096 fits in the 64 limbs.
        }
    }

    /// @dev Reduce a 512-byte big-endian value modulo a 256-byte big-endian
    ///      modulus via the modexp precompile with exponent 1.
    function _reduce(bytes memory wide, bytes memory n)
        internal
        view
        returns (bytes memory out)
    {
        out = new bytes(256);
        // modexp input layout (EIP-198): Blen ‖ Elen ‖ Mlen ‖ B ‖ E ‖ M.
        // Blen = 512, Elen = 1, Mlen = 256, E = 0x01.
        bytes memory input =
            abi.encodePacked(uint256(512), uint256(1), uint256(256), wide, bytes1(0x01), n);
        assembly {
            if iszero(staticcall(gas(), 0x05, add(input, 32), mload(input), add(out, 32), 256)) {
                revert(0, 0)
            }
        }
    }
}
