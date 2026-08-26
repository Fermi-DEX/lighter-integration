// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ContinuumLighterBinding, IProofVerifierV3} from "../ContinuumLighterBinding.sol";

contract MockVerifier is IProofVerifierV3 {
    bool public result = true;

    function setResult(bool value) external {
        result = value;
    }

    function verify(bytes calldata, bytes32) external view returns (bool) {
        return result;
    }
}

contract ContinuumLighterBindingTest {
    MockVerifier internal seqVerifier;
    MockVerifier internal execVerifier;
    ContinuumLighterBinding internal binding;

    function setUp() public {
        seqVerifier = new MockVerifier();
        execVerifier = new MockVerifier();
        ContinuumLighterBinding.Head memory genesis =
            ContinuumLighterBinding.Head({
                domainHash: bytes32(uint256(1)),
                sequenceVerifierId: bytes32(uint256(2)),
                executionVerifierId: bytes32(uint256(3)),
                epoch: 4,
                globalCursor: 10,
                namespaceCount: 5,
                transcriptRoot: bytes32(uint256(6)),
                lighterStateRoot: bytes32(uint256(7)),
                priorityHead: 8,
                lastCBind: bytes32(0)
            });
        binding = new ContinuumLighterBinding(
            seqVerifier, execVerifier, genesis
        );
    }

    function testBothProofsAdvanceAtomically() public {
        bytes32 batchId = bytes32(uint256(100));
        binding.commitBatch(
            batchId,
            bytes32(uint256(101)),
            bytes32(uint256(102)),
            bytes32(uint256(42))
        );

        binding.verifyAndAdvance(
            batchId,
            hex"01",
            hex"02",
            sequencePublic(bytes32(uint256(42))),
            executionPublic(bytes32(uint256(42)))
        );

        (
            ,
            ,
            ,
            ,
            uint64 globalCursor,
            uint64 namespaceCount,
            bytes32 transcriptRoot,
            bytes32 lighterStateRoot,
            uint64 priorityHead,
            bytes32 lastCBind
        ) = binding.head();

        require(globalCursor == 14, "cursor");
        require(namespaceCount == 7, "namespace count");
        require(transcriptRoot == bytes32(uint256(9)), "transcript");
        require(lighterStateRoot == bytes32(uint256(10)), "state root");
        require(priorityHead == 9, "priority");
        require(lastCBind == bytes32(uint256(42)), "binding");
    }

    function testBindingMismatchRevertsWithoutAdvancing() public {
        bytes32 batchId = bytes32(uint256(100));
        binding.commitBatch(
            batchId,
            bytes32(uint256(101)),
            bytes32(uint256(102)),
            bytes32(uint256(42))
        );

        (bool ok,) = address(binding).call(
            abi.encodeCall(
                binding.verifyAndAdvance,
                (
                    batchId,
                    hex"01",
                    hex"02",
                    sequencePublic(bytes32(uint256(42))),
                    executionPublic(bytes32(uint256(43)))
                )
            )
        );
        require(!ok, "mismatch accepted");

        (,,,, uint64 globalCursor,,,,,) = binding.head();
        require(globalCursor == 10, "partial cursor advance");
        (,,, bool exists) = binding.pendingBatches(batchId);
        require(exists, "commit consumed on revert");
    }

    function testOneInvalidProofCannotAdvance() public {
        bytes32 batchId = bytes32(uint256(100));
        binding.commitBatch(
            batchId,
            bytes32(uint256(101)),
            bytes32(uint256(102)),
            bytes32(uint256(42))
        );
        seqVerifier.setResult(false);

        (bool ok,) = address(binding).call(
            abi.encodeCall(
                binding.verifyAndAdvance,
                (
                    batchId,
                    hex"01",
                    hex"02",
                    sequencePublic(bytes32(uint256(42))),
                    executionPublic(bytes32(uint256(42)))
                )
            )
        );
        require(!ok, "invalid sequence proof accepted");
        (,,,, uint64 globalCursor,,,,,) = binding.head();
        require(globalCursor == 10, "partial cursor advance");
    }

    function sequencePublic(bytes32 cBind)
        internal
        pure
        returns (ContinuumLighterBinding.SequencePublic memory)
    {
        return ContinuumLighterBinding.SequencePublic({
            domainHash: bytes32(uint256(1)),
            verifierId: bytes32(uint256(2)),
            epoch: 4,
            oldGlobalCursor: 10,
            newGlobalCursor: 14,
            oldTranscriptRoot: bytes32(uint256(6)),
            newTranscriptRoot: bytes32(uint256(9)),
            oldNamespaceCount: 5,
            newNamespaceCount: 7,
            orderedItemRoot: bytes32(uint256(11)),
            executionStreamRoot: bytes32(uint256(12)),
            orderedItemCount: 2,
            priorityStart: 8,
            priorityEnd: 9,
            cBind: cBind
        });
    }

    function executionPublic(bytes32 cBind)
        internal
        pure
        returns (ContinuumLighterBinding.ExecutionPublic memory)
    {
        return ContinuumLighterBinding.ExecutionPublic({
            domainHash: bytes32(uint256(1)),
            verifierId: bytes32(uint256(3)),
            epoch: 4,
            oldStateRoot: bytes32(uint256(7)),
            newStateRoot: bytes32(uint256(10)),
            orderedItemRoot: bytes32(uint256(11)),
            executionStreamRoot: bytes32(uint256(12)),
            orderedItemCount: 2,
            batchCommitment: bytes32(uint256(101)),
            blobVersionedHash: bytes32(uint256(102)),
            blobCBind: bytes32(uint256(42)),
            cBind: cBind
        });
    }
}
