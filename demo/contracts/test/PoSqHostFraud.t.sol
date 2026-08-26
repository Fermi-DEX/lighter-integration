// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {PoSqHost} from "../src/PoSqHost.sol";
import {PoSqHostTestBase} from "./PoSqHostHarness.sol";

/// Fraud-proof and forced-inclusion coverage (§10.2 / §10.3) against a
/// PoSqHost whose configured sequencer is a key the test controls (vm.sign).
/// Signed TickRecord / Receipt / anchor objects are built through the harness's
/// exposed encoders, so the bytes signed here are byte-identical to those the
/// contract re-derives and checks.
contract PoSqHostFraudTest is PoSqHostTestBase {
    uint256 internal constant BOND = 10 ether;

    function setUp() public {
        _deployHost();
        _fundBond(BOND);
    }

    // ------------------------------------------------------------------
    // Proof 2 — Reorder
    // ------------------------------------------------------------------

    /// A receipt places `h` at (tick, pos); the signed batch root places a
    /// *different* entry at the same slot -> reorder proven -> full slash,
    /// rescue mode, 10% bounty to the challenger.
    function test_proveReorder_slashesBondAndEntersRescue() public {
        uint64 tick = 42;
        uint32 pos = 0;

        PoSqHost.BatchEntryFields memory conflicting = PoSqHost.BatchEntryFields({
            tick: tick,
            pos: pos,
            h: keccak256("evil-commitment"),
            bucket: 1,
            ticketId: bytes16(uint128(0xC0FFEE)),
            receiptSigHash: keccak256("batch-entry-sig")
        });
        // Single-leaf batch: root == the leaf, empty Merkle path.
        bytes32 root = host.xEntryLeaf(conflicting);
        bytes32[] memory path = new bytes32[](0);

        PoSqHost.TickRecordFields memory record = PoSqHost.TickRecordFields({
            segment: 3,
            tick: tick,
            xHash: keccak256("x"),
            cPrev: keccak256("cPrev"),
            cT: keccak256("cT"),
            batchRoot: root,
            daRef: keccak256("da")
        });
        bytes memory recordSig = _signRecord(record);

        // The honest receipt for a DIFFERENT commitment at the same slot.
        PoSqHost.ReceiptFields memory receipt = _makeReceipt(
            tick, pos, keccak256("honest-commitment"), 1, bytes16(uint128(1)), keccak256("dPrev")
        );
        bytes memory receiptSig = _signReceipt(receipt);

        assertEq(host.bond(), BOND, "bond funded");
        assertFalse(host.rescueMode(), "not yet in rescue");
        uint256 balBefore = address(this).balance;

        host.proveReorder(receipt, receiptSig, record, recordSig, conflicting, path);

        assertEq(host.bond(), 0, "bond fully slashed");
        assertTrue(host.rescueMode(), "rescue mode entered");
        assertEq(address(this).balance, balBefore + BOND / 10, "10% bounty paid");
    }

    /// A well-formed honest pair (the batch entry at the slot IS the receipt's
    /// commitment) cannot be turned into a reorder proof: the `h != receipt.h`
    /// guard reverts and nothing is slashed.
    function test_proveReorder_honestPairDoesNotSlash() public {
        uint64 tick = 42;
        uint32 pos = 0;
        bytes32 h = keccak256("agreed-commitment");

        PoSqHost.BatchEntryFields memory entry = PoSqHost.BatchEntryFields({
            tick: tick,
            pos: pos,
            h: h, // SAME as the receipt -> honest
            bucket: 1,
            ticketId: bytes16(uint128(1)),
            receiptSigHash: keccak256("sig")
        });
        bytes32 root = host.xEntryLeaf(entry);
        bytes32[] memory path = new bytes32[](0);

        PoSqHost.TickRecordFields memory record = PoSqHost.TickRecordFields({
            segment: 3,
            tick: tick,
            xHash: keccak256("x"),
            cPrev: keccak256("cPrev"),
            cT: keccak256("cT"),
            batchRoot: root,
            daRef: keccak256("da")
        });
        bytes memory recordSig = _signRecord(record);

        PoSqHost.ReceiptFields memory receipt =
            _makeReceipt(tick, pos, h, 1, bytes16(uint128(1)), keccak256("dPrev"));
        bytes memory receiptSig = _signReceipt(receipt);

        vm.expectRevert(bytes("same commitment"));
        host.proveReorder(receipt, receiptSig, record, recordSig, entry, path);

        assertEq(host.bond(), BOND, "bond untouched");
        assertFalse(host.rescueMode(), "no rescue for honest pair");
    }

    // ------------------------------------------------------------------
    // Proof 1 — Equivocation
    // ------------------------------------------------------------------

    /// Two signed tick records for the same (epoch, tick) with different
    /// content -> equivocation -> slash.
    function test_proveEquivocation_slashes() public {
        PoSqHost.TickRecordFields memory a = PoSqHost.TickRecordFields({
            segment: 1,
            tick: 9,
            xHash: keccak256("x"),
            cPrev: keccak256("cp"),
            cT: keccak256("ct"),
            batchRoot: keccak256("rootA"),
            daRef: keccak256("da")
        });
        // Independent struct (memory assignment aliases, so build a fresh one):
        // same (epoch, tick) but a different batch root == equivocation.
        PoSqHost.TickRecordFields memory b = PoSqHost.TickRecordFields({
            segment: 1,
            tick: 9,
            xHash: keccak256("x"),
            cPrev: keccak256("cp"),
            cT: keccak256("ct"),
            batchRoot: keccak256("rootB"),
            daRef: keccak256("da")
        });

        bytes memory sigA = _signRecord(a);
        bytes memory sigB = _signRecord(b);

        host.proveEquivocation(a, sigA, b, sigB);
        assertEq(host.bond(), 0, "slashed");
        assertTrue(host.rescueMode(), "rescue");
    }

    // ------------------------------------------------------------------
    // Proof 5 — Invalid log transition
    // ------------------------------------------------------------------

    /// Consecutive signed records whose log chaining is broken -> slash.
    function test_proveInvalidLog_slashes() public {
        PoSqHost.TickRecordFields memory prev = PoSqHost.TickRecordFields({
            segment: 1,
            tick: 100,
            xHash: keccak256("xp"),
            cPrev: keccak256("cpp"),
            cT: keccak256("prev-cT"),
            batchRoot: keccak256("rootP"),
            daRef: keccak256("daP")
        });
        PoSqHost.TickRecordFields memory curr = PoSqHost.TickRecordFields({
            segment: 1,
            tick: 101, // consecutive
            xHash: keccak256("xc"),
            cPrev: prev.cT, // correct link on cPrev...
            cT: keccak256("bogus-cT"), // ...but cT is not the honest logStep
            batchRoot: keccak256("rootC"),
            daRef: keccak256("daC")
        });
        bytes32 expected = host.xLogStep(curr.cPrev, curr.tick, curr.batchRoot, curr.xHash);
        assertTrue(curr.cT != expected, "test setup: cT must be wrong");

        bytes memory sigPrev = _signRecord(prev);
        bytes memory sigCurr = _signRecord(curr);

        host.proveInvalidLog(prev, sigPrev, curr, sigCurr);
        assertEq(host.bond(), 0, "slashed");
        assertTrue(host.rescueMode(), "rescue");
    }

    // ------------------------------------------------------------------
    // Forced inclusion (§10.3): enqueue -> discharge (happy path)
    // ------------------------------------------------------------------

    function test_forceInclude_thenDischarge() public {
        bytes32 h = keccak256("forced-tx-envelope");
        host.forceInclude(h);

        (bytes32 storedH,, bool dischargedBefore) = host.forcedQueue(0);
        assertEq(storedH, h, "commitment stored");
        assertFalse(dischargedBefore, "not yet discharged");

        // Sequencer discharges by exhibiting a receipt for h.
        PoSqHost.ReceiptFields memory receipt =
            _makeReceipt(7, 0, h, 2, bytes16(uint128(0xAB)), keccak256("dPrev"));
        bytes memory sig = _signReceipt(receipt);

        host.dischargeForced(0, receipt, sig);

        (,, bool dischargedAfter) = host.forcedQueue(0);
        assertTrue(dischargedAfter, "discharged");
        assertFalse(host.rescueMode(), "no slash on a proper discharge");
    }

    /// dischargeForced with a receipt for the wrong commitment reverts.
    function test_dischargeForced_wrongCommitmentReverts() public {
        host.forceInclude(keccak256("forced-tx"));
        PoSqHost.ReceiptFields memory receipt = _makeReceipt(
            7, 0, keccak256("some-other-h"), 2, bytes16(uint128(1)), keccak256("dPrev")
        );
        bytes memory sig = _signReceipt(receipt);
        vm.expectRevert(bytes("wrong h"));
        host.dischargeForced(0, receipt, sig);
    }

    // ------------------------------------------------------------------
    // Proof 6 — Forced-inclusion default after the deadline
    // ------------------------------------------------------------------

    /// An undischarged forced entry, plus an anchor accepted past
    /// enqueue + challengeWindow that covers >= fForce ticks -> default proven
    /// -> slash. Uses the block.timestamp clock (submitAnchor stamps acceptedAt
    /// = block.timestamp; forceInclude stamps enqueuedAt = block.timestamp).
    function test_proveForcedDefault_afterDeadline() public {
        vm.warp(1_000);
        host.forceInclude(keccak256("ignored-forced-tx")); // enqueuedAt = 1000

        // Move past enqueue + challengeWindow so a later anchor is "on time".
        vm.warp(1_000 + CHALLENGE_WINDOW + 1); // 1101

        bytes32[] memory noSegs = new bytes32[](0);
        uint256 anchorId = _submitAnchor(
            0, // firstSegment
            0, // lastSegment
            0, // firstTick
            uint64(F_FORCE + 5), // lastTick -> span 15 >= fForce (10)
            keccak256("xB"),
            keccak256("cB"),
            keccak256("receiptsRoot"),
            keccak256("da"),
            keccak256("transcript"),
            noSegs
        );

        assertEq(host.bond(), BOND, "bond present");
        host.proveForcedDefault(0, anchorId);
        assertEq(host.bond(), 0, "slashed on forced default");
        assertTrue(host.rescueMode(), "rescue");
    }

    /// Before the deadline the default proof is rejected ("too early").
    function test_proveForcedDefault_tooEarlyReverts() public {
        vm.warp(1_000);
        host.forceInclude(keccak256("forced")); // enqueuedAt = 1000

        // Anchor accepted only 1s later: acceptedAt (1001) <= enqueuedAt + window (1100).
        vm.warp(1_001);
        bytes32[] memory noSegs = new bytes32[](0);
        uint256 anchorId = _submitAnchor(
            0, 0, 0, uint64(F_FORCE + 5), keccak256("xB"), keccak256("cB"),
            keccak256("rr"), keccak256("da"), keccak256("tc"), noSegs
        );

        vm.expectRevert(bytes("too early"));
        host.proveForcedDefault(0, anchorId);
        assertEq(host.bond(), BOND, "bond untouched");
    }
}
