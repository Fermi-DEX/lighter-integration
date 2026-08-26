// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {PoSqHostTestBase} from "./PoSqHostHarness.sol";
import {LighterBridgeDemo} from "../src/LighterBridgeDemo.sol";

/// Coverage for LighterBridgeDemo bound to a REAL PoSqHost (the harness), with
/// a genuinely signed anchor submitted so the bridge's `_findCoveringAnchor`
/// scan reads real anchor state through the auto-generated getter.
contract LighterBridgeDemoTest is PoSqHostTestBase {
    LighterBridgeDemo internal bridge;

    // Anchor covering [firstTick,lastTick] with commitment cB.
    uint64 internal constant A_FIRST = 0;
    uint64 internal constant A_LAST = 1000;
    bytes32 internal constant CB = keccak256("batch-commitment-cB");

    function setUp() public {
        _deployHost();
        bridge = new LighterBridgeDemo(address(host));

        bytes32[] memory noSegs = new bytes32[](0);
        _submitAnchor(
            0, 4, A_FIRST, A_LAST, keccak256("xB"), CB,
            keccak256("receiptsRoot"), keccak256("da"), keccak256("transcript"), noSegs
        );
    }

    // ------------------------------------------------------------------
    // Constructor
    // ------------------------------------------------------------------

    function test_constructorRejectsZeroHost() public {
        vm.expectRevert(bytes("host zero"));
        new LighterBridgeDemo(address(0));
    }

    function test_hostWired() public view {
        assertEq(address(bridge.host()), address(host), "host wired");
    }

    // ------------------------------------------------------------------
    // B1 — proposeSpan happy path
    // ------------------------------------------------------------------

    function test_proposeSpan_happyPath() public {
        uint64 firstTick = 100;
        uint64 lastTick = 200;
        uint64 lastSegment = 4;
        bytes32 streamCommitment = keccak256("s_n");

        uint256 spanId =
            bridge.proposeSpan(EPOCH, firstTick, lastTick, lastSegment, CB, streamCommitment);
        assertEq(spanId, 0, "first span id");
        assertEq(bridge.spanCount(), 1, "span stored");

        (
            uint64 epoch,
            uint64 sFirst,
            uint64 sLast,
            uint64 sSeg,
            bytes32 cb,
            bytes32 sc,
            uint256 anchorId,
            uint64 acceptedAt,
            uint64 challengeDeadline,
            bool rejected
        ) = bridge.spans(0);

        assertEq(epoch, EPOCH, "epoch");
        assertEq(sFirst, firstTick, "firstTick");
        assertEq(sLast, lastTick, "lastTick");
        assertEq(sSeg, lastSegment, "lastSegment");
        assertEq(cb, CB, "cB");
        assertEq(sc, streamCommitment, "streamCommitment stored");
        assertEq(anchorId, 0, "bound to anchor 0");
        assertEq(uint256(acceptedAt), block.timestamp, "acceptedAt");
        assertEq(
            uint256(challengeDeadline),
            block.timestamp + host.challengeWindow(),
            "deadline = acceptedAt + window"
        );
        assertFalse(rejected, "not rejected");
    }

    /// A span exactly matching the anchor's full [firstTick,lastTick] is covered.
    function test_proposeSpan_exactSpanCovered() public {
        uint256 spanId = bridge.proposeSpan(EPOCH, A_FIRST, A_LAST, 4, CB, keccak256("s"));
        assertEq(spanId, 0, "covered by exact anchor");
    }

    // ------------------------------------------------------------------
    // B1 — validation / revert branches
    // ------------------------------------------------------------------

    function test_proposeSpan_emptySpanReverts() public {
        vm.expectRevert(bytes("empty span"));
        bridge.proposeSpan(EPOCH, 200, 100, 4, CB, keccak256("s")); // lastTick < firstTick
    }

    function test_proposeSpan_noCoveringAnchor_wrongCb() public {
        vm.expectRevert(bytes("no covering anchor"));
        bridge.proposeSpan(EPOCH, 100, 200, 4, keccak256("different-cB"), keccak256("s"));
    }

    function test_proposeSpan_noCoveringAnchor_tickOutOfRange() public {
        // lastTick beyond the anchor's coverage.
        vm.expectRevert(bytes("no covering anchor"));
        bridge.proposeSpan(EPOCH, 900, A_LAST + 1, 4, CB, keccak256("s"));
    }

    function test_proposeSpan_noAnchorsAtAll() public {
        // Fresh host with no anchors -> the scan finds nothing.
        _deployHost();
        LighterBridgeDemo emptyBridge = new LighterBridgeDemo(address(host));
        vm.expectRevert(bytes("no covering anchor"));
        emptyBridge.proposeSpan(EPOCH, 0, 1, 0, CB, keccak256("s"));
    }

    // ------------------------------------------------------------------
    // B2 — challengeStream / challenge-window logic
    // ------------------------------------------------------------------

    function test_challengeStream_withinWindowRejectsSpan() public {
        uint256 spanId = bridge.proposeSpan(EPOCH, 100, 200, 4, CB, keccak256("s"));
        bridge.challengeStream(spanId, 3, true);

        (,,,,,,,,, bool rejected) = bridge.spans(spanId);
        assertTrue(rejected, "span rejected by successful challenge");
        assertFalse(bridge.isFinal(spanId), "rejected span is never final");
    }

    function test_challengeStream_notProvenReverts() public {
        uint256 spanId = bridge.proposeSpan(EPOCH, 100, 200, 4, CB, keccak256("s"));
        vm.expectRevert(bytes("mismatch not proven"));
        bridge.challengeStream(spanId, 3, false);
    }

    function test_challengeStream_unknownSpanReverts() public {
        vm.expectRevert(bytes("no span"));
        bridge.challengeStream(0, 0, true);
    }

    function test_challengeStream_afterWindowReverts() public {
        uint256 spanId = bridge.proposeSpan(EPOCH, 100, 200, 4, CB, keccak256("s"));
        vm.warp(block.timestamp + CHALLENGE_WINDOW + 1);
        vm.expectRevert(bytes("window closed"));
        bridge.challengeStream(spanId, 3, true);
    }

    function test_challengeStream_doubleChallengeReverts() public {
        uint256 spanId = bridge.proposeSpan(EPOCH, 100, 200, 4, CB, keccak256("s"));
        bridge.challengeStream(spanId, 3, true);
        vm.expectRevert(bytes("already rejected"));
        bridge.challengeStream(spanId, 4, true);
    }

    // ------------------------------------------------------------------
    // Finality view
    // ------------------------------------------------------------------

    function test_isFinal_falseBeforeDeadlineTrueAfter() public {
        uint256 spanId = bridge.proposeSpan(EPOCH, 100, 200, 4, CB, keccak256("s"));
        assertFalse(bridge.isFinal(spanId), "not final within window");

        vm.warp(block.timestamp + CHALLENGE_WINDOW + 1);
        assertTrue(bridge.isFinal(spanId), "final once window elapses unchallenged");
    }

    function test_isFinal_unknownSpanReverts() public {
        vm.expectRevert(bytes("no span"));
        bridge.isFinal(0);
    }

    function test_spanCount_tracksProposals() public {
        assertEq(bridge.spanCount(), 0, "no spans initially");
        bridge.proposeSpan(EPOCH, 100, 200, 4, CB, keccak256("s1"));
        bridge.proposeSpan(EPOCH, 300, 400, 4, CB, keccak256("s2"));
        assertEq(bridge.spanCount(), 2, "two spans");
    }
}
