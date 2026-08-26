use continuum_lighter_plugin::{
    advance_head, compute_c_bind, compute_execution_stream_root, compute_ordered_item_root,
    verify_sequence_transition, BindingInputsV3, DerivedItemV3, ExecutionItemV3, ExecutionPublicV3,
    JoinError, ReceiptV3, ResolutionV3, ResolvedItemV3, SequenceStateV3, SequenceTransitionError,
    SequenceTransitionPublicV3, SequenceTransitionWitnessV3, SettlementHeadV3,
    Sha256ReferenceHasher, TransitionAuthenticator,
};

fn h(byte: u8) -> [u8; 32] {
    [byte; 32]
}

struct Auth;

impl TransitionAuthenticator for Auth {
    fn verify_receipt(&self, receipt: &ReceiptV3) -> bool {
        receipt.signature == vec![1]
    }

    fn verify_da(&self, receipt: &ReceiptV3, expected_da_commitment: [u8; 32]) -> bool {
        receipt.da_leaf == expected_da_commitment
    }

    fn verify_resolution(&self, _receipt: &ReceiptV3, item: &ResolvedItemV3) -> bool {
        item.opening_commitment != [0; 32]
    }

    fn verify_transcript_transition(
        &self,
        _old_transcript_root: [u8; 32],
        _receipts: &[ReceiptV3],
        new_transcript_root: [u8; 32],
    ) -> bool {
        new_transcript_root == h(41)
    }

    fn verify_frame_commitments(
        &self,
        _witness: &SequenceTransitionWitnessV3,
        public: &SequenceTransitionPublicV3,
    ) -> bool {
        public.binding.frame_plan_root == h(51)
    }
}

fn fixture() -> (SequenceTransitionPublicV3, SequenceTransitionWitnessV3) {
    let receipts = vec![
        ReceiptV3 {
            epoch: 4,
            global_cursor: 11,
            tick: 10,
            position: 0,
            namespace_id: 7,
            envelope_hash: h(70),
            previous_receipt_digest: h(30),
            receipt_digest: h(31),
            da_leaf: h(20),
            signature: vec![1],
        },
        ReceiptV3 {
            epoch: 4,
            global_cursor: 12,
            tick: 10,
            position: 1,
            namespace_id: 7,
            envelope_hash: h(71),
            previous_receipt_digest: h(31),
            receipt_digest: h(32),
            da_leaf: h(20),
            signature: vec![1],
        },
    ];
    let resolved_items = vec![
        ResolvedItemV3 {
            receipt_digest: h(31),
            derived: DerivedItemV3 {
                domain_hash: h(1),
                frame_id: 7,
                chunk_id: 0,
                tick: 10,
                position: 0,
                envelope_hash: h(70),
                receipt_digest: h(31),
                resolution: ResolutionV3::Clear,
                cleartext_length: 64,
                cleartext_hash: [1, 2, 3, 4],
                terminal_reason: 0,
            },
            execution: ExecutionItemV3 {
                logical_index: 0,
                tx_type: 1,
                tx_hash: [5, 6, 7, 8, 9],
                outcome_class: 0,
                terminal_noop: false,
            },
            opening_commitment: h(80),
        },
        ResolvedItemV3 {
            receipt_digest: h(32),
            derived: DerivedItemV3 {
                domain_hash: h(1),
                frame_id: 7,
                chunk_id: 0,
                tick: 10,
                position: 1,
                envelope_hash: h(71),
                receipt_digest: h(32),
                resolution: ResolutionV3::BadAead,
                cleartext_length: 0,
                cleartext_hash: [0; 4],
                terminal_reason: 1,
            },
            execution: ExecutionItemV3 {
                logical_index: 1,
                tx_type: 0,
                tx_hash: [0; 5],
                outcome_class: 1,
                terminal_noop: true,
            },
            opening_commitment: h(81),
        },
    ];

    let old_state = SequenceStateV3 {
        domain_hash: h(1),
        epoch: 4,
        global_cursor: 10,
        namespace_item_count: 5,
        transcript_root: h(40),
        receipt_chain_root: h(30),
        frame_plan_root: h(50),
        da_commitment: h(20),
        config_hash: h(60),
    };
    let new_state = SequenceStateV3 {
        domain_hash: h(1),
        epoch: 4,
        global_cursor: 12,
        namespace_item_count: 7,
        transcript_root: h(41),
        receipt_chain_root: h(32),
        frame_plan_root: h(51),
        da_commitment: h(20),
        config_hash: h(60),
    };

    let hasher = Sha256ReferenceHasher;
    let derived: Vec<_> = resolved_items.iter().map(|x| x.derived.clone()).collect();
    let execution: Vec<_> = resolved_items.iter().map(|x| x.execution.clone()).collect();
    let binding = BindingInputsV3 {
        domain_hash: h(1),
        epoch: 4,
        old_global_cursor: 10,
        new_global_cursor: 12,
        old_transcript_root: h(40),
        new_transcript_root: h(41),
        frame_plan_root: h(51),
        ordered_item_root: compute_ordered_item_root(&hasher, h(1), 7, 10, &derived),
        execution_stream_root: compute_execution_stream_root(&hasher, h(1), 10, &execution),
        ordered_item_count: 2,
        priority_start: 8,
        priority_end: 9,
        priority_root: h(90),
        oracle_snapshot_root: h(91),
        protocol_event_root: h(92),
        l1_origin_hash: h(93),
        policy_hash: h(94),
        decryption_module_id: h(95),
    };
    let c_bind = compute_c_bind(&hasher, &binding);

    (
        SequenceTransitionPublicV3 {
            namespace_id: 7,
            old_state,
            new_state,
            binding,
            c_bind,
        },
        SequenceTransitionWitnessV3 {
            receipts,
            resolved_items,
        },
    )
}

#[test]
fn valid_transition_passes() {
    let (public, witness) = fixture();
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Ok(())
    );
}

#[test]
fn missing_terminal_item_is_not_a_gap() {
    let (public, mut witness) = fixture();
    witness.resolved_items.pop();
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Err(SequenceTransitionError::MissingResolution)
    );
}

#[test]
fn duplicate_receipt_is_rejected() {
    let (public, mut witness) = fixture();
    witness.receipts[1].receipt_digest = witness.receipts[0].receipt_digest;
    witness.receipts[1].previous_receipt_digest = h(31);
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Err(SequenceTransitionError::DuplicateReceipt)
    );
}

#[test]
fn duplicate_envelope_is_rejected() {
    let (public, mut witness) = fixture();
    witness.receipts[1].envelope_hash = witness.receipts[0].envelope_hash;
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Err(SequenceTransitionError::DuplicateEnvelope)
    );
}

#[test]
fn implicit_config_upgrade_is_rejected() {
    let (mut public, witness) = fixture();
    public.new_state.config_hash = h(61);
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Err(SequenceTransitionError::Config)
    );
}

#[test]
fn execution_stream_mutation_is_rejected() {
    let (mut public, witness) = fixture();
    public.binding.execution_stream_root = h(99);
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Err(SequenceTransitionError::ExecutionStreamRoot)
    );
}

#[test]
fn terminal_item_cannot_smuggle_a_transaction_hash() {
    let (public, mut witness) = fixture();
    witness.resolved_items[1].execution.tx_hash[0] = 1;
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Err(SequenceTransitionError::InvalidTerminalSemantics)
    );
}

#[test]
fn cursor_skip_is_rejected() {
    let (public, mut witness) = fixture();
    witness.receipts[1].global_cursor = 13;
    assert_eq!(
        verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness),
        Err(SequenceTransitionError::Cursor)
    );
}

#[test]
fn verified_sequence_and_lighter_public_inputs_advance_atomically() {
    let (public, witness) = fixture();
    verify_sequence_transition(&Sha256ReferenceHasher, &Auth, &public, &witness).unwrap();

    let sequence_verifier_id = h(2);
    let execution_verifier_id = h(3);
    let head = SettlementHeadV3 {
        domain_hash: public.old_state.domain_hash,
        sequence_verifier_id,
        execution_verifier_id,
        epoch: public.old_state.epoch,
        global_cursor: public.old_state.global_cursor,
        namespace_count: public.old_state.namespace_item_count,
        transcript_root: public.old_state.transcript_root,
        lighter_state_root: h(7),
        priority_head: public.binding.priority_start,
        last_c_bind: h(0),
    };
    let sequence = public.to_join_public(sequence_verifier_id);
    let mut execution = ExecutionPublicV3 {
        domain_hash: public.old_state.domain_hash,
        verifier_id: execution_verifier_id,
        epoch: public.old_state.epoch,
        old_state_root: h(7),
        new_state_root: h(8),
        ordered_item_root: public.binding.ordered_item_root,
        execution_stream_root: public.binding.execution_stream_root,
        ordered_item_count: public.binding.ordered_item_count,
        blob_c_bind: public.c_bind,
        c_bind: public.c_bind,
    };

    let next = advance_head(&head, &sequence, &execution).unwrap();
    assert_eq!(next.global_cursor, public.new_state.global_cursor);
    assert_eq!(next.namespace_count, public.new_state.namespace_item_count);
    assert_eq!(next.last_c_bind, public.c_bind);

    execution.execution_stream_root[0] ^= 1;
    assert_eq!(
        advance_head(&head, &sequence, &execution),
        Err(JoinError::StreamMismatch)
    );
}
