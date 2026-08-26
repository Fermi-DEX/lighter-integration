// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {PoSqHost, IBigMulMod} from "../src/PoSqHost.sol";
import {BigMulMod} from "../src/BigMulMod.sol";

/// Native on-chain Wesolowski verification against ONE real PoSq segment proof.
///
/// The vector in vectors/segment.json is a genuine proof emitted by the pure-
/// Python re-implementation of the `crates/vdf` byte layouts
/// (vectors/gen_segment_vector.py) and satisfies the Wesolowski identity
///   pi^l * y^(2^t mod l) == xEnd   (mod N).
/// This suite loads it, drives `PoSqHost.verifySegmentProof`, reports the
/// verification gas (the headline demo number), and checks the negative paths
/// (corrupt proof, wrong T, wrong segment output).
contract PoSqHostSegmentProofTest is Test {
    PoSqHost internal host;

    // Vector fields (see segment.json _schema for the param mapping).
    bytes internal y; // verifySegmentProof param `y`  (segment base x_s)
    bytes internal xEnd; // verifySegmentProof param `xEnd` (segment output y_s)
    bytes internal pi; // verifySegmentProof param `pi` (Wesolowski proof)
    bytes internal N; // RSA-2048 modulus
    uint64 internal t; // squarings = q * segmentTicks
    uint256 internal l; // challenge prime
    uint64 internal qSquarings;
    uint64 internal segmentTicks;

    function setUp() public {
        string memory json = vm.readFile("./vectors/segment.json");
        y = vm.parseJsonBytes(json, ".y_base");
        xEnd = vm.parseJsonBytes(json, ".xEnd_output");
        pi = vm.parseJsonBytes(json, ".pi");
        N = vm.parseJsonBytes(json, ".N");
        t = uint64(vm.parseJsonUint(json, ".t"));
        qSquarings = uint64(vm.parseJsonUint(json, ".q_squarings"));
        segmentTicks = uint64(vm.parseJsonUint(json, ".segment_ticks"));
        // `l` is a decimal string (exceeds JSON-safe integer range).
        l = vm.parseUint(vm.parseJsonString(json, ".l"));

        assertEq(y.length, 256, "y len");
        assertEq(xEnd.length, 256, "xEnd len");
        assertEq(pi.length, 256, "pi len");
        assertEq(N.length, 256, "N len");
        assertEq(uint256(t), uint256(qSquarings) * uint256(segmentTicks), "t == q*seg");

        BigMulMod big = new BigMulMod();
        // Only qSquarings/segmentTicks (the T check) and modulusN matter here;
        // the rest are placeholders unrelated to segment verification.
        host = new PoSqHost(
            address(0xBEEF), // sequencer (unused by verifySegmentProof)
            1, // epoch
            N, // modulusN — must be the vector's N
            0, // bondFloor
            qSquarings, // q  -> gates the T check
            segmentTicks, // segmentTicks
            0, // fForce
            0, // challengeWindow
            IBigMulMod(address(big))
        );
    }

    // ------------------------------------------------------------------
    // Happy path: the real proof verifies, and we report the gas.
    // ------------------------------------------------------------------

    function test_realSegmentProofVerifies() public view {
        bool ok = host.verifySegmentProof(y, xEnd, pi, t, l);
        assertTrue(ok, "genuine Wesolowski segment proof must verify");
    }

    /// Reports the native verification gas — the single strongest demo artifact
    /// (Ethereum verifying a live VDF segment proof). Asserts a sane upper bound
    /// so a regression that blows up cost fails CI.
    function test_segmentProofGas() public {
        uint256 gasBefore = gasleft();
        bool ok = host.verifySegmentProof(y, xEnd, pi, t, l);
        uint256 used = gasBefore - gasleft();
        assertTrue(ok, "must verify");
        emit log_named_uint("verifySegmentProof gas (incl. external CALL)", used);
        // Two 2048-bit modexps + Miller-Rabin + 4096-bit mulmod + reduction.
        // Comfortably under this bound; tighten if it ever regresses.
        assertLt(used, 3_000_000, "segment verification gas out of bound");
    }

    // ------------------------------------------------------------------
    // Negative paths.
    // ------------------------------------------------------------------

    /// Corrupt a single byte of the proof pi -> identity fails -> returns false.
    function test_corruptProofFailsVerification() public view {
        bytes memory badPi = pi;
        badPi[128] = bytes1(uint8(badPi[128]) ^ 0x01); // flip one bit
        bool ok = host.verifySegmentProof(y, xEnd, badPi, t, l);
        assertFalse(ok, "corrupted proof must not verify");
    }

    /// Corrupt the very last byte of pi as well (independent perturbation).
    function test_corruptProofLastByteFails() public view {
        bytes memory badPi = pi;
        badPi[255] = bytes1(uint8(badPi[255]) ^ 0xff);
        bool ok = host.verifySegmentProof(y, xEnd, badPi, t, l);
        assertFalse(ok, "corrupted proof (last byte) must not verify");
    }

    /// Wrong T -> the `t == q * segmentTicks` guard reverts.
    function test_wrongTReverts() public {
        vm.expectRevert(bytes("wrong T"));
        host.verifySegmentProof(y, xEnd, pi, t + 1, l);
    }

    /// Wrong segment output (xEnd) -> the recomputed challenge preimage changes,
    /// so the supplied `l` is no longer within the bounded gap of the candidate
    /// prime and the "challenge out of range" guard reverts. (If a perturbation
    /// ever landed in-range, the final identity would still fail -> false; the
    /// helper below accepts either rejection mode.)
    function test_wrongDomainOutputRejected() public {
        bytes memory badEnd = xEnd;
        badEnd[64] = bytes1(uint8(badEnd[64]) ^ 0x01);
        assertFalse(_verifies(y, badEnd, pi, t, l), "wrong output must be rejected");
    }

    /// Wrong base y -> likewise changes the challenge preimage / identity.
    function test_wrongBaseRejected() public {
        bytes memory badY = y;
        badY[0] = bytes1(uint8(badY[0]) ^ 0x01);
        assertFalse(_verifies(badY, xEnd, pi, t, l), "wrong base must be rejected");
    }

    /// Wrong challenge prime `l` (off by a small amount, no longer prime nor the
    /// deterministic candidate) -> guarded revert or false.
    function test_wrongChallengePrimeRejected() public {
        assertFalse(_verifies(y, xEnd, pi, t, l + 2), "wrong l must be rejected");
    }

    /// Verify wrapper that treats a guarded revert as "did not verify", so
    /// negative cases can perturb inputs without caring which guard trips.
    function _verifies(
        bytes memory _y,
        bytes memory _xEnd,
        bytes memory _pi,
        uint64 _t,
        uint256 _l
    ) internal view returns (bool) {
        try host.verifySegmentProof(_y, _xEnd, _pi, _t, _l) returns (bool ok) {
            return ok;
        } catch {
            return false;
        }
    }
}
