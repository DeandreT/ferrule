// IntoBlocks fixtures reuse the Starting Project with a parent block size
// (node 7). Execution is covered by the runtime and CLI parity hosts.
fn blocks_445_project(owner: SourceOwner) -> ::mapping::Project {
    let mut p = starting_443_project(owner);
    p.root.children[0].group_starting_with = None;
    p.root.children[0].group_into_blocks = Some(7);
    starting_443_sync(&mut p);
    p
}

fn blocks_445_program(project: &::mapping::Project) -> Program {
    codegen::lower_with_basic_owned_group_into_blocks(project).unwrap()
}

#[test]
fn public_basic_group_into_blocks_445_keeps_old_profiles_and_structural_errors() {
    let program = blocks_445_program(&blocks_445_project(SourceOwner::Primary));
    for old in [
        crate::emit,
        crate::emit_with_basic_owned_sources,
        crate::emit_with_basic_owned_filters,
        crate::emit_with_basic_owned_sort,
        crate::emit_with_basic_owned_windows,
        crate::emit_with_basic_owned_row_failures,
        crate::emit_with_basic_owned_group_by,
        crate::emit_with_basic_owned_sequence_item_at,
        crate::emit_with_basic_owned_group_adjacent_by,
        crate::emit_with_basic_owned_group_starting_with,
        crate::emit_with_basic_owned_group_ending_with,
    ] {
        assert_eq!(
            old(&program),
            Err(EmitError::ProgramValidation(starting_443_refusal()))
        );
    }
    let mut missing = program.clone();
    missing.root.children[0].iteration = Some(
        missing.root.children[0]
            .iteration
            .clone()
            .unwrap()
            .with_grouping(codegen::GroupingPlan::IntoBlocks { size: 1000 }),
    );
    assert_eq!(
        crate::emit_with_basic_owned_group_into_blocks(&missing),
        Err(EmitError::ProgramValidation(
            ProgramValidationError::MissingGroupingExpression {
                expression: 1000,
                role: codegen::GroupingExpressionRole::BlockSize,
                target_path: path(&["Groups"])
            }
        ))
    );
}

#[test]
fn public_basic_group_into_blocks_445_parent_size_and_existing_callback_roles() {
    let mut p = blocks_445_project(SourceOwner::Primary);
    let scope = &mut p.root.children[0];
    scope.sort_by = Some(6);
    scope.filter = Some(9);
    scope.post_group_filter = Some(2);
    scope.windows = vec![::mapping::SequenceWindow::FromTo { first: 8, last: 7 }];
    starting_443_sync(&mut p);
    let artifacts =
        crate::emit_with_basic_owned_group_into_blocks(&blocks_445_program(&p)).unwrap();
    let source = starting_443_source(&artifacts);
    assert_eq!(source.matches(".MapGroupIntoBlocksSourceCohort(").count(), 2);
    assert!(source.contains("class GroupIntoBlocksControls_1 : global::Ferrule.Runtime.IFerruleBasicGroupIntoBlocksControls"));
    assert!(source.contains("public ulong BlockSize(global::Ferrule.Runtime.FerruleBasicScopeContext parent)\n            => global::Ferrule.Runtime.FerruleSequences.PositiveBlockSize(7U, Node_7(parent));"));
    assert!(!source.contains(" GroupKey("));
    assert!(!source.contains(" StartsGroup("));
    assert!(!source.contains(" EndsGroup("));
    assert!(source.contains("RequireBoolean(Node_2(candidate), 2U)"));
    assert!(source.contains("RequireBoolean(Node_9(candidate), 9U)"));
    assert!(source.contains("0 => Node_6(candidate)"));
    assert!(source.contains("public static ExecutionOutputs ExecuteWithBasicOwnedSources("));
}

#[test]
fn public_basic_group_into_blocks_445_conserves_earlier_grouping_bytes() {
    for mode in 0..4 {
        let mut p = starting_443_project(SourceOwner::Primary);
        if mode != 2 {
            let predicate = p.root.children[0].group_starting_with.take();
            match mode {
                0 => p.root.children[0].group_by = Some(2),
                1 => p.root.children[0].group_adjacent_by = Some(2),
                _ => p.root.children[0].group_ending_with = predicate,
            }
            starting_443_sync(&mut p);
        }
        let program = blocks_445_program(&p);
        assert_eq!(
            crate::emit_with_basic_owned_group_into_blocks(&program),
            crate::emit_with_basic_owned_group_ending_with(&program)
        );
    }
}
