// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice Minimal interface for a pinned proof verifier.
interface IProofVerifierV3 {
    function verify(bytes calldata proof, bytes32 publicInputsHash)
        external
        view
        returns (bool);
}

/// @notice Reference-only atomic join for Continuum sequence validity and
/// Lighter execution validity. Production must integrate this relation into
/// Lighter's existing settlement, governance, priority queue, and escape hatch.
contract ContinuumLighterBinding {
    bytes32 private constant SEQUENCE_PUBLIC_V3 =
        keccak256("CONTINUUM_SEQUENCE_PUBLIC_V3");
    bytes32 private constant EXECUTION_PUBLIC_V3 =
        keccak256("CONTINUUM_LIGHTER_EXECUTION_PUBLIC_V3");

    struct PendingBatch {
        bytes32 batchCommitment;
        bytes32 blobVersionedHash;
        bytes32 cBind;
        bool exists;
    }

    struct Head {
        bytes32 domainHash;
        bytes32 sequenceVerifierId;
        bytes32 executionVerifierId;
        uint64 epoch;
        uint64 globalCursor;
        uint64 namespaceCount;
        bytes32 transcriptRoot;
        bytes32 lighterStateRoot;
        uint64 priorityHead;
        bytes32 lastCBind;
    }

    struct SequencePublic {
        bytes32 domainHash;
        bytes32 verifierId;
        uint64 epoch;
        uint64 oldGlobalCursor;
        uint64 newGlobalCursor;
        bytes32 oldTranscriptRoot;
        bytes32 newTranscriptRoot;
        uint64 oldNamespaceCount;
        uint64 newNamespaceCount;
        bytes32 orderedItemRoot;
        bytes32 executionStreamRoot;
        uint64 orderedItemCount;
        uint64 priorityStart;
        uint64 priorityEnd;
        bytes32 cBind;
    }

    struct ExecutionPublic {
        bytes32 domainHash;
        bytes32 verifierId;
        uint64 epoch;
        bytes32 oldStateRoot;
        bytes32 newStateRoot;
        bytes32 orderedItemRoot;
        bytes32 executionStreamRoot;
        uint64 orderedItemCount;
        bytes32 batchCommitment;
        bytes32 blobVersionedHash;
        bytes32 blobCBind;
        bytes32 cBind;
    }

    IProofVerifierV3 public immutable sequenceVerifier;
    IProofVerifierV3 public immutable executionVerifier;
    Head public head;
    mapping(bytes32 batchId => PendingBatch) public pendingBatches;

    event BatchCommitted(
        bytes32 indexed batchId,
        bytes32 batchCommitment,
        bytes32 blobVersionedHash,
        bytes32 cBind
    );
    event BatchFinalized(
        bytes32 indexed batchId,
        bytes32 indexed cBind,
        bytes32 newStateRoot,
        uint64 newGlobalCursor
    );

    error InvalidProof();
    error InvalidTransition();
    error BindingMismatch();
    error BatchAlreadyCommitted();
    error BatchNotCommitted();

    constructor(
        IProofVerifierV3 sequenceVerifier_,
        IProofVerifierV3 executionVerifier_,
        Head memory genesis
    ) {
        if (
            address(sequenceVerifier_) == address(0)
                || address(executionVerifier_) == address(0)
                || genesis.domainHash == bytes32(0)
        ) revert InvalidTransition();
        sequenceVerifier = sequenceVerifier_;
        executionVerifier = executionVerifier_;
        head = genesis;
    }

    function commitBatch(
        bytes32 batchId,
        bytes32 batchCommitment,
        bytes32 blobVersionedHash,
        bytes32 cBind
    ) external {
        if (pendingBatches[batchId].exists) revert BatchAlreadyCommitted();
        if (
            batchId == bytes32(0) || batchCommitment == bytes32(0)
                || blobVersionedHash == bytes32(0) || cBind == bytes32(0)
        ) revert InvalidTransition();

        pendingBatches[batchId] = PendingBatch({
            batchCommitment: batchCommitment,
            blobVersionedHash: blobVersionedHash,
            cBind: cBind,
            exists: true
        });
        emit BatchCommitted(batchId, batchCommitment, blobVersionedHash, cBind);
    }

    /// @dev Both verifier calls and all continuity checks happen before the
    /// head changes. A revert leaves every settlement cursor unchanged.
    function verifyAndAdvance(
        bytes32 batchId,
        bytes calldata sequenceProof,
        bytes calldata executionProof,
        SequencePublic calldata seq,
        ExecutionPublic calldata exec
    ) external {
        PendingBatch memory pending = pendingBatches[batchId];
        if (!pending.exists) revert BatchNotCommitted();

        Head memory old = head;
        if (
            seq.domainHash != old.domainHash
                || exec.domainHash != old.domainHash
                || seq.epoch != old.epoch
                || exec.epoch != old.epoch
                || seq.verifierId != old.sequenceVerifierId
                || exec.verifierId != old.executionVerifierId
                || seq.oldGlobalCursor != old.globalCursor
                || seq.newGlobalCursor <= seq.oldGlobalCursor
                || seq.oldNamespaceCount != old.namespaceCount
                || seq.newNamespaceCount < seq.oldNamespaceCount
                || seq.orderedItemCount
                    != seq.newNamespaceCount - seq.oldNamespaceCount
                || seq.orderedItemCount != exec.orderedItemCount
                || seq.orderedItemRoot != exec.orderedItemRoot
                || seq.executionStreamRoot != exec.executionStreamRoot
                || seq.oldTranscriptRoot != old.transcriptRoot
                || exec.oldStateRoot != old.lighterStateRoot
                || seq.priorityStart != old.priorityHead
                || seq.priorityEnd < seq.priorityStart
                || exec.batchCommitment != pending.batchCommitment
                || exec.blobVersionedHash != pending.blobVersionedHash
        ) revert InvalidTransition();

        if (
            seq.cBind != exec.cBind || exec.blobCBind != exec.cBind
                || pending.cBind != exec.cBind
        ) revert BindingMismatch();

        bytes32 sequencePublicHash =
            keccak256(abi.encode(SEQUENCE_PUBLIC_V3, seq));
        bytes32 executionPublicHash =
            keccak256(abi.encode(EXECUTION_PUBLIC_V3, batchId, exec));

        if (!sequenceVerifier.verify(sequenceProof, sequencePublicHash)) {
            revert InvalidProof();
        }
        if (!executionVerifier.verify(executionProof, executionPublicHash)) {
            revert InvalidProof();
        }

        head.globalCursor = seq.newGlobalCursor;
        head.namespaceCount = seq.newNamespaceCount;
        head.transcriptRoot = seq.newTranscriptRoot;
        head.lighterStateRoot = exec.newStateRoot;
        head.priorityHead = seq.priorityEnd;
        head.lastCBind = seq.cBind;
        delete pendingBatches[batchId];

        emit BatchFinalized(
            batchId, seq.cBind, exec.newStateRoot, seq.newGlobalCursor
        );
    }
}
