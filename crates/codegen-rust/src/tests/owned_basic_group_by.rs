//! Genuine-Project fixtures and literal oracles for the explicit By companion.
//! The shared three-engine corpus is independently owned by #819.

use super::*;
use mapping::{OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn owned(owner: &SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> mapping::Node {
    mapping::Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: owner.clone(),
            path: path(parts),
        },
        frame: frame.map(path),
    })
}

fn binding(name: &str, node: u32) -> mapping::Binding {
    mapping::Binding {
        target_field: name.into(),
        node,
    }
}

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Source",
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

fn target_schema() -> SchemaNode {
    SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![
                    SchemaNode::scalar("First", ScalarType::String),
                    SchemaNode::scalar("GroupPosition", ScalarType::Int),
                    SchemaNode::scalar("RootMarker", ScalarType::String),
                    SchemaNode::group(
                        "Members",
                        vec![
                            SchemaNode::scalar("Label", ScalarType::String),
                            SchemaNode::scalar("MemberPosition", ScalarType::Int),
                            SchemaNode::scalar("RootMarker", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}

fn project(owner: SourceOwner) -> mapping::Project {
    let iteration = || mapping::ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner.clone(),
            path: path(&["Rows"]),
        },
        output_path: None,
    };
    let root = mapping::Scope {
        children: vec![mapping::Scope {
            target_field: "Groups".into(),
            iteration: iteration(),
            group_by: Some(1),
            bindings: vec![
                binding("First", 2),
                binding("GroupPosition", 3),
                binding("RootMarker", 4),
            ],
            children: vec![mapping::Scope {
                target_field: "Members".into(),
                iteration: iteration(),
                bindings: vec![
                    binding("Label", 2),
                    binding("MemberPosition", 3),
                    binding("RootMarker", 4),
                ],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    mapping::Project {
        source: source_schema(),
        target: target_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Rows", " Røws💠 "]
            .into_iter()
            .map(|name| mapping::NamedSource {
                name: name.into(),
                path: "unused.json".into(),
                schema: source_schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![mapping::NamedTarget {
            name: " Exact audit 雪 ".into(),
            path: None,
            schema: target_schema(),
            options: Default::default(),
            root: root.clone(),
        }],
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (1, owned(&owner, &["Rows", "Key"], None)),
                (2, owned(&owner, &["Rows", "Label"], Some(&["Rows"]))),
                (
                    3,
                    mapping::Node::OwnedPosition(OwnedPosition {
                        owner: owner.clone(),
                        collection: Some(path(&["Rows"])),
                    }),
                ),
                (4, owned(&owner, &["Marker"], Some(&[]))),
                (5, owned(&owner, &["Rows", "Keep"], None)),
                (6, owned(&owner, &["Rows", "Rank"], None)),
                (7, owned(&SourceOwner::Primary, &["Limit"], None)),
                (
                    8,
                    mapping::Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    9,
                    mapping::Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    10,
                    mapping::Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (
                    11,
                    mapping::Node::Call {
                        function: "greater_than".into(),
                        args: vec![3, 10],
                    },
                ),
                (
                    12,
                    mapping::Node::Call {
                        function: "greater_than".into(),
                        args: vec![3, 8],
                    },
                ),
                (
                    99,
                    mapping::Node::Const {
                        value: Value::String("unused expression_ literal 雪".into()),
                    },
                ),
            ]),
        },
        root,
        user_functions: Default::default(),
        failure_rules: Vec::new(),
    }
}

fn program(project: &mapping::Project) -> Program {
    // Lower the actual saved representation, not a hand-written Program proxy.
    let saved = serde_json::to_vec(project).unwrap();
    let restored: mapping::Project = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), saved);
    codegen::lower_with_basic_owned_group_by(&restored).expect("explicit By project lowers")
}

fn options() -> Options {
    Options {
        package_name: "owned_group_by_host".into(),
        runtime_dependency: RuntimeDependency::Path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
        ),
    }
}

fn generated_source(artifacts: &ArtifactSet) -> &str {
    std::str::from_utf8(
        &artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap()
            .contents,
    )
    .unwrap()
}

fn artifacts(project: &mapping::Project) -> ArtifactSet {
    emit_with_basic_owned_group_by(&program(project), &options()).unwrap()
}

#[test]
fn owned_by_entry_is_typed_explicit_and_keeps_every_older_gate_closed() {
    let p = program(&project(SourceOwner::Primary));
    for old in [
        emit,
        emit_with_basic_owned_sources,
        emit_with_basic_owned_filters,
        emit_with_basic_owned_sort,
        emit_with_basic_owned_windows,
        emit_with_basic_owned_row_failures,
    ] {
        assert!(matches!(
            old(&p, &options()),
            Err(EmitError::InvalidProgram(
                ProgramValidationError::OwnedSource { .. }
            ))
        ));
    }
    let files = emit_with_basic_owned_group_by(&p, &options()).unwrap();
    let source = generated_source(&files);
    assert_eq!(files.len(), 2);
    assert!(source.contains("pub fn execute_with_basic_owned_sources<'name, 'src>("));
    assert!(source.contains("pub fn execute_selected_with_basic_owned_sources<'name, 'src>("));
    assert!(
        source.contains("execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,")
    );
    assert!(!source.contains("pub fn execute_json"));
    assert!(!source.contains("NamedJsonInput"));
    assert!(!source.contains("OwnedSourceLimits::default"));
    assert!(!source.contains("fn expression_99("));

    let mut invalid_options = options();
    invalid_options.package_name = "0invalid".into();
    assert!(matches!(
        emit_with_basic_owned_group_by(&p, &invalid_options),
        Err(EmitError::InvalidPackageName(_))
    ));
    let mut malformed = p.clone();
    malformed.expressions.push(malformed.expressions[0].clone());
    assert!(matches!(
        emit_with_basic_owned_group_by(&malformed, &invalid_options),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::DuplicateExpression { node: 1 }
        ))
    ));
}

fn legacy_control_project(owned_control: bool, post_only: bool) -> mapping::Project {
    let mut p = project(SourceOwner::Primary);
    p.extra_targets.clear();
    let groups = &mut p.root.children[0];
    groups.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
    groups.children[0].iteration = mapping::ScopeIteration::Source(Vec::new());
    if post_only {
        groups.post_group_filter = Some(5);
    }
    for (id, name) in [(1, "Key"), (2, "Label"), (5, "Keep"), (6, "Rank")] {
        if owned_control && id == if post_only { 5 } else { 1 } {
            continue;
        }
        p.graph.nodes.insert(
            id,
            mapping::Node::SourceField {
                path: path(&[name]),
                frame: None,
            },
        );
    }
    p.graph.nodes.insert(
        3,
        mapping::Node::Position {
            collection: path(&["Rows"]),
        },
    );
    p.graph.nodes.insert(
        4,
        mapping::Node::Const {
            value: Value::String("P.root".into()),
        },
    );
    p
}

#[test]
fn owned_key_or_post_alone_demands_paired_grouped_ancestry_but_not_legacy_siblings() {
    for post_only in [false, true] {
        let files = artifacts(&legacy_control_project(true, post_only));
        let source = generated_source(&files);
        assert!(source.contains("fn scope_root_0(context: &BasicScopeContext<'_>)"));
        assert!(source.contains("context.walk_source_cohort(&[\"Rows\"])?"));
        assert!(source.contains("fn scope_root_0_0(context: &BasicScopeContext<'_>)"));
        assert!(!source.contains("fn legacy_scope_root_0_0("));
        assert!(source.contains(&format!(
            "fn expression_{}(context: &BasicScopeContext<'_>)",
            if post_only { 5 } else { 1 }
        )));
        assert!(!source.contains("GroupedItems::by("));
    }
    let mut mixed = legacy_control_project(true, false);
    mixed.root.children.push(mapping::Scope {
        target_field: "Legacy".into(),
        iteration: mapping::ScopeIteration::Source(path(&["Rows"])),
        group_by: Some(99),
        bindings: vec![binding("Value", 99)],
        ..Default::default()
    });
    let SchemaKind::Group { children, .. } = &mut mixed.target.kind else {
        unreachable!()
    };
    children.push(
        SchemaNode::group(
            "Legacy",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
        .repeating(),
    );
    let files = artifacts(&mixed);
    let source = generated_source(&files);
    assert!(source.contains("fn legacy_scope_root_1(context: &ScopeContext<'_>)"));
    assert!(source.contains("GroupedItems::by("));
    assert!(source.contains(&rust_string("unused expression_ literal 雪")));
    assert!(!source.contains("unused legacy_expression_ literal"));
    let files = artifacts(&legacy_control_project(false, false));
    let source = generated_source(&files);
    assert!(source.contains("fn legacy_scope_root_0(context: &ScopeContext<'_>)"));
    assert!(source.contains("GroupedItems::by(grouping_candidates, Some(\"Rows\"))"));
    assert!(source.contains("legacy_expression_1(&candidate)?"));
}

#[test]
fn legacy_grouping_islands_keep_all_modes_without_rewriting_user_literals() {
    for (mode, expected) in [
        (0, "by"),
        (1, "adjacent_by"),
        (2, "starting_with"),
        (3, "ending_with"),
        (4, "into_blocks"),
    ] {
        let mut p = legacy_control_project(false, false);
        let groups = &mut p.root.children[0];
        groups.group_by = None;
        match mode {
            0 => groups.group_by = Some(1),
            1 => groups.group_adjacent_by = Some(1),
            2 => groups.group_starting_with = Some(5),
            3 => groups.group_ending_with = Some(5),
            4 => groups.group_into_blocks = Some(8),
            _ => unreachable!(),
        }
        let files = artifacts(&p);
        let source = generated_source(&files);
        assert!(source.contains(&format!("GroupedItems::{expected}(")));
        assert!(source.contains("legacy_expression_"));
        assert!(!source.contains("map_group_by_source_cohort"));
    }
}

#[test]
fn owned_by_profile_refuses_other_grouping_modes_without_mutating_the_program() {
    for mode in 1..5 {
        let mut p = project(SourceOwner::Primary);
        let groups = &mut p.root.children[0];
        groups.group_by = None;
        match mode {
            1 => groups.group_adjacent_by = Some(1),
            2 => groups.group_starting_with = Some(5),
            3 => groups.group_ending_with = Some(5),
            4 => groups.group_into_blocks = Some(8),
            _ => unreachable!(),
        }
        let saved = serde_json::to_vec(&p).unwrap();
        assert!(codegen::lower_with_basic_owned_group_by(&p).is_err());
        assert_eq!(serde_json::to_vec(&p).unwrap(), saved);
    }
}

#[test]
fn paired_by_controls_keep_candidate_parent_member_and_output_roles_distinct() {
    // Scope's saved-project default is SortThenFilter. Specify both policies
    // instead of assuming that setting sort_by also selects FilterThenSort.
    for (sort_filter_order, filter_before_sort) in [
        (mapping::SortFilterOrder::FilterThenSort, true),
        (mapping::SortFilterOrder::SortThenFilter, false),
    ] {
        let mut p = project(SourceOwner::Primary);
        p.extra_targets.clear();
        configure_groups(&mut p, |groups| {
            groups.group_by = Some(7);
            groups.filter = Some(12);
            groups.sort_by = Some(6);
            groups.sort_filter_order = sort_filter_order;
            groups.post_group_filter = Some(5);
            groups.windows = vec![
                mapping::SequenceWindow::FromTo { first: 8, last: 7 },
                mapping::SequenceWindow::Last { count: 7 },
            ];
            groups.iteration_output = mapping::IterationOutput::First;
        });
        let SchemaKind::Group { children, .. } = &mut p.target.kind else {
            unreachable!()
        };
        children[0].repeating = false;
        let files = artifacts(&p);
        let source = generated_source(&files);
        let start = source.find("fn scope_root_0(context:").unwrap();
        let body = source[start..].split("\n}\n").next().unwrap();
        assert!(
            body.contains("impl codegen_runtime::BasicGroupByControls<RuntimeError> for Controls")
        );
        let metadata = body
            .split_once("codegen_runtime::BasicGroupByMetadata {")
            .unwrap()
            .1
            .split_once('}')
            .unwrap()
            .0;
        let fields = metadata
            .split(',')
            .map(|field| field.split_whitespace().collect::<String>())
            .collect::<Vec<_>>();
        for field in [
            format!("filter_before_sort:{filter_before_sort}"),
            "explicit_window_count:2".into(),
            "implicit_first:true".into(),
            "has_filter:true".into(),
            "has_post_filter:true".into(),
        ] {
            assert!(
                fields.contains(&field),
                "missing {field} in metadata {metadata}; scope:\n{body}"
            );
        }
        assert_eq!(
            metadata
                .matches("codegen_runtime::SortDirection::Ascending")
                .count(),
            1,
            "{metadata}"
        );
        assert!(body.contains("expression_12(predicate_context)?"));
        assert!(body.contains("require_bool(12, value)"));
        assert!(body.contains("0 => expression_6(key_context)"));
        assert!(body.contains("expression_8(parent_context)?"));
        assert!(body.contains("expression_7(parent_context)?"));
        assert!(
            body.find("expression_8(parent_context)?").unwrap()
                < body.find("expression_7(parent_context)?").unwrap()
        );
        assert!(body.contains("expression_7(group_key_context)"));
        assert!(body.contains("expression_5(member_context)?"));
        assert!(body.contains("require_bool(5, value)"));
        assert!(body.contains(
            "fn target(item_context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError>"
        ));
        assert!(body.contains("expression_2(item_context)?"));
        assert!(
            body.contains(
                "context.map_group_by_source_cohort(&cohort, &mut Controls, &mut target)?"
            )
        );
        assert!(!body.contains("Vec::with_capacity"));
        assert!(!body.contains("outputs.push"));
        assert!(!body.contains("visit_windowed_source_cohort"));
        assert!(!body.contains("GroupedItems"));
        assert!(body.contains("outputs.into_iter().next().unwrap_or_else(|| group(Vec::new()))"));
    }
}

#[test]
fn by_profile_composes_primary_row_failures_before_checked_target_dispatch() {
    let (_, p) = consumer_projects()
        .into_iter()
        .find(|(name, _)| *name == "failure")
        .unwrap();
    let files = artifacts(&p);
    let source = generated_source(&files);
    assert!(source.contains("context.visit_primary_failure_rows(&[\"Rows\"]"));
    let start = source
        .find("fn execute_private_checked_basic_context(")
        .unwrap();
    let body = &source[start..];
    assert!(
        body.find("evaluate_basic_failure_rules(context)?").unwrap()
            < body.find("if let Some(selected)").unwrap()
    );
    assert_eq!(
        body.matches("evaluate_basic_failure_rules(context)?")
            .count(),
        1
    );
}

#[test]
fn by_companion_preserves_row_failure_profile_artifacts_when_no_grouping_is_requested() {
    let mut p = project(SourceOwner::Primary);
    configure_groups(&mut p, |groups| groups.group_by = None);
    let p = program(&p);
    assert_eq!(
        emit_with_basic_owned_group_by(&p, &options()).unwrap(),
        emit_with_basic_owned_row_failures(&p, &options()).unwrap()
    );
}

fn configure_groups(project: &mut mapping::Project, configure: impl Fn(&mut mapping::Scope)) {
    configure(&mut project.root.children[0]);
    for target in &mut project.extra_targets {
        configure(&mut target.root.children[0]);
    }
}

fn singleton_project() -> mapping::Project {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Marker", ScalarType::String),
        ],
    );
    configure_groups(&mut p, |groups| {
        let iteration = || mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: Vec::new(),
            },
            output_path: None,
        };
        groups.iteration = iteration();
        for scope in &mut groups.children {
            scope.iteration = iteration();
        }
    });
    p.graph
        .nodes
        .insert(1, owned(&SourceOwner::Primary, &["Key"], None));
    p.graph
        .nodes
        .insert(2, owned(&SourceOwner::Primary, &["Label"], Some(&[])));
    p.graph.nodes.insert(
        3,
        mapping::Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(Vec::new()),
        }),
    );
    p.graph.nodes.retain(|id, _| *id <= 4);
    p
}

fn scalar_target_project() -> mapping::Project {
    let mut p = project(SourceOwner::Primary);
    p.extra_targets.clear();
    p.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Groups", ScalarType::String).repeating()],
    );
    let scope = &mut p.root.children[0];
    scope.bindings.clear();
    scope.children.clear();
    scope.construction = mapping::ScopeConstruction::Scalar { value: 2 };
    p
}

fn first_group_project() -> mapping::Project {
    let mut p = project(SourceOwner::Primary);
    configure_groups(&mut p, |groups| {
        groups.iteration_output = mapping::IterationOutput::First
    });
    let SchemaKind::Group { children, .. } = &mut p.target.kind else {
        unreachable!()
    };
    children[0].repeating = false;
    for target in &mut p.extra_targets {
        let SchemaKind::Group { children, .. } = &mut target.schema.kind else {
            unreachable!()
        };
        children[0].repeating = false;
    }
    p
}

fn consumer_projects() -> Vec<(&'static str, mapping::Project)> {
    let primary = project(SourceOwner::Primary);
    let mut post = primary.clone();
    configure_groups(&mut post, |g| g.post_group_filter = Some(5));
    let mut sorted = primary.clone();
    configure_groups(&mut sorted, |g| {
        g.sort_by = Some(6);
        g.post_group_filter = Some(11);
    });
    let mut last = sorted.clone();
    configure_groups(&mut last, |g| {
        g.windows = vec![mapping::SequenceWindow::Last { count: 7 }]
    });
    let mut filter_before = post.clone();
    configure_groups(&mut filter_before, |g| {
        g.sort_by = Some(6);
        g.filter = Some(12);
        g.sort_filter_order = mapping::SortFilterOrder::FilterThenSort;
    });
    let mut filter_after = filter_before.clone();
    configure_groups(&mut filter_after, |g| {
        g.sort_filter_order = mapping::SortFilterOrder::SortThenFilter
    });
    let mut regroup = primary.clone();
    configure_groups(&mut regroup, |g| g.children[0].group_by = Some(3));
    let mut key_error = post.clone();
    key_error
        .graph
        .nodes
        .insert(1, mapping::Node::Raise { message: None });
    key_error
        .graph
        .nodes
        .insert(5, mapping::Node::Raise { message: None });
    let mut post_error = post.clone();
    post_error
        .graph
        .nodes
        .insert(5, mapping::Node::Raise { message: None });
    let mut first_zero = key_error.clone();
    configure_groups(&mut first_zero, |g| {
        g.windows = vec![mapping::SequenceWindow::First { count: 9 }]
    });
    let mut sort_error = key_error.clone();
    configure_groups(&mut sort_error, |g| {
        g.sort_by = Some(6);
        g.windows = vec![mapping::SequenceWindow::First { count: 7 }];
    });
    sort_error
        .graph
        .nodes
        .insert(6, mapping::Node::Raise { message: None });
    sort_error
        .graph
        .nodes
        .insert(7, mapping::Node::Raise { message: None });
    let mut bound_error = key_error.clone();
    configure_groups(&mut bound_error, |g| {
        g.windows = vec![mapping::SequenceWindow::First { count: 7 }]
    });
    bound_error
        .graph
        .nodes
        .insert(7, mapping::Node::Raise { message: None });
    let mut nonbool = post.clone();
    nonbool.graph.nodes.insert(
        5,
        mapping::Node::Const {
            value: Value::String("not a bool".into()),
        },
    );
    let mut invalid_bound = primary.clone();
    configure_groups(&mut invalid_bound, |g| {
        g.windows = vec![mapping::SequenceWindow::First { count: 7 }]
    });
    invalid_bound.graph.nodes.insert(
        7,
        mapping::Node::Const {
            value: Value::String("not a count".into()),
        },
    );
    let mut dropped_target = post.clone();
    dropped_target.graph.nodes.insert(
        5,
        mapping::Node::Const {
            value: Value::Bool(false),
        },
    );
    dropped_target
        .graph
        .nodes
        .insert(2, mapping::Node::Raise { message: None });
    let mut failure = key_error.clone();
    failure
        .graph
        .nodes
        .insert(5, owned(&SourceOwner::Primary, &["Rows", "Keep"], None));
    failure.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 5 },
        message: Some(2),
    });
    let mut selection = primary.clone();
    selection
        .graph
        .nodes
        .insert(71, mapping::Node::Raise { message: None });
    selection.extra_targets.push(mapping::NamedTarget {
        name: "Bad".into(),
        path: None,
        options: Default::default(),
        schema: SchemaNode::group("Bad", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        root: mapping::Scope {
            bindings: vec![binding("Value", 71)],
            ..Default::default()
        },
    });
    let mut post_before_filter = post_error.clone();
    configure_groups(&mut post_before_filter, |g| g.filter = Some(40));
    post_before_filter.graph.nodes.extend([
        (
            40,
            mapping::Node::If {
                condition: 41,
                then: 43,
                else_: 42,
            },
        ),
        (
            41,
            mapping::Node::Call {
                function: "equal".into(),
                args: vec![3, 8],
            },
        ),
        (42, mapping::Node::Raise { message: None }),
        (
            43,
            mapping::Node::Const {
                value: Value::Bool(true),
            },
        ),
    ]);
    let mut prefilter_error = post_before_filter.clone();
    configure_groups(&mut prefilter_error, |g| {
        g.sort_by = Some(6);
        g.sort_filter_order = mapping::SortFilterOrder::FilterThenSort;
    });
    let mut later_post_error = post.clone();
    later_post_error.graph.nodes.extend([
        (
            5,
            mapping::Node::If {
                condition: 44,
                then: 45,
                else_: 43,
            },
        ),
        (
            43,
            mapping::Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            44,
            mapping::Node::Call {
                function: "equal".into(),
                args: vec![3, 10],
            },
        ),
        (45, mapping::Node::Raise { message: None }),
    ]);
    let mut window_dropped_target = dropped_target.clone();
    configure_groups(&mut window_dropped_target, |g| {
        g.windows = vec![mapping::SequenceWindow::First { count: 9 }]
    });
    window_dropped_target.graph.nodes.insert(
        5,
        mapping::Node::Const {
            value: Value::Bool(true),
        },
    );
    let mut legacy_adjacent = legacy_control_project(false, false);
    configure_groups(&mut legacy_adjacent, |g| {
        g.group_by = None;
        g.group_adjacent_by = Some(1);
    });
    let mut legacy_starting = legacy_control_project(false, false);
    configure_groups(&mut legacy_starting, |g| {
        g.group_by = None;
        g.group_starting_with = Some(5);
    });
    let mut legacy_ending = legacy_control_project(false, false);
    configure_groups(&mut legacy_ending, |g| {
        g.group_by = None;
        g.group_ending_with = Some(5);
    });
    let mut legacy_blocks = legacy_control_project(false, false);
    configure_groups(&mut legacy_blocks, |g| {
        g.group_by = None;
        g.group_into_blocks = Some(8);
    });
    vec![
        ("primary", primary),
        (
            "named",
            project(SourceOwner::Named {
                name: "Rows".into(),
            }),
        ),
        (
            "exact",
            project(SourceOwner::Named {
                name: " Røws💠 ".into(),
            }),
        ),
        ("post", post),
        ("sorted", sorted),
        ("last", last),
        ("filter_before", filter_before),
        ("filter_after", filter_after),
        ("regroup", regroup),
        ("legacy_key", legacy_control_project(true, false)),
        ("legacy_post", legacy_control_project(true, true)),
        ("singleton", singleton_project()),
        ("scalar_target", scalar_target_project()),
        ("first_group", first_group_project()),
        ("key_error", key_error),
        ("post_error", post_error),
        ("first_zero", first_zero),
        ("sort_error", sort_error),
        ("bound_error", bound_error),
        ("nonbool", nonbool),
        ("invalid_bound", invalid_bound),
        ("dropped_target", dropped_target),
        ("failure", failure),
        ("selection", selection),
        ("post_before_filter", post_before_filter),
        ("prefilter_error", prefilter_error),
        ("later_post_error", later_post_error),
        ("window_dropped_target", window_dropped_target),
        ("legacy_adjacent", legacy_adjacent),
        ("legacy_starting", legacy_starting),
        ("legacy_ending", legacy_ending),
        ("legacy_blocks", legacy_blocks),
    ]
}

#[test]
fn owned_by_genuine_project_consumers_preserve_complete_trees_errors_and_api_policy() {
    let directory = TempDir::new("owned_by_consumer");
    let mut library = String::new();
    for (name, project) in consumer_projects() {
        let saved = serde_json::to_vec_pretty(&project).unwrap();
        fs::write(directory.path().join(format!("{name}.Project.json")), saved).unwrap();
        let files = artifacts(&project);
        library.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            generated_source(&files)
        ));
    }
    fs::create_dir_all(directory.path().join("src")).unwrap();
    fs::create_dir_all(directory.path().join("tests")).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        render_manifest(&options()),
    )
    .unwrap();
    fs::write(directory.path().join("src/lib.rs"), library).unwrap();
    fs::write(directory.path().join("tests/consumer.rs"), CONSUMER).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--offline", "--test", "consumer"])
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .unwrap();
    fs::write(directory.path().join("stdout.complete"), &output.stdout).unwrap();
    fs::write(directory.path().join("stderr.complete"), &output.stderr).unwrap();
    if !output.status.success() {
        let retained = directory.path().to_owned();
        std::mem::forget(directory);
        panic!(
            "complete owned By consumer failure retained at {}:\n{}\n{}",
            retained.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

const CONSUMER: &str = r#"
use owned_group_by_host::*;
use codegen_runtime::{BasicNamedSourceInput,Instance,OwnedSourceLimits,RuntimeError,TargetSelection,
    Value,field,group,repeated,scalar};

fn text(value:&str)->Instance {scalar(Value::String(value.into()))}
fn fields(values:impl IntoIterator<Item=(&'static str,Instance)>)->Instance {group(values.into_iter().map(|(name,value)|field(name,value)))}
fn row(key:&str,label:&str,keep:bool,rank:i64)->Instance {fields([
    ("Key",text(key)),("Label",text(label)),("Keep",scalar(Value::Bool(keep))),("Rank",scalar(Value::Int(rank))),
])}
fn input(marker:&str,prefix:&str)->Instance {fields([
    ("Marker",text(marker)),("Limit",scalar(Value::Int(1))),
    ("Rows",repeated([
        row("A",&format!("{prefix}-a1"),false,30),row("B",&format!("{prefix}-b1"),true,10),
        row("A",&format!("{prefix}-a2"),true,20),row("C",&format!("{prefix}-c1"),false,40),
        row("B",&format!("{prefix}-b2"),false,15),
    ])),
])}
fn empty_input()->Instance {fields([("Marker",text("P.root")),("Limit",scalar(Value::Int(1))),("Rows",repeated([]))])}
fn expected(marker:&str,groups:&[(&str,&[&str])])->Instance {fields([("Groups",repeated(groups.iter().enumerate().map(|(index,(first,members))|fields([
    ("First",text(first)),("GroupPosition",scalar(Value::Int((index+1) as i64))),
    ("RootMarker",text(marker)),("Members",repeated(members.iter().enumerate().map(|(index,label)|fields([
        ("Label",text(label)),("MemberPosition",scalar(Value::Int((index+1) as i64))),("RootMarker",text(marker)),
    ])))),
]))))])}
fn limits()->OwnedSourceLimits {OwnedSourceLimits {max_roots:8,max_active_frames:16,max_path_segments:64,max_metadata_bytes:1_048_576,max_walk_contexts:128}}
macro_rules! check {
    ($module:ident,$source:expr,$inputs:expr,$expected:expr)=>{{
        let source=$source;let inputs=$inputs;let expected=$expected;
        assert_eq!($module::execute_with_basic_owned_sources(source,inputs,None,limits()),Ok($module::ExecutionOutputs {
            primary:expected.clone(),extras:vec![$module::NamedOutput {name:" Exact audit 雪 ".into(),instance:expected.clone()}],
        }));
        assert_eq!($module::execute_selected_with_basic_owned_sources(source,inputs,None,limits(),TargetSelection::Primary),Ok($module::SelectedTargetOutput::Primary(expected.clone())));
        assert_eq!($module::execute_selected_with_basic_owned_sources(source,inputs,None,limits(),TargetSelection::Named(" Exact audit 雪 ")),Ok($module::SelectedTargetOutput::Named($module::NamedOutput {name:" Exact audit 雪 ".into(),instance:expected})));
    }};
}
macro_rules! fails {
    ($module:ident,$source:expr,$error:expr)=>{{
        let expected=$error;
        assert_eq!($module::execute_with_basic_owned_sources($source,&[],None,limits()).unwrap_err(),expected);
    }};
}

#[test] fn original_order_named_owner_collisions_and_first_member_reads() {
    let p=input("P.root","P");let n=input("N.root","N");let u=input("U.root","U");
    let supplied=[BasicNamedSourceInput {name:"Rows",instance:&n},BasicNamedSourceInput {name:" Røws💠 ",instance:&u}];
    let p_expected=expected("P.root",&[("P-a1",&["P-a1","P-a2"]),("P-b1",&["P-b1","P-b2"]),("P-c1",&["P-c1"])]);
    check!(primary,&p,&supplied,p_expected.clone());check!(regroup,&p,&supplied,p_expected);
    check!(named,&p,&supplied,expected("N.root",&[("N-a1",&["N-a1","N-a2"]),("N-b1",&["N-b1","N-b2"]),("N-c1",&["N-c1"])]));
    check!(exact,&p,&supplied,expected("U.root",&[("U-a1",&["U-a1","U-a2"]),("U-b1",&["U-b1","U-b2"]),("U-c1",&["U-c1"])]));
}

#[test] fn retained_groups_keep_false_members_and_windows_compact_group_not_member_positions() {
    let p=input("P.root","P");
    check!(post,&p,&[],expected("P.root",&[("P-a1",&["P-a1","P-a2"]),("P-b1",&["P-b1","P-b2"])]));
    check!(sorted,&p,&[],expected("P.root",&[("P-a2",&["P-a2","P-a1"]),("P-c1",&["P-c1"])]));
    check!(last,&p,&[],expected("P.root",&[("P-c1",&["P-c1"])]));
    check!(filter_before,&p,&[],expected("P.root",&[("P-b1",&["P-b1","P-b2"]),("P-a2",&["P-a2"])]));
    check!(filter_after,&p,&[],expected("P.root",&[("P-a2",&["P-a2","P-a1"])]));
    check!(post,&empty_input(),&[],expected("P.root",&[]));
    check!(dropped_target,&p,&[],expected("P.root",&[]));
    check!(window_dropped_target,&p,&[],expected("P.root",&[]));
}

#[test] fn singleton_root_has_one_group_one_member_and_immutable_root_pin() {
    let p=fields([("Key",text("A")),("Label",text("only")),("Marker",text("P.root"))]);
    check!(singleton,&p,&[],expected("P.root",&[("only",&["only"])]));
}

#[test] fn implicit_group_first_keeps_complete_members_and_empty_group_fallback() {
    let p=input("P.root","P");
    let wanted=fields([("Groups",fields([
        ("First",text("P-a1")),("GroupPosition",scalar(Value::Int(1))),("RootMarker",text("P.root")),
        ("Members",repeated([
            fields([("Label",text("P-a1")),("MemberPosition",scalar(Value::Int(1))),("RootMarker",text("P.root"))]),
            fields([("Label",text("P-a2")),("MemberPosition",scalar(Value::Int(2))),("RootMarker",text("P.root"))]),
        ])),
    ]))]);
    check!(first_group,&p,&[],wanted);
    check!(first_group,&empty_input(),&[],fields([("Groups",fields([]))]));
}

#[test] fn scalar_targets_and_key_post_only_owned_demand_do_not_disappear() {
    let p=input("P.root","P");
    let scalar_expected=fields([("Groups",repeated([text("P-a1"),text("P-b1"),text("P-c1")]))]);
    assert_eq!(scalar_target::execute_with_basic_owned_sources(&p,&[],None,limits()),Ok(scalar_target::ExecutionOutputs {primary:scalar_expected,extras:vec![]}));
    let n=input("N.root","N");
    let required=[BasicNamedSourceInput {name:"Rows",instance:&n}];
    // Admission requires the alias; the unchanged P-* oracle also proves primary-source priority.
    let all=expected("P.root",&[("P-a1",&["P-a1","P-a2"]),("P-b1",&["P-b1","P-b2"]),("P-c1",&["P-c1"])]);
    assert_eq!(legacy_key::execute_with_basic_owned_sources(&p,&required,None,limits()),Ok(legacy_key::ExecutionOutputs {primary:all,extras:vec![]}));
    let retained=expected("P.root",&[("P-a1",&["P-a1","P-a2"]),("P-b1",&["P-b1","P-b2"])]);
    assert_eq!(legacy_post::execute_with_basic_owned_sources(&p,&required,None,limits()),Ok(legacy_post::ExecutionOutputs {primary:retained,extras:vec![]}));
}

#[test] fn reached_error_phase_order_is_not_reordered_by_empty_output_or_retention() {
    let p=input("P.root","P");
    fails!(key_error,&p,RuntimeError::MappingException {node:1,message:None});
    fails!(post_error,&p,RuntimeError::MappingException {node:5,message:None});
    fails!(first_zero,&p,RuntimeError::MappingException {node:1,message:None});
    fails!(sort_error,&p,RuntimeError::MappingException {node:6,message:None});
    fails!(bound_error,&p,RuntimeError::MappingException {node:7,message:None});
    fails!(nonbool,&p,RuntimeError::NotABool {node:5,found:"string"});
    fails!(invalid_bound,&empty_input(),RuntimeError::NotAnItemCount {node:7,found:"string"});
    fails!(failure,&p,RuntimeError::MappingFailure {rule:1,message:Some("P-b1".into())});
    fails!(post_before_filter,&p,RuntimeError::MappingException {node:5,message:None});
    fails!(prefilter_error,&p,RuntimeError::MappingException {node:42,message:None});
    fails!(later_post_error,&p,RuntimeError::MappingException {node:45,message:None});
}

#[test] fn ordinary_grouping_modes_remain_ordinary_outside_owned_ancestry() {
    let p=input("P.root","P");
    let n=input("N.root","N");
    let required=[BasicNamedSourceInput {name:"Rows",instance:&n}];
    let singletons=expected("P.root",&[("P-a1",&["P-a1"]),("P-b1",&["P-b1"]),("P-a2",&["P-a2"]),("P-c1",&["P-c1"]),("P-b2",&["P-b2"])]);
    assert_eq!(legacy_adjacent::execute_with_basic_owned_sources(&p,&required,None,limits()),Ok(legacy_adjacent::ExecutionOutputs {primary:singletons.clone(),extras:vec![]}));
    assert_eq!(legacy_blocks::execute_with_basic_owned_sources(&p,&required,None,limits()),Ok(legacy_blocks::ExecutionOutputs {primary:singletons,extras:vec![]}));
    let starting=expected("P.root",&[("P-a1",&["P-a1"]),("P-b1",&["P-b1"]),("P-a2",&["P-a2","P-c1","P-b2"])]);
    assert_eq!(legacy_starting::execute_with_basic_owned_sources(&p,&required,None,limits()),Ok(legacy_starting::ExecutionOutputs {primary:starting,extras:vec![]}));
    let ending=expected("P.root",&[("P-a1",&["P-a1","P-b1"]),("P-a2",&["P-a2"]),("P-c1",&["P-c1","P-b2"])]);
    assert_eq!(legacy_ending::execute_with_basic_owned_sources(&p,&required,None,limits()),Ok(legacy_ending::ExecutionOutputs {primary:ending,extras:vec![]}));
}

#[test] fn selection_is_exact_precedes_policy_and_skips_unselected_target_failures() {
    let p=input("P.root","P");let mut zero=limits();zero.max_roots=0;
    assert_eq!(selection::execute_selected_with_basic_owned_sources(&p,&[],None,zero,TargetSelection::Named("exact audit 雪")),Err(RuntimeError::UnknownTarget {name:"exact audit 雪".into()}));
    let wanted=expected("P.root",&[("P-a1",&["P-a1","P-a2"]),("P-b1",&["P-b1","P-b2"]),("P-c1",&["P-c1"])]);
    assert_eq!(selection::execute_selected_with_basic_owned_sources(&p,&[],None,limits(),TargetSelection::Primary),Ok(selection::SelectedTargetOutput::Primary(wanted)));
    fails!(selection,&p,RuntimeError::MappingException {node:71,message:None});
}

#[test] fn named_admission_precedes_group_callbacks_and_limits_remain_explicit() {
    use codegen_runtime::{OwnedSourceError,OwnedSourceLimitKind};
    let p=input("P.root","P");let mut zero=limits();zero.max_roots=0;
    assert_eq!(key_error::execute_selected_with_basic_owned_sources(&p,&[],None,zero,TargetSelection::Primary),Err(RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {kind:OwnedSourceLimitKind::Roots,used:0,requested:3,max:0})));
    let duplicate=[BasicNamedSourceInput {name:"Rows",instance:&p};2];
    assert_eq!(key_error::execute_with_basic_owned_sources(&p,&duplicate,None,limits()),Err(RuntimeError::DuplicateNamedSource {name:"Rows"}));
    let unexpected=[BasicNamedSourceInput {name:"rows",instance:&p}];
    assert_eq!(key_error::execute_with_basic_owned_sources(&p,&unexpected,None,limits()),Err(RuntimeError::UnexpectedNamedSource {name:"rows".into()}));
    // A colliding primary Rows collection never satisfies a required named binding.
    fails!(legacy_key,&p,RuntimeError::MissingNamedSource {name:"Rows"});
    assert_eq!(legacy_post::execute_selected_with_basic_owned_sources(&p,&[],None,limits(),TargetSelection::Primary),Err(RuntimeError::MissingNamedSource {name:"Rows"}));
}
"#;

#[test]
fn owned_by_empty_catalogue_public_consumers_build_with_warnings_denied() {
    let directory = TempDir::new("owned_by_empty_catalogue");
    let mut options = options();
    options.package_name = "owned_empty_catalogue_host".into();
    let mut plain = project(SourceOwner::Primary);
    plain.source = SchemaNode::scalar("Input", ScalarType::String);
    plain.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    plain.extra_sources.clear();
    plain.extra_targets.clear();
    plain.graph.nodes = BTreeMap::from([(1, owned(&SourceOwner::Primary, &[], None))]);
    plain.root = mapping::Scope {
        bindings: vec![binding("Value", 1)],
        ..Default::default()
    };
    let plain_program = program(&plain);
    let mut library = String::from("#![deny(warnings)]\n");
    // All checked Basic companion families share the empty-catalogue import.
    // Compile their public consumers, rather than just checking source text.
    for (name, emit) in [
        (
            "control_free",
            emit_with_basic_owned_sources
                as fn(&Program, &Options) -> Result<ArtifactSet, EmitError>,
        ),
        ("filters", emit_with_basic_owned_filters),
        ("sort", emit_with_basic_owned_sort),
        ("windows", emit_with_basic_owned_windows),
        ("row_failures", emit_with_basic_owned_row_failures),
        ("group_profile", emit_with_basic_owned_group_by),
    ] {
        let files = emit(&plain_program, &options).unwrap();
        let source = generated_source(&files);
        library.push_str(&format!("pub mod {name} {{\n{source}\n}}\n"));
    }
    let mut grouped = plain.clone();
    let types = ir::ScalarTypeSet::new([
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ])
    .unwrap();
    grouped.source = SchemaNode::scalar_union("Input", types).repeating();
    grouped.target = SchemaNode::scalar_union("Output", types).repeating();
    grouped.graph.nodes = BTreeMap::from([
        (1, owned(&SourceOwner::Primary, &[], None)),
        (2, owned(&SourceOwner::Primary, &[], None)),
    ]);
    grouped.root = mapping::Scope {
        iteration: mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: Vec::new(),
            },
            output_path: None,
        },
        construction: mapping::ScopeConstruction::Scalar { value: 2 },
        group_by: Some(1),
        ..Default::default()
    };
    let files = emit_with_basic_owned_group_by(&program(&grouped), &options).unwrap();
    library.push_str(&format!(
        "pub mod root_groups {{\n{}\n}}\n",
        generated_source(&files)
    ));
    fs::create_dir_all(directory.path().join("src")).unwrap();
    fs::create_dir_all(directory.path().join("tests")).unwrap();
    fs::write(
        directory.path().join("plain.Project.json"),
        serde_json::to_vec_pretty(&plain).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("root_groups.Project.json"),
        serde_json::to_vec_pretty(&grouped).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        render_manifest(&options),
    )
    .unwrap();
    fs::write(directory.path().join("src/lib.rs"), library).unwrap();
    fs::write(
        directory.path().join("tests/public_api.rs"),
        EMPTY_CATALOGUE_CONSUMER,
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["test", "--offline", "--test", "public_api"])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .unwrap();
    fs::write(directory.path().join("stdout.complete"), &output.stdout).unwrap();
    fs::write(directory.path().join("stderr.complete"), &output.stderr).unwrap();
    if !output.status.success() {
        let retained = directory.path().to_owned();
        std::mem::forget(directory);
        panic!(
            "warnings-denied empty-catalogue consumer failure retained at {}:\n{}\n{}",
            retained.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

const EMPTY_CATALOGUE_CONSUMER: &str = r#"
#![deny(warnings)]
use owned_empty_catalogue_host::*;
use codegen_runtime::{BasicNamedSourceInput,ExecutionContext,Instance,OwnedSourceError,
    OwnedSourceLimitKind,OwnedSourceLimits,RuntimeError,TargetSelection,Value,field,group,repeated,scalar};
use std::path::Path;

fn limits()->OwnedSourceLimits {OwnedSourceLimits {max_roots:1,max_active_frames:16,
    max_path_segments:32,max_metadata_bytes:65536,max_walk_contexts:32}}
fn source()->Instance {repeated([Value::Int(3),Value::String("3".into()),Value::Int(3),
    Value::Float(3.0),Value::Bool(true),Value::String("3".into())].into_iter().map(scalar))}
fn expected()->Instance {repeated([Value::Int(3),Value::String("3".into()),
    Value::Float(3.0),Value::Bool(true)].into_iter().map(scalar))}

#[test] fn root_by_without_named_inputs_returns_complete_tagged_outputs_and_errors() {
    let source=source();let before=source.clone();let expected=expected();
    let execution=ExecutionContext::new(Path::new("empty-catalogue.mapping"));
    assert_eq!(root_groups::execute_with_basic_owned_sources(&source,&[],Some(&execution),limits()),
        Ok(root_groups::ExecutionOutputs {primary:expected.clone(),extras:vec![]}));
    assert_eq!(root_groups::execute_selected_with_basic_owned_sources(&source,&[],None,limits(),TargetSelection::Primary),
        Ok(root_groups::SelectedTargetOutput::Primary(expected)));
    assert_eq!(source,before);
    assert_eq!(root_groups::execute_with_basic_owned_sources(&repeated([]),&[],None,limits()),
        Ok(root_groups::ExecutionOutputs {primary:repeated([]),extras:vec![]}));
    let mut zero=limits();zero.max_roots=0;
    let unexpected=[BasicNamedSourceInput {name:"Rows",instance:&source}];
    assert_eq!(root_groups::execute_selected_with_basic_owned_sources(&source,&unexpected,None,zero,TargetSelection::Named("missing")),
        Err(RuntimeError::UnknownTarget {name:"missing".into()}));
    assert_eq!(root_groups::execute_selected_with_basic_owned_sources(&source,&[],None,zero,TargetSelection::Primary),
        Err(RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {kind:OwnedSourceLimitKind::Roots,used:0,requested:1,max:0})));
    assert_eq!(root_groups::execute_with_basic_owned_sources(&source,&unexpected,None,limits()),
        Err(RuntimeError::UnexpectedNamedSource {name:"Rows".into()}));
}

#[test] fn every_basic_companion_with_empty_catalogues_has_a_strict_public_consumer() {
    let source=scalar(Value::String("only".into()));
    let expected=group([field("Value",scalar(Value::String("only".into())))]);
    let limits=OwnedSourceLimits {max_roots:1,max_active_frames:0,max_path_segments:0,
        max_metadata_bytes:0,max_walk_contexts:0};
    macro_rules! check {($module:ident)=>{{
        assert_eq!($module::execute_with_basic_owned_sources(&source,&[],None,limits),
            Ok($module::ExecutionOutputs {primary:expected.clone(),extras:vec![]}));
        assert_eq!($module::execute_selected_with_basic_owned_sources(&source,&[],None,limits,TargetSelection::Primary),
            Ok($module::SelectedTargetOutput::Primary(expected.clone())));
    }};}
    check!(control_free);check!(filters);check!(sort);check!(windows);check!(row_failures);check!(group_profile);
}
"#;
