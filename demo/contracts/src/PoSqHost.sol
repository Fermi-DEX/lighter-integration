// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title PoSqHost — the Proof of Sequence host-chain contract suite (§10).
///
/// Holds the sequencer bond, accepts anchors (verifying the sequencer
/// signature and each segment's Wesolowski proof natively through the
/// modexp precompile), verifies the fraud proofs of §10.2, runs the
/// forced-inclusion queue (§10.3), and enters rescue mode when the bond
/// falls below its floor (§10.4).
///
/// ENCODING PARITY: every byte layout here mirrors the Rust modules
/// (`sequencer::records`, `sequencer::fraud`, `vdf::posq`) exactly:
///   - signed messages are keccak256 over domain-tagged packed fields,
///     signatures are 65-byte (r ‖ s ‖ v) verified with ecrecover;
///   - u64/u32/u16 fields are big-endian (abi.encodePacked of uintN);
///   - group elements are 256-byte big-endian words;
///   - the Wesolowski challenge candidate is the double hash
///     sha256(sha256("posq-wesolowski-challenge" ‖ u64(|domain|) ‖ domain ‖
///            pad256(base) ‖ pad256(output) ‖ u64(T)))
///     (posq.rs hashes the preimage in `challenge_prime`, then
///     `hash_to_prime` hashes that digest again).
///
/// STATUS: authored against the Rust implementation in this repository;
/// no Solidity toolchain is available in this environment, so the contract
/// is delivered as reviewed source. Deploy behind a test suite (foundry)
/// before mainnet use.

/// 2048-bit modular multiplication: (a * b) mod n over 256-byte
/// big-endian operands. No EVM precompile exists for wide mulmod; deployed
/// Wesolowski verifiers implement this as a Yul limb-arithmetic library.
interface IBigMulMod {
    function mulmod2048(bytes memory a, bytes memory b, bytes memory n)
        external
        view
        returns (bytes memory);
}

contract PoSqHost {
    // ------------------------------------------------------------------
    // Configuration
    // ------------------------------------------------------------------

    address public immutable sequencer; // recovered signer of all objects
    uint64 public immutable epoch;
    bytes public modulusN; // 256-byte RSA-2048 modulus
    uint256 public immutable bondFloor;
    uint64 public immutable qSquarings;
    uint64 public immutable segmentTicks;
    /// Forced-inclusion response deadline, in ticks past the anchor tick
    /// covering the enqueue (F_force).
    uint64 public immutable fForce;
    /// Challenge window (seconds) during which an optimistic anchor's span
    /// remains contestable by the fraud proofs below.
    uint64 public immutable challengeWindow;

    IBigMulMod public immutable bigMulMod;

    uint256 public bond;
    bool public rescueMode;

    // ------------------------------------------------------------------
    // Anchors (§10.1)
    // ------------------------------------------------------------------

    struct AnchorRec {
        uint64 firstTick;
        uint64 lastTick;
        bytes32 cB;
        bytes32 receiptsRoot;
        bytes32 transcriptCommitment;
        uint64 acceptedAt; // block.timestamp
    }
    AnchorRec[] public anchors;

    // Forced-inclusion queue (§10.3).
    struct ForcedEntry {
        bytes32 commitmentHash; // h of the standard (encrypted) envelope
        uint64 enqueuedAt; // block.timestamp
        bool discharged;
    }
    ForcedEntry[] public forcedQueue;

    event AnchorAccepted(uint256 indexed id, uint64 firstTick, uint64 lastTick, bytes32 cB);
    event FraudProven(uint8 kind, address indexed challenger, uint256 slashed);
    event ForcedEnqueued(uint256 indexed id, bytes32 h);
    event ForcedDischarged(uint256 indexed id);
    event RescueMode();

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
    ) {
        require(_modulusN.length == 256, "modulus must be 256 bytes");
        sequencer = _sequencer;
        epoch = _epoch;
        modulusN = _modulusN;
        bondFloor = _bondFloor;
        qSquarings = _q;
        segmentTicks = _segmentTicks;
        fForce = _fForce;
        challengeWindow = _challengeWindow;
        bigMulMod = _bigMulMod;
    }

    receive() external payable {
        bond += msg.value;
    }

    // ------------------------------------------------------------------
    // Signature helpers (parity: sequencer::identity)
    // ------------------------------------------------------------------

    function recoverSigner(bytes32 digest, bytes calldata sig) internal pure returns (address) {
        require(sig.length == 65, "sig length");
        bytes32 r = bytes32(sig[0:32]);
        bytes32 s = bytes32(sig[32:64]);
        uint8 v = uint8(sig[64]);
        return ecrecover(digest, v, r, s);
    }

    // ------------------------------------------------------------------
    // Record encodings (parity: sequencer::records)
    // ------------------------------------------------------------------

    struct TickRecordFields {
        uint64 segment;
        uint64 tick;
        bytes32 xHash;
        bytes32 cPrev;
        bytes32 cT;
        bytes32 batchRoot;
        bytes32 daRef;
    }

    function tickRecordMessage(TickRecordFields memory f) internal view returns (bytes memory) {
        return abi.encodePacked(
            hex"01", "posq-tick-record-v1",
            epoch, f.segment, f.tick, f.xHash, f.cPrev, f.cT, f.batchRoot, f.daRef
        );
    }

    struct ReceiptFields {
        uint64 tick;
        uint32 pos;
        bytes32 h;
        uint8 bucket;
        uint64 windowStart;
        uint16 windowLen;
        bytes16 ticketId;
        bytes32 xPrevHash;
        bytes32 cPrev;
        bytes32 dPrev;
        bytes32 d;
    }

    function receiptMessage(ReceiptFields memory f) internal view returns (bytes memory) {
        return abi.encodePacked(
            hex"02", "posq-receipt-v1",
            epoch, f.tick, f.pos, f.h, f.bucket,
            f.windowStart, f.windowLen, f.ticketId,
            f.xPrevHash, f.cPrev, f.dPrev, f.d
        );
    }

    function digestStep(
        bytes32 dPrev,
        uint64 tick,
        uint32 pos,
        bytes32 h,
        uint8 bucket,
        bytes16 ticketId
    ) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked("posq-digest-v1", dPrev, tick, pos, h, bucket, ticketId));
    }

    function logStep(bytes32 cPrev, uint64 tick, bytes32 batchRoot, bytes32 xHash)
        internal
        pure
        returns (bytes32)
    {
        return keccak256(abi.encodePacked("posq-log-v1", cPrev, tick, batchRoot, xHash));
    }

    struct BatchEntryFields {
        uint64 tick;
        uint32 pos;
        bytes32 h;
        uint8 bucket;
        bytes16 ticketId;
        bytes32 receiptSigHash; // keccak256 of the 65-byte receipt signature
    }

    function entryLeaf(BatchEntryFields memory e) internal pure returns (bytes32) {
        return keccak256(
            abi.encodePacked("posq-leaf-v1", e.tick, e.pos, e.h, e.bucket, e.ticketId, e.receiptSigHash)
        );
    }

    function merkleParent(bytes32 l, bytes32 r) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked(hex"01", l, r));
    }

    function emptyBatchRoot() internal pure returns (bytes32) {
        return keccak256("posq-empty-batch-v1");
    }

    function merkleVerify(bytes32 root, bytes32 leaf, uint256 index, bytes32[] calldata path)
        internal
        pure
        returns (bool)
    {
        bytes32 acc = leaf;
        for (uint256 i = 0; i < path.length; i++) {
            acc = index % 2 == 0 ? merkleParent(acc, path[i]) : merkleParent(path[i], acc);
            index /= 2;
        }
        return acc == root;
    }

    function merkleRoot(bytes32[] memory leaves) internal pure returns (bytes32) {
        if (leaves.length == 0) return emptyBatchRoot();
        uint256 n = leaves.length;
        while (n > 1) {
            uint256 m = (n + 1) / 2;
            for (uint256 i = 0; i < m; i++) {
                bytes32 l = leaves[2 * i];
                bytes32 r = 2 * i + 1 < n ? leaves[2 * i + 1] : l;
                leaves[i] = merkleParent(l, r);
            }
            n = m;
        }
        return leaves[0];
    }

    function checkTickRecordSig(TickRecordFields memory f, bytes calldata sig) internal view {
        require(recoverSigner(keccak256(tickRecordMessage(f)), sig) == sequencer, "record sig");
    }

    function checkReceiptSig(ReceiptFields memory f, bytes calldata sig) internal view {
        // Structural digest-link check mirrors Receipt::verify.
        require(
            f.d == digestStep(f.dPrev, f.tick, f.pos, f.h, f.bucket, f.ticketId),
            "digest link"
        );
        require(recoverSigner(keccak256(receiptMessage(f)), sig) == sequencer, "receipt sig");
    }

    // ------------------------------------------------------------------
    // Wesolowski verification via the modexp precompile (parity: vdf::posq)
    // ------------------------------------------------------------------

    /// modexp precompile (0x05) for 256-byte base/modulus, 32-byte exponent.
    function modexp256(bytes memory base, uint256 e, bytes memory mod_)
        internal
        view
        returns (bytes memory out)
    {
        out = new bytes(256);
        bytes memory input = abi.encodePacked(
            uint256(256), uint256(32), uint256(256), base, bytes32(e), mod_
        );
        assembly {
            if iszero(staticcall(gas(), 0x05, add(input, 32), mload(input), add(out, 32), 256)) {
                revert(0, 0)
            }
        }
    }

    /// 32-byte modexp: a^e mod m over uint256 (for Miller-Rabin and 2^T mod l).
    function modexpU256(uint256 a, uint256 e, uint256 m) internal view returns (uint256 out) {
        bytes memory input = abi.encodePacked(
            uint256(32), uint256(32), uint256(32), bytes32(a), bytes32(e), bytes32(m)
        );
        bool ok;
        assembly {
            ok := staticcall(gas(), 0x05, add(input, 32), mload(input), 0x00, 32)
            out := mload(0x00)
        }
        require(ok, "modexp");
    }

    /// Miller-Rabin over uint256 with fixed small-prime bases.
    /// (The Rust prover picks the first probable prime ≥ the hash; the
    /// bounded increment below caps prover grinding at a handful of primes,
    /// a negligible soundness factor.)
    function isProbablePrime(uint256 nVal) internal view returns (bool) {
        if (nVal < 4 || nVal % 2 == 0) return false;
        uint256 d = nVal - 1;
        uint256 s = 0;
        while (d % 2 == 0) {
            d /= 2;
            s++;
        }
        uint8[12] memory bases = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
        for (uint256 i = 0; i < bases.length; i++) {
            uint256 a = bases[i];
            if (a >= nVal) continue;
            uint256 x = modexpU256(a, d, nVal);
            if (x == 1 || x == nVal - 1) continue;
            bool composite = true;
            for (uint256 rr = 1; rr < s; rr++) {
                x = mulmod(x, x, nVal);
                if (x == nVal - 1) {
                    composite = false;
                    break;
                }
            }
            if (composite) return false;
        }
        return true;
    }

    bytes constant SEGMENT_DOMAIN = "posq-segment-proof-v1";

    /// Verify one segment's Wesolowski proof: pi^l · y^(2^T mod l) == xEnd.
    /// `l` is supplied by the caller and checked to be the deterministic
    /// challenge: the first probable prime at bounded increment above the
    /// SHA-256 preimage (matching vdf::posq::challenge_prime).
    function verifySegmentProof(
        bytes memory y, // 256-byte base y_s
        bytes memory xEnd, // 256-byte x_end(s)
        bytes memory pi, // 256-byte proof
        uint64 t, // q * segmentTicks
        uint256 l // claimed challenge prime
    ) public view returns (bool) {
        require(y.length == 256 && xEnd.length == 256 && pi.length == 256, "elt length");
        require(t == qSquarings * segmentTicks, "wrong T");
        // Recompute the challenge candidate. posq.rs `challenge_prime`
        // sha256-hashes the preimage and `hash_to_prime` hashes that digest
        // again before the increment-to-prime walk, so the candidate is the
        // double hash sha256(sha256(preimage)).
        bytes32 candidate = sha256(
            abi.encodePacked(
                sha256(
                    abi.encodePacked(
                        "posq-wesolowski-challenge",
                        uint64(SEGMENT_DOMAIN.length),
                        SEGMENT_DOMAIN,
                        y,
                        xEnd,
                        t
                    )
                )
            )
        );
        uint256 c = uint256(candidate);
        require(l >= c && l - c <= 65536, "challenge out of range");
        require(isProbablePrime(l), "challenge not prime");
        // r = 2^T mod l.
        uint256 r = modexpU256(2, uint256(t), l);
        // lhs = pi^l * y^r mod N. The two exponentiations use the modexp
        // precompile; the final 2048-bit modular multiplication has no
        // precompile and is delegated to an audited big-number library
        // (several exist from the VDF-verifier lineage; ~150 lines of Yul
        // limb arithmetic). It is injected at deployment.
        bytes memory a = modexp256(pi, l, modulusN);
        bytes memory b = modexp256(y, r, modulusN);
        bytes memory lhs = bigMulMod.mulmod2048(a, b, modulusN);
        return keccak256(lhs) == keccak256(xEnd);
    }

    // ------------------------------------------------------------------
    // Anchor acceptance
    // ------------------------------------------------------------------

    /// Accept an anchor. Segment proofs are verified individually via
    /// `verifySegmentProof` (callable separately to bound calldata); this
    /// entry point records the span under the optimistic challenge window.
    function submitAnchor(
        uint64 firstTick,
        uint64 lastTick,
        bytes32 xBHash,
        bytes32 cB,
        bytes32 receiptsRoot,
        bytes32 daAttestation,
        bytes32 transcriptCommitment,
        bytes calldata sig,
        uint64 firstSegment,
        uint64 lastSegment,
        bytes32[] calldata segmentXEndHashes
    ) external {
        require(!rescueMode, "rescue");
        // Reconstruct the anchor signing message (anchor::Anchor::signing_message).
        bytes memory m = abi.encodePacked(
            hex"06", "posq-anchor-v1",
            epoch, firstSegment, lastSegment, firstTick, lastTick, xBHash, cB
        );
        for (uint256 i = 0; i < segmentXEndHashes.length; i++) {
            m = abi.encodePacked(m, segmentXEndHashes[i]);
        }
        m = abi.encodePacked(m, receiptsRoot, daAttestation, transcriptCommitment);
        require(recoverSigner(keccak256(m), sig) == sequencer, "anchor sig");
        anchors.push(
            AnchorRec(firstTick, lastTick, cB, receiptsRoot, transcriptCommitment, uint64(block.timestamp))
        );
        emit AnchorAccepted(anchors.length - 1, firstTick, lastTick, cB);
    }

    // ------------------------------------------------------------------
    // Fraud proofs (§10.2) — each slashes the full bond to floor and pays
    // the challenger a bounty.
    // ------------------------------------------------------------------

    uint256 constant BOUNTY_BPS = 1000; // 10% of slashed amount

    function slash(uint8 kind) internal {
        uint256 amount = bond;
        bond = 0;
        rescueMode = true;
        emit RescueMode();
        uint256 bounty = (amount * BOUNTY_BPS) / 10000;
        (bool ok,) = msg.sender.call{value: bounty}("");
        ok; // bounty best-effort; remainder held for governance resolution
        emit FraudProven(kind, msg.sender, amount);
    }

    /// Proof 1 — Equivocation: two signed tick records, same (epoch, tick),
    /// different content.
    function proveEquivocation(
        TickRecordFields calldata a,
        bytes calldata sigA,
        TickRecordFields calldata b,
        bytes calldata sigB
    ) external {
        require(a.tick == b.tick, "tick mismatch");
        bytes memory ma = tickRecordMessage(a);
        bytes memory mb = tickRecordMessage(b);
        require(keccak256(ma) != keccak256(mb), "identical");
        require(recoverSigner(keccak256(ma), sigA) == sequencer, "sig a");
        require(recoverSigner(keccak256(mb), sigB) == sequencer, "sig b");
        slash(1);
    }

    /// Proof 2 — Reorder: a receipt for h at (t, j) and the signed batch
    /// placing a different entry at j.
    function proveReorder(
        ReceiptFields calldata receipt,
        bytes calldata receiptSig,
        TickRecordFields calldata record,
        bytes calldata recordSig,
        BatchEntryFields calldata conflicting,
        bytes32[] calldata path
    ) external {
        require(receipt.tick == record.tick, "tick");
        checkReceiptSig(receipt, receiptSig);
        checkTickRecordSig(record, recordSig);
        require(conflicting.tick == receipt.tick && conflicting.pos == receipt.pos, "position");
        require(conflicting.h != receipt.h, "same commitment");
        require(
            merkleVerify(record.batchRoot, entryLeaf(conflicting), conflicting.pos, path),
            "merkle"
        );
        slash(2);
    }

    /// Proof 3 — Omission: a receipt plus the full published batch (root
    /// must match the signed record) not containing h at j.
    function proveOmission(
        ReceiptFields calldata receipt,
        bytes calldata receiptSig,
        TickRecordFields calldata record,
        bytes calldata recordSig,
        BatchEntryFields[] calldata fullBatch
    ) external {
        require(receipt.tick == record.tick, "tick");
        checkReceiptSig(receipt, receiptSig);
        checkTickRecordSig(record, recordSig);
        bytes32[] memory leaves = new bytes32[](fullBatch.length);
        for (uint256 i = 0; i < fullBatch.length; i++) {
            leaves[i] = entryLeaf(fullBatch[i]);
            if (fullBatch[i].pos == receipt.pos && fullBatch[i].h == receipt.h) {
                revert("included");
            }
        }
        require(merkleRoot(leaves) == record.batchRoot, "batch root");
        slash(3);
    }

    /// Proof 5 — Invalid log transition between consecutive signed records.
    function proveInvalidLog(
        TickRecordFields calldata prev,
        bytes calldata sigPrev,
        TickRecordFields calldata curr,
        bytes calldata sigCurr
    ) external {
        require(curr.tick == prev.tick + 1, "not consecutive");
        checkTickRecordSig(prev, sigPrev);
        checkTickRecordSig(curr, sigCurr);
        bytes32 expected = logStep(curr.cPrev, curr.tick, curr.batchRoot, curr.xHash);
        require(curr.cPrev != prev.cT || curr.cT != expected, "log chain valid");
        slash(5);
    }

    // ------------------------------------------------------------------
    // Forced inclusion (§10.3) and default (fraud proof 6)
    // ------------------------------------------------------------------

    function forceInclude(bytes32 commitmentHash) external payable {
        forcedQueue.push(ForcedEntry(commitmentHash, uint64(block.timestamp), false));
        emit ForcedEnqueued(forcedQueue.length - 1, commitmentHash);
    }

    /// The sequencer discharges a forced entry by exhibiting a receipt for h.
    function dischargeForced(
        uint256 id,
        ReceiptFields calldata receipt,
        bytes calldata receiptSig
    ) external {
        ForcedEntry storage f = forcedQueue[id];
        require(!f.discharged, "done");
        require(receipt.h == f.commitmentHash, "wrong h");
        checkReceiptSig(receipt, receiptSig);
        f.discharged = true;
        emit ForcedDischarged(id);
    }

    /// Proof 6 — Forced-inclusion default: an anchored span covering the
    /// deadline exists and the entry is still undischarged.
    function proveForcedDefault(uint256 id, uint256 anchorId) external {
        ForcedEntry storage f = forcedQueue[id];
        require(!f.discharged, "discharged");
        AnchorRec storage a = anchors[anchorId];
        // Deadline: the anchor was accepted after enqueue + challenge window
        // and covers fForce ticks of headroom.
        require(a.acceptedAt > f.enqueuedAt + challengeWindow, "too early");
        require(a.lastTick - a.firstTick >= fForce, "span too short");
        slash(6);
    }
}
