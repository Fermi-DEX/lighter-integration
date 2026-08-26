// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// Minimal read view of the deployed PoSqHost the bridge needs. Matches the
/// auto-generated getters of `PoSqHost` (`AnchorRec[] public anchors`,
/// `uint64 public immutable challengeWindow`) so the bridge binds to a real
/// host without modifying it.
interface IPoSqHostView {
    function anchors(uint256 id)
        external
        view
        returns (
            uint64 firstTick,
            uint64 lastTick,
            bytes32 cB,
            bytes32 receiptsRoot,
            bytes32 transcriptCommitment,
            uint64 acceptedAt
        );

    function challengeWindow() external view returns (uint64);
}

/// @title LighterBridgeDemo — binds a Lighter batch span to a PoSqHost anchor.
///
/// Implements binding rungs B1 (co-anchored) and the B2 (optimistic
/// stream-equality) challenge hook from `integrations-v2/lighter-integration.md`
/// §5/§6.
///
///  - `proposeSpan` (B1): asserts a `PoSqHost` anchor covers the claimed tick
///    span with the claimed batch commitment `cB`, then records the batch's
///    opened-stream commitment `s_n` under an optimistic challenge window.
///  - `challengeStream` (B2): the stream-equality fraud game. The *faithful*
///    on-chain verifier (spec §5, steps 1–5) is documented inline; v1 ships a
///    demo-grade hook that accepts an off-chain-proven mismatch index and
///    rejects the span. A full on-chain envelope-open + ChaCha20-Poly1305
///    decrypt is deliberately out of scope for v1 (see notes below).
///
/// The bridge never holds funds and never slashes directly: slashing lives on
/// `PoSqHost` (proofs 2/3 for reorder/omission). B2 rejection here marks the
/// span un-final and is the signal for the bridge's "declared FCFS" degraded
/// mode (spec §6 failure matrix).
contract LighterBridgeDemo {
    IPoSqHostView public immutable host;

    struct Span {
        uint64 epoch;
        uint64 firstTick;
        uint64 lastTick;
        uint64 lastSegment;
        bytes32 cB; // batch commitment, must match the covering anchor
        bytes32 streamCommitment; // s_n over the opened tape prefix (spec §5 B2)
        uint256 anchorId; // the PoSqHost anchor this span was bound to
        uint64 acceptedAt; // block.timestamp at proposal
        uint64 challengeDeadline; // acceptedAt + host.challengeWindow()
        bool rejected; // flipped true by a successful challengeStream
    }

    Span[] public spans;

    event SpanProposed(
        uint256 indexed spanId,
        uint64 indexed epoch,
        uint256 indexed anchorId,
        uint64 firstTick,
        uint64 lastTick,
        bytes32 cB,
        bytes32 streamCommitment,
        uint64 challengeDeadline
    );

    event StreamChallenged(
        uint256 indexed spanId, uint256 mismatchIndex, address indexed challenger
    );

    constructor(address _host) {
        require(_host != address(0), "host zero");
        host = IPoSqHostView(_host);
    }

    // ------------------------------------------------------------------
    // B1 — co-anchored span proposal
    // ------------------------------------------------------------------

    /// @notice Propose a Lighter batch span bound to a PoSqHost anchor.
    /// @dev Scans `PoSqHost.anchors` for an anchor that covers
    ///      `[firstTick, lastTick]` and commits to the same batch `cB`. The
    ///      scan uses try/catch on the public array getter (reverts with
    ///      Panic(0x32) past the end) so the bridge needs no length getter and
    ///      `PoSqHost` stays byte-identical to the deployed contract.
    /// @param epoch PoSq epoch of the span.
    /// @param firstTick first tick of the batch span (inclusive).
    /// @param lastTick last tick of the batch span (inclusive).
    /// @param lastSegment last PoSq segment index of the span (batches align to
    ///        whole segments; stored for the joined `(t,j) ↔ (block,tx)` history).
    /// @param cB batch commitment — must equal the covering anchor's `cB`.
    /// @param streamCommitment `s_n`, the opened-stream commitment (spec §5 B2).
    /// @return spanId index of the stored span.
    function proposeSpan(
        uint64 epoch,
        uint64 firstTick,
        uint64 lastTick,
        uint64 lastSegment,
        bytes32 cB,
        bytes32 streamCommitment
    ) external returns (uint256 spanId) {
        require(lastTick >= firstTick, "empty span");
        (bool found, uint256 anchorId) = _findCoveringAnchor(firstTick, lastTick, cB);
        require(found, "no covering anchor");

        uint64 window = host.challengeWindow();
        uint64 deadline = uint64(block.timestamp) + window;

        spanId = spans.length;
        spans.push(
            Span({
                epoch: epoch,
                firstTick: firstTick,
                lastTick: lastTick,
                lastSegment: lastSegment,
                cB: cB,
                streamCommitment: streamCommitment,
                anchorId: anchorId,
                acceptedAt: uint64(block.timestamp),
                challengeDeadline: deadline,
                rejected: false
            })
        );

        emit SpanProposed(
            spanId, epoch, anchorId, firstTick, lastTick, cB, streamCommitment, deadline
        );
    }

    /// @dev Linear scan for an anchor covering [firstTick, lastTick] with cB.
    ///      Bounded implicitly by the number of anchors on the host.
    function _findCoveringAnchor(uint64 firstTick, uint64 lastTick, bytes32 cB)
        internal
        view
        returns (bool found, uint256 anchorId)
    {
        for (uint256 i = 0;; i++) {
            try host.anchors(i) returns (
                uint64 aFirst, uint64 aLast, bytes32 aCb, bytes32, bytes32, uint64
            ) {
                if (aFirst <= firstTick && aLast >= lastTick && aCb == cB) {
                    return (true, i);
                }
            } catch {
                // Past the end of the anchors array (Panic 0x32).
                return (false, 0);
            }
        }
    }

    // ------------------------------------------------------------------
    // B2 — optimistic stream-equality challenge (spec §5)
    // ------------------------------------------------------------------

    /// @notice Challenge a proposed span by exhibiting an index where Lighter's
    ///         committed transaction stream diverges from the PoSq tape.
    ///
    /// @dev FAITHFUL on-chain game (spec §5 B2, steps 1–5) — NOT fully
    ///      implemented in v1:
    ///        1. Merkle path of `BatchEntry(t, j, h, …)` to the signed tick
    ///           record's `batch_root` — reuse `PoSqHost.merkleVerify` +
    ///           `checkTickRecordSig`.
    ///        2. Envelope preimage: bytes with
    ///           `commitment_hash(epoch, bytes) == h` (binds namespace/window).
    ///        3. Opening: verify `w = u^{2^{T_k}}` (Wesolowski, via the modexp
    ///           path `PoSqHost` already uses) then ChaCha20-Poly1305 decrypt
    ///           of ≤ 642 bytes on-chain — the expensive step (~1M gas-order).
    ///        4. Inclusion proof of `tx_i` at stream index `i` in Lighter's
    ///           batch commitment (the single disclosure required from Lighter).
    ///        5. Compare; on mismatch slash the PoSq bond and flip the bridge to
    ///           declared-FCFS degraded mode.
    ///
    ///      A full on-chain envelope-open + AEAD decrypt is deliberately out of
    ///      scope for v1: this hook accepts an off-chain-proven mismatch and
    ///      records the rejection so the demo can exercise scenario 6 ("stream
    ///      mismatch"). `provenOffChain` stands in for steps 1–4 above; a
    ///      production deployment replaces it with the calldata + verifier of
    ///      spec §6 (`challengeStream(i, batchEntry, merklePath, tickRecord+sig,
    ///      envelopeBytes, opening, lighterTxProof)`).
    ///
    /// @param spanId span being challenged.
    /// @param mismatchIndex the stream index `i` where the divergence occurs.
    /// @param provenOffChain demo attestation that steps 1–4 verified off-chain.
    function challengeStream(uint256 spanId, uint256 mismatchIndex, bool provenOffChain)
        external
    {
        require(spanId < spans.length, "no span");
        Span storage sp = spans[spanId];
        require(!sp.rejected, "already rejected");
        require(block.timestamp <= sp.challengeDeadline, "window closed");
        require(provenOffChain, "mismatch not proven");

        sp.rejected = true;
        emit StreamChallenged(spanId, mismatchIndex, msg.sender);
    }

    // ------------------------------------------------------------------
    // Views for the dashboard
    // ------------------------------------------------------------------

    /// @notice Number of proposed spans.
    function spanCount() external view returns (uint256) {
        return spans.length;
    }

    /// @notice True once a span's challenge window has elapsed with no
    ///         successful challenge — the bridge-side finality signal.
    function isFinal(uint256 spanId) external view returns (bool) {
        require(spanId < spans.length, "no span");
        Span storage sp = spans[spanId];
        return !sp.rejected && block.timestamp > sp.challengeDeadline;
    }
}
