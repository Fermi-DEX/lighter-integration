use continuum_lighter_plugin::{
    advance_head, compute_execution_stream_root, compute_ordered_item_root, DerivedItemV3,
    ExecutionItemV3, ExecutionPublicV3, JoinError, OrderedAccumulator, ResolutionV3,
    SequencePublicV3, SettlementHeadV3, Sha256ReferenceHasher,
};

fn h(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn item(position: u32, resolution: ResolutionV3) -> DerivedItemV3 {
    DerivedItemV3 {
        domain_hash: h(1),
        frame_id: 7,
        chunk_id: 0,
        tick: 10,
        position,
        envelope_hash: h(position as u8),
        receipt_digest: h(9),
        resolution,
        cleartext_length: if resolution == ResolutionV3::Clear {
            64
        } else {
            0
        },
        cleartext_hash: [11, 12, 13, 14],
        terminal_reason: if resolution == ResolutionV3::Clear {
            0
        } else {
            1
        },
    }
}

#[test]
fn ordering_mutations_change_the_root() {
    let hasher = Sha256ReferenceHasher;
    let a = item(0, ResolutionV3::Clear);
    let b = item(1, ResolutionV3::BadEncoding);
    let ordered = compute_ordered_item_root(&hasher, h(1), 7, 0, &[a.clone(), b.clone()]);
    let reordered = compute_ordered_item_root(&hasher, h(1), 7, 0, &[b, a]);
    assert_ne!(ordered, reordered);
}

#[test]
fn compact_execution_stream_preserves_global_order() {
    let hasher = Sha256ReferenceHasher;
    let a = ExecutionItemV3 {
        logical_index: 0,
        tx_type: 1,
        tx_hash: [1, 2, 3, 4, 5],
        outcome_class: 0,
        terminal_noop: false,
    };
    let b = ExecutionItemV3 {
        logical_index: 1,
        tx_type: 2,
        tx_hash: [6, 7, 8, 9, 10],
        outcome_class: 0,
        terminal_noop: false,
    };
    assert_ne!(
        compute_execution_stream_root(&hasher, h(1), 0, &[a.clone(), b.clone()]),
        compute_execution_stream_root(&hasher, h(1), 0, &[b, a]),
    );
}

#[test]
fn declared_count_and_index_are_enforced() {
    let hasher = Sha256ReferenceHasher;
    let mut accumulator = OrderedAccumulator::new(&hasher, h(1), 7, 0, 2);
    accumulator
        .advance(&hasher, 0, &item(0, ResolutionV3::Clear))
        .unwrap();
    assert!(accumulator.finish().is_err());
}

#[test]
fn atomic_join_accepts_only_one_binding() {
    let head = SettlementHeadV3 {
        domain_hash: h(1),
        sequence_verifier_id: h(2),
        execution_verifier_id: h(3),
        epoch: 4,
        global_cursor: 10,
        namespace_count: 5,
        transcript_root: h(6),
        lighter_state_root: h(7),
        priority_head: 8,
        last_c_bind: h(0),
    };
    let seq = SequencePublicV3 {
        domain_hash: h(1),
        verifier_id: h(2),
        epoch: 4,
        old_global_cursor: 10,
        new_global_cursor: 14,
        old_transcript_root: h(6),
        new_transcript_root: h(9),
        old_namespace_count: 5,
        new_namespace_count: 7,
        ordered_item_root: h(11),
        execution_stream_root: h(12),
        ordered_item_count: 2,
        priority_start: 8,
        priority_end: 9,
        c_bind: h(42),
    };
    let mut exec = ExecutionPublicV3 {
        domain_hash: h(1),
        verifier_id: h(3),
        epoch: 4,
        old_state_root: h(7),
        new_state_root: h(10),
        ordered_item_root: h(11),
        execution_stream_root: h(12),
        ordered_item_count: 2,
        blob_c_bind: h(42),
        c_bind: h(42),
    };
    let next = advance_head(&head, &seq, &exec).unwrap();
    assert_eq!(next.global_cursor, 14);
    assert_eq!(next.lighter_state_root, h(10));

    exec.c_bind = h(43);
    assert_eq!(
        advance_head(&head, &seq, &exec),
        Err(JoinError::BindingMismatch)
    );
}

#[test]
fn terminal_items_are_not_gaps() {
    let hasher = Sha256ReferenceHasher;
    let clear_only =
        compute_ordered_item_root(&hasher, h(1), 7, 0, &[item(0, ResolutionV3::Clear)]);
    let with_terminal = compute_ordered_item_root(
        &hasher,
        h(1),
        7,
        0,
        &[item(0, ResolutionV3::Clear), item(1, ResolutionV3::BadAead)],
    );
    assert_ne!(clear_only, with_terminal);
}

#[test]
fn stream_root_mismatch_cannot_advance() {
    let head = SettlementHeadV3 {
        domain_hash: h(1),
        sequence_verifier_id: h(2),
        execution_verifier_id: h(3),
        epoch: 4,
        global_cursor: 10,
        namespace_count: 5,
        transcript_root: h(6),
        lighter_state_root: h(7),
        priority_head: 8,
        last_c_bind: h(0),
    };
    let seq = SequencePublicV3 {
        domain_hash: h(1),
        verifier_id: h(2),
        epoch: 4,
        old_global_cursor: 10,
        new_global_cursor: 14,
        old_transcript_root: h(6),
        new_transcript_root: h(9),
        old_namespace_count: 5,
        new_namespace_count: 7,
        ordered_item_root: h(11),
        execution_stream_root: h(12),
        ordered_item_count: 2,
        priority_start: 8,
        priority_end: 9,
        c_bind: h(42),
    };
    let exec = ExecutionPublicV3 {
        domain_hash: h(1),
        verifier_id: h(3),
        epoch: 4,
        old_state_root: h(7),
        new_state_root: h(10),
        ordered_item_root: h(11),
        execution_stream_root: h(13),
        ordered_item_count: 2,
        blob_c_bind: h(42),
        c_bind: h(42),
    };
    assert_eq!(
        advance_head(&head, &seq, &exec),
        Err(JoinError::StreamMismatch)
    );
}
