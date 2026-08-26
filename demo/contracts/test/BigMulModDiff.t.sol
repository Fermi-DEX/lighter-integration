// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {BigMulMod} from "../src/BigMulMod.sol";

/// Differential tests for BigMulMod.mulmod2048.
///
/// Loads vectors/mulmod.json — 66 vectors whose `expected = (a * b) mod n` was
/// computed with Python arbitrary-precision integers (the `num-bigint`
/// reference standing in for the Rust differential per the plan) — and asserts
/// byte-for-byte parity against the deployed Yul limb-arithmetic contract for
/// every vector: random full-width 2048-bit operands plus edge shapes
/// (a=0, a=n-1, a=b squares, tiny/narrow moduli). Reproduce the JSON with
/// `python3 vectors/gen_mulmod_vectors.py > vectors/mulmod.json`.
contract BigMulModDiffTest is Test {
    BigMulMod internal big;

    bytes[] internal aVec;
    bytes[] internal bVec;
    bytes[] internal nVec;
    bytes[] internal expVec;

    function setUp() public {
        big = new BigMulMod();
        string memory json = vm.readFile("./vectors/mulmod.json");
        aVec = vm.parseJsonBytesArray(json, ".a");
        bVec = vm.parseJsonBytesArray(json, ".b");
        nVec = vm.parseJsonBytesArray(json, ".n");
        expVec = vm.parseJsonBytesArray(json, ".expected");
    }

    function test_vectorsWellFormed() public view {
        uint256 n = aVec.length;
        assertGt(n, 0, "no vectors loaded");
        assertEq(bVec.length, n, "b count");
        assertEq(nVec.length, n, "n count");
        assertEq(expVec.length, n, "expected count");
    }

    /// Every differential vector must match the contract exactly.
    function test_allDifferentialVectors() public view {
        uint256 count = aVec.length;
        for (uint256 i = 0; i < count; i++) {
            assertEq(aVec[i].length, 256, "a length");
            assertEq(bVec[i].length, 256, "b length");
            assertEq(nVec[i].length, 256, "n length");
            bytes memory got = big.mulmod2048(aVec[i], bVec[i], nVec[i]);
            assertEq(got.length, 256, "output length");
            assertEq(
                keccak256(got),
                keccak256(expVec[i]),
                string.concat("mulmod mismatch at vector ", vm.toString(i))
            );
        }
    }
}
