//! Common-only Starting admission checks; no runtime partition/resource oracle.
//! This child module shares the existing Adjacent scaffold and literal helpers.
use super::*;

// #444 Ending admission reuses this module's static scaffold.
#[path = "owned_group_ending_with.rs"]
mod ending_with;
// #445 IntoBlocks admission reuses the same static scaffold.
#[path = "owned_group_into_blocks.rs"]
mod into_blocks;

fn starting(owner: SourceOwner) -> mapping::Project {
    let mut p = project(owner);
    p.root.children[0].group_adjacent_by = None;
    p.root.children[0].group_starting_with = Some(2);
    p
}

#[test]
fn starting_controls_preserve_ir_and_every_old_companion_refuses() {
    for owner in [SourceOwner::Primary, named("Rows"), named(" Røws💠 ")] {
        for mask in 0..32 {
            let mut p = starting(owner.clone());
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
            let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
            let before = program.clone();
            assert_eq!(
                validate_program_with_basic_owned_group_starting_with(&program),
                Ok(())
            );
            assert_eq!(
                validate_basic_owned_group_starting_with_emission(&program),
                Ok(())
            );
            let iteration = program.root.children[0].iteration.as_ref().unwrap();
            assert_eq!(
                iteration.grouping(),
                Some(GroupingPlan::StartingWith { predicate: 2 })
            );
            assert_eq!(
                iteration.post_group_filter(),
                p.root.children[0].post_group_filter
            );
            assert_eq!(iteration.filter(), p.root.children[0].filter);
            assert_eq!(
                iteration.windows(),
                p.root.children[0]
                    .windows
                    .iter()
                    .copied()
                    .map(SequenceWindow::from)
                    .collect::<Vec<_>>()
            );
            if let Some(sort) = iteration.sort() {
                assert_eq!(
                    sort.filter_order(),
                    p.root.children[0].sort_filter_order.into()
                );
            }
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
            ] {
                let ProgramValidationError::OwnedSource { diagnostic } = &expected else {
                    unreachable!()
                };
                assert_eq!(
                    old_lower(&p).unwrap_err().diagnostics(),
                    &[Diagnostic::OwnedSource {
                        diagnostic: diagnostic.clone(),
                    }]
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
            ] {
                assert_eq!(old_plan(&program).unwrap_err(), expected);
            }
            assert_eq!(program, before);
            assert_eq!(serde_json::to_string(&p).unwrap(), wire);
        }
    }
}

#[test]
fn starting_profile_keeps_by_adjacent_positive_and_ending_blocks_closed() {
    for mode in 0..5 {
        let mut p = starting(SourceOwner::Primary);
        let g = &mut p.root.children[0];
        g.group_starting_with = None;
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
        if mode < 3 {
            let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
            assert_eq!(
                program.root.children[0]
                    .iteration
                    .as_ref()
                    .unwrap()
                    .grouping(),
                Some(grouping)
            );
        } else {
            let expected = iteration_refusal(
                route(),
                Some(SourceOwner::Primary),
                OwnedSourceCapabilityReason::Grouping,
            );
            let error = lower_with_basic_owned_group_starting_with(&p).unwrap_err();
            let ProgramValidationError::OwnedSource { diagnostic } = &expected else {
                unreachable!()
            };
            assert_eq!(
                error.diagnostics(),
                &[Diagnostic::OwnedSource {
                    diagnostic: diagnostic.clone()
                }]
            );
            let mut direct =
                lower_with_basic_owned_group_starting_with(&starting(SourceOwner::Primary))
                    .unwrap();
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
                validate_basic_owned_group_starting_with_emission(&direct),
                Err(expected)
            );
        }
    }
}

#[test]
fn starting_boundary_or_post_only_legacy_demand_marks_descendants_but_not_siblings() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    for post_only in [false, true] {
        let mut p = starting(SourceOwner::Primary);
        // There are no owned target/member bindings or selectors in this branch.
        // Only the boundary, or independently the post predicate, demands ownership.
        p.graph.nodes.insert(
            31,
            Node::SourceField {
                path: path(&["Label"]),
                frame: Some(path(&["Rows"])),
            },
        );
        p.graph.nodes.insert(
            32,
            Node::Position {
                collection: path(&["Rows"]),
            },
        );
        p.graph.nodes.insert(
            33,
            Node::SourceField {
                path: path(&["Keep"]),
                frame: Some(path(&["Rows"])),
            },
        );
        let g = &mut p.root.children[0];
        g.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
        g.group_starting_with = Some(if post_only { 33 } else { 2 });
        g.post_group_filter = post_only.then_some(2);
        g.bindings = vec![
            mapping::Binding {
                target_field: "First".into(),
                node: 31,
            },
            mapping::Binding {
                target_field: "Position".into(),
                node: 32,
            },
        ];
        let member = &mut g.children[0];
        member.iteration = mapping::ScopeIteration::Source(vec![]);
        member.bindings = vec![
            mapping::Binding {
                target_field: "Label".into(),
                node: 31,
            },
            mapping::Binding {
                target_field: "Position".into(),
                node: 32,
            },
        ];
        let island = mapping::Scope {
            target_field: "Legacy".into(),
            iteration: mapping::ScopeIteration::Source(path(&["Rows"])),
            group_ending_with: Some(8),
            ..Default::default()
        };
        let SchemaKind::Group { children, .. } = &mut p.target.kind else {
            unreachable!()
        };
        children.push(SchemaNode::group("Legacy", vec![]).repeating());
        p.root.children.push(island);
        let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_starting_with(&program)
                .unwrap(),
            plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
        );
        // The wholly ordinary Ending sibling remains an island. The same mode
        // nested under the demanded group must see inherited GroupMembers.
        p.root.children[0].children[0].group_ending_with = Some(8);
        let expected = iteration_refusal(
            route().descendant(mapping::SourceScopeStep::Child(0)),
            None,
            OwnedSourceCapabilityReason::Grouping,
        );
        let ProgramValidationError::OwnedSource { diagnostic } = expected else {
            unreachable!()
        };
        assert_eq!(
            lower_with_basic_owned_group_starting_with(&p)
                .unwrap_err()
                .diagnostics(),
            &[Diagnostic::OwnedSource { diagnostic }]
        );
    }
}

#[test]
fn starting_catalogues_keep_owned_optional_lazy_legacy_and_global_dependencies() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    let mut p = starting(SourceOwner::Primary);
    let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_starting_with(&program).unwrap(),
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
    let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_starting_with(&program).unwrap(),
        plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
    );
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::All,
        message: Some(31),
    });
    let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_starting_with(&program).unwrap(),
        plan([O, R, O, U], [O, R, O, U], [O, R, O, U], [O, R, O, U])
    );
}

#[test]
fn starting_item_at_all_four_producers_and_nested_inputs_keep_four_receiving_phases() {
    for producer in 0..4 {
        let mut p = starting(SourceOwner::Primary);
        add_item_at(&mut p, producer);
        // Equal compares a selected value with itself to keep a Boolean boundary.
        p.graph.nodes.insert(
            101,
            Node::Call {
                function: "equal".into(),
                args: vec![100, 100],
            },
        );
        let g = &mut p.root.children[0];
        g.group_starting_with = Some(101);
        g.post_group_filter = Some(101);
        g.windows = vec![mapping::SequenceWindow::First { count: 100 }];
        g.bindings[1].node = 100;
        g.children[0].bindings[1].node = 100;
        let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
        let visits =
            crate::owned_admission::primary_visits_for_test_with_basic_owned_group_starting_with(
                &program,
            )
            .unwrap();
        for id in [5, 100] {
            let contexts = visits
                .iter()
                .filter(|(namespace, _, node)| namespace.is_none() && *node == id)
                .map(|(_, context, _)| *context)
                .collect::<BTreeSet<_>>();
            assert_eq!(contexts.len(), 4, "parent/candidate/group/member for {id}");
        }
        assert_eq!(
            visits.iter().filter(|(_, _, node)| *node == 101).count(),
            1,
            "boundary and post share the candidate environment"
        );
    }
    let mut p = starting(SourceOwner::Primary);
    add_item_at(&mut p, 0);
    p.graph.nodes.insert(91, legacy(&[]));
    p.graph.nodes.insert(
        101,
        Node::SequenceItemAt {
            sequence: mapping::SequenceExpr::Generate {
                from: None,
                to: 9,
                item: 91,
            },
            index: 100,
        },
    );
    p.graph.nodes.insert(
        102,
        Node::Call {
            function: "equal".into(),
            args: vec![101, 101],
        },
    );
    p.root.children[0].group_starting_with = Some(102);
    assert!(lower_with_basic_owned_group_starting_with(&p).is_ok());
}

#[test]
fn starting_root_alias_group_first_and_scalar_shapes_preserve_descriptors() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    for alias in [false, true] {
        let mut p = starting(named("Rows"));
        p.source = SchemaNode::group("Input", vec![]);
        let g = &mut p.root.children[0];
        g.iteration = if alias {
            mapping::ScopeIteration::Source(path(&["Rows"]))
        } else {
            mapping::ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: named("Rows"),
                    path: vec![],
                },
                output_path: None,
            }
        };
        g.group_starting_with = Some(8);
        g.post_group_filter = None;
        g.bindings.retain(|binding| binding.node == 6);
        g.children.clear();
        let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
        let i = program.root.children[0].iteration.as_ref().unwrap();
        if alias {
            assert_eq!(i.source_iteration().unwrap().path(), path(&["Rows"]));
        } else {
            assert!(i.owned_source_iteration().unwrap().source().path.is_empty());
        }
        let required = if alias { R } else { O };
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_starting_with(&program)
                .unwrap(),
            plan(
                [O, required, O, U],
                [O, required, O, U],
                [O, O, O, U],
                [O, O, O, U]
            )
        );
    }
    let mut p = starting(SourceOwner::Primary);
    child_mut(&mut p.target, "Groups").repeating = false;
    p.root.children[0].iteration_output = mapping::IterationOutput::First;
    assert!(lower_with_basic_owned_group_starting_with(&p).is_ok());
    let mut p = starting(SourceOwner::Primary);
    child_mut(child_mut(&mut p.target, "Groups"), "Members").kind = SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    let member = &mut p.root.children[0].children[0];
    member.bindings.clear();
    member.construction = mapping::ScopeConstruction::Scalar { value: 4 };
    assert!(lower_with_basic_owned_group_starting_with(&p).is_ok());
    let mut p = starting(SourceOwner::Primary);
    p.source = SchemaNode::scalar("Input", ScalarType::Bool).repeating();
    p.target = SchemaNode::scalar("Output", ScalarType::Bool).repeating();
    p.graph.nodes.insert(
        50,
        Node::OwnedSourceField(field(SourceOwner::Primary, &[], Some(&[]))),
    );
    p.graph.nodes.retain(|id, _| [20, 21, 30, 50].contains(id));
    p.root = mapping::Scope {
        iteration: mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec![],
            },
            output_path: None,
        },
        group_starting_with: Some(50),
        construction: mapping::ScopeConstruction::Scalar { value: 50 },
        ..Default::default()
    };
    assert!(lower_with_basic_owned_group_starting_with(&p).is_ok());
    p.target.repeating = false;
    p.root.iteration_output = mapping::IterationOutput::First;
    assert!(
        lower_with_basic_owned_group_starting_with(&p).is_err(),
        "scalar First stays closed"
    );
}

#[test]
fn starting_missing_predicate_and_post_keep_structural_precedence() {
    for (missing, post) in [(99, 2), (2, 99)] {
        let mut p = starting(SourceOwner::Primary);
        p.root.children[0].group_starting_with = Some(missing);
        p.root.children[0].post_group_filter = Some(post);
        let ordinary = lower(&p).unwrap_err();
        let expected_message = if missing == 99 {
            "group-starting-with predicate references missing node 99"
        } else {
            "post-group filter references missing node 99"
        };
        assert!(
            ordinary
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(diagnostic,
            Diagnostic::Validation { message, .. } if message == expected_message))
        );
        assert_eq!(
            lower_with_basic_owned_group_starting_with(&p),
            Err(ordinary)
        );
        let mut direct =
            lower_with_basic_owned_group_starting_with(&starting(SourceOwner::Primary)).unwrap();
        direct.root.children[0].iteration = Some(
            IterationPlan::owned_source(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            })
            .with_filtered_grouping(GroupingPlan::StartingWith { predicate: missing }, post),
        );
        let expected = if missing == 99 {
            ProgramValidationError::MissingGroupingExpression {
                target_path: path(&["Groups"]),
                role: GroupingExpressionRole::StartingPredicate,
                expression: 99,
            }
        } else {
            ProgramValidationError::MissingPostGroupFilterExpression {
                target_path: path(&["Groups"]),
                expression: 99,
            }
        };
        assert_eq!(
            validate_program_with_basic_owned_group_starting_with(&direct),
            Err(expected.clone())
        );
        assert_eq!(
            validate_basic_owned_group_starting_with_emission(&direct),
            Err(expected.clone())
        );
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_starting_with(&direct)
                .unwrap_err(),
            expected
        );
    }
    for malformed in 0..4 {
        let mut p = starting(SourceOwner::Primary);
        let g = &mut p.root.children[0];
        let message = match malformed {
            0 => {
                g.group_starting_with = None;
                "post-group filter requires one grouping control"
            }
            1 => {
                g.group_by = Some(1);
                "scope grouping modes are mutually exclusive"
            }
            2 => {
                g.group_adjacent_by = Some(1);
                "scope grouping modes are mutually exclusive"
            }
            _ => {
                g.iteration = mapping::ScopeIteration::None;
                "group-starting-with predicate has no iterated source"
            }
        };
        let wire = serde_json::to_string(&p).unwrap();
        let error = lower_with_basic_owned_group_starting_with(&p).unwrap_err();
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(diagnostic,
            Diagnostic::Validation { message: actual, .. } if actual == message))
        );
        assert_eq!(serde_json::to_string(&p).unwrap(), wire);
    }
}

#[test]
fn starting_shared_owned_leaf_keeps_parent_boundary_group_and_member_environments() {
    use NamedInputAvailability::{Optional as O, ReservedUnavailable as U};
    let mut p = starting(SourceOwner::Primary);
    p.graph.nodes.insert(
        700,
        Node::OwnedSourceField(field(named("Rows"), &["Limit"], Some(&[]))),
    );
    p.graph.nodes.insert(
        701,
        Node::Call {
            function: "equal".into(),
            args: vec![700, 9],
        },
    );
    let g = &mut p.root.children[0];
    g.group_starting_with = Some(701);
    g.windows = vec![mapping::SequenceWindow::First { count: 700 }];
    g.bindings[1].node = 700;
    g.children[0].bindings[1].node = 700;
    let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
    let visits =
        crate::owned_admission::primary_visits_for_test_with_basic_owned_group_starting_with(
            &program,
        )
        .unwrap();
    assert_eq!(
        visits
            .iter()
            .filter(|(namespace, _, node)| namespace.is_none() && *node == 700)
            .map(|(_, context, _)| *context)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_starting_with(&program).unwrap(),
        plan([O, O, O, U], [O, O, O, U], [O, O, O, U], [O, O, O, U])
    );
}

#[test]
fn starting_profile_preserves_all_five_ordinary_grouping_islands() {
    for grouping in [
        GroupingPlan::By { key: 9 },
        GroupingPlan::AdjacentBy { key: 9 },
        GroupingPlan::StartingWith { predicate: 8 },
        GroupingPlan::EndingWith { predicate: 8 },
        GroupingPlan::IntoBlocks { size: 9 },
    ] {
        let mut p = starting(SourceOwner::Primary);
        let SchemaKind::Group { children, .. } = &mut p.target.kind else {
            unreachable!()
        };
        children.push(SchemaNode::group("Island", vec![]).repeating());
        let mut island = mapping::Scope {
            target_field: "Island".into(),
            iteration: mapping::ScopeIteration::Source(path(&["Rows"])),
            filter: Some(8),
            sort_by: Some(9),
            post_group_filter: Some(8),
            windows: vec![mapping::SequenceWindow::First { count: 9 }],
            ..Default::default()
        };
        match grouping {
            GroupingPlan::By { key } => island.group_by = Some(key),
            GroupingPlan::AdjacentBy { key } => island.group_adjacent_by = Some(key),
            GroupingPlan::StartingWith { predicate } => {
                island.group_starting_with = Some(predicate)
            }
            GroupingPlan::EndingWith { predicate } => island.group_ending_with = Some(predicate),
            GroupingPlan::IntoBlocks { size } => island.group_into_blocks = Some(size),
        }
        p.root.children.push(island);
        let program = lower_with_basic_owned_group_starting_with(&p).unwrap();
        assert_eq!(
            program.root.children[1]
                .iteration
                .as_ref()
                .unwrap()
                .grouping(),
            Some(grouping)
        );
        assert_eq!(
            validate_basic_owned_group_starting_with_emission(&program),
            Ok(())
        );
    }
}

#[test]
fn starting_item_at_does_not_open_specialized_dependencies_or_generated_iteration() {
    let mut p = starting(SourceOwner::Primary);
    add_item_at(&mut p, 0);
    p.root.children[0].bindings[1].node = 100;
    let base = lower_with_basic_owned_group_starting_with(&p).unwrap();
    for specialized in [
        Expression::Aggregate {
            function: AggregateFunction::Count,
            collection: path(&["Rows"]),
            value: AggregateValue::Path(vec![]),
            arg: None,
        },
        Expression::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Key"]),
            matches: 9,
            value: path(&["Label"]),
        },
    ] {
        let mut direct = base.clone();
        direct.expressions.push(ExpressionNode {
            id: 101,
            expression: specialized,
        });
        let Expression::SequenceItemAt { index, .. } = expression(&mut direct, 100) else {
            unreachable!()
        };
        *index = 101;
        assert_eq!(
            validate_basic_owned_group_starting_with_emission(&direct),
            Err(expression_refusal(
                101,
                OwnedSourceCapabilityReason::AggregateLookup
            ))
        );
    }
    let mut direct = base;
    direct.expressions.push(ExpressionNode {
        id: 91,
        expression: Expression::SourceField {
            path: vec![],
            frame: None,
        },
    });
    direct.root.children[0].children[0].iteration =
        Some(IterationPlan::generated(GeneratedSequence::Range {
            from: None,
            to: 9,
            item: 91,
        }));
    assert_eq!(
        validate_basic_owned_group_starting_with_emission(&direct),
        Err(iteration_refusal(
            route().descendant(mapping::SourceScopeStep::Child(0)),
            Some(SourceOwner::Primary),
            OwnedSourceCapabilityReason::GeneratedItem,
        ))
    );
}

#[test]
fn starting_retains_dynamic_aliases_hidden_below_boundary_and_post_item_at() {
    for post in [false, true] {
        let mut p = starting(SourceOwner::Primary);
        add_item_at(&mut p, 0);
        p.graph
            .nodes
            .insert(31, legacy(&["unavailableDynamic", "Limit"]));
        p.graph.nodes.insert(
            32,
            Node::ValueMap {
                input: 31,
                input_type: None,
                table: vec![],
                default: None,
            },
        );
        p.graph.nodes.insert(
            7,
            Node::RuntimeParameterDefault {
                name: "hidden".into(),
                ty: ScalarType::Int,
                default: 32,
                preview: None,
            },
        );
        p.graph.nodes.insert(
            101,
            Node::Call {
                function: "equal".into(),
                args: vec![100, 100],
            },
        );
        if post {
            p.root.children[0].post_group_filter = Some(101);
        } else {
            p.root.children[0].group_starting_with = Some(101);
        }
        let error = lower_with_basic_owned_group_starting_with(&p).unwrap_err();
        assert!(
            matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
            if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression { namespace: OwnedSourceNamespace::Main, node: 31, scope: None },
                request: None, reason: OwnedSourceCapabilityReason::UnsupportedBasicContext,
                origin: OwnedSourceCapabilityOrigin::RetainedExpression { namespace: OwnedSourceNamespace::Main, node: 31 },
            }))
        );
    }
}

#[test]
fn starting_target_item_at_does_not_widen_primary_row_failure_expressions() {
    let mut p = starting(SourceOwner::Primary);
    add_item_at(&mut p, 0);
    p.root.children[0].bindings[1].node = 100;
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 2 },
        message: Some(4),
    });
    let base = lower_with_basic_owned_group_starting_with(&p).unwrap();
    assert!(is_basic_owned_primary_row_failure(
        &base,
        &base.failure_rules[0]
    ));
    for predicate in [false, true] {
        let mut direct = base.clone();
        if predicate {
            direct.failure_rules[0].selection = FailureSelection::WhenTrue(100);
        } else {
            direct.failure_rules[0].message = Some(100);
        }
        assert!(!is_basic_owned_primary_row_failure(
            &direct,
            &direct.failure_rules[0]
        ));
        let (node, parts, pin) = if predicate {
            (7, path(&["Limit"]), vec![])
        } else {
            (2, path(&["Rows", "Keep"]), path(&["Rows"]))
        };
        let expected = ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node,
                    scope: None,
                },
                request: Some(OwnedSourceDiagnosticRequest::Field {
                    owner: OwnedSourceDiagnosticOwner::Primary,
                    path: parts,
                    frame: Some(pin),
                }),
                reason: OwnedSourceCapabilityReason::FailureRule,
                origin: OwnedSourceCapabilityOrigin::Failure { index: 0 },
            }),
        };
        let before = direct.clone();
        assert_eq!(
            validate_basic_owned_group_starting_with_emission(&direct),
            Err(expected)
        );
        assert_eq!(direct, before);
    }
}
