//! Native adjacent controls with literal run lists, complete trees and typed errors.
//! Expected constructors expand listed fields/members; they never partition input.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, NamedSource, NamedTarget, Node, NodeId, OwnedPosition,
    OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration, SequenceExpr, SequenceWindow,
    SortFilterOrder, SourceOwner,
};

use crate::{EngineError, OwnedCapabilityContext as Domain, OwnedSourceError as Access};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}
fn text(value: &str) -> Instance {
    scalar(Value::String(value.into()))
}
fn int(value: i64) -> Instance {
    scalar(Value::Int(value))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn constant(value: Value) -> Node {
    Node::Const { value }
}
fn binding(name: &str, node: NodeId) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn field(owner: SourceOwner, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: pin.map(path),
    })
}
fn position(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner,
        collection: Some(path(parts)),
    })
}
fn owned(owner: SourceOwner, parts: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        output_path: None,
    }
}
fn source_schema() -> SchemaNode {
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
                    SchemaNode::scalar("Limit", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
}
fn groups_schema(marker: bool) -> SchemaNode {
    let mut fields = vec![
        SchemaNode::scalar("First", ScalarType::String),
        SchemaNode::scalar("GroupPosition", ScalarType::Int),
    ];
    if marker {
        fields.push(SchemaNode::scalar("RootMarker", ScalarType::String));
    }
    fields.push(
        SchemaNode::group(
            "Members",
            vec![
                SchemaNode::scalar("Label", ScalarType::String),
                SchemaNode::scalar("MemberPosition", ScalarType::Int),
                SchemaNode::scalar("LegacyPosition", ScalarType::Int),
            ],
        )
        .repeating(),
    );
    SchemaNode::group("Groups", fields).repeating()
}
fn project(owner: SourceOwner) -> Project {
    Project {
        source: source_schema(),
        target: SchemaNode::group("Output", vec![groups_schema(true)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Rows", " Røws💠 "]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("{name}.json"),
                schema: source_schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (1, field(owner.clone(), &["Rows", "Key"], None)),
                (2, field(owner.clone(), &["Rows", "Label"], Some(&["Rows"]))),
                (3, position(owner.clone(), &["Rows"])),
                (4, field(owner.clone(), &["Marker"], Some(&[]))),
                (5, field(owner.clone(), &["Rows", "Keep"], None)),
                (6, field(owner.clone(), &["Rows", "Rank"], None)),
                (7, field(owner.clone(), &["Limit"], Some(&[]))),
                (
                    8,
                    Node::Position {
                        collection: path(&["Rows"]),
                    },
                ),
                (9, constant(Value::Int(2))),
                (10, constant(Value::Int(1))),
                (11, constant(Value::Int(0))),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Groups".into(),
                iteration: owned(owner.clone(), &["Rows"]),
                group_adjacent_by: Some(1),
                bindings: vec![
                    binding("First", 2),
                    binding("GroupPosition", 3),
                    binding("RootMarker", 4),
                ],
                children: vec![Scope {
                    target_field: "Members".into(),
                    iteration: owned(owner, &["Rows"]),
                    bindings: vec![
                        binding("Label", 2),
                        binding("MemberPosition", 3),
                        binding("LegacyPosition", 8),
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}
fn controls(p: &mut Project) -> &mut Scope {
    &mut p.root.children[0]
}
fn row(key: Value, label: &str, keep: bool, rank: i64) -> Instance {
    group(vec![
        ("Key", scalar(key)),
        ("Label", text(label)),
        ("Keep", scalar(Value::Bool(keep))),
        ("Rank", int(rank)),
        ("Limit", int(0)),
    ])
}
fn input(prefix: &str) -> Instance {
    let keeps = if prefix == "N" {
        [true, false, true, false, true, false]
    } else {
        [false, true, false, true, false, true]
    };
    let rows = if prefix == "U" {
        vec![row(Value::String("A".into()), "U-only", true, 7)]
    } else {
        [
            ("A", "a1", 30),
            ("B", "b1", 10),
            ("A", "a2", 20),
            ("A", "a3", 25),
            ("C", "c1", 40),
            ("A", "a4", 50),
        ]
        .into_iter()
        .zip(keeps)
        .map(|((key, label, rank), keep)| {
            row(
                Value::String(key.into()),
                &format!("{prefix}-{label}"),
                keep,
                if prefix == "N" { rank * 10 } else { rank },
            )
        })
        .collect()
    };
    group(vec![
        ("Marker", text(&format!("{prefix}.root"))),
        ("Limit", int(if prefix == "N" { 1 } else { 2 })),
        ("Rows", Instance::Repeated(rows)),
    ])
}
fn supplied() -> Vec<(String, Instance)> {
    vec![("Rows".into(), input("N")), (" Røws💠 ".into(), input("U"))]
}
fn run(p: &Project) -> Result<Instance, EngineError> {
    crate::run_with_sources(p, &input("P"), supplied())
}
fn member(label: &str, ordinal: i64) -> Instance {
    group(vec![
        ("Label", text(label)),
        ("MemberPosition", int(ordinal)),
        ("LegacyPosition", int(ordinal)),
    ])
}
fn expected_group(ordinal: i64, first: &str, marker: Option<&str>, members: &[&str]) -> Instance {
    let mut fields = vec![("First", text(first)), ("GroupPosition", int(ordinal))];
    if let Some(marker) = marker {
        fields.push(("RootMarker", text(marker)));
    }
    fields.push((
        "Members",
        Instance::Repeated(
            members
                .iter()
                .enumerate()
                .map(|(index, label)| member(label, index as i64 + 1))
                .collect(),
        ),
    ));
    group(fields)
}
fn output(groups: Vec<Instance>) -> Instance {
    group(vec![("Groups", Instance::Repeated(groups))])
}
fn expected(prefix: &str, runs: &[(&str, &[&str])]) -> Instance {
    output(
        runs.iter()
            .enumerate()
            .map(|(index, (first, members))| {
                let labels = members
                    .iter()
                    .map(|label| format!("{prefix}-{label}"))
                    .collect::<Vec<_>>();
                let labels = labels.iter().map(String::as_str).collect::<Vec<_>>();
                expected_group(
                    index as i64 + 1,
                    &format!("{prefix}-{first}"),
                    Some(&format!("{prefix}.root")),
                    &labels,
                )
            })
            .collect(),
    )
}
fn five(prefix: &str) -> Instance {
    expected(
        prefix,
        &[
            ("a1", &["a1"]),
            ("b1", &["b1"]),
            ("a2", &["a2", "a3"]),
            ("c1", &["c1"]),
            ("a4", &["a4"]),
        ],
    )
}
fn call(p: &mut Project, node: NodeId, function: &str, args: &[NodeId]) {
    p.graph.nodes.insert(
        node,
        Node::Call {
            function: function.into(),
            args: args.to_vec(),
        },
    );
}
fn raises(p: &mut Project, node: NodeId, message: &str) {
    p.graph
        .nodes
        .insert(node + 1000, constant(Value::String(message.into())));
    p.graph.nodes.insert(
        node,
        Node::Raise {
            message: Some(node + 1000),
        },
    );
}
fn exception(node: NodeId, message: &str) -> EngineError {
    EngineError::MappingException {
        node,
        message: Some(message.into()),
    }
}

#[test]
fn contiguous_runs_keep_exact_owner_complete_members_and_independent_positions() {
    for (owner, prefix) in [(SourceOwner::Primary, "P"), (named("Rows"), "N")] {
        assert_eq!(run(&project(owner)), Ok(five(prefix)));
    }
    assert_eq!(
        run(&project(named(" Røws💠 "))),
        Ok(output(vec![expected_group(
            1,
            "U-only",
            Some("U.root"),
            &["U-only"]
        ),]))
    );
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).group_adjacent_by = None;
    controls(&mut p).group_by = Some(1);
    // Original global By still merges separated equal keys.
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[
                ("a1", &["a1", "a2", "a3", "a4"]),
                ("b1", &["b1"]),
                ("c1", &["c1"]),
            ]
        ))
    );
}

#[test]
fn retention_keeps_false_representative_and_never_joins_across_dropped_runs() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).post_group_filter = Some(5);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[("b1", &["b1"]), ("a2", &["a2", "a3"]), ("a4", &["a4"]),]
        ))
    );
    p.graph.nodes.insert(5, constant(Value::Bool(false)));
    assert_eq!(run(&p), Ok(output(vec![])));
    controls(&mut p).post_group_filter = None;
    p.graph
        .nodes
        .insert(20, constant(Value::String("A".into())));
    call(&mut p, 21, "equal", &[1, 20]);
    controls(&mut p).filter = Some(21);
    // A preselection does remove separators before partitioning.
    assert_eq!(
        run(&p),
        Ok(expected("P", &[("a1", &["a1", "a2", "a3", "a4"])]))
    );
    controls(&mut p).filter = None;
    controls(&mut p).group_adjacent_by = Some(20);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[("a1", &["a1", "b1", "a2", "a3", "c1", "a4"])]
        ))
    );
}

#[test]
fn stable_sort_and_both_filter_orders_feed_selected_rank_to_key_and_post() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).sort_by = Some(6);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[
                ("b1", &["b1"]),
                ("a2", &["a2", "a3", "a1"]),
                ("c1", &["c1"]),
                ("a4", &["a4"]),
            ]
        ))
    );
    call(&mut p, 20, "greater_than", &[3, 9]);
    controls(&mut p).filter = Some(20);
    controls(&mut p).sort_filter_order = SortFilterOrder::FilterThenSort;
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[("a2", &["a2", "a3"]), ("c1", &["c1"]), ("a4", &["a4"]),]
        ))
    );
    controls(&mut p).sort_filter_order = SortFilterOrder::SortThenFilter;
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[("a3", &["a3", "a1"]), ("c1", &["c1"]), ("a4", &["a4"]),]
        ))
    );
    p.graph.nodes.insert(21, constant(Value::Int(4)));
    call(&mut p, 22, "equal", &[3, 21]);
    controls(&mut p).post_group_filter = Some(22);
    assert_eq!(run(&p), Ok(expected("P", &[("a3", &["a3", "a1"])])));
    controls(&mut p).filter = None;
    controls(&mut p).post_group_filter = None;
    p.graph.nodes.insert(21, constant(Value::Int(3)));
    call(&mut p, 22, "less_or_equal", &[3, 21]);
    controls(&mut p).group_adjacent_by = Some(22);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[("b1", &["b1", "a2", "a3"]), ("a1", &["a1", "c1", "a4"]),]
        ))
    );
}

#[test]
fn sort_owned_position_is_raw_and_group_bounds_are_parent_values_not_row_limits() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).sort_by = Some(3);
    controls(&mut p).sort_descending = true;
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[
                ("a4", &["a4"]),
                ("c1", &["c1"]),
                ("a3", &["a3", "a2"]),
                ("b1", &["b1"]),
                ("a1", &["a1"]),
            ]
        ))
    );
    controls(&mut p).sort_by = Some(6);
    controls(&mut p).sort_descending = false;
    controls(&mut p).windows = vec![SequenceWindow::First { count: 7 }];
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[("b1", &["b1"]), ("a2", &["a2", "a3", "a1"]),]
        ))
    );
}

#[test]
fn stored_windows_apply_to_retained_groups_in_order_and_compact_only_group_ordinals() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).windows = vec![
        SequenceWindow::SkipFirst { count: 10 },
        SequenceWindow::Last { count: 9 },
    ];
    assert_eq!(
        run(&p),
        Ok(expected("P", &[("c1", &["c1"]), ("a4", &["a4"])]))
    );
    controls(&mut p).windows.reverse();
    assert_eq!(run(&p), Ok(expected("P", &[("a4", &["a4"])])));
    p.graph.nodes.insert(20, constant(Value::Int(3)));
    p.graph.nodes.insert(21, constant(Value::Int(4)));
    controls(&mut p).windows = vec![SequenceWindow::FromTo {
        first: 20,
        last: 21,
    }];
    assert_eq!(
        run(&p),
        Ok(expected("P", &[("a2", &["a2", "a3"]), ("c1", &["c1"])]))
    );
    controls(&mut p).windows.clear();
    controls(&mut p).iteration_output = IterationOutput::First;
    p.target = SchemaNode::group(
        "Output",
        vec![{
            let mut schema = groups_schema(true);
            schema.repeating = false;
            schema
        }],
    );
    assert_eq!(
        run(&p),
        Ok(group(vec![(
            "Groups",
            expected_group(1, "P-a1", Some("P.root"), &["P-a1"])
        )]))
    );
    assert_eq!(
        crate::run(&p, &group(vec![("Rows", Instance::Repeated(vec![]))])),
        Ok(group(vec![("Groups", group(vec![]))]))
    );
}

#[test]
fn fused_key_and_post_errors_precede_later_selection_but_filter_then_sort_is_eager() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 101, "key");
    raises(&mut p, 102, "filter");
    raises(&mut p, 105, "post");
    call(&mut p, 20, "greater_than", &[3, 10]);
    p.graph.nodes.insert(21, constant(Value::Bool(true)));
    p.graph.nodes.insert(
        22,
        Node::If {
            condition: 20,
            then: 102,
            else_: 21,
        },
    );
    controls(&mut p).filter = Some(22);
    controls(&mut p).group_adjacent_by = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(run(&p), Err(exception(101, "key")));
    controls(&mut p).group_adjacent_by = Some(1);
    assert_eq!(run(&p), Err(exception(105, "post")));
    raises(&mut p, 103, "sort");
    raises(&mut p, 104, "bound");
    controls(&mut p).sort_by = Some(103);
    controls(&mut p).windows = vec![SequenceWindow::First { count: 104 }];
    controls(&mut p).sort_filter_order = SortFilterOrder::FilterThenSort;
    assert_eq!(run(&p), Err(exception(102, "filter")));
    controls(&mut p).sort_filter_order = SortFilterOrder::SortThenFilter;
    assert_eq!(run(&p), Err(exception(103, "sort")));
    controls(&mut p).sort_by = Some(6);
    assert_eq!(run(&p), Err(exception(104, "bound")));
}

#[test]
fn first_zero_and_implicit_first_do_not_hide_key_post_or_later_bound_errors() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 101, "key");
    raises(&mut p, 104, "bound");
    raises(&mut p, 105, "post");
    controls(&mut p).windows = vec![SequenceWindow::First { count: 11 }];
    controls(&mut p).group_adjacent_by = Some(101);
    assert_eq!(run(&p), Err(exception(101, "key")));
    controls(&mut p).iteration_output = IterationOutput::First;
    p.target = SchemaNode::group(
        "Output",
        vec![{
            let mut schema = groups_schema(true);
            schema.repeating = false;
            schema
        }],
    );
    assert_eq!(run(&p), Err(exception(101, "key")));
    controls(&mut p).group_adjacent_by = Some(1);
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(run(&p), Err(exception(105, "post")));
    controls(&mut p)
        .windows
        .push(SequenceWindow::Last { count: 104 });
    assert_eq!(run(&p), Err(exception(104, "bound")));
}

#[test]
fn every_post_runs_after_a_true_member_and_typed_empty_bound_errors_keep_nodes() {
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(20, constant(Value::String("A".into())));
    controls(&mut p).group_adjacent_by = Some(20);
    raises(&mut p, 105, "post");
    call(&mut p, 21, "greater_than", &[3, 10]);
    p.graph.nodes.insert(22, constant(Value::Bool(true)));
    p.graph.nodes.insert(
        23,
        Node::If {
            condition: 21,
            then: 105,
            else_: 22,
        },
    );
    controls(&mut p).post_group_filter = Some(23);
    assert_eq!(run(&p), Err(exception(105, "post")));
    p.graph
        .nodes
        .insert(105, constant(Value::String("not bool".into())));
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(
        run(&p),
        Err(EngineError::NotABool {
            node: 105,
            found: "string"
        })
    );
    p.graph.nodes.insert(104, constant(Value::Bool(true)));
    controls(&mut p).windows = vec![SequenceWindow::First { count: 104 }];
    assert_eq!(
        crate::run(&p, &group(vec![("Rows", Instance::Repeated(vec![]))])),
        Err(EngineError::NotAnItemCount {
            node: 104,
            found: "bool"
        })
    );
}

#[test]
fn excluded_targets_and_all_filtered_candidates_are_lazy_but_bounds_still_run() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 101, "key");
    raises(&mut p, 104, "bound");
    raises(&mut p, 105, "post");
    raises(&mut p, 106, "target");
    controls(&mut p).bindings[0].node = 106;
    controls(&mut p).windows = vec![SequenceWindow::First { count: 11 }];
    assert_eq!(run(&p), Ok(output(vec![])));
    controls(&mut p).windows.clear();
    p.graph.nodes.insert(20, constant(Value::Bool(false)));
    controls(&mut p).post_group_filter = Some(20);
    assert_eq!(run(&p), Ok(output(vec![])));
    controls(&mut p).group_adjacent_by = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    controls(&mut p).filter = Some(20);
    assert_eq!(run(&p), Ok(output(vec![])));
    controls(&mut p).windows = vec![SequenceWindow::First { count: 104 }];
    assert_eq!(run(&p), Err(exception(104, "bound")));
}

fn root_fixture(
    owner: SourceOwner,
    alias: bool,
    declared_repeating: bool,
) -> (Project, Instance, Vec<(String, Instance)>) {
    let mut p = project(owner.clone());
    let mut schema = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    );
    schema.repeating = declared_repeating;
    let rows = Instance::Repeated(vec![
        row(Value::String("A".into()), "a1", true, 1),
        row(Value::String("B".into()), "b1", true, 2),
        row(Value::String("A".into()), "a2", true, 3),
        row(Value::String("A".into()), "a3", true, 4),
    ]);
    let (primary, supplied) = match &owner {
        SourceOwner::Primary => {
            p.source = schema;
            p.extra_sources.clear();
            (rows, vec![])
        }
        SourceOwner::Named { name } => {
            p.extra_sources = vec![NamedSource {
                name: name.clone(),
                path: "root.json".into(),
                schema,
                options: Default::default(),
                dynamic_path: None,
            }];
            (input("P"), vec![(name.clone(), rows)])
        }
    };
    p.graph.nodes = BTreeMap::from([
        (1, field(owner.clone(), &["Key"], None)),
        (2, field(owner.clone(), &["Label"], None)),
        (3, position(owner.clone(), &[])),
        (8, Node::Position { collection: vec![] }),
        (41, field(owner.clone(), &["Key"], Some(&[]))),
        (42, field(owner.clone(), &["Label"], Some(&[]))),
    ]);
    let scope = controls(&mut p);
    scope.iteration = if alias {
        ScopeIteration::Source(path(&["Alias"]))
    } else {
        owned(owner.clone(), &[])
    };
    scope.bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
    scope.children[0].iteration = owned(owner, &[]);
    scope.children[0].bindings[0].node = 42;
    p.target = SchemaNode::group("Output", vec![groups_schema(false)]);
    (p, primary, supplied)
}

fn root_expected() -> Instance {
    output(vec![
        expected_group(1, "a1", None, &["a1"]),
        expected_group(2, "b1", None, &["b1"]),
        expected_group(3, "a2", None, &["a2", "a3"]),
    ])
}

#[test]
fn repeated_root_groups_preserve_latest_exact_member_empty_pin_and_legacy_alias_identity() {
    for (owner, alias) in [
        (SourceOwner::Primary, false),
        (named("Alias"), false),
        (named("Alias"), true),
    ] {
        for declared_repeating in [false, true] {
            let (mut p, primary, supplied) = root_fixture(owner.clone(), alias, declared_repeating);
            assert_eq!(
                crate::run_with_sources(&p, &primary, supplied.clone()),
                Ok(root_expected())
            );
            if alias {
                // Actual alias wrappers preserve raw [Alias], but canonical C is [].
                let mut legacy = p.clone();
                controls(&mut legacy).children[0].iteration = ScopeIteration::Source(vec![]);
                legacy.graph.nodes.insert(
                    8,
                    Node::Position {
                        collection: path(&["Alias"]),
                    },
                );
                assert_eq!(
                    crate::run_with_sources(&legacy, &primary, supplied.clone()),
                    Ok(root_expected())
                );
            }
            // Ordinary active root Items retain the old empty-pin rule, too.
            controls(&mut p).group_adjacent_by = None;
            controls(&mut p).bindings[0].node = 42;
            controls(&mut p).children.clear();
            assert_eq!(
                crate::run_with_sources(&p, &primary, supplied),
                Ok(output(vec![
                    group(vec![("First", text("a1")), ("GroupPosition", int(1))]),
                    group(vec![("First", text("b1")), ("GroupPosition", int(2))]),
                    group(vec![("First", text("a2")), ("GroupPosition", int(3))]),
                    group(vec![("First", text("a3")), ("GroupPosition", int(4))]),
                ]))
            );
        }
    }
}

#[test]
fn synthetic_root_group_empty_pin_reads_immutable_root_regardless_of_schema_repetition_flag() {
    for (owner, alias) in [
        (SourceOwner::Primary, false),
        (named("Alias"), false),
        (named("Alias"), true),
    ] {
        for declared_repeating in [false, true] {
            let (mut p, primary, supplied) = root_fixture(owner.clone(), alias, declared_repeating);
            controls(&mut p).bindings = vec![binding("First", 41)];
            controls(&mut p).children.clear();
            assert_eq!(
                crate::run_with_sources(&p, &primary, supplied),
                Err(EngineError::OwnedSourceAccess {
                    node: Some(41),
                    scope: None,
                    source: Access::UniteratedOwnedRepetition {
                        path: path(&["Key"])
                    },
                })
            );
        }
    }
}

#[test]
fn newer_other_owner_item_cannot_shadow_root_group_and_nonrepeating_singletons_still_pin_root() {
    let (mut p, primary, _) = root_fixture(SourceOwner::Primary, false, true);
    p.extra_sources = vec![NamedSource {
        name: "Alias".into(),
        path: "alias.json".into(),
        schema: p.source.clone(),
        options: Default::default(),
        dynamic_path: None,
    }];
    controls(&mut p).children[0].iteration = owned(named("Alias"), &[]);
    controls(&mut p).children[0].bindings = vec![binding("Label", 41)];
    assert_eq!(
        crate::run_with_sources(
            &p,
            &primary,
            vec![(
                "Alias".into(),
                Instance::Repeated(vec![row(Value::String("A".into()), "foreign", true, 1),])
            )]
        ),
        Err(EngineError::OwnedSourceAccess {
            node: Some(41),
            scope: None,
            source: Access::UniteratedOwnedRepetition {
                path: path(&["Key"])
            },
        })
    );
    for owner in [SourceOwner::Primary, named("Alias")] {
        let (mut p, _, _) = root_fixture(owner.clone(), false, false);
        controls(&mut p).bindings[0].node = 42;
        let singleton = row(Value::String("A".into()), "single", true, 1);
        let (primary, supplied) = match owner {
            SourceOwner::Primary => (singleton, vec![]),
            SourceOwner::Named { name } => (input("P"), vec![(name, singleton)]),
        };
        assert_eq!(
            crate::run_with_sources(&p, &primary, supplied),
            Ok(output(
                vec![expected_group(1, "single", None, &["single"]),]
            ))
        );
    }
}

#[test]
fn nonempty_group_does_not_create_root_pin_or_descendant_representative_permission() {
    let mut p = project(SourceOwner::Primary);
    p.source.repeating = true;
    // Root payload is a Group, but declaration does not authorize the singleton bypass.
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(4),
            scope: None,
            source: Access::InactiveSourceFrame {
                owner: SourceOwner::Primary,
                collection: Some(vec![])
            },
        })
    );
    p.source.repeating = false;
    assert_eq!(run(&p), Ok(five("P")));
    p.graph.nodes.insert(
        2,
        field(SourceOwner::Primary, &["Rows", "Label"], Some(&[])),
    );
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: Access::UniteratedOwnedRepetition {
                path: path(&["Rows", "Label"])
            },
        })
    );
    p.graph.nodes.insert(
        2,
        field(SourceOwner::Primary, &["Rows", "Details", "Value"], None),
    );
    let source = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![
            ("Key", text("A")),
            (
                "Details",
                Instance::Repeated(vec![group(vec![("Value", text("no"))])]),
            ),
        ])]),
    )]);
    assert_eq!(
        crate::run(&p, &source),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: Access::UniteratedOwnedRepetition {
                path: path(&["Details", "Value"])
            },
        })
    );
}

#[test]
fn owned_absence_and_foreign_pins_never_fall_back_to_colliding_primary_payload() {
    let p = project(named("Rows"));
    let error = crate::run_with_sources(&p, &input("P"), vec![(" Røws💠 ".into(), input("U"))])
        .unwrap_err();
    assert!(matches!(error, EngineError::OwnedSourceAccess {
        node: None, scope: Some(_), source: Access::SourceUnavailable { owner }
    } if owner == named("Rows")));
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(2, field(named("Rows"), &["Rows", "Label"], Some(&["Rows"])));
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: Access::InactiveSourceFrame {
                owner: named("Rows"),
                collection: Some(path(&["Rows"]))
            },
        })
    );
    p.graph
        .nodes
        .insert(2, field(named("Rows"), &["Rows", "Label"], None));
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: Access::UniteratedOwnedRepetition {
                path: path(&["Rows", "Label"])
            },
        })
    );
    p.graph
        .nodes
        .insert(2, field(named("Røws💠"), &["Rows", "Label"], None));
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: Access::UnknownOwner {
                name: "Røws💠".into()
            },
        })
    );
}

/// Complete recursive comparison including tags, float bits, field order and XML origin.
fn assert_lossless(actual: &Instance, expected: &Instance) {
    match (actual, expected) {
        (Instance::Scalar(Value::Float(actual)), Instance::Scalar(Value::Float(expected))) => {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
        (Instance::Group(actual), Instance::Group(expected)) => {
            assert_eq!(actual.xml_type_origin(), expected.xml_type_origin());
            assert_eq!(actual.len(), expected.len());
            for ((actual_name, actual), (expected_name, expected)) in
                actual.iter().zip(expected.iter())
            {
                assert_eq!(actual_name, expected_name);
                assert_lossless(actual, expected);
            }
        }
        (Instance::Repeated(actual), Instance::Repeated(expected)) => {
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(expected) {
                assert_lossless(actual, expected);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

#[test]
fn exact_tagged_last_run_comparison_preserves_zero_nan_null_and_unicode_bits() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).bindings = vec![
        binding("Key", 1),
        binding("First", 2),
        binding("GroupPosition", 3),
    ];
    controls(&mut p).children[0]
        .bindings
        .insert(0, binding("Key", 1));
    // Empty target groups preserve scalar tags instead of adapting to String fields.
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![SchemaNode::group("Members", vec![]).repeating()],
            )
            .repeating(),
        ],
    );
    let nan = f64::from_bits(0x7ff8_0000_0000_0042);
    let keys = vec![
        ("i1", Value::Int(1)),
        ("f1", Value::Float(1.0)),
        ("i2", Value::Int(1)),
        ("zp", Value::Float(0.0)),
        ("zn", Value::Float(-0.0)),
        ("nan1", Value::Float(nan)),
        ("nan2", Value::Float(nan)),
        ("n1", Value::Null),
        ("jn", Value::JsonNull(ir::JsonNull)),
        ("n2", Value::Null),
        ("sA1", Value::String("A".into())),
        ("sa", Value::String("a".into())),
        ("sA2", Value::String("A".into())),
    ];
    let source = group(vec![(
        "Rows",
        Instance::Repeated(
            keys.iter()
                .enumerate()
                .map(|(index, (label, key))| row(key.clone(), label, true, index as i64))
                .collect(),
        ),
    )]);
    // This is a literal expected run list, not derived by comparing input keys.
    let runs = vec![
        (Value::Int(1), "i1", vec![(Value::Int(1), "i1")]),
        (Value::Float(1.0), "f1", vec![(Value::Float(1.0), "f1")]),
        (Value::Int(1), "i2", vec![(Value::Int(1), "i2")]),
        (
            Value::Float(0.0),
            "zp",
            vec![(Value::Float(0.0), "zp"), (Value::Float(-0.0), "zn")],
        ),
        (Value::Float(nan), "nan1", vec![(Value::Float(nan), "nan1")]),
        (Value::Float(nan), "nan2", vec![(Value::Float(nan), "nan2")]),
        (Value::Null, "n1", vec![(Value::Null, "n1")]),
        (
            Value::JsonNull(ir::JsonNull),
            "jn",
            vec![(Value::JsonNull(ir::JsonNull), "jn")],
        ),
        (Value::Null, "n2", vec![(Value::Null, "n2")]),
        (
            Value::String("A".into()),
            "sA1",
            vec![(Value::String("A".into()), "sA1")],
        ),
        (
            Value::String("a".into()),
            "sa",
            vec![(Value::String("a".into()), "sa")],
        ),
        (
            Value::String("A".into()),
            "sA2",
            vec![(Value::String("A".into()), "sA2")],
        ),
    ];
    let expected = output(
        runs.into_iter()
            .enumerate()
            .map(|(index, (key, label, members))| {
                group(vec![
                    ("Key", scalar(key)),
                    ("First", text(label)),
                    ("GroupPosition", int(index as i64 + 1)),
                    (
                        "Members",
                        Instance::Repeated(
                            members
                                .into_iter()
                                .enumerate()
                                .map(|(index, (key, label))| {
                                    group(vec![
                                        ("Key", scalar(key)),
                                        ("Label", text(label)),
                                        ("MemberPosition", int(index as i64 + 1)),
                                        ("LegacyPosition", int(index as i64 + 1)),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    );
    assert_lossless(&crate::run(&p, &source).unwrap(), &expected);
    controls(&mut p).bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
    controls(&mut p).children[0].bindings.remove(0);
    let source = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            row(Value::String("é".into()), "one", true, 1),
            row(Value::String("é".into()), "two", true, 2),
            row(Value::String("e\u{301}".into()), "three", true, 3),
            row(Value::String("é".into()), "four", true, 4),
        ]),
    )]);
    assert_eq!(
        crate::run(&p, &source),
        Ok(output(vec![
            expected_group(1, "one", None, &["one", "two"]),
            expected_group(2, "three", None, &["three"]),
            expected_group(3, "four", None, &["four"]),
        ]))
    );
}

#[test]
fn null_source_slots_are_pruned_without_compacting_key_and_post_positions() {
    let mut p = project(SourceOwner::Primary);
    let source = group(vec![
        ("Marker", text("P.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                row(Value::String("A".into()), "a1", true, 1),
                scalar(Value::Null),
                row(Value::String("A".into()), "a2", true, 2),
                scalar(Value::JsonNull(ir::JsonNull)),
                row(Value::String("B".into()), "b1", true, 3),
            ]),
        ),
    ]);
    call(&mut p, 20, "greater_than", &[3, 9]);
    controls(&mut p).post_group_filter = Some(20);
    assert_eq!(
        crate::run(&p, &source),
        Ok(output(vec![
            expected_group(1, "a1", Some("P.root"), &["a1", "a2"]),
            expected_group(2, "b1", Some("P.root"), &["b1"]),
        ]))
    );
    controls(&mut p).post_group_filter = None;
    controls(&mut p).group_adjacent_by = Some(3);
    assert_eq!(
        crate::run(&p, &source),
        Ok(output(vec![
            expected_group(1, "a1", Some("P.root"), &["a1"]),
            expected_group(2, "a2", Some("P.root"), &["a2"]),
            expected_group(3, "b1", Some("P.root"), &["b1"]),
        ]))
    );
}

#[test]
fn key_only_and_post_only_legacy_scopes_authenticate_primary_and_keep_descendants_paired() {
    for key_owned in [true, false] {
        let mut p = project(SourceOwner::Primary);
        controls(&mut p).iteration = ScopeIteration::Source(path(&["Rows"]));
        controls(&mut p).children.clear();
        controls(&mut p).bindings = vec![binding("First", 20)];
        p.graph.nodes.insert(
            20,
            Node::SourceField {
                path: path(&["Label"]),
                frame: None,
            },
        );
        if !key_owned {
            p.graph.nodes.insert(
                1,
                Node::SourceField {
                    path: path(&["Key"]),
                    frame: None,
                },
            );
            controls(&mut p).post_group_filter = Some(5);
        }
        let labels: &[&str] = if key_owned {
            &["P-a1", "P-b1", "P-a2", "P-c1", "P-a4"]
        } else {
            &["P-b1", "P-a2", "P-a4"]
        };
        assert_eq!(
            run(&p),
            Ok(output(
                labels
                    .iter()
                    .map(|label| group(vec![("First", text(label))]))
                    .collect()
            ))
        );
        // No owned selector/binding in this child: grouped ancestry must still
        // close specialized consumers and keep the original scope location.
        controls(&mut p).children.push(Scope {
            target_field: "Members".into(),
            iteration: ScopeIteration::Source(vec![]),
            group_ending_with: Some(21),
            ..Scope::default()
        });
        p.graph.nodes.insert(21, constant(Value::Bool(true)));
        assert_eq!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: Some(
                    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
                        .descendant(mapping::SourceScopeStep::Child(0))
                        .descendant(mapping::SourceScopeStep::Child(0))
                ),
                context: Domain::Grouping,
            })
        );
    }
}

#[test]
fn all_five_ordinary_sibling_grouping_islands_keep_ordinary_dispatch() {
    for mode in 0..5 {
        let mut p = project(SourceOwner::Primary);
        p.graph.nodes.insert(
            20,
            Node::SourceField {
                path: path(&["Key"]),
                frame: None,
            },
        );
        p.graph.nodes.insert(
            21,
            Node::SourceField {
                path: path(&["Label"]),
                frame: None,
            },
        );
        p.graph.nodes.insert(22, constant(Value::Bool(true)));
        let mut legacy = Scope {
            target_field: "Legacy".into(),
            iteration: ScopeIteration::Source(path(&["Rows"])),
            bindings: vec![binding("First", 21)],
            ..Scope::default()
        };
        let labels: &[&str] = match mode {
            0 => {
                legacy.group_by = Some(20);
                &["P-a1", "P-b1", "P-c1"]
            }
            1 => {
                legacy.group_adjacent_by = Some(20);
                &["P-a1", "P-b1", "P-a2", "P-c1", "P-a4"]
            }
            2 => {
                legacy.group_starting_with = Some(22);
                &["P-a1", "P-b1", "P-a2", "P-a3", "P-c1", "P-a4"]
            }
            3 => {
                legacy.group_ending_with = Some(22);
                &["P-a1", "P-b1", "P-a2", "P-a3", "P-c1", "P-a4"]
            }
            _ => {
                legacy.group_into_blocks = Some(9);
                &["P-a1", "P-a2", "P-c1"]
            }
        };
        p.root.children.push(legacy);
        p.target = SchemaNode::group(
            "Output",
            vec![
                groups_schema(true),
                SchemaNode::group(
                    "Legacy",
                    vec![SchemaNode::scalar("First", ScalarType::String)],
                )
                .repeating(),
            ],
        );
        let expected = group(vec![
            ("Groups", five("P").field("Groups").unwrap().clone()),
            (
                "Legacy",
                Instance::Repeated(
                    labels
                        .iter()
                        .map(|label| group(vec![("First", text(label))]))
                        .collect(),
                ),
            ),
        ]);
        assert_eq!(run(&p), Ok(expected), "ordinary mode {mode}");
    }
}

#[test]
fn same_owner_nested_adjacent_and_by_regroup_only_selected_run_and_leave_outer_ordinal_intact() {
    for adjacent in [true, false] {
        let mut p = project(SourceOwner::Primary);
        p.graph.nodes.insert(20, constant(Value::Int(3)));
        controls(&mut p).windows = vec![SequenceWindow::FromTo {
            first: 20,
            last: 20,
        }];
        let mut inner = controls(&mut p).clone();
        inner.target_field = "Inner".into();
        inner.windows.clear();
        if !adjacent {
            inner.group_adjacent_by = None;
            inner.group_by = Some(1);
        }
        controls(&mut p).children.push(inner);
        let mut groups = groups_schema(true);
        let mut inner_schema = groups_schema(true);
        inner_schema.name = "Inner".into();
        // Schema metadata is not needed to authorize the owned runtime, but is
        // supplied so nested complete target construction follows public usage.
        if let ir::SchemaKind::Group { children, .. } = &mut groups.kind {
            children.push(inner_schema);
        }
        p.target = SchemaNode::group("Output", vec![groups]);
        let expected = output(vec![group(vec![
            ("First", text("P-a2")),
            ("GroupPosition", int(1)),
            ("RootMarker", text("P.root")),
            (
                "Members",
                Instance::Repeated(vec![member("P-a2", 1), member("P-a3", 2)]),
            ),
            (
                "Inner",
                Instance::Repeated(vec![expected_group(
                    1,
                    "P-a2",
                    Some("P.root"),
                    &["P-a2", "P-a3"],
                )]),
            ),
        ])]);
        assert_eq!(run(&p), Ok(expected));
    }
}

#[test]
fn nested_cross_owner_adjacent_groups_preserve_outer_owner_and_restart_inner_domain() {
    for (outer_owner, inner_owner, outer_prefix, inner_prefix) in [
        (SourceOwner::Primary, named("Rows"), "P", "N"),
        (named("Rows"), SourceOwner::Primary, "N", "P"),
    ] {
        let mut p = project(outer_owner);
        controls(&mut p).windows = vec![SequenceWindow::First { count: 9 }];
        p.graph
            .nodes
            .insert(20, field(inner_owner.clone(), &["Rows", "Key"], None));
        p.graph
            .nodes
            .insert(21, field(inner_owner.clone(), &["Rows", "Label"], None));
        p.graph
            .nodes
            .insert(22, position(inner_owner.clone(), &["Rows"]));
        p.graph
            .nodes
            .insert(24, field(inner_owner.clone(), &["Marker"], Some(&[])));
        let inner = Scope {
            target_field: "Inner".into(),
            children: vec![Scope {
                target_field: "Groups".into(),
                iteration: owned(inner_owner.clone(), &["Rows"]),
                group_adjacent_by: Some(20),
                bindings: vec![
                    binding("First", 21),
                    binding("GroupPosition", 22),
                    binding("RootMarker", 24),
                ],
                children: vec![Scope {
                    target_field: "Members".into(),
                    iteration: owned(inner_owner, &["Rows"]),
                    bindings: vec![
                        binding("Label", 21),
                        binding("MemberPosition", 22),
                        binding("LegacyPosition", 8),
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        };
        controls(&mut p).bindings = vec![binding("OuterFirst", 2), binding("OuterPosition", 3)];
        controls(&mut p).children = vec![inner];
        p.target = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        SchemaNode::scalar("OuterFirst", ScalarType::String),
                        SchemaNode::scalar("OuterPosition", ScalarType::Int),
                        SchemaNode::group("Inner", vec![groups_schema(true)]),
                    ],
                )
                .repeating(),
            ],
        );
        let expected = output(
            [("a1", 1), ("b1", 2)]
                .into_iter()
                .map(|(first, ordinal)| {
                    group(vec![
                        ("OuterFirst", text(&format!("{outer_prefix}-{first}"))),
                        ("OuterPosition", int(ordinal)),
                        ("Inner", five(inner_prefix)),
                    ])
                })
                .collect(),
        );
        assert_eq!(run(&p), Ok(expected));
    }
}

#[test]
fn flattened_adjacent_run_keeps_first_intermediate_prefix_for_every_member() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Batches",
                vec![
                    SchemaNode::scalar("BatchName", ScalarType::String),
                    source_schema().child("Rows").unwrap().clone(),
                ],
            )
            .repeating(),
        ],
    );
    controls(&mut p).iteration = owned(SourceOwner::Primary, &["Batches", "Rows"]);
    controls(&mut p).children[0].iteration = owned(SourceOwner::Primary, &["Batches", "Rows"]);
    p.graph.nodes.insert(
        1,
        field(SourceOwner::Primary, &["Batches", "Rows", "Key"], None),
    );
    p.graph.nodes.insert(
        2,
        field(SourceOwner::Primary, &["Batches", "Rows", "Label"], None),
    );
    p.graph
        .nodes
        .insert(3, position(SourceOwner::Primary, &["Batches", "Rows"]));
    p.graph.nodes.insert(
        4,
        field(
            SourceOwner::Primary,
            &["Batches", "BatchName"],
            Some(&["Batches"]),
        ),
    );
    p.graph.nodes.insert(
        8,
        Node::Position {
            collection: path(&["Batches", "Rows"]),
        },
    );
    controls(&mut p).children[0]
        .bindings
        .push(binding("BatchName", 4));
    let batch = |name: &str, rows| {
        group(vec![
            ("BatchName", text(name)),
            ("Rows", Instance::Repeated(rows)),
        ])
    };
    let source = group(vec![(
        "Batches",
        Instance::Repeated(vec![
            batch(
                "X",
                vec![
                    row(Value::String("A".into()), "x1", true, 1),
                    row(Value::String("B".into()), "x2", true, 2),
                ],
            ),
            batch(
                "Y",
                vec![
                    row(Value::String("B".into()), "y1", true, 3),
                    row(Value::String("A".into()), "y2", true, 4),
                ],
            ),
        ]),
    )]);
    let expected = output(
        [
            (1, "x1", "X", &["x1"][..]),
            (2, "x2", "X", &["x2", "y1"][..]),
            (3, "y2", "Y", &["y2"][..]),
        ]
        .into_iter()
        .map(|(ordinal, first, batch, labels)| {
            group(vec![
                ("First", text(first)),
                ("GroupPosition", int(ordinal)),
                ("RootMarker", text(batch)),
                (
                    "Members",
                    Instance::Repeated(
                        labels
                            .iter()
                            .enumerate()
                            .map(|(index, label)| {
                                group(vec![
                                    ("Label", text(label)),
                                    ("MemberPosition", int(index as i64 + 1)),
                                    ("LegacyPosition", int(index as i64 + 1)),
                                    ("BatchName", text(batch)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ])
        })
        .collect(),
    );
    assert_eq!(crate::run(&p, &source), Ok(expected));
}

#[test]
fn group_bounds_read_incoming_active_batch_not_first_row_or_registered_root() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::group(
                "Batches",
                vec![
                    SchemaNode::scalar("Limit", ScalarType::Int),
                    source_schema().child("Rows").unwrap().clone(),
                ],
            )
            .repeating(),
        ],
    );
    for (node, leaf) in [(1, "Key"), (2, "Label")] {
        p.graph.nodes.insert(
            node,
            field(SourceOwner::Primary, &["Batches", "Rows", leaf], None),
        );
    }
    p.graph
        .nodes
        .insert(3, position(SourceOwner::Primary, &["Batches", "Rows"]));
    p.graph.nodes.insert(
        7,
        field(
            SourceOwner::Primary,
            &["Batches", "Limit"],
            Some(&["Batches"]),
        ),
    );
    p.graph.nodes.insert(
        8,
        Node::Position {
            collection: path(&["Batches", "Rows"]),
        },
    );
    controls(&mut p).iteration = owned(SourceOwner::Primary, &["Batches", "Rows"]);
    controls(&mut p).windows = vec![SequenceWindow::First { count: 7 }];
    controls(&mut p)
        .bindings
        .retain(|binding| binding.target_field != "RootMarker");
    controls(&mut p).children[0].iteration = owned(SourceOwner::Primary, &["Batches", "Rows"]);
    let grouped = controls(&mut p).clone();
    p.root = Scope {
        iteration: owned(SourceOwner::Primary, &["Batches"]),
        children: vec![grouped],
        ..Scope::default()
    };
    p.target = SchemaNode::group("Output", vec![groups_schema(false)]).repeating();
    let batch = |limit, rows| {
        group(vec![
            ("Limit", int(limit)),
            ("Rows", Instance::Repeated(rows)),
        ])
    };
    let source = group(vec![
        ("Limit", int(0)),
        (
            "Batches",
            Instance::Repeated(vec![
                batch(
                    1,
                    vec![
                        row(Value::String("A".into()), "x1", true, 1),
                        row(Value::String("B".into()), "x2", true, 2),
                        row(Value::String("A".into()), "x3", true, 3),
                    ],
                ),
                batch(
                    2,
                    vec![
                        row(Value::String("B".into()), "y1", true, 1),
                        row(Value::String("A".into()), "y2", true, 2),
                        row(Value::String("B".into()), "y3", true, 3),
                    ],
                ),
            ]),
        ),
    ]);
    assert_eq!(
        crate::run(&p, &source),
        Ok(Instance::Repeated(vec![
            output(vec![expected_group(1, "x1", None, &["x1"])]),
            output(vec![
                expected_group(1, "y1", None, &["y1"]),
                expected_group(2, "y2", None, &["y2"])
            ]),
        ]))
    );
}

#[derive(Default)]
struct Events {
    events: RefCell<Vec<crate::TraceEvent>>,
    grouped: std::cell::Cell<usize>,
}
impl crate::TraceSink for Events {
    fn record(&self, event: crate::TraceEvent) {
        self.events.borrow_mut().push(event);
    }
    fn wants_owned_context(&self) -> bool {
        true
    }
    fn wants_owned_context_for(&self, _: &crate::TraceEvent) -> bool {
        true
    }
    fn record_with_owned_context(
        &self,
        event: crate::TraceEvent,
        witness: Result<crate::TraceOwnedWitness<'_>, &crate::TraceOwnedContextError>,
    ) {
        if let Ok(witness) = witness
            && witness.domain == crate::TraceOwnedDomain::Grouped
        {
            self.grouped.set(self.grouped.get() + 1);
            assert!(matches!(
                witness.snapshot(),
                Err(crate::TraceOwnedWitnessError::UnsupportedDomain {
                    domain: "grouped source",
                    ..
                })
            ));
        }
        self.record(event);
    }
}

#[test]
fn trace_reports_adjacent_runs_before_retention_and_grouped_observer_stays_closed() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).post_group_filter = Some(5);
    let events = Events::default();
    let execution =
        crate::ExecutionContext::new(Path::new("adjacent.json")).with_trace_sink(&events);
    assert_eq!(
        crate::run_with_sources_and_context(&p, &input("P"), supplied(), &execution),
        Ok(expected(
            "P",
            &[("b1", &["b1"]), ("a2", &["a2", "a3"]), ("a4", &["a4"])]
        ))
    );
    let groups = events
        .events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            crate::TraceEvent::GroupProduced {
                grouping,
                group_index,
                member_count,
                retained,
                ..
            } => Some((*grouping, *group_index, *member_count, *retained)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        groups,
        vec![
            (crate::TraceGrouping::AdjacentBy { node: 1 }, 1, 1, false),
            (crate::TraceGrouping::AdjacentBy { node: 1 }, 2, 1, true),
            (crate::TraceGrouping::AdjacentBy { node: 1 }, 3, 2, true),
            (crate::TraceGrouping::AdjacentBy { node: 1 }, 4, 1, false),
            (crate::TraceGrouping::AdjacentBy { node: 1 }, 5, 1, true),
        ]
    );
    assert_eq!(
        events
            .events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                crate::TraceEvent::FilterDecision {
                    phase: crate::TraceFilterPhase::PostGroupMember,
                    ..
                }
            ))
            .count(),
        6
    );
    assert!(events.grouped.get() > 0);
}

#[test]
fn item_at_tokenize_keys_and_inclusive_generate_binding_use_candidate_group_and_member_positions() {
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(30, constant(Value::String("A|B|A".into())));
    p.graph
        .nodes
        .insert(31, constant(Value::String("|".into())));
    p.graph.nodes.insert(
        90,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(
        40,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Tokenize {
                input: 30,
                delimiter: 31,
                item: 90,
            },
            index: 3,
        },
    );
    controls(&mut p).group_adjacent_by = Some(40);
    let source = group(vec![
        ("Marker", text("P.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                row(Value::String("ignored".into()), "one", true, 1),
                row(Value::String("ignored".into()), "two", true, 2),
                row(Value::String("ignored".into()), "three", true, 3),
            ]),
        ),
    ]);
    assert_eq!(
        crate::run(&p, &source),
        Ok(output(vec![
            expected_group(1, "one", Some("P.root"), &["one"]),
            expected_group(2, "two", Some("P.root"), &["two"]),
            expected_group(3, "three", Some("P.root"), &["three"]),
        ]))
    );
    controls(&mut p).group_adjacent_by = Some(1);
    p.graph.nodes.insert(30, constant(Value::Int(10)));
    p.graph.nodes.insert(31, constant(Value::Int(12)));
    // I02 uses the actual inclusive, unit-step Generate shape (no invented fields).
    p.graph.nodes.insert(
        40,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: Some(30),
                to: 31,
                item: 90,
            },
            index: 3,
        },
    );
    controls(&mut p).bindings = vec![binding("Generated", 40), binding("GroupPosition", 3)];
    controls(&mut p).children[0].bindings =
        vec![binding("Generated", 40), binding("MemberPosition", 3)];
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![
                    SchemaNode::scalar("Generated", ScalarType::Int),
                    SchemaNode::scalar("GroupPosition", ScalarType::Int),
                    SchemaNode::group(
                        "Members",
                        vec![
                            SchemaNode::scalar("Generated", ScalarType::Int),
                            SchemaNode::scalar("MemberPosition", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let expected = output(
        [
            (1, Value::Int(10), &[10][..]),
            (2, Value::Int(11), &[10][..]),
            (3, Value::Int(12), &[10, 11][..]),
            (4, Value::Null, &[10][..]),
            (5, Value::Null, &[10][..]),
        ]
        .into_iter()
        .map(|(ordinal, value, members)| {
            group(vec![
                ("Generated", scalar(value)),
                ("GroupPosition", int(ordinal)),
                (
                    "Members",
                    Instance::Repeated(
                        members
                            .iter()
                            .enumerate()
                            .map(|(index, value)| {
                                group(vec![
                                    ("Generated", int(*value)),
                                    ("MemberPosition", int(index as i64 + 1)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ])
        })
        .collect(),
    );
    assert_eq!(run(&p), Ok(expected));
}

#[test]
fn item_at_parent_window_uses_exact_named_root_and_empty_producer_still_evaluates_index() {
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(30, constant(Value::String("0|2".into())));
    p.graph
        .nodes
        .insert(31, constant(Value::String("|".into())));
    p.graph
        .nodes
        .insert(32, field(named("Rows"), &["Index"], Some(&[])));
    p.graph.nodes.insert(
        90,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(
        40,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Tokenize {
                input: 30,
                delimiter: 31,
                item: 90,
            },
            index: 32,
        },
    );
    controls(&mut p).sort_by = Some(6);
    controls(&mut p).windows = vec![SequenceWindow::First { count: 40 }];
    let mut primary = input("P");
    let Instance::Group(fields) = &mut primary else {
        unreachable!()
    };
    fields.push(("Index".into(), int(1)));
    let mut named_input = input("N");
    let Instance::Group(fields) = &mut named_input else {
        unreachable!()
    };
    fields.push(("Index".into(), int(2)));
    assert_eq!(
        crate::run_with_sources(&p, &primary, vec![("Rows".into(), named_input)]),
        Ok(expected(
            "P",
            &[("b1", &["b1"]), ("a2", &["a2", "a3", "a1"])]
        ))
    );
    raises(&mut p, 32, "index");
    p.graph.nodes.insert(30, constant(Value::String("".into())));
    assert_eq!(run(&p), Err(exception(32, "index")));
    raises(&mut p, 30, "producer");
    assert_eq!(run(&p), Err(exception(30, "producer")));
}

#[test]
fn selected_targets_keep_exact_owner_demand_optional_until_reached_and_unknown_target_wins() {
    let mut p = project(named(" Røws💠 "));
    p.graph
        .nodes
        .insert(20, field(named("Rows"), &["Rows", "Key"], None));
    p.graph
        .nodes
        .insert(21, field(named("Rows"), &["Rows", "Label"], None));
    p.graph.nodes.insert(22, position(named("Rows"), &["Rows"]));
    p.graph
        .nodes
        .insert(24, field(named("Rows"), &["Marker"], Some(&[])));
    let mut audit = project(named("Rows")).root;
    audit.children[0].group_adjacent_by = Some(20);
    audit.children[0].bindings = vec![
        binding("First", 21),
        binding("GroupPosition", 22),
        binding("RootMarker", 24),
    ];
    audit.children[0].children[0].bindings = vec![
        binding("Label", 21),
        binding("MemberPosition", 22),
        binding("LegacyPosition", 8),
    ];
    p.extra_targets.push(NamedTarget {
        name: " Audit ".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: audit,
    });
    let execution = crate::ExecutionContext::new(Path::new("adjacent.json"));
    let u = output(vec![expected_group(
        1,
        "U-only",
        Some("U.root"),
        &["U-only"],
    )]);
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input("P"),
            vec![(" Røws💠 ".into(), input("U"))],
            &execution,
            crate::TargetSelection::Primary
        ),
        Ok(crate::SelectedTargetOutput::Primary(u.clone()))
    );
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input("P"),
            vec![("Rows".into(), input("N"))],
            &execution,
            crate::TargetSelection::Named(" Audit ")
        ),
        Ok(crate::SelectedTargetOutput::Named(crate::NamedOutput {
            name: " Audit ".into(),
            instance: five("N")
        }))
    );
    let all = crate::run_outputs_with_sources_and_context(&p, &input("P"), supplied(), &execution)
        .unwrap();
    assert_eq!(all.primary, u);
    assert_eq!(
        all.extras,
        vec![crate::NamedOutput {
            name: " Audit ".into(),
            instance: five("N")
        }]
    );
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input("P"),
            vec![],
            &execution,
            crate::TargetSelection::Named("Audit")
        ),
        Err(EngineError::UnknownTarget {
            name: "Audit".into()
        })
    );
    // An excluded binding must not demand even a declared, unavailable exact owner.
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).bindings[0].node = 21;
    p.graph
        .nodes
        .insert(21, field(named("Rows"), &["Marker"], Some(&[])));
    controls(&mut p).windows = vec![SequenceWindow::First { count: 11 }];
    assert_eq!(crate::run(&p, &input("P")), Ok(output(vec![])));
    controls(&mut p).windows.clear();
    assert_eq!(
        crate::run(&p, &input("P")),
        Err(EngineError::OwnedSourceAccess {
            node: Some(21),
            scope: None,
            source: Access::SourceUnavailable {
                owner: named("Rows")
            },
        })
    );
}

#[test]
fn primary_row_failure_precedes_every_adjacent_key_in_all_and_selected_dispatch() {
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(50, position(SourceOwner::Primary, &["Rows"]));
    call(&mut p, 51, "greater_than", &[50, 9]);
    p.graph
        .nodes
        .insert(52, constant(Value::String("primary row".into())));
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenFalse { predicate: 51 },
        message: Some(52),
    });
    raises(&mut p, 101, "key");
    controls(&mut p).group_adjacent_by = Some(101);
    p.extra_targets.push(NamedTarget {
        name: "Audit".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: p.root.clone(),
    });
    let expected = || EngineError::MappingFailure {
        rule: 1,
        message: Some("primary row".into()),
    };
    let execution = crate::ExecutionContext::new(Path::new("adjacent.json"));
    assert_eq!(run(&p), Err(expected()));
    assert_eq!(
        crate::run_outputs_with_sources_and_context(&p, &input("P"), supplied(), &execution)
            .map(|_| ()),
        Err(expected())
    );
    for selection in [
        crate::TargetSelection::Primary,
        crate::TargetSelection::Named("Audit"),
    ] {
        assert_eq!(
            crate::run_selected_target_with_sources_and_context(
                &p,
                &input("P"),
                supplied(),
                &execution,
                selection
            ),
            Err(expected())
        );
    }
    p.graph.nodes.insert(
        90,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(
        51,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 9,
                item: 90,
            },
            index: 50,
        },
    );
    // Target ItemAt permission never leaks into owned primary-row failure rules.
    assert_eq!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(51),
            function: None,
            scope: None,
            context: Domain::FailureRule,
        })
    );
}

#[test]
fn only_two_keyed_families_are_admitted_and_specialized_consumers_remain_closed() {
    for mode in 0..5 {
        let mut p = project(SourceOwner::Primary);
        controls(&mut p).group_adjacent_by = None;
        match mode {
            0 => controls(&mut p).group_starting_with = Some(5),
            1 => controls(&mut p).group_ending_with = Some(5),
            2 => controls(&mut p).group_into_blocks = Some(9),
            3 => controls(&mut p).post_group_filter = Some(5),
            _ => {
                controls(&mut p).group_by = Some(1);
                controls(&mut p).group_adjacent_by = Some(1);
            }
        }
        if mode == 0 {
            assert_eq!(
                run(&p),
                Ok(expected(
                    "P",
                    &[
                        ("a1", &["a1"]),
                        ("b1", &["b1", "a2"]),
                        ("a3", &["a3", "c1"]),
                        ("a4", &["a4"])
                    ]
                ))
            );
            continue;
        }
        assert!(matches!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                node: None,
                scope: Some(_),
                context: Domain::Grouping,
                ..
            })
        ));
    }
    let sequence = SequenceExpr::Generate {
        from: None,
        to: 9,
        item: 90,
    };
    for (node, domain) in [
        (
            Node::Lookup {
                collection: path(&["Rows"]),
                key: path(&["Key"]),
                matches: 1,
                value: path(&["Label"]),
            },
            Domain::AggregateLookup,
        ),
        (
            Node::Aggregate {
                function: mapping::AggregateOp::Count,
                collection: path(&["Rows"]),
                value: vec![],
                expression: None,
                arg: None,
            },
            Domain::AggregateLookup,
        ),
        (
            Node::CollectionFind {
                collection: path(&["Rows"]),
                predicate: 5,
                value: 2,
            },
            Domain::AggregateLookup,
        ),
        (
            Node::SequenceExists {
                sequence: sequence.clone(),
                predicate: 5,
            },
            Domain::GeneratedItem,
        ),
        (
            Node::SequenceAggregate {
                function: mapping::AggregateOp::Count,
                sequence,
                predicate: None,
                expression: None,
                arg: None,
            },
            Domain::GeneratedItem,
        ),
    ] {
        let mut p = project(SourceOwner::Primary);
        p.graph.nodes.insert(
            90,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
        p.graph.nodes.insert(40, node);
        controls(&mut p).bindings[0].node = 40;
        assert_eq!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(40),
                function: None,
                scope: None,
                context: domain,
            })
        );
    }
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 9,
        item: 90,
    });
    p.graph.nodes.insert(
        90,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: None,
            scope: Some(_),
            context: Domain::GeneratedItem,
            ..
        })
    ));
}

#[test]
fn native_work_budget_keeps_post_before_new_string_key_charge_even_for_first_zero() {
    let mut p = project(SourceOwner::Primary);
    let limit = crate::owned_context::OwnedLimits::default().metadata_bytes;
    p.graph
        .nodes
        .insert(101, constant(Value::String("k".repeat(limit + 1))));
    raises(&mut p, 105, "post");
    controls(&mut p).group_adjacent_by = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    controls(&mut p).windows = vec![SequenceWindow::First { count: 11 }];
    // Deliberately native: generated runtimes have a different late-key frontier.
    assert_eq!(run(&p), Err(exception(105, "post")));
    controls(&mut p).post_group_filter = None;
    assert!(matches!(run(&p), Err(EngineError::OwnedSourceAccess {
        node: None, scope: Some(_), source: Access::ResourceLimit { resource: "metadata bytes", limit: actual }
    }) if actual == limit));
    controls(&mut p).group_adjacent_by = None;
    controls(&mut p).group_by = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(run(&p), Err(exception(105, "post")));
}

#[test]
fn saved_ordinary_adjacent_wire_and_complete_literal_tree_remain_unchanged() {
    let mut p = project(SourceOwner::Primary);
    p.extra_sources.clear();
    controls(&mut p).iteration = ScopeIteration::Source(path(&["Rows"]));
    controls(&mut p).children[0].iteration = ScopeIteration::Source(vec![]);
    p.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: path(&["Key"]),
                frame: None,
            },
        ),
        (
            2,
            Node::SourceField {
                path: path(&["Label"]),
                frame: None,
            },
        ),
        (
            3,
            Node::Position {
                collection: path(&["Rows"]),
            },
        ),
        (
            4,
            Node::SourceField {
                path: path(&["Marker"]),
                frame: None,
            },
        ),
        (
            8,
            Node::Position {
                collection: path(&["Rows"]),
            },
        ),
    ]);
    let saved = serde_json::to_vec_pretty(&p).unwrap();
    let restored: Project = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec_pretty(&restored).unwrap(), saved);
    assert!(!String::from_utf8(saved).unwrap().contains("owned_source"));
    let source = group(vec![
        ("Marker", text("ordinary.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                row(Value::String("A".into()), "one", true, 1),
                row(Value::String("A".into()), "two", true, 2),
                row(Value::String("B".into()), "three", true, 3),
                row(Value::String("A".into()), "four", true, 4),
                row(Value::String("A".into()), "five", true, 5),
            ]),
        ),
    ]);
    let expected = output(vec![
        expected_group(1, "one", Some("ordinary.root"), &["one", "two"]),
        expected_group(2, "three", Some("ordinary.root"), &["three"]),
        expected_group(3, "four", Some("ordinary.root"), &["four", "five"]),
    ]);
    assert_eq!(crate::run(&restored, &source), Ok(expected));
}

#[test]
fn repeated_scalar_target_and_scalar_root_keep_three_tagged_adjacent_representatives() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::scalar("Input", ScalarType::String).repeating();
    p.target = SchemaNode::scalar("Output", ScalarType::String).repeating();
    p.graph.nodes = BTreeMap::from([(1, field(SourceOwner::Primary, &[], None))]);
    p.root = Scope {
        iteration: owned(SourceOwner::Primary, &[]),
        group_adjacent_by: Some(1),
        construction: mapping::ScopeConstruction::Scalar { value: 1 },
        ..Scope::default()
    };
    // Construction of a scalar scope preserves the representative tag rather
    // than using target-field String adaptation.
    let source = Instance::Repeated(vec![int(1), text("1"), int(1)]);
    assert_eq!(
        crate::run(&p, &source),
        Ok(Instance::Repeated(vec![int(1), text("1"), int(1)]))
    );
}

#[test]
fn all_four_item_at_producers_and_nested_index_execute_in_adjacent_post_candidate_context() {
    for producer in 0..4 {
        let mut p = project(SourceOwner::Primary);
        p.graph
            .nodes
            .insert(30, constant(Value::String("0|1|0|1|0|1".into())));
        p.graph
            .nodes
            .insert(31, constant(Value::String("|".into())));
        p.graph.nodes.insert(32, constant(Value::Int(1)));
        p.graph.nodes.insert(33, constant(Value::Int(6)));
        p.graph
            .nodes
            .insert(34, constant(Value::String("1".into())));
        p.graph.nodes.insert(
            90,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
        p.graph.nodes.insert(
            91,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
        p.graph.nodes.insert(
            42,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 33,
                    item: 91,
                },
                index: 3,
            },
        );
        let sequence = match producer {
            0 => SequenceExpr::Tokenize {
                input: 30,
                delimiter: 31,
                item: 90,
            },
            1 => {
                p.graph
                    .nodes
                    .insert(30, constant(Value::String("010101".into())));
                SequenceExpr::TokenizeByLength {
                    input: 30,
                    length: 32,
                    item: 90,
                }
            }
            2 => {
                p.graph
                    .nodes
                    .insert(30, constant(Value::String("0,1;0,1;0,1".into())));
                p.graph
                    .nodes
                    .insert(31, constant(Value::String("[,;]".into())));
                SequenceExpr::TokenizeRegex {
                    input: 30,
                    pattern: 31,
                    flags: None,
                    item: 90,
                }
            }
            _ => {
                p.graph.nodes.insert(30, constant(Value::Int(10)));
                p.graph.nodes.insert(31, constant(Value::Int(15)));
                p.graph.nodes.insert(34, constant(Value::Int(12)));
                SequenceExpr::Generate {
                    from: Some(30),
                    to: 31,
                    item: 90,
                }
            }
        };
        p.graph.nodes.insert(
            41,
            Node::SequenceItemAt {
                sequence,
                index: 42,
            },
        );
        call(
            &mut p,
            105,
            if producer == 3 {
                "greater_than"
            } else {
                "equal"
            },
            &[41, 34],
        );
        controls(&mut p).post_group_filter = Some(105);
        let expected = if producer == 3 {
            expected(
                "P",
                &[("a2", &["a2", "a3"]), ("c1", &["c1"]), ("a4", &["a4"])],
            )
        } else {
            expected(
                "P",
                &[("b1", &["b1"]), ("a2", &["a2", "a3"]), ("a4", &["a4"])],
            )
        };
        assert_eq!(run(&p), Ok(expected), "producer {producer}");
    }
}

#[test]
fn complete_cohort_domain_error_precedes_key_even_when_first_zero_would_hide_output() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Batches",
                vec![source_schema().child("Rows").unwrap().clone()],
            )
            .repeating(),
        ],
    );
    raises(&mut p, 101, "key");
    controls(&mut p).iteration = owned(SourceOwner::Primary, &["Batches", "Rows"]);
    controls(&mut p).group_adjacent_by = Some(101);
    controls(&mut p).windows = vec![SequenceWindow::First { count: 11 }];
    let source = group(vec![(
        "Batches",
        Instance::Repeated(vec![
            group(vec![(
                "Rows",
                Instance::Repeated(vec![row(Value::String("A".into()), "first", true, 1)]),
            )]),
            group(vec![("Rows", Instance::DocumentSet(vec![]))]),
        ]),
    )]);
    assert!(matches!(
        crate::run(&p, &source),
        Err(EngineError::UnsupportedOwnedContext {
            node: None,
            scope: Some(_),
            context: Domain::Document,
            ..
        })
    ));
}
