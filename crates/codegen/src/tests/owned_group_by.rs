use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::collections::{BTreeMap, BTreeSet};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn child_mut<'a>(schema: &'a mut SchemaNode, name: &str) -> &'a mut SchemaNode {
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("expected group schema");
    };
    children
        .iter_mut()
        .find(|child| child.name == name)
        .unwrap()
}

fn owned(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}

fn source() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Marker", ScalarType::String),
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
}

fn project(owner: SourceOwner) -> mapping::Project {
    let member = SchemaNode::group(
        "Members",
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Position", ScalarType::Int),
        ],
    )
    .repeating();
    mapping::Project {
        source: source(),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("Ordinal", ScalarType::Int),
                        SchemaNode::scalar("Root", ScalarType::String),
                        member,
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: [
            "Unused",
            "Rows",
            "Key",
            "Post",
            "Bound",
            "Output",
            "Global",
            " Røws💠 ",
        ]
        .into_iter()
        .map(|name| mapping::NamedSource {
            name: name.into(),
            path: "unused.json".into(),
            schema: source(),
            options: Default::default(),
            dynamic_path: None,
        })
        .collect(),
        extra_targets: vec![],
        user_functions: Default::default(),
        failure_rules: vec![],
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (1, owned(owner.clone(), &["Rows", "Key"], Some(&["Rows"]))),
                (2, owned(owner.clone(), &["Rows", "Keep"], Some(&["Rows"]))),
                (3, owned(owner.clone(), &["Rows", "Rank"], Some(&["Rows"]))),
                (4, owned(owner.clone(), &["Rows", "Label"], Some(&["Rows"]))),
                (
                    5,
                    Node::OwnedPosition(OwnedPosition {
                        owner: owner.clone(),
                        collection: Some(path(&["Rows"])),
                    }),
                ),
                (6, owned(owner.clone(), &["Marker"], Some(&[]))),
                (7, owned(owner.clone(), &["Limit"], Some(&[]))),
                (
                    8,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    9,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]),
        },
        root: mapping::Scope {
            children: vec![mapping::Scope {
                target_field: "Groups".into(),
                iteration: mapping::ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: owner.clone(),
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                group_by: Some(1),
                post_group_filter: Some(2),
                bindings: vec![
                    mapping::Binding {
                        target_field: "First".into(),
                        node: 4,
                    },
                    mapping::Binding {
                        target_field: "Ordinal".into(),
                        node: 5,
                    },
                    mapping::Binding {
                        target_field: "Root".into(),
                        node: 6,
                    },
                ],
                children: vec![mapping::Scope {
                    target_field: "Members".into(),
                    iteration: mapping::ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner,
                            path: path(&["Rows"]),
                        },
                        output_path: None,
                    },
                    bindings: vec![
                        mapping::Binding {
                            target_field: "Label".into(),
                            node: 4,
                        },
                        mapping::Binding {
                            target_field: "Position".into(),
                            node: 5,
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

fn route() -> mapping::SourceScopeLocation {
    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0))
}

fn grouping_refusal(owner: SourceOwner) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Iteration { scope: route() },
            request: Some(OwnedSourceDiagnosticRequest::Iteration {
                owner: (&owner).into(),
                path: path(&["Rows"]),
            }),
            reason: OwnedSourceCapabilityReason::Grouping,
            origin: OwnedSourceCapabilityOrigin::Scope { scope: route() },
        }),
    }
}

fn avail(catalogue: &NamedInputCatalogue<'_>) -> Vec<NamedInputAvailability> {
    catalogue
        .entries
        .iter()
        .map(|entry| entry.availability)
        .collect()
}

fn legacy(parts: &[&str]) -> Node {
    Node::SourceField {
        path: path(parts),
        frame: None,
    }
}

#[test]
fn owned_group_by_profile_matrix_preserves_controls_and_every_old_gate() {
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named {
            name: "Rows".into(),
        },
        SourceOwner::Named {
            name: " Røws💠 ".into(),
        },
    ] {
        for mask in 0..32 {
            let mut p = project(owner.clone());
            let scope = &mut p.root.children[0];
            scope.post_group_filter = (mask & 1 != 0).then_some(2);
            scope.filter = (mask & 2 != 0).then_some(8);
            scope.sort_by = (mask & 4 != 0).then_some(3);
            scope.sort_filter_order = if mask & 8 == 0 {
                mapping::SortFilterOrder::SortThenFilter
            } else {
                mapping::SortFilterOrder::FilterThenSort
            };
            scope.windows = if mask & 16 == 0 {
                vec![]
            } else {
                vec![
                    mapping::SequenceWindow::First { count: 7 },
                    mapping::SequenceWindow::Last { count: 9 },
                ]
            };
            let program = lower_with_basic_owned_group_by(&p).expect("static By profile");
            assert_eq!(validate_program_with_basic_owned_group_by(&program), Ok(()));
            assert_eq!(validate_basic_owned_group_by_emission(&program), Ok(()));
            let iteration = program.root.children[0].iteration.as_ref().unwrap();
            assert_eq!(iteration.grouping(), Some(GroupingPlan::By { key: 1 }));
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
            for validate in [
                validate_program,
                validate_basic_owned_emission,
                validate_basic_owned_filter_emission,
                validate_basic_owned_sort_emission,
                validate_basic_owned_window_emission,
                validate_basic_owned_row_failure_emission,
            ] {
                assert_eq!(validate(&program), Err(grouping_refusal(owner.clone())));
            }
            assert!(lower(&p).is_err());
            assert!(lower_with_basic_owned_row_failures(&p).is_err());
        }
    }
}

#[test]
fn owned_group_by_composes_implicit_first_and_nested_member_windows() {
    let mut p = project(SourceOwner::Primary);
    child_mut(&mut p.target, "Groups").repeating = false;
    let group = &mut p.root.children[0];
    group.iteration_output = mapping::IterationOutput::First;
    group.windows = vec![mapping::SequenceWindow::Last { count: 7 }];
    group.children[0].filter = Some(2);
    group.children[0].sort_by = Some(3);
    group.children[0].windows = vec![mapping::SequenceWindow::SkipFirst { count: 9 }];
    for member_source in [
        mapping::ScopeIteration::Source(vec![]),
        group.children[0].iteration.clone(),
    ] {
        p.root.children[0].children[0].iteration = member_source;
        let program = lower_with_basic_owned_group_by(&p).unwrap();
        let group = &program.root.children[0];
        assert_eq!(
            group.iteration.as_ref().unwrap().output(),
            IterationOutput::First
        );
        assert_eq!(
            group.iteration.as_ref().unwrap().windows(),
            &[SequenceWindow::Last { count: 7 }]
        );
        assert_eq!(
            group.children[0].iteration.as_ref().unwrap().filter(),
            Some(2)
        );
        assert_eq!(validate_basic_owned_group_by_emission(&program), Ok(()));
    }
}

#[test]
fn owned_group_by_accepts_empty_path_singleton_and_repeated_root_without_new_ir() {
    for repeating in [false, true] {
        let mut p = project(SourceOwner::Primary);
        p.source.repeating = repeating;
        let group = &mut p.root.children[0];
        group.iteration = mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec![],
            },
            output_path: None,
        };
        group.group_by = Some(9);
        group.post_group_filter = None;
        group
            .bindings
            .retain(|binding| binding.target_field == "Root");
        group.children.clear();
        let program = lower_with_basic_owned_group_by(&p).unwrap();
        let iteration = program.root.children[0].iteration.as_ref().unwrap();
        assert_eq!(
            iteration.owned_source_iteration().unwrap().source(),
            &OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec![]
            }
        );
        assert_eq!(iteration.grouping(), Some(GroupingPlan::By { key: 9 }));
        assert_eq!(program.source, p.source);
    }
}

#[test]
fn owned_group_by_accepts_repeated_scalar_and_nested_scalar_construction() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Values", ScalarType::String).repeating()],
    );
    p.target = SchemaNode::scalar("Output", ScalarType::String).repeating();
    p.graph.nodes = BTreeMap::from([(
        1,
        owned(SourceOwner::Primary, &["Values"], Some(&["Values"])),
    )]);
    p.root = mapping::Scope {
        iteration: mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Values"]),
            },
            output_path: None,
        },
        group_by: Some(1),
        construction: mapping::ScopeConstruction::Scalar { value: 1 },
        ..Default::default()
    };
    let program = lower_with_basic_owned_group_by(&p).unwrap();
    assert!(matches!(
        program.root.construction,
        TargetConstruction::Scalar { expression: 1, .. }
    ));
    assert_eq!(validate_basic_owned_group_by_emission(&program), Ok(()));
    // A member scalar needs no redundant filter/sort/window to read its item.
    let mut p = project(SourceOwner::Primary);
    child_mut(child_mut(&mut p.target, "Groups"), "Members").kind = ir::SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    let member = &mut p.root.children[0].children[0];
    member.bindings.clear();
    member.construction = mapping::ScopeConstruction::Scalar { value: 4 };
    assert!(lower_with_basic_owned_group_by(&p).is_ok());
}

#[test]
fn owned_group_by_discovers_key_only_and_post_only_owned_demand_on_legacy_source() {
    for owned_root in [1, 2] {
        let mut p = project(SourceOwner::Primary);
        let group = &mut p.root.children[0];
        group.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
        group.children.clear();
        group.bindings.clear();
        group.group_by = Some(if owned_root == 1 { 1 } else { 9 });
        group.post_group_filter = (owned_root == 2).then_some(2);
        let program = lower_with_basic_owned_group_by(&p).unwrap();
        assert_eq!(
            program
                .expressions
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            if owned_root == 1 { vec![1] } else { vec![2, 9] }
        );
        assert_eq!(validate_basic_owned_group_by_emission(&program), Ok(()));
        let error = validate_program(&program).unwrap_err();
        assert!(
            matches!(error, ProgramValidationError::OwnedSource { diagnostic }
            if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression { node, scope: Some(ref scope), .. },
                reason: OwnedSourceCapabilityReason::Grouping, ..
            } if node == owned_root && *scope == route()))
        );
    }
}

#[test]
fn owned_group_by_keeps_primary_row_failures_orthogonal_and_same_spelling_named_input_lazy() {
    let mut p = project(SourceOwner::Primary);
    p.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 2 },
        message: Some(4),
    }];
    let program = lower_with_basic_owned_group_by(&p).unwrap();
    assert!(is_basic_owned_primary_row_failure(
        &program,
        &program.failure_rules[0]
    ));
    assert_eq!(
        program.failure_rules[0],
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: FailureSelection::WhenTrue(2),
            message: Some(4),
        }
    );
    let plan = legacy_named_input_admission_plan_with_basic_owned_group_by(&program).unwrap();
    assert_eq!(avail(&plan.primary), [NamedInputAvailability::Optional; 8]);
    assert!(legacy_named_input_admission_plan_with_basic_owned_row_failures(&program).is_err());
    // Adding By permission does not qualify named row-position failure rules.
    let mut program = program;
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == 4)
        .unwrap()
        .expression = Expression::OwnedPosition(OwnedPosition {
        owner: SourceOwner::Named {
            name: "Rows".into(),
        },
        collection: None,
    });
    assert!(!is_basic_owned_primary_row_failure(
        &program,
        &program.failure_rules[0]
    ));
    assert!(matches!(validate_basic_owned_group_by_emission(&program),
    Err(ProgramValidationError::OwnedSource { diagnostic })
    if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
        reason: OwnedSourceCapabilityReason::FailureRule,
        origin: OwnedSourceCapabilityOrigin::Failure { index: 0 }, ..
    })));
}

#[test]
fn owned_group_by_catalogues_keep_global_and_selected_target_dependencies_and_exact_ordinals() {
    let mut p = project(SourceOwner::Named {
        name: "Rows".into(),
    });
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(30, vec![]));
    p.graph.nodes.insert(
        30,
        Node::Const {
            value: Value::String("unused.json".into()),
        },
    );
    p.graph.nodes.insert(1, legacy(&["Key", "Marker"]));
    p.graph.nodes.insert(2, legacy(&["Post", "Marker"]));
    p.graph.nodes.insert(7, legacy(&["Bound", "Limit"]));
    p.graph.nodes.insert(20, legacy(&["Output", "Marker"]));
    p.graph.nodes.insert(21, legacy(&["Global", "Marker"]));
    p.root.children[0].windows = vec![mapping::SequenceWindow::First { count: 7 }];
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::All,
        message: Some(21),
    });
    p.extra_targets.push(mapping::NamedTarget {
        name: " exact target💠 ".into(),
        path: None,
        options: Default::default(),
        schema: SchemaNode::group(
            "Named",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 20,
            }],
            ..Default::default()
        },
    });
    let program = lower_with_basic_owned_group_by(&p).unwrap();
    let plan = legacy_named_input_admission_plan_with_basic_owned_group_by(&program).unwrap();
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    assert_eq!(avail(&plan.primary), [U, O, R, R, R, O, R, O]);
    assert_eq!(
        avail(&plan.named_targets[0].catalogue),
        [U, O, O, O, O, R, R, O]
    );
    assert_eq!(avail(&plan.all_outputs), [U, O, R, R, R, R, R, O]);
    assert_eq!(plan.named_targets[0].name, " exact target💠 ");
    assert_eq!(plan.named_targets[0].target_index, 0);
    assert_eq!(
        plan.primary
            .entries
            .iter()
            .map(|entry| (entry.declaration_index, entry.name))
            .collect::<Vec<_>>(),
        vec![
            (0, "Unused"),
            (1, "Rows"),
            (2, "Key"),
            (3, "Post"),
            (4, "Bound"),
            (5, "Output"),
            (6, "Global"),
            (7, " Røws💠 ")
        ]
    );
    assert!(std::ptr::eq(
        plan.primary.entries[7].name.as_ptr(),
        program.extra_sources[7].name.as_ptr()
    ));
}

#[test]
fn owned_group_by_context_reuse_preserves_parent_candidate_grouped_and_member_roles() {
    let mut p = project(SourceOwner::Primary);
    // The same scalar graph node is intentionally a bound, a key, a member
    // retention predicate and a grouped binding. Value type checking is runtime.
    p.root.children[0].group_by = Some(5);
    p.root.children[0].post_group_filter = Some(5);
    p.root.children[0].windows = vec![mapping::SequenceWindow::First { count: 5 }];
    let program = lower_with_basic_owned_group_by(&p).unwrap();
    let visits =
        crate::owned_admission::primary_visits_for_test_with_basic_owned_group_by(&program)
            .unwrap();
    let contexts = visits
        .iter()
        .filter(|(namespace, _, node)| namespace.is_none() && *node == 5)
        .map(|(_, context, _)| *context)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        contexts.len(),
        4,
        "parent, candidate, grouped output and member contexts must remain distinct"
    );
    // Reusing a harmless parent's key node cannot hide its disallowed child use.
    let mut program = program;
    program.root.children[0].children[0].iteration =
        Some(IterationPlan::source(vec![]).with_grouping(GroupingPlan::AdjacentBy { key: 5 }));
    let child_route = route().descendant(mapping::SourceScopeStep::Child(0));
    assert!(matches!(validate_basic_owned_group_by_emission(&program),
        Err(ProgramValidationError::OwnedSource { diagnostic })
        if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Iteration { ref scope },
            origin: OwnedSourceCapabilityOrigin::Scope { scope: ref origin },
            reason: OwnedSourceCapabilityReason::Grouping, ..
        } if *scope == child_route && *origin == child_route)));
}

#[test]
fn owned_group_by_admission_puts_post_in_candidate_not_grouped_environment() {
    let mut p = project(SourceOwner::Primary);
    let group = &mut p.root.children[0];
    group.children.clear();
    group.bindings.clear();
    group.group_by = Some(2);
    group.post_group_filter = Some(2);
    let program = lower_with_basic_owned_group_by(&p).unwrap();
    let visits =
        crate::owned_admission::primary_visits_for_test_with_basic_owned_group_by(&program)
            .unwrap();
    assert_eq!(
        visits.iter().filter(|(_, _, node)| *node == 2).count(),
        1,
        "key and retention share the original candidate, not the grouped wrapper"
    );
}

#[test]
fn owned_group_by_keeps_other_owned_modes_closed_with_original_scope_provenance() {
    for grouping in [
        GroupingPlan::AdjacentBy { key: 1 },
        GroupingPlan::StartingWith { predicate: 2 },
        GroupingPlan::EndingWith { predicate: 2 },
        GroupingPlan::IntoBlocks { size: 7 },
    ] {
        // Lower a valid By scaffold with this mode's expression so reachability
        // retains its root before changing only the grouping capability.
        let mut project = project(SourceOwner::Primary);
        project.root.children[0].group_by = Some(grouping.expression());
        let mut p = lower_with_basic_owned_group_by(&project).unwrap();
        let iteration = p.root.children[0].iteration.take().unwrap();
        p.root.children[0].iteration = Some(iteration.with_filtered_grouping(grouping, 2));
        assert_eq!(
            validate_basic_owned_group_by_emission(&p),
            Err(grouping_refusal(SourceOwner::Primary))
        );
    }
}

#[test]
fn owned_group_by_keeps_ordinary_legacy_grouping_islands_and_prior_profiles_unchanged() {
    for mode in 0..5 {
        let mut p = project(SourceOwner::Primary);
        let mut island = mapping::Scope {
            target_field: "Legacy".into(),
            iteration: mapping::ScopeIteration::Source(path(&["Rows"])),
            filter: Some(8),
            sort_by: Some(9),
            windows: vec![mapping::SequenceWindow::First { count: 9 }],
            post_group_filter: Some(8),
            ..Default::default()
        };
        match mode {
            0 => island.group_by = Some(9),
            1 => island.group_adjacent_by = Some(9),
            2 => island.group_starting_with = Some(8),
            3 => island.group_ending_with = Some(8),
            _ => island.group_into_blocks = Some(9),
        }
        if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
            children.push(SchemaNode::group("Legacy", vec![]).repeating());
        }
        p.root.children.push(island);
        let program = lower_with_basic_owned_group_by(&p).unwrap();
        assert_eq!(validate_basic_owned_group_by_emission(&program), Ok(()));
        // The ordinary grouping representation and legacy eager alias survive.
        assert_eq!(
            program.root.children[1]
                .iteration
                .as_ref()
                .unwrap()
                .source_iteration()
                .unwrap()
                .path(),
            path(&["Rows"])
        );
        let plan = legacy_named_input_admission_plan_with_basic_owned_group_by(&program).unwrap();
        assert_eq!(
            plan.primary.entries[1].availability,
            NamedInputAvailability::RequiredLegacy
        );
        // Giving that same island an owned key/post dependency closes non-By.
        if mode != 0 {
            let mut program = program;
            let iteration = program.root.children[1].iteration.as_ref().unwrap();
            program.root.children[1].iteration = Some(
                IterationPlan::new_grouped(
                    iteration.input().clone(),
                    iteration.filter(),
                    iteration.sort().cloned(),
                    iteration.grouping(),
                    iteration.windows().to_vec(),
                    iteration.output(),
                )
                .with_filtered_grouping(iteration.grouping().unwrap(), 2),
            );
            assert!(matches!(validate_basic_owned_group_by_emission(&program),
            Err(ProgramValidationError::OwnedSource { diagnostic })
            if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
                reason: OwnedSourceCapabilityReason::Grouping, ..
            })));
        }
    }
}

#[test]
fn owned_group_by_reports_missing_controls_before_capability_fallback() {
    let base = lower_with_basic_owned_group_by(&project(SourceOwner::Primary)).unwrap();
    for (grouping, role) in [
        (GroupingPlan::By { key: 99 }, GroupingExpressionRole::Key),
        (
            GroupingPlan::AdjacentBy { key: 99 },
            GroupingExpressionRole::AdjacentKey,
        ),
        (
            GroupingPlan::IntoBlocks { size: 99 },
            GroupingExpressionRole::BlockSize,
        ),
    ] {
        let mut p = base.clone();
        p.root.children[0].iteration = Some(
            IterationPlan::owned_source(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            })
            .with_grouping(grouping),
        );
        assert_eq!(
            validate_basic_owned_group_by_emission(&p),
            Err(ProgramValidationError::MissingGroupingExpression {
                target_path: path(&["Groups"]),
                role,
                expression: 99
            })
        );
    }
    let mut p = base;
    p.root.children[0].iteration = Some(
        IterationPlan::owned_source(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        })
        .with_filtered_grouping(GroupingPlan::By { key: 1 }, 99),
    );
    assert_eq!(
        validate_basic_owned_group_by_emission(&p),
        Err(ProgramValidationError::MissingPostGroupFilterExpression {
            target_path: path(&["Groups"]),
            expression: 99
        })
    );
}

#[test]
fn owned_group_by_preserves_malformed_project_diagnostics_exactly() {
    for malformed in 0..3 {
        let mut p = project(SourceOwner::Primary);
        let scope = &mut p.root.children[0];
        let message = match malformed {
            0 => {
                scope.group_by = None;
                "post-group filter requires one grouping control"
            }
            1 => {
                scope.group_adjacent_by = Some(1);
                "scope grouping modes are mutually exclusive"
            }
            _ => {
                scope.iteration = mapping::ScopeIteration::None;
                "group-by key has no iterated source"
            }
        };
        let old = lower(&p).unwrap_err();
        assert!(
            old.diagnostics()
                .iter()
                .any(|diagnostic| matches!(diagnostic,
            Diagnostic::Validation { message: actual, .. } if actual == message))
        );
        assert_eq!(lower_with_basic_owned_group_by(&p), Err(old));
    }
}

#[test]
fn owned_group_by_key_and_post_keep_dynamic_legacy_dependency_gates() {
    for root in [1, 2] {
        let mut p = project(SourceOwner::Primary);
        p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(30, vec![]));
        p.graph.nodes.insert(
            30,
            Node::Const {
                value: Value::String("dynamic.json".into()),
            },
        );
        p.graph.nodes.insert(root, legacy(&["Unused", "Marker"]));
        // A ValueMap can hide an ordinary alias under either new control root.
        p.graph.nodes.insert(
            31,
            Node::ValueMap {
                input: root,
                input_type: None,
                table: vec![],
                default: None,
            },
        );
        if root == 1 {
            p.root.children[0].group_by = Some(31);
        } else {
            p.root.children[0].post_group_filter = Some(31);
        }
        let error = lower_with_basic_owned_group_by(&p).unwrap_err();
        assert!(
            matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
            if matches!(**diagnostic, OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression { node, .. },
                reason: OwnedSourceCapabilityReason::UnsupportedBasicContext,
                origin: OwnedSourceCapabilityOrigin::RetainedExpression { node: origin, .. },
                ..
            } if node == root && origin == root))
        );
    }
}

#[test]
fn owned_group_by_preserves_preexisting_control_profiles_without_grouping() {
    for mask in 0..8 {
        let mut p = project(SourceOwner::Primary);
        let scope = &mut p.root.children[0];
        scope.group_by = None;
        scope.post_group_filter = None;
        scope.filter = (mask & 1 != 0).then_some(2);
        scope.sort_by = (mask & 2 != 0).then_some(3);
        scope.windows = if mask & 4 == 0 {
            vec![]
        } else {
            vec![mapping::SequenceWindow::Last { count: 7 }]
        };
        let prior = lower_with_basic_owned_windows(&p).unwrap();
        assert_eq!(lower_with_basic_owned_group_by(&p), Ok(prior.clone()));
        assert_eq!(validate_basic_owned_window_emission(&prior), Ok(()));
        assert_eq!(validate_basic_owned_group_by_emission(&prior), Ok(()));
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_windows(&prior).unwrap(),
            legacy_named_input_admission_plan_with_basic_owned_group_by(&prior).unwrap()
        );
    }
}

#[test]
fn owned_group_by_legacy_key_demand_establishes_grouped_ancestry_for_children() {
    let mut p = project(SourceOwner::Primary);
    let group = &mut p.root.children[0];
    group.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
    group.post_group_filter = None;
    group.bindings.clear();
    let member = &mut group.children[0];
    member.iteration = mapping::ScopeIteration::Source(vec![]);
    member.bindings.clear();
    member.group_adjacent_by = Some(9);
    let error = lower_with_basic_owned_group_by(&p).unwrap_err();
    let child_route = route().descendant(mapping::SourceScopeStep::Child(0));
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if matches!(**diagnostic, OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Iteration { ref scope },
            reason: OwnedSourceCapabilityReason::Grouping,
            origin: OwnedSourceCapabilityOrigin::Scope { scope: ref origin }, ..
        } if *scope == child_route && *origin == child_route))
    );
}

#[test]
fn owned_group_by_specialized_grouped_consumer_keeps_expression_and_scope_provenance() {
    let mut p = project(SourceOwner::Primary);
    p.root.children[0].iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
    p.root.children[0].post_group_filter = None;
    p.root.children[0].children.clear();
    p.root.children[0].bindings.clear();
    let mut program = lower_with_basic_owned_group_by(&p).unwrap();
    program.expressions.push(ExpressionNode {
        id: 80,
        expression: Expression::Aggregate {
            function: AggregateFunction::Count,
            collection: vec![],
            value: AggregateValue::Path(vec![]),
            arg: None,
        },
    });
    program.root.children[0].bindings.push(Binding {
        target_field: "Ordinal".into(),
        expression: 80,
        target_domain: ScalarType::Int.into(),
        repeating: false,
    });
    assert_eq!(
        validate_basic_owned_group_by_emission(&program),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 80,
                    scope: Some(route()),
                },
                request: None,
                reason: OwnedSourceCapabilityReason::AggregateLookup,
                origin: OwnedSourceCapabilityOrigin::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 80
                },
            })
        })
    );
}

#[test]
fn owned_group_by_saved_legacy_project_roundtrip_and_lowering_remain_identical() {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes = BTreeMap::from([
        (1, legacy(&["Key"])),
        (2, legacy(&["Keep"])),
        (4, legacy(&["Label"])),
        (5, Node::Position { collection: vec![] }),
        (
            6,
            Node::Const {
                value: Value::String("legacy".into()),
            },
        ),
    ]);
    let group = &mut p.root.children[0];
    group.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
    group.children[0].iteration = mapping::ScopeIteration::Source(vec![]);
    let serialized = serde_json::to_string(&p).unwrap();
    assert!(!serialized.contains("owned_source"));
    let restored: mapping::Project = serde_json::from_str(&serialized).unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), serialized);
    let ordinary = lower(&restored).unwrap();
    assert_eq!(
        lower_with_basic_owned_group_by(&restored),
        Ok(ordinary.clone())
    );
    assert_eq!(validate_program(&ordinary), Ok(()));
    assert_eq!(
        legacy_named_input_admission_plan(&ordinary).unwrap(),
        legacy_named_input_admission_plan_with_basic_owned_group_by(&ordinary).unwrap()
    );
}
