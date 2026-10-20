// Ending fixtures reuse the Starting Project with its boundary moved to the
// Ending field. Execution is covered by the runtime and CLI parity hosts.
fn ending_444_project(owner: SourceOwner) -> ::mapping::Project {
    let mut p = starting_443_project(owner);
    p.root.children[0].group_starting_with = None;
    p.root.children[0].group_ending_with = Some(101);
    starting_443_sync(&mut p);
    p
}

fn ending_444_program(project: &::mapping::Project) -> Program {
    codegen::lower_with_basic_owned_group_ending_with(project).unwrap()
}

#[test]
fn public_basic_group_ending_with_444_keeps_old_profiles_and_structural_errors() {
    let program = ending_444_program(&ending_444_project(SourceOwner::Primary));
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
            .with_grouping(codegen::GroupingPlan::EndingWith { predicate: 1000 }),
    );
    assert_eq!(
        crate::emit_with_basic_owned_group_ending_with(&missing),
        Err(EmitError::ProgramValidation(
            ProgramValidationError::MissingGroupingExpression {
                expression: 1000,
                role: codegen::GroupingExpressionRole::EndingPredicate,
                target_path: path(&["Groups"])
            }
        ))
    );
    let mut blocks = program.clone();
    blocks.expressions.push(codegen::ExpressionNode {
        id: 103,
        expression: codegen::Expression::Const {
            value: Value::Int(2),
        },
    });
    blocks.root.children[0].iteration = Some(
        blocks.root.children[0]
            .iteration
            .clone()
            .unwrap()
            .with_grouping(codegen::GroupingPlan::IntoBlocks { size: 103 }),
    );
    assert_eq!(
        crate::emit_with_basic_owned_group_ending_with(&blocks),
        Err(EmitError::ProgramValidation(starting_443_refusal()))
    );
}

#[test]
fn public_basic_group_ending_with_444_bool_control_and_existing_callback_roles() {
    let mut p = ending_444_project(SourceOwner::Primary);
    let scope = &mut p.root.children[0];
    scope.sort_by = Some(6);
    scope.filter = Some(9);
    scope.post_group_filter = Some(2);
    scope.windows = vec![::mapping::SequenceWindow::FromTo { first: 8, last: 7 }];
    starting_443_sync(&mut p);
    let artifacts = crate::emit_with_basic_owned_group_ending_with(&ending_444_program(&p)).unwrap();
    let source = starting_443_source(&artifacts);
    assert_eq!(source.matches(".MapGroupEndingWithSourceCohort(").count(), 2);
    assert!(source.contains("class GroupEndingWithControls_1 : global::Ferrule.Runtime.IFerruleBasicGroupEndingWithControls"));
    assert!(source.contains("public bool EndsGroup(global::Ferrule.Runtime.FerruleBasicScopeContext candidate)\n            => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_101(candidate), 101U);"));
    assert!(!source.contains(" GroupKey("));
    assert!(!source.contains(" StartsGroup("));
    assert!(source.contains("RequireBoolean(Node_2(candidate), 2U)"));
    assert!(source.contains("RequireBoolean(Node_9(candidate), 9U)"));
    assert!(source.contains("0 => Node_6(candidate)"));
    assert!(
        source.find("ItemCount(8U, Node_8(parent))").unwrap()
            < source.find("ItemCount(7U, Node_7(parent))").unwrap()
    );
    assert!(source.contains("public static ExecutionOutputs ExecuteWithBasicOwnedSources("));
}

#[test]
fn public_basic_group_ending_with_444_conserves_by_adjacent_and_starting_bytes() {
    for mode in 0..3 {
        let mut p = starting_443_project(SourceOwner::Primary);
        if mode < 2 {
            p.root.children[0].group_starting_with = None;
            if mode == 0 {
                p.root.children[0].group_by = Some(2);
            } else {
                p.root.children[0].group_adjacent_by = Some(2);
            }
            starting_443_sync(&mut p);
        }
        let program = ending_444_program(&p);
        assert_eq!(
            crate::emit_with_basic_owned_group_ending_with(&program),
            crate::emit_with_basic_owned_group_starting_with(&program)
        );
    }
}
