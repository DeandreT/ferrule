//! Static capability/admission tests only. Runtime grouping oracles belong to
//! the paired runtimes and the independent three-engine corpus.
use crate::*;
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::collections::{BTreeMap, BTreeSet};

// Reuse the established static scaffold, not any observed output or diagnostic.
#[path = "owned_group_starting_with.rs"]
mod starting_with;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}

fn field(owner: SourceOwner, parts: &[&str], pin: Option<&[&str]>) -> OwnedSourceField {
    OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: pin.map(path),
    }
}

fn legacy(parts: &[&str]) -> Node {
    Node::SourceField {
        path: path(parts),
        frame: None,
    }
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Marker", ScalarType::String),
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}

fn project(owner: SourceOwner) -> mapping::Project {
    mapping::Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("Position", ScalarType::Int),
                        SchemaNode::scalar("Root", ScalarType::String),
                        SchemaNode::group(
                            "Members",
                            vec![
                                SchemaNode::scalar("Label", ScalarType::String),
                                SchemaNode::scalar("Position", ScalarType::Int),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["unusedStatic", "Rows", " Røws💠 ", "unavailableDynamic"]
            .into_iter()
            .map(|name| mapping::NamedSource {
                name: name.into(),
                path: "unused.json".into(),
                schema: schema(),
                options: Default::default(),
                dynamic_path: (name == "unavailableDynamic")
                    .then_some(mapping::DynamicSourcePath::legacy(30, vec![])),
            })
            .collect(),
        extra_targets: [(" exact audit💠 ", 20), (" U ", 21)]
            .into_iter()
            .map(|(name, node)| mapping::NamedTarget {
                name: name.into(),
                path: None,
                options: Default::default(),
                schema: SchemaNode::group(
                    "Audit",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                ),
                root: mapping::Scope {
                    bindings: vec![mapping::Binding {
                        target_field: "Value".into(),
                        node,
                    }],
                    ..Default::default()
                },
            })
            .collect(),
        user_functions: Default::default(),
        failure_rules: vec![],
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::OwnedSourceField(field(owner.clone(), &["Rows", "Key"], Some(&["Rows"]))),
                ),
                (
                    2,
                    Node::OwnedSourceField(field(
                        owner.clone(),
                        &["Rows", "Keep"],
                        Some(&["Rows"]),
                    )),
                ),
                (
                    3,
                    Node::OwnedSourceField(field(
                        owner.clone(),
                        &["Rows", "Rank"],
                        Some(&["Rows"]),
                    )),
                ),
                (
                    4,
                    Node::OwnedSourceField(field(
                        owner.clone(),
                        &["Rows", "Label"],
                        Some(&["Rows"]),
                    )),
                ),
                (
                    5,
                    Node::OwnedPosition(OwnedPosition {
                        owner: owner.clone(),
                        collection: Some(path(&["Rows"])),
                    }),
                ),
                (
                    6,
                    Node::OwnedSourceField(field(owner.clone(), &["Marker"], Some(&[]))),
                ),
                (
                    7,
                    Node::OwnedSourceField(field(owner.clone(), &["Limit"], Some(&[]))),
                ),
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
                (
                    20,
                    Node::OwnedSourceField(field(named("Rows"), &["Marker"], Some(&[]))),
                ),
                (
                    21,
                    Node::OwnedSourceField(field(named(" Røws💠 "), &["Marker"], Some(&[]))),
                ),
                (
                    30,
                    Node::Const {
                        value: Value::String("unused.json".into()),
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
                group_adjacent_by: Some(1),
                post_group_filter: Some(2),
                bindings: vec![
                    mapping::Binding {
                        target_field: "First".into(),
                        node: 4,
                    },
                    mapping::Binding {
                        target_field: "Position".into(),
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

fn iteration_refusal(
    scope: mapping::SourceScopeLocation,
    owner: Option<SourceOwner>,
    reason: OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Iteration {
                scope: scope.clone(),
            },
            request: owner.map(|owner| OwnedSourceDiagnosticRequest::Iteration {
                owner: (&owner).into(),
                path: path(&["Rows"]),
            }),
            reason,
            origin: OwnedSourceCapabilityOrigin::Scope { scope },
        }),
    }
}

fn expression_refusal(node: u32, reason: OwnedSourceCapabilityReason) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
                scope: Some(route()),
            },
            request: None,
            reason,
            origin: OwnedSourceCapabilityOrigin::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}

fn expression(program: &mut Program, id: u32) -> &mut Expression {
    &mut program
        .expressions
        .iter_mut()
        .find(|node| node.id == id)
        .unwrap()
        .expression
}

fn child_mut<'a>(schema: &'a mut SchemaNode, name: &str) -> &'a mut SchemaNode {
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("group")
    };
    children
        .iter_mut()
        .find(|child| child.name == name)
        .unwrap()
}

fn catalogue(availability: [NamedInputAvailability; 4]) -> NamedInputCatalogue<'static> {
    // Explicit original slots, never derived from a Program or actual plan.
    NamedInputCatalogue {
        entries: vec![
            NamedInputAdmission {
                declaration_index: 0,
                name: "unusedStatic",
                availability: availability[0],
            },
            NamedInputAdmission {
                declaration_index: 1,
                name: "Rows",
                availability: availability[1],
            },
            NamedInputAdmission {
                declaration_index: 2,
                name: " Røws💠 ",
                availability: availability[2],
            },
            NamedInputAdmission {
                declaration_index: 3,
                name: "unavailableDynamic",
                availability: availability[3],
            },
        ],
    }
}

fn plan(
    all: [NamedInputAvailability; 4],
    primary: [NamedInputAvailability; 4],
    audit: [NamedInputAvailability; 4],
    unicode: [NamedInputAvailability; 4],
) -> NamedInputAdmissionPlan<'static> {
    NamedInputAdmissionPlan {
        all_outputs: catalogue(all),
        primary: catalogue(primary),
        named_targets: vec![
            NamedTargetInputCatalogue {
                target_index: 0,
                name: " exact audit💠 ",
                catalogue: catalogue(audit),
            },
            NamedTargetInputCatalogue {
                target_index: 1,
                name: " U ",
                catalogue: catalogue(unicode),
            },
        ],
    }
}

#[test]
fn owned_group_adjacent_by_all_control_combinations_preserve_ir_and_old_api_refusals() {
    for owner in [SourceOwner::Primary, named("Rows"), named(" Røws💠 ")] {
        for mask in 0..32 {
            let mut p = project(owner.clone());
            let group = &mut p.root.children[0];
            group.post_group_filter = (mask & 1 != 0).then_some(2);
            group.filter = (mask & 2 != 0).then_some(8);
            group.sort_by = (mask & 4 != 0).then_some(3);
            group.sort_filter_order = if mask & 8 == 0 {
                mapping::SortFilterOrder::SortThenFilter
            } else {
                mapping::SortFilterOrder::FilterThenSort
            };
            group.windows = if mask & 16 == 0 {
                vec![]
            } else {
                vec![
                    mapping::SequenceWindow::First { count: 7 },
                    mapping::SequenceWindow::Last { count: 9 },
                ]
            };
            let wire = serde_json::to_string(&p).unwrap();
            let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
            assert_eq!(
                validate_program_with_basic_owned_group_adjacent_by(&program),
                Ok(())
            );
            assert_eq!(
                validate_basic_owned_group_adjacent_by_emission(&program),
                Ok(())
            );
            assert_eq!(serde_json::to_string(&p).unwrap(), wire);
            let iteration = program.root.children[0].iteration.as_ref().unwrap();
            assert_eq!(
                iteration.grouping(),
                Some(GroupingPlan::AdjacentBy { key: 1 })
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
            let before = program.clone();
            for validate in [
                validate_program,
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
                validate_owned_emission,
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
            ] {
                assert_eq!(
                    old_lower(&p).unwrap_err().diagnostics(),
                    &[Diagnostic::OwnedSource {
                        diagnostic: match &expected {
                            ProgramValidationError::OwnedSource { diagnostic } =>
                                diagnostic.clone(),
                            _ => unreachable!(),
                        },
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
            ] {
                assert_eq!(old_plan(&program).unwrap_err(), expected);
            }
            assert_eq!(program, before, "rejecting APIs must not mutate a Program");
        }
    }
}

#[test]
fn owned_group_adjacent_by_full_literal_catalogues_keep_optional_alias_global_and_reserved_slots() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    let mut p = project(SourceOwner::Primary);
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap(),
        plan([O, O, O, U], [O, O, O, U], [O, O, O, U], [O, O, O, U])
    );
    p.graph.nodes.insert(6, legacy(&["Rows", "Marker"]));
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap(),
        plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
    );
    p.graph.nodes.insert(31, legacy(&["Rows", "Marker"]));
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::All,
        message: Some(31),
    });
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    let actual =
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap();
    assert_eq!(
        actual,
        plan([O, R, O, U], [O, R, O, U], [O, R, O, U], [O, R, O, U])
    );
    assert!(std::ptr::eq(
        actual.primary.entries[2].name.as_ptr(),
        program.extra_sources[2].name.as_ptr()
    ));
}

#[test]
fn owned_group_adjacent_by_all_control_dependencies_have_literal_complete_target_catalogues() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(1, legacy(&[" Røws💠 ", "Rows", "Key"]));
    p.graph
        .nodes
        .insert(2, legacy(&["unusedStatic", "Rows", "Keep"]));
    p.graph.nodes.insert(7, legacy(&["Rows", "Limit"]));
    p.root.children[0].windows = vec![mapping::SequenceWindow::First { count: 7 }];
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap(),
        plan([R, R, R, U], [R, R, R, U], [O, O, O, U], [O, O, O, U])
    );
    p.graph.nodes.insert(31, legacy(&[" Røws💠 ", "Marker"]));
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::All,
        message: Some(31),
    });
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap(),
        plan([R, R, R, U], [R, R, R, U], [O, O, R, U], [O, O, R, U])
    );
}

#[test]
fn owned_group_adjacent_by_named_root_alias_is_eager_but_owned_empty_path_is_canonical() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    for alias in [false, true] {
        let mut p = project(named("Rows"));
        p.source = SchemaNode::group("Input", vec![]);
        // Only the named-root routes remain meaningful in this variation.
        let group = &mut p.root.children[0];
        group.iteration = if alias {
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
        group.group_adjacent_by = Some(9);
        group.post_group_filter = None;
        group.bindings.retain(|binding| binding.node == 6);
        group.children.clear();
        let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
        let iteration = program.root.children[0].iteration.as_ref().unwrap();
        if alias {
            assert_eq!(
                iteration.source_iteration().unwrap().path(),
                path(&["Rows"])
            );
        } else {
            assert!(
                iteration
                    .owned_source_iteration()
                    .unwrap()
                    .source()
                    .path
                    .is_empty()
            );
        }
        let expected = if alias {
            plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
        } else {
            plan([O, O, O, U], [O, O, O, U], [O, O, O, U], [O, O, O, U])
        };
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap(),
            expected
        );
    }
}

#[test]
fn owned_group_adjacent_by_key_or_post_only_demand_keeps_legacy_source_alias_required() {
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    for post_only in [false, true] {
        let mut p = project(SourceOwner::Primary);
        let group = &mut p.root.children[0];
        group.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
        group.group_adjacent_by = Some(if post_only { 9 } else { 1 });
        group.post_group_filter = post_only.then_some(2);
        group.bindings.clear();
        group.children.clear();
        let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap(),
            plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
        );
        let node = if post_only { 2 } else { 1 };
        let mut expected = expression_refusal(node, OwnedSourceCapabilityReason::Grouping);
        let ProgramValidationError::OwnedSource { diagnostic } = &mut expected else {
            unreachable!()
        };
        let OwnedSourceDiagnostic::Capability {
            request, origin, ..
        } = diagnostic.as_mut()
        else {
            unreachable!()
        };
        *request = Some(OwnedSourceDiagnosticRequest::Field {
            owner: OwnedSourceDiagnosticOwner::Primary,
            path: path(&["Rows", if post_only { "Keep" } else { "Key" }]),
            frame: Some(path(&["Rows"])),
        });
        *origin = OwnedSourceCapabilityOrigin::Scope { scope: route() };
        assert_eq!(
            validate_program_with_basic_owned_group_by(&program),
            Err(expected)
        );
    }
}

#[test]
fn owned_group_adjacent_by_ordinary_islands_keep_all_five_modes_and_owned_ancestry_closes_three() {
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
        let expected = match mode {
            0 => {
                island.group_by = Some(9);
                GroupingPlan::By { key: 9 }
            }
            1 => {
                island.group_adjacent_by = Some(9);
                GroupingPlan::AdjacentBy { key: 9 }
            }
            2 => {
                island.group_starting_with = Some(8);
                GroupingPlan::StartingWith { predicate: 8 }
            }
            3 => {
                island.group_ending_with = Some(8);
                GroupingPlan::EndingWith { predicate: 8 }
            }
            _ => {
                island.group_into_blocks = Some(9);
                GroupingPlan::IntoBlocks { size: 9 }
            }
        };
        let SchemaKind::Group { children, .. } = &mut p.target.kind else {
            panic!("target group")
        };
        children.push(SchemaNode::group("Legacy", vec![]).repeating());
        p.root.children.push(island);
        let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
        assert_eq!(
            program.root.children[1]
                .iteration
                .as_ref()
                .unwrap()
                .grouping(),
            Some(expected)
        );
        assert_eq!(
            validate_basic_owned_group_adjacent_by_emission(&program),
            Ok(())
        );
        use NamedInputAvailability::{
            Optional as O, RequiredLegacy as R, ReservedUnavailable as U,
        };
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program).unwrap(),
            plan([O, R, O, U], [O, R, O, U], [O, O, O, U], [O, O, O, U])
        );
        if mode >= 2 {
            p.root.children[1].post_group_filter = Some(2);
            let error = lower_with_basic_owned_group_adjacent_by(&p).unwrap_err();
            assert!(
                matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
                if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
                    location: OwnedSourceDiagnosticLocation::Expression { node: 2, scope: Some(scope), .. },
                    reason: OwnedSourceCapabilityReason::Grouping,
                    origin: OwnedSourceCapabilityOrigin::Scope { scope: origin }, ..
                } if *scope == mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
                    .descendant(mapping::SourceScopeStep::Child(1)) && scope == origin))
            );
        }
    }
}

#[test]
fn owned_group_adjacent_by_closed_modes_preserve_exact_project_and_program_scope_diagnostics() {
    for mode in 0..3 {
        let mut p = project(SourceOwner::Primary);
        let group = &mut p.root.children[0];
        group.group_adjacent_by = None;
        let grouping = match mode {
            0 => {
                group.group_starting_with = Some(2);
                GroupingPlan::StartingWith { predicate: 2 }
            }
            1 => {
                group.group_ending_with = Some(2);
                GroupingPlan::EndingWith { predicate: 2 }
            }
            _ => {
                group.group_into_blocks = Some(9);
                GroupingPlan::IntoBlocks { size: 9 }
            }
        };
        let expected = iteration_refusal(
            route(),
            Some(SourceOwner::Primary),
            OwnedSourceCapabilityReason::Grouping,
        );
        let error = lower_with_basic_owned_group_adjacent_by(&p).unwrap_err();
        assert!(
            matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
            if ProgramValidationError::OwnedSource { diagnostic: diagnostic.clone() } == expected)
        );
        let mut program =
            lower_with_basic_owned_group_adjacent_by(&project(SourceOwner::Primary)).unwrap();
        program.expressions.push(ExpressionNode {
            id: 9,
            expression: Expression::Const {
                value: Value::Int(1),
            },
        });
        program.root.children[0].iteration = Some(
            IterationPlan::owned_source(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            })
            .with_filtered_grouping(grouping, 2),
        );
        assert_eq!(
            validate_basic_owned_group_adjacent_by_emission(&program),
            Err(expected)
        );
    }
}

#[test]
fn owned_group_adjacent_by_keyed_ancestry_is_inherited_from_both_modes_and_legacy_control_demand() {
    for adjacent in [false, true] {
        for post_only in [false, true] {
            let mut p = project(SourceOwner::Primary);
            let group = &mut p.root.children[0];
            group.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
            group.bindings.clear();
            group.group_adjacent_by = adjacent.then_some(if post_only { 9 } else { 1 });
            group.group_by = (!adjacent).then_some(if post_only { 9 } else { 1 });
            group.post_group_filter = post_only.then_some(2);
            let member = &mut group.children[0];
            member.iteration = mapping::ScopeIteration::Source(vec![]);
            member.bindings.clear();
            member.group_by = Some(9);
            assert!(lower_with_basic_owned_group_adjacent_by(&p).is_ok());
            p.root.children[0].children[0].group_by = None;
            p.root.children[0].children[0].group_starting_with = Some(8);
            let error = lower_with_basic_owned_group_adjacent_by(&p).unwrap_err();
            let child_route = route().descendant(mapping::SourceScopeStep::Child(0));
            assert!(
                matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
                if ProgramValidationError::OwnedSource { diagnostic: diagnostic.clone() }
                    == iteration_refusal(child_route, None, OwnedSourceCapabilityReason::Grouping))
            );
        }
    }
}

fn add_item_at(p: &mut mapping::Project, producer: u8) {
    for (id, value) in [
        (40, Value::String("A|B|A".into())),
        (41, Value::String("|".into())),
        (42, Value::String("[|]".into())),
    ] {
        p.graph.nodes.insert(id, Node::Const { value });
    }
    p.graph.nodes.insert(90, legacy(&[]));
    p.graph.nodes.insert(
        100,
        Node::SequenceItemAt {
            sequence: match producer {
                0 => mapping::SequenceExpr::Generate {
                    from: Some(9),
                    to: 7,
                    item: 90,
                },
                1 => mapping::SequenceExpr::Tokenize {
                    input: 40,
                    delimiter: 41,
                    item: 90,
                },
                2 => mapping::SequenceExpr::TokenizeByLength {
                    input: 40,
                    length: 9,
                    item: 90,
                },
                _ => mapping::SequenceExpr::TokenizeRegex {
                    input: 40,
                    pattern: 42,
                    flags: None,
                    item: 90,
                },
            },
            index: 5,
        },
    );
}

#[test]
fn owned_group_adjacent_by_item_at_all_producers_keep_parent_candidate_group_and_member_visits() {
    for producer in 0..4 {
        let mut p = project(SourceOwner::Primary);
        add_item_at(&mut p, producer);
        let group = &mut p.root.children[0];
        group.group_adjacent_by = Some(100);
        group.filter = Some(100);
        group.sort_by = Some(100);
        group.post_group_filter = Some(100);
        group.windows = vec![mapping::SequenceWindow::First { count: 100 }];
        group.bindings[1].node = 100;
        group.children[0].bindings[1].node = 100;
        let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
        assert_eq!(
            validate_basic_owned_group_adjacent_by_emission(&program),
            Ok(())
        );
        let visits =
            crate::owned_admission::primary_visits_for_test_with_basic_owned_group_adjacent_by(
                &program,
            )
            .unwrap();
        for node in [5, 100] {
            let contexts = visits
                .iter()
                .filter(|(namespace, _, id)| namespace.is_none() && *id == node)
                .map(|(_, context, _)| *context)
                .collect::<BTreeSet<_>>();
            assert_eq!(
                contexts.len(),
                4,
                "node {node}: parent/candidate/group/member must be distinct"
            );
        }
        assert_eq!(
            program.root.children[0]
                .iteration
                .as_ref()
                .unwrap()
                .grouping(),
            Some(GroupingPlan::AdjacentBy { key: 100 })
        );
    }
}

#[test]
fn owned_group_adjacent_by_post_shares_candidate_phase_and_nested_item_at_keeps_receiving_context()
{
    let mut p = project(SourceOwner::Primary);
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
    let group = &mut p.root.children[0];
    group.group_adjacent_by = Some(101);
    group.post_group_filter = Some(101);
    group.bindings.clear();
    group.children.clear();
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    let visits =
        crate::owned_admission::primary_visits_for_test_with_basic_owned_group_adjacent_by(
            &program,
        )
        .unwrap();
    for id in [5, 100, 101] {
        assert_eq!(
            visits.iter().filter(|(_, _, node)| *node == id).count(),
            1,
            "key/post and nested arguments must use one candidate environment"
        );
    }
    let mut program = program;
    // A reused node already seen in valid candidate context must not mask a
    // descendant's inherited closure in the full capability Visit key.
    let mut child = program.root.children[0].clone();
    child.target_field = "Members".into();
    child.iteration = Some(
        IterationPlan::source(vec![]).with_grouping(GroupingPlan::EndingWith { predicate: 101 }),
    );
    program.root.children[0].children.push(child);
    let child_route = route().descendant(mapping::SourceScopeStep::Child(0));
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&program),
        Err(iteration_refusal(
            child_route,
            Some(SourceOwner::Primary),
            OwnedSourceCapabilityReason::Grouping
        ))
    );
}

#[test]
fn owned_group_adjacent_by_retains_hidden_dynamic_aliases_in_every_item_at_control_phase() {
    for phase in 0..6 {
        let mut p = project(SourceOwner::Primary);
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
        let group = &mut p.root.children[0];
        match phase {
            0 => group.group_adjacent_by = Some(100),
            1 => group.post_group_filter = Some(100),
            2 => group.filter = Some(100),
            3 => group.sort_by = Some(100),
            4 => group.windows = vec![mapping::SequenceWindow::First { count: 100 }],
            _ => group.bindings[1].node = 100,
        }
        let error = lower_with_basic_owned_group_adjacent_by(&p).unwrap_err();
        assert!(
            matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
            if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression { namespace: OwnedSourceNamespace::Main, node: 31, scope: None },
                request: None,
                reason: OwnedSourceCapabilityReason::UnsupportedBasicContext,
                origin: OwnedSourceCapabilityOrigin::RetainedExpression { namespace: OwnedSourceNamespace::Main, node: 31 },
            })),
            "phase {phase}: {error:#?}"
        );
    }
}

#[test]
fn owned_group_adjacent_by_scalar_construction_and_group_only_first_keep_existing_shapes() {
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
        group.group_adjacent_by = Some(9);
        group.post_group_filter = None;
        group.bindings.retain(|binding| binding.node == 6);
        group.children.clear();
        let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
        assert_eq!(
            program.root.children[0]
                .iteration
                .as_ref()
                .unwrap()
                .owned_source_iteration()
                .unwrap()
                .source()
                .path,
            Vec::<String>::new()
        );
    }
    let mut p = project(SourceOwner::Primary);
    child_mut(&mut p.target, "Groups").repeating = false;
    p.root.children[0].iteration_output = mapping::IterationOutput::First;
    p.root.children[0].windows = vec![mapping::SequenceWindow::Last { count: 7 }];
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    assert_eq!(
        program.root.children[0]
            .iteration
            .as_ref()
            .unwrap()
            .output(),
        IterationOutput::First
    );
    let mut p = project(SourceOwner::Primary);
    child_mut(child_mut(&mut p.target, "Groups"), "Members").kind = SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    let member = &mut p.root.children[0].children[0];
    member.bindings.clear();
    member.construction = mapping::ScopeConstruction::Scalar { value: 4 };
    assert!(
        lower_with_basic_owned_group_adjacent_by(&p).is_ok(),
        "control-free member scalar stays admitted"
    );
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::scalar("Input", ScalarType::String).repeating();
    p.target = SchemaNode::scalar("Output", ScalarType::String).repeating();
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
        group_adjacent_by: Some(50),
        construction: mapping::ScopeConstruction::Scalar { value: 50 },
        ..Default::default()
    };
    let program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    assert!(matches!(
        program.root.construction,
        TargetConstruction::Scalar { expression: 50, .. }
    ));
    p.target.repeating = false;
    p.root.iteration_output = mapping::IterationOutput::First;
    assert!(
        lower_with_basic_owned_group_adjacent_by(&p).is_err(),
        "scalar First remains unsupported"
    );
}

#[test]
fn owned_group_adjacent_by_structure_precedes_capability_in_project_and_direct_programs() {
    for malformed in 0..5 {
        let mut p = project(SourceOwner::Primary);
        let group = &mut p.root.children[0];
        let expected_message = match malformed {
            0 => {
                group.group_adjacent_by = None;
                "post-group filter requires one grouping control"
            }
            1 => {
                group.group_by = Some(1);
                "scope grouping modes are mutually exclusive"
            }
            2 => {
                group.iteration = mapping::ScopeIteration::None;
                "group-adjacent-by key has no iterated source"
            }
            3 => {
                group.group_adjacent_by = Some(99);
                "group-adjacent-by key references missing node 99"
            }
            _ => {
                group.post_group_filter = Some(99);
                "post-group filter references missing node 99"
            }
        };
        let old = lower(&p).unwrap_err();
        assert!(
            old.diagnostics()
                .iter()
                .any(|diagnostic| matches!(diagnostic,
            Diagnostic::Validation { message, .. } if message == expected_message)),
            "{old:#?}"
        );
        assert_eq!(lower_with_basic_owned_group_adjacent_by(&p), Err(old));
    }
    let base = lower_with_basic_owned_group_adjacent_by(&project(SourceOwner::Primary)).unwrap();
    for (key, post, expected) in [
        (
            99,
            2,
            ProgramValidationError::MissingGroupingExpression {
                target_path: path(&["Groups"]),
                role: GroupingExpressionRole::AdjacentKey,
                expression: 99,
            },
        ),
        (
            1,
            99,
            ProgramValidationError::MissingPostGroupFilterExpression {
                target_path: path(&["Groups"]),
                expression: 99,
            },
        ),
    ] {
        let mut p = base.clone();
        p.root.children[0].iteration = Some(
            IterationPlan::owned_source(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            })
            .with_filtered_grouping(GroupingPlan::AdjacentBy { key }, post),
        );
        for validate in [
            validate_program_with_basic_owned_group_adjacent_by,
            validate_basic_owned_group_adjacent_by_emission,
            validate_program,
        ] {
            assert_eq!(validate(&p), Err(expected.clone()));
        }
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&p).unwrap_err(),
            expected
        );
    }
    let mut p = base;
    p.expressions.push(p.expressions[0].clone());
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&p),
        Err(ProgramValidationError::DuplicateExpression { node: 1 })
    );
}

#[test]
fn owned_group_adjacent_by_exact_unknown_and_dynamic_owners_keep_typed_provenance() {
    for owner in [named("Rows "), named("Røws💠"), named("")] {
        let mut p =
            lower_with_basic_owned_group_adjacent_by(&project(SourceOwner::Primary)).unwrap();
        *expression(&mut p, 1) =
            Expression::OwnedSourceField(field(owner.clone(), &["Rows", "Key"], Some(&["Rows"])));
        let expected = ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
                location: OwnedSourceDiagnosticLocation::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 1,
                    scope: None,
                },
                request: OwnedSourceDiagnosticRequest::Field {
                    owner: (&owner).into(),
                    path: path(&["Rows", "Key"]),
                    frame: Some(path(&["Rows"])),
                },
                reason: if owner == named("") {
                    OwnedSourceStructureReason::InvalidNamedOwner
                } else {
                    OwnedSourceStructureReason::UnknownOwner
                },
            }),
        };
        assert_eq!(
            validate_basic_owned_group_adjacent_by_emission(&p),
            Err(expected.clone())
        );
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&p).unwrap_err(),
            expected
        );
        let mut project = project(SourceOwner::Primary);
        project.graph.nodes.insert(
            1,
            Node::OwnedSourceField(field(owner, &["Rows", "Key"], Some(&["Rows"]))),
        );
        assert!(lower_with_basic_owned_group_adjacent_by(&project).is_err());
    }
    let mut p = lower_with_basic_owned_group_adjacent_by(&project(SourceOwner::Primary)).unwrap();
    let owner = named("unavailableDynamic");
    *expression(&mut p, 1) =
        Expression::OwnedSourceField(field(owner.clone(), &["Rows", "Key"], Some(&["Rows"])));
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&p),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 1,
                    scope: Some(route())
                },
                request: Some(OwnedSourceDiagnosticRequest::Field {
                    owner: (&owner).into(),
                    path: path(&["Rows", "Key"]),
                    frame: Some(path(&["Rows"]))
                }),
                reason: OwnedSourceCapabilityReason::DynamicOwner,
                origin: OwnedSourceCapabilityOrigin::DynamicSource {
                    index: 3,
                    name: "unavailableDynamic".into()
                },
            }),
        })
    );
    let mut p = lower_with_basic_owned_group_adjacent_by(&project(SourceOwner::Primary)).unwrap();
    p.root.children[0].iteration = Some(
        IterationPlan::owned_source(OwnedSourceRef {
            owner: owner.clone(),
            path: path(&["Rows"]),
        })
        .with_grouping(GroupingPlan::AdjacentBy { key: 1 }),
    );
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&p),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Iteration { scope: route() },
                request: Some(OwnedSourceDiagnosticRequest::Iteration {
                    owner: (&owner).into(),
                    path: path(&["Rows"])
                }),
                reason: OwnedSourceCapabilityReason::DynamicOwner,
                origin: OwnedSourceCapabilityOrigin::DynamicSource {
                    index: 3,
                    name: "unavailableDynamic".into()
                },
            }),
        })
    );
}

#[test]
fn owned_group_adjacent_by_primary_row_failures_do_not_gain_target_item_at_permission() {
    let mut p = project(SourceOwner::Primary);
    add_item_at(&mut p, 0);
    p.root.children[0].bindings[1].node = 100;
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 2 },
        message: Some(4),
    });
    let base = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    assert!(is_basic_owned_primary_row_failure(
        &base,
        &base.failure_rules[0]
    ));
    use NamedInputAvailability::{Optional as O, ReservedUnavailable as U};
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&base).unwrap(),
        plan([O, O, O, U], [O, O, O, U], [O, O, O, U], [O, O, O, U])
    );
    for predicate in [false, true] {
        let mut p = base.clone();
        if predicate {
            p.failure_rules[0].selection = FailureSelection::WhenTrue(100);
        } else {
            p.failure_rules[0].message = Some(100);
        }
        assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
        let (node, parts, pin) = if predicate {
            (7, path(&["Limit"]), vec![])
        } else {
            (2, path(&["Rows", "Keep"]), path(&["Rows"]))
        };
        assert_eq!(
            validate_basic_owned_group_adjacent_by_emission(&p),
            Err(ProgramValidationError::OwnedSource {
                diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                    location: OwnedSourceDiagnosticLocation::Expression {
                        namespace: OwnedSourceNamespace::Main,
                        node,
                        scope: None
                    },
                    request: Some(OwnedSourceDiagnosticRequest::Field {
                        owner: OwnedSourceDiagnosticOwner::Primary,
                        path: parts,
                        frame: Some(pin),
                    }),
                    reason: OwnedSourceCapabilityReason::FailureRule,
                    origin: OwnedSourceCapabilityOrigin::Failure { index: 0 },
                }),
            })
        );
    }
}

#[test]
fn owned_group_adjacent_by_specialized_consumers_cannot_hide_under_eligible_item_at() {
    for (outer, reason) in [
        (
            Expression::Aggregate {
                function: AggregateFunction::Count,
                collection: path(&["Rows"]),
                value: AggregateValue::Path(vec![]),
                arg: None,
            },
            OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            Expression::Lookup {
                collection: path(&["Rows"]),
                key: path(&["Key"]),
                matches: 9,
                value: path(&["Label"]),
            },
            OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            Expression::CollectionFind {
                collection: path(&["Rows"]),
                predicate: 8,
                value: 9,
            },
            OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            Expression::SequenceExists {
                sequence: GeneratedSequence::Range {
                    from: None,
                    to: 9,
                    item: 91,
                },
                predicate: 8,
            },
            OwnedSourceCapabilityReason::GeneratedItem,
        ),
        (
            Expression::SequenceAggregate {
                function: AggregateFunction::Count,
                sequence: GeneratedSequence::Range {
                    from: None,
                    to: 9,
                    item: 91,
                },
                predicate: None,
                expression: None,
                arg: None,
            },
            OwnedSourceCapabilityReason::GeneratedItem,
        ),
    ] {
        let mut p = project(SourceOwner::Primary);
        add_item_at(&mut p, 0);
        p.root.children[0].bindings[1].node = 100;
        let base = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
        for hidden in [false, true] {
            let mut p = base.clone();
            p.expressions.push(ExpressionNode {
                id: 8,
                expression: Expression::Const {
                    value: Value::Bool(true),
                },
            });
            p.expressions.push(ExpressionNode {
                id: 91,
                expression: Expression::SourceField {
                    path: vec![],
                    frame: None,
                },
            });
            p.expressions.push(ExpressionNode {
                id: 101,
                expression: outer.clone(),
            });
            if hidden {
                let Expression::SequenceItemAt { index, .. } = expression(&mut p, 100) else {
                    panic!("ItemAt")
                };
                *index = 101;
            } else {
                p.root.children[0].bindings[1].expression = 101;
            }
            assert_eq!(
                validate_basic_owned_group_adjacent_by_emission(&p),
                Err(expression_refusal(101, reason))
            );
        }
    }
}

#[test]
fn owned_group_adjacent_by_generated_iteration_and_special_construction_stay_closed() {
    let mut p = project(SourceOwner::Primary);
    add_item_at(&mut p, 0);
    p.root.children[0].bindings[1].node = 100;
    let base = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    let mut p = base.clone();
    let member = &mut p.root.children[0].children[0];
    member.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 9,
        item: 91,
    }));
    p.expressions.push(ExpressionNode {
        id: 91,
        expression: Expression::SourceField {
            path: vec![],
            frame: None,
        },
    });
    let child_route = route().descendant(mapping::SourceScopeStep::Child(0));
    // The nearest actual owned ancestor supplies the request on an inherited
    // specialized iteration; no invented generated-owner identity is exposed.
    let mut expected = iteration_refusal(
        child_route,
        Some(SourceOwner::Primary),
        OwnedSourceCapabilityReason::GeneratedItem,
    );
    let ProgramValidationError::OwnedSource { diagnostic } = &mut expected else {
        unreachable!()
    };
    let OwnedSourceDiagnostic::Capability { origin, .. } = diagnostic.as_mut() else {
        unreachable!()
    };
    *origin = OwnedSourceCapabilityOrigin::Scope {
        scope: route().descendant(mapping::SourceScopeStep::Child(0)),
    };
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&p),
        Err(expected)
    );
    let mut p = base;
    p.root.children[0].children[0].construction = TargetConstruction::CopyCurrentSource;
    p.root.children[0].children[0].bindings.clear();
    let source_kind = p.source.child("Rows").unwrap().kind.clone();
    child_mut(child_mut(&mut p.target, "Groups"), "Members").kind = source_kind;
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&p),
        Err(iteration_refusal(
            route().descendant(mapping::SourceScopeStep::Child(0)),
            Some(SourceOwner::Primary),
            OwnedSourceCapabilityReason::SpecialConstruction,
        ))
    );
}

#[test]
fn owned_group_adjacent_by_recursive_and_filter_map_item_at_producers_stay_closed() {
    let mut p = project(SourceOwner::Primary);
    add_item_at(&mut p, 0);
    p.root.children[0].bindings[1].node = 100;
    let base = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    let mut recursive = base.clone();
    let SchemaKind::Group { children, .. } = &mut recursive.source.kind else {
        panic!("source group")
    };
    children.push(SchemaNode::group(
        "Tree",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Values",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::recursive_group("Children", "Tree").repeating(),
        ],
    ));
    for (id, value) in [(40, ""), (41, "/")] {
        recursive.expressions.push(ExpressionNode {
            id,
            expression: Expression::Const {
                value: Value::String(value.into()),
            },
        });
    }
    *expression(&mut recursive, 100) = Expression::SequenceItemAt {
        sequence: GeneratedSequence::RecursiveCollect {
            collection: path(&["Tree"]),
            children: path(&["Children"]),
            descent_value: path(&["Name"]),
            values: path(&["Values"]),
            value: path(&["Name"]),
            prefix: 40,
            separator: 41,
            item: 90,
        },
        index: 5,
    };
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&recursive),
        Err(expression_refusal(
            100,
            OwnedSourceCapabilityReason::GeneratedItem
        ))
    );
    let mut filter_map = base;
    filter_map.expressions.push(ExpressionNode {
        id: 91,
        expression: Expression::SourceField {
            path: vec![],
            frame: None,
        },
    });
    for (id, ty, output) in [
        (
            1,
            ScalarType::Bool,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        (
            2,
            ScalarType::Int,
            Expression::FunctionParameter {
                parameter: mapping::FunctionParameterId::new(1),
            },
        ),
    ] {
        filter_map.user_functions.push(UserFunctionProgram {
            id: mapping::FunctionId::new(id),
            library: "tests".into(),
            name: format!("stage{id}"),
            parameters: vec![
                UserFunctionParameter {
                    id: mapping::FunctionParameterId::new(1),
                    ty: ScalarType::Int,
                },
                UserFunctionParameter {
                    id: mapping::FunctionParameterId::new(2),
                    ty: ScalarType::Int,
                },
            ],
            output_type: ty,
            expressions: vec![ExpressionNode {
                id: 0,
                expression: output,
            }],
            output: 0,
        });
    }
    *expression(&mut filter_map, 100) = Expression::SequenceItemAt {
        sequence: GeneratedSequence::FilterMapV1(GeneratedFilterMapV1 {
            source: GeneratedRange {
                from: None,
                to: 9,
                item: 90,
            },
            item: 91,
            predicate: mapping::FunctionId::new(1),
            mapper: mapping::FunctionId::new(2),
            output_type: ScalarType::Int,
            captures: vec![],
        }),
        index: 5,
    };
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&filter_map),
        Err(expression_refusal(
            100,
            OwnedSourceCapabilityReason::GeneratedItem
        ))
    );
}

#[test]
fn owned_group_adjacent_by_closed_named_target_keeps_exact_index_name_and_scope() {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::Int(1),
        },
    );
    p.root.children[0].windows = vec![mapping::SequenceWindow::First { count: 9 }];
    let mut program = lower_with_basic_owned_group_adjacent_by(&p).unwrap();
    program.extra_targets[0].target = program.target.clone();
    program.extra_targets[0].root = program.root.clone();
    let iteration = program.extra_targets[0].root.children[0]
        .iteration
        .take()
        .unwrap();
    program.extra_targets[0].root.children[0].iteration =
        Some(iteration.with_grouping(GroupingPlan::IntoBlocks { size: 9 }));
    let scope = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Named {
        index: 0,
        name: " exact audit💠 ".into(),
    })
    .descendant(mapping::SourceScopeStep::Child(0));
    assert_eq!(
        validate_basic_owned_group_adjacent_by_emission(&program),
        Err(iteration_refusal(
            scope,
            Some(SourceOwner::Primary),
            OwnedSourceCapabilityReason::Grouping
        ))
    );
}

#[test]
fn owned_group_adjacent_by_preserves_by_item_at_and_ordinary_adjacent_lowering_and_wire() {
    let mut p = project(SourceOwner::Primary);
    let group = &mut p.root.children[0];
    group.group_by = group.group_adjacent_by.take();
    let by = lower_with_basic_owned_group_by(&p).unwrap();
    assert_eq!(lower_with_basic_owned_group_adjacent_by(&p), Ok(by.clone()));
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&by).unwrap(),
        legacy_named_input_admission_plan_with_basic_owned_group_by(&by).unwrap()
    );
    add_item_at(&mut p, 0);
    p.root.children[0].bindings[1].node = 100;
    let item_at = lower_with_basic_owned_sequence_item_at(&p).unwrap();
    assert_eq!(
        lower_with_basic_owned_group_adjacent_by(&p),
        Ok(item_at.clone())
    );
    assert_eq!(
        validate_basic_owned_group_by_emission(&item_at),
        Err(expression_refusal(
            100,
            OwnedSourceCapabilityReason::GeneratedItem
        ))
    );
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&item_at).unwrap(),
        legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&item_at).unwrap()
    );
    let mut p = project(SourceOwner::Primary);
    p.extra_targets.clear();
    for (id, node) in [
        (1, legacy(&["Key"])),
        (2, legacy(&["Keep"])),
        (4, legacy(&["Label"])),
        (5, Node::Position { collection: vec![] }),
        (
            6,
            Node::Const {
                value: Value::String("ordinary".into()),
            },
        ),
    ] {
        p.graph.nodes.insert(id, node);
    }
    p.graph
        .nodes
        .retain(|id, _| [1, 2, 4, 5, 6, 30].contains(id));
    p.root.children[0].iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
    p.root.children[0].children[0].iteration = mapping::ScopeIteration::Source(vec![]);
    let wire = serde_json::to_string_pretty(&p).unwrap() + "\n";
    let restored: mapping::Project = serde_json::from_str(&wire).unwrap();
    assert_eq!(
        serde_json::to_string_pretty(&restored).unwrap() + "\n",
        wire
    );
    assert!(!wire.contains("owned_source"));
    let ordinary = lower(&restored).unwrap();
    assert_eq!(
        lower_with_basic_owned_group_adjacent_by(&restored),
        Ok(ordinary.clone())
    );
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&ordinary).unwrap(),
        legacy_named_input_admission_plan(&ordinary).unwrap()
    );
}
