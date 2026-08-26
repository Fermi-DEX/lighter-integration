// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {PoSqHost, IBigMulMod} from "../src/PoSqHost.sol";
import {BigMulMod} from "../src/BigMulMod.sol";

/// Harness exposing PoSqHost's internal message-encoding helpers so tests sign
/// EXACTLY the bytes the contract will re-derive and check — guaranteeing
/// encoding parity between the signed test objects and the fraud-proof
/// verifiers (no hand-mirrored encoding to drift out of sync).
contract PoSqHostHarness is PoSqHost {
    constructor(
        address _sequencer,
        uint64 _epoch,
        bytes memory _modulusN,
        uint256 _bondFloor,
        uint64 _q,
        uint64 _segmentTicks,
        uint64 _fForce,
        uint64 _challengeWindow,
        IBigMulMod _bigMulMod
    )
        PoSqHost(
            _sequencer,
            _epoch,
            _modulusN,
            _bondFloor,
            _q,
            _segmentTicks,
            _fForce,
            _challengeWindow,
            _bigMulMod
        )
    {}

    function xTickRecordMessage(TickRecordFields memory f)
        external
        view
        returns (bytes memory)
    {
        return tickRecordMessage(f);
    }

    function xReceiptMessage(ReceiptFields memory f) external view returns (bytes memory) {
        return receiptMessage(f);
    }

    function xDigestStep(
        bytes32 dPrev,
        uint64 tick,
        uint32 pos,
        bytes32 h,
        uint8 bucket,
        bytes16 ticketId
    ) external pure returns (bytes32) {
        return digestStep(dPrev, tick, pos, h, bucket, ticketId);
    }

    function xLogStep(bytes32 cPrev, uint64 tick, bytes32 batchRoot, bytes32 xHash)
        external
        pure
        returns (bytes32)
    {
        return logStep(cPrev, tick, batchRoot, xHash);
    }

    function xEntryLeaf(BatchEntryFields memory e) external pure returns (bytes32) {
        return entryLeaf(e);
    }

    function xMerkleParent(bytes32 l, bytes32 r) external pure returns (bytes32) {
        return merkleParent(l, r);
    }

    function xMerkleRoot(bytes32[] memory leaves) external pure returns (bytes32) {
        return merkleRoot(leaves);
    }
}

/// Shared base with a configured sequencer key (via vm.sign), a funded bond,
/// and helpers to build/sign the domain-tagged objects the fraud proofs and
/// anchor path consume.
abstract contract PoSqHostTestBase is Test {
    PoSqHostHarness internal host;

    // Sequencer identity the host is configured with; tests own the key.
    uint256 internal constant SEQ_PK = uint256(0xA11CE5EED);
    address internal seq;

    uint64 internal constant EPOCH = 7;
    uint64 internal constant Q = 3600;
    uint64 internal constant SEG_TICKS = 256;
    uint64 internal constant F_FORCE = 10;
    uint64 internal constant CHALLENGE_WINDOW = 100;

    uint256 internal anchorCount; // mirrors host.anchors.length (no getter on host)

    receive() external payable {}

    function _deployHost() internal {
        seq = vm.addr(SEQ_PK);
        BigMulMod big = new BigMulMod();
        host = new PoSqHostHarness(
            seq,
            EPOCH,
            _dummyModulus(),
            0, // bondFloor
            Q,
            SEG_TICKS,
            F_FORCE,
            CHALLENGE_WINDOW,
            IBigMulMod(address(big))
        );
        anchorCount = 0;
    }

    /// A valid 256-byte, non-zero modulus. Its value is irrelevant to the fraud
    /// and anchor paths (only verifySegmentProof consumes it arithmetically).
    function _dummyModulus() internal pure returns (bytes memory m) {
        m = new bytes(256);
        for (uint256 i = 0; i < 256; i++) {
            m[i] = bytes1(uint8((i * 7 + 1) % 251 + 1)); // never zero
        }
    }

    /// Fund the host bond via its `receive()` (bond += msg.value).
    function _fundBond(uint256 amount) internal {
        vm.deal(address(this), address(this).balance + amount);
        (bool ok,) = address(host).call{value: amount}("");
        require(ok, "fund bond");
    }

    function _sign(bytes32 digest) internal pure returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(SEQ_PK, digest);
        return abi.encodePacked(r, s, v);
    }

    function _signRecord(PoSqHost.TickRecordFields memory f) internal view returns (bytes memory) {
        return _sign(keccak256(host.xTickRecordMessage(f)));
    }

    function _signReceipt(PoSqHost.ReceiptFields memory f) internal view returns (bytes memory) {
        return _sign(keccak256(host.xReceiptMessage(f)));
    }

    /// Build a receipt with a valid digest-link (`d == digestStep(...)`) so
    /// `checkReceiptSig` passes on the structural check as well as the signature.
    function _makeReceipt(
        uint64 tick,
        uint32 pos,
        bytes32 h,
        uint8 bucket,
        bytes16 ticketId,
        bytes32 dPrev
    ) internal view returns (PoSqHost.ReceiptFields memory r) {
        r.tick = tick;
        r.pos = pos;
        r.h = h;
        r.bucket = bucket;
        r.windowStart = 0;
        r.windowLen = 1;
        r.ticketId = ticketId;
        r.xPrevHash = keccak256("xPrev");
        r.cPrev = keccak256("cPrev");
        r.dPrev = dPrev;
        r.d = host.xDigestStep(dPrev, tick, pos, h, bucket, ticketId);
    }

    /// Submit a signed anchor spanning [firstTick,lastTick] with commitment cB.
    /// Mirrors PoSqHost.submitAnchor's inline anchor::signing_message; a wrong
    /// encoding would trip the on-chain "anchor sig" guard, so this helper is
    /// self-checking.
    function _submitAnchor(
        uint64 firstSegment,
        uint64 lastSegment,
        uint64 firstTick,
        uint64 lastTick,
        bytes32 xBHash,
        bytes32 cB,
        bytes32 receiptsRoot,
        bytes32 daAttestation,
        bytes32 transcriptCommitment,
        bytes32[] memory segHashes
    ) internal returns (uint256 id) {
        bytes memory m = abi.encodePacked(
            hex"06",
            "posq-anchor-v1",
            EPOCH,
            firstSegment,
            lastSegment,
            firstTick,
            lastTick,
            xBHash,
            cB
        );
        for (uint256 i = 0; i < segHashes.length; i++) {
            m = abi.encodePacked(m, segHashes[i]);
        }
        m = abi.encodePacked(m, receiptsRoot, daAttestation, transcriptCommitment);
        bytes memory sig = _sign(keccak256(m));
        host.submitAnchor(
            firstTick,
            lastTick,
            xBHash,
            cB,
            receiptsRoot,
            daAttestation,
            transcriptCommitment,
            sig,
            firstSegment,
            lastSegment,
            segHashes
        );
        id = anchorCount;
        anchorCount++;
    }
}
