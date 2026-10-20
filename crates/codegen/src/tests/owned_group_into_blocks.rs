//! Common-only IntoBlocks admission checks; no runtime partition/resource oracle.
//! This child module shares the existing Adjacent/Starting scaffold.
use super::*;

/// Owned blocks sized by the receiving parent's root-pinned `Limit` (node 7).
fn blocks(owner: SourceOwner) -> mapping::Project {
    let mut p = starting(owner);
    p.root.children[0].group_starting_with = None;
    p.root.children[0].group_into_blocks = Some(7);
    p
}

fn owned_refusal(expected: &ProgramValidationError) -> Vec<Diagnostic> {
    let ProgramValidationError::OwnedSource { diagnostic } = expected else {
        unreachable!("owned capability refusal")
    };
    vec![Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    }]
}

#[test]
fn blocks_controls_preserve_ir_and_every_old_companion_refuses() {
    for owner in [SourceOwner::Primary, named("Rows"), named(" Røws💠 ")] {
        for mask in 0..32 {
            let mut p = blocks(owner.clone());
            let g = &mut p.root.children[0];
            g.post_group_filter = (mask & 1 != 0).then_some(2);
            g.filter = (mask & 2 != 0).then_some(8);
            g.sort_by = (mask & 4 != 0).then_some(3);
            g.sort_filter_order = if mask & 8 == 0 {
                mapping::SortFilterOrder::SortThenFilter
            } else {
                mapping::SortFilterOrder::FilterThenSort
            };
            g.windows = if mask & 16 == 0 {
                vec![]
            } else {
                vec![mapping::SequenceWindow::First { count: 7 }]
            };
            let wire = serde_json::to_string(&p).unwrap();
            let program = lower_with_basic_owned_group_into_blocks(&p).unwrap();
            let before = program.clone();
            assert_eq!(
                validate_program_with_basic_owned_group_into_blocks(&program),
                Ok(())
            );
            assert_eq!(
                validate_basic_owned_group_into_blocks_emission(&program),
                Ok(())
            );
            assert!(
                legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(&program)
                    .is_ok()
            );
            let iteration = program.root.children[0].iteration.as_ref().unwrap();
            assert_eq!(
                iteration.grouping(),
                Some(GroupingPlan::IntoBlocks { size: 7 })
            );
            assert_eq!(
                iteration.post_group_filter(),
                p.root.children[0].post_group_filter
            );
            assert_eq!(iteration.filter(), p.root.children[0].filter);
            let expected = iteration_refusal(
                route(),
                Some(owner.clone()),
                OwnedSourceCapabilityReason::Grouping,
            );
            for validate in [
                validate_program,
                validate_owned_emission,
                validate_basic_owned_emission,
                validate_program_with_basic_owned_filters,
                validate_basic_owned_filter_emission,
                validate_program_with_basic_owned_sort,
                validate_basic_owned_sort_emission,
                validate_program_with_basic_owned_windows,
                validate_basic_owned_window_emission,
                validate_program_with_basic_owned_row_failures,
                validate_basic_owned_row_failure_emission,
                validate_program_with_basic_owned_group_by,
                validate_basic_owned_group_by_emission,
                validate_program_with_basic_owned_sequence_item_at,
                validate_basic_owned_sequence_item_at_emission,
                validate_program_with_basic_owned_group_adjacent_by,
                validate_basic_owned_group_adjacent_by_emission,
                validate_program_with_basic_owned_group_starting_with,
                validate_basic_owned_group_starting_with_emission,
                validate_program_with_basic_owned_group_ending_with,
                validate_basic_owned_group_ending_with_emission,
                validate_program_with_basic_owned_failure_sources,
                validate_basic_owned_failure_source_emission,
                validate_program_with_basic_owned_path_hierarchy,
                validate_basic_owned_path_hierarchy_emission,
                validate_program_with_basic_owned_adjacency_tree,
                validate_basic_owned_adjacency_tree_emission,
            ] {
                assert_eq!(validate(&program), Err(expected.clone()));
            }
            for old_lower in [
                lower,
                lower_with_basic_owned_filters,
                lower_with_basic_owned_sort,
                lower_with_basic_owned_windows,
                lower_with_basic_owned_row_failures,
                lower_with_basic_owned_group_by,
                lower_with_basic_owned_sequence_item_at,
                lower_with_basic_owned_group_adjacent_by,
                lower_with_basic_owned_group_starting_with,
                lower_with_basic_owned_group_ending_with,
                lower_with_basic_owned_failure_sources,
                lower_with_basic_owned_path_hierarchy,
                lower_with_basic_owned_adjacency_tree,
            ] {
                assert_eq!(
                    old_lower(&p).unwrap_err().diagnostics(),
                    owned_refusal(&expected).as_slice()
                );
            }
            for old_plan in [
                legacy_named_input_admission_plan,
                legacy_named_input_admission_plan_with_basic_owned_filters,
                legacy_named_input_admission_plan_with_basic_owned_sort,
                legacy_named_input_admission_plan_with_basic_owned_windows,
                legacy_named_input_admission_plan_with_basic_owned_row_failures,
                legacy_named_input_admission_plan_with_basic_owned_group_by,
                legacy_named_input_admission_plan_with_basic_owned_sequence_item_at,
                legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by,
                legacy_named_input_admission_plan_with_basic_owned_group_starting_with,
                legacy_named_input_admission_plan_with_basic_owned_group_ending_with,
                legacy_named_input_admission_plan_with_basic_owned_failure_sources,
                legacy_named_input_admission_plan_with_basic_owned_path_hierarchy,
                legacy_named_input_admission_plan_with_basic_owned_adjacency_tree,
            ] {
                assert_eq!(old_plan(&program).unwrap_err(), expected);
            }
            assert_eq!(program, before);
            assert_eq!(serde_json::to_string(&p).unwrap(), wire);
        }
    }
}

#[test]
fn blocks_profile_admits_all_five_modes() {
    for mode in 0..5 {
        let mut p = blocks(SourceOwner::Primary);
        let g = &mut p.root.children[0];
        g.group_into_blocks = None;
        let grouping = match mode {
            0 => {
                g.group_by = Some(1);
                GroupingPlan::By { key: 1 }
            }
            1 => {
                g.group_adjacent_by = Some(1);
                GroupingPlan::AdjacentBy { key: 1 }
            }
            2 => {
                g.group_starting_with = Some(2);
                GroupingPlan::StartingWith { predicate: 2 }
            }
            3 => {
                g.group_ending_with = Some(2);
                GroupingPlan::EndingWith { predicate: 2 }
            }
            _ => {
                g.group_into_blocks = Some(9);
                GroupingPlan::IntoBlocks { size: 9 }
            }
        };
        let program = lower_with_basic_owned_group_into_blocks(&p).unwrap();
        assert_eq!(
            program.root.children[0]
                .iteration
                .as_ref()
                .unwrap()
                .grouping(),
            Some(grouping)
        );
    }
}

#[test]
fn nested_blocks_under_a_demanded_group_are_admitted_only_by_the_blocks_profile() {
    let mut p = blocks(SourceOwner::Primary);
    p.root.children[0].children[0].group_into_blocks = Some(9);
    let program = lower_with_basic_owned_group_into_blocks(&p).unwrap();
    assert_eq!(
        program.root.children[0].children[0]
            .iteration
            .as_ref()
            .unwrap()
            .grouping(),
        Some(GroupingPlan::IntoBlocks { size: 9 })
    );
    // Under an admitted Ending parent, the Ending companion still refuses the
    // nested blocks scope.
    p.root.children[0].group_into_blocks = None;
    p.root.children[0].group_ending_with = Some(2);
    assert!(lower_with_basic_owned_group_into_blocks(&p).is_ok());
    assert!(matches!(
        lower_with_basic_owned_group_ending_with(&p),
        Err(error) if matches!(
            error.diagnostics(),
            [Diagnostic::OwnedSource { diagnostic }]
                if matches!(**diagnostic, OwnedSourceDiagnostic::Capability {
                    reason: OwnedSourceCapabilityReason::Grouping,
                    ..
                })
        )
    ));
}

#[test]
fn block_size_is_a_parent_dependency_in_named_input_catalogues() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    let mut p = blocks(SourceOwner::Primary);
    let program = lower_with_basic_owned_group_into_blocks(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(&program).unwrap(),
        plan([O, O, O, U], [O, O, O, U], [O, O, O, U], [O, O, O, U])
    );
    // A legacy named read in the size is eager for the primary target only.
    p.graph.nodes.insert(31, legacy(&["Rows", "Limit"]));
    p.root.children[0].group_into_blocks = Some(31);
    let program = lower_with_basic_owned_group_into_blocks(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(&program).unwrap(),
        plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
    );
}
