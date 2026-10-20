//! Common-only Ending admission checks; no runtime partition/resource oracle.
//! This child module shares the existing Adjacent/Starting scaffold.
use super::*;

fn ending(owner: SourceOwner) -> mapping::Project {
    let mut p = starting(owner);
    p.root.children[0].group_starting_with = None;
    p.root.children[0].group_ending_with = Some(2);
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
fn ending_controls_preserve_ir_and_every_old_companion_refuses() {
    for owner in [SourceOwner::Primary, named("Rows"), named(" Røws💠 ")] {
        for mask in 0..32 {
            let mut p = ending(owner.clone());
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
            let program = lower_with_basic_owned_group_ending_with(&p).unwrap();
            let before = program.clone();
            assert_eq!(
                validate_program_with_basic_owned_group_ending_with(&program),
                Ok(())
            );
            assert_eq!(
                validate_basic_owned_group_ending_with_emission(&program),
                Ok(())
            );
            assert!(
                legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&program)
                    .is_ok()
            );
            let iteration = program.root.children[0].iteration.as_ref().unwrap();
            assert_eq!(
                iteration.grouping(),
                Some(GroupingPlan::EndingWith { predicate: 2 })
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
            ] {
                assert_eq!(old_plan(&program).unwrap_err(), expected);
            }
            assert_eq!(program, before);
            assert_eq!(serde_json::to_string(&p).unwrap(), wire);
        }
    }
}

#[test]
fn ending_profile_admits_every_earlier_mode_and_keeps_blocks_closed() {
    for mode in 0..5 {
        let mut p = ending(SourceOwner::Primary);
        let g = &mut p.root.children[0];
        g.group_ending_with = None;
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
        if mode < 4 {
            let program = lower_with_basic_owned_group_ending_with(&p).unwrap();
            assert_eq!(
                program.root.children[0]
                    .iteration
                    .as_ref()
                    .unwrap()
                    .grouping(),
                Some(grouping)
            );
            continue;
        }
        let expected = iteration_refusal(
            route(),
            Some(SourceOwner::Primary),
            OwnedSourceCapabilityReason::Grouping,
        );
        assert_eq!(
            lower_with_basic_owned_group_ending_with(&p)
                .unwrap_err()
                .diagnostics(),
            owned_refusal(&expected).as_slice()
        );
        let mut direct =
            lower_with_basic_owned_group_ending_with(&ending(SourceOwner::Primary)).unwrap();
        direct.expressions.push(ExpressionNode {
            id: 9,
            expression: Expression::Const {
                value: Value::Int(1),
            },
        });
        direct.root.children[0].iteration = Some(
            IterationPlan::owned_source(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            })
            .with_filtered_grouping(grouping, 2),
        );
        assert_eq!(
            validate_basic_owned_group_ending_with_emission(&direct),
            Err(expected.clone())
        );
        assert_eq!(
            validate_program_with_basic_owned_group_ending_with(&direct),
            Err(expected.clone())
        );
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&direct)
                .unwrap_err(),
            expected
        );
    }
}

#[test]
fn nested_ending_under_a_demanded_group_is_admitted_and_blocks_stay_closed() {
    let mut p = ending(SourceOwner::Primary);
    p.root.children[0].children[0].group_ending_with = Some(8);
    let program = lower_with_basic_owned_group_ending_with(&p).unwrap();
    assert_eq!(
        program.root.children[0].children[0]
            .iteration
            .as_ref()
            .unwrap()
            .grouping(),
        Some(GroupingPlan::EndingWith { predicate: 8 })
    );
    // The Starting companion still refuses the nested Ending scope.
    assert!(lower_with_basic_owned_group_starting_with(&p).is_err());
    p.root.children[0].children[0].group_ending_with = None;
    p.root.children[0].children[0].group_into_blocks = Some(9);
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
fn ending_catalogues_keep_owned_optional_lazy_legacy_and_global_dependencies() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    let mut p = ending(SourceOwner::Primary);
    let program = lower_with_basic_owned_group_ending_with(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&program).unwrap(),
        plan([O, O, O, U], [O, O, O, U], [O, O, O, U], [O, O, O, U])
    );
    p.graph.nodes.insert(31, legacy(&["Rows", "Marker"]));
    p.graph.nodes.insert(
        6,
        Node::If {
            condition: 8,
            then: 20,
            else_: 31,
        },
    );
    let program = lower_with_basic_owned_group_ending_with(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&program).unwrap(),
        plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
    );
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::All,
        message: Some(31),
    });
    let program = lower_with_basic_owned_group_ending_with(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&program).unwrap(),
        plan([O, R, O, U], [O, R, O, U], [O, R, O, U], [O, R, O, U])
    );
}
