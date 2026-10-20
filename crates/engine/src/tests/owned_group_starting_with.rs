//! Literal native Starting controls. Expected lists never partition source data.
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, NamedSource, Node, NodeId, OwnedPosition, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SequenceExpr, SequenceWindow, SortFilterOrder,
    SourceOwner,
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
fn binding(name: &str, node: NodeId) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn constant(value: Value) -> Node {
    Node::Const { value }
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
fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Start", ScalarType::Bool),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Keep", ScalarType::Bool),
            SchemaNode::scalar("Rank", ScalarType::Int),
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::scalar("Key", ScalarType::String),
        ],
    )
    .repeating()
}
fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Marker", ScalarType::String),
            SchemaNode::scalar("Limit", ScalarType::Int),
            row_schema(),
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
                (1, field(owner.clone(), &["Rows", "Start"], None)),
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
                (12, field(owner.clone(), &["Rows", "Key"], None)),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Groups".into(),
                iteration: owned(owner.clone(), &["Rows"]),
                group_starting_with: Some(1),
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
fn row(start: bool, label: &str, keep: bool, rank: i64, key: &str) -> Instance {
    group(vec![
        ("Start", scalar(Value::Bool(start))),
        ("Label", text(label)),
        ("Keep", scalar(Value::Bool(keep))),
        ("Rank", int(rank)),
        ("Limit", int(0)),
        ("Key", text(key)),
    ])
}
fn input(owner: &str) -> Instance {
    let rows = match owner {
        "P" => vec![
            row(false, "p0", false, 30, "A"),
            row(false, "p1", true, 10, "B"),
            row(true, "p2", false, 20, "A"),
            row(true, "p3", false, 25, "A"),
            row(false, "p4", true, 40, "C"),
            row(true, "p5", false, 50, "A"),
            row(false, "p6", true, 60, "A"),
        ],
        "N" => vec![
            row(true, "n0", false, 300, "A"),
            row(false, "n1", true, 100, "B"),
            row(false, "n2", false, 200, "A"),
            row(true, "n3", false, 250, "A"),
            row(false, "n4", true, 400, "C"),
            row(false, "n5", false, 500, "A"),
            row(true, "n6", true, 600, "A"),
        ],
        "U" => vec![row(false, "u0", true, 7, "A")],
        _ => unreachable!("finite independent endpoint fixture"),
    };
    group(vec![
        ("Marker", text(&format!("{owner}.root"))),
        ("Limit", int(if owner == "N" { 1 } else { 2 })),
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
fn expected(marker: &str, runs: &[(&str, &[&str])]) -> Instance {
    output(
        runs.iter()
            .enumerate()
            .map(|(index, (first, members))| {
                expected_group(index as i64 + 1, first, Some(marker), members)
            })
            .collect(),
    )
}
fn p4() -> Instance {
    expected(
        "P.root",
        &[
            ("p0", &["p0", "p1"]),
            ("p2", &["p2"]),
            ("p3", &["p3", "p4"]),
            ("p5", &["p5", "p6"]),
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
fn first_false_prefix_consecutive_true_and_exact_owners_have_literal_complete_products() {
    assert_eq!(run(&project(SourceOwner::Primary)), Ok(p4()));
    assert_eq!(
        run(&project(named("Rows"))),
        Ok(expected(
            "N.root",
            &[
                ("n0", &["n0", "n1", "n2"]),
                ("n3", &["n3", "n4", "n5"]),
                ("n6", &["n6"]),
            ]
        ))
    );
    assert_eq!(
        run(&project(named(" Røws💠 "))),
        Ok(expected("U.root", &[("u0", &["u0"])]))
    );
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(101, constant(Value::Bool(false)));
    controls(&mut p).group_starting_with = Some(101);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p0", &["p0", "p1", "p2", "p3", "p4", "p5", "p6"])]
        ))
    );
    p.graph.nodes.insert(101, constant(Value::Bool(true)));
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0"]),
                ("p1", &["p1"]),
                ("p2", &["p2"]),
                ("p3", &["p3"]),
                ("p4", &["p4"]),
                ("p5", &["p5"]),
                ("p6", &["p6"]),
            ]
        ))
    );
    controls(&mut p).group_starting_with = Some(1);
    let source = group(vec![
        ("Marker", text("P.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                row(true, "p2", false, 20, "A"),
                row(true, "p3", false, 25, "A"),
                row(false, "p4", true, 40, "C"),
            ]),
        ),
    ]);
    assert_eq!(
        crate::run(&p, &source),
        Ok(expected(
            "P.root",
            &[("p2", &["p2"]), ("p3", &["p3", "p4"])]
        ))
    );
}

#[test]
fn post_retains_false_representatives_without_joining_removed_boundary_runs() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).post_group_filter = Some(5);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0", "p1"]),
                ("p3", &["p3", "p4"]),
                ("p5", &["p5", "p6"]),
            ]
        ))
    );
    p.graph.nodes.insert(105, constant(Value::Bool(false)));
    controls(&mut p).post_group_filter = Some(105);
    raises(&mut p, 106, "target");
    controls(&mut p).bindings[0].node = 106;
    assert_eq!(run(&p), Ok(output(vec![])));
    controls(&mut p).bindings[0].node = 2;
    controls(&mut p).post_group_filter = None;
    p.graph.nodes.insert(20, constant(Value::Bool(false)));
    call(&mut p, 21, "equal", &[1, 20]);
    controls(&mut p).filter = Some(21);
    assert_eq!(
        run(&p),
        Ok(expected("P.root", &[("p0", &["p0", "p1", "p4", "p6"])]))
    );
}

#[test]
fn stable_sort_filter_order_and_boundary_post_positions_preserve_selected_gaps() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).sort_by = Some(6);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p1", &["p1"]),
                ("p2", &["p2"]),
                ("p3", &["p3", "p0", "p4"]),
                ("p5", &["p5", "p6"]),
            ]
        ))
    );
    call(&mut p, 20, "greater_than", &[3, 9]);
    controls(&mut p).filter = Some(20);
    controls(&mut p).sort_filter_order = SortFilterOrder::FilterThenSort;
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p2", &["p2"]),
                ("p3", &["p3", "p4"]),
                ("p5", &["p5", "p6"]),
            ]
        ))
    );
    controls(&mut p).sort_filter_order = SortFilterOrder::SortThenFilter;
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p3", &["p3", "p0", "p4"]), ("p5", &["p5", "p6"]),]
        ))
    );
    p.graph.nodes.insert(21, constant(Value::Int(4)));
    call(&mut p, 22, "equal", &[3, 21]);
    controls(&mut p).post_group_filter = Some(22);
    assert_eq!(
        run(&p),
        Ok(expected("P.root", &[("p3", &["p3", "p0", "p4"])]))
    );
    controls(&mut p).filter = None;
    controls(&mut p).post_group_filter = None;
    p.graph.nodes.insert(21, constant(Value::Int(3)));
    call(&mut p, 22, "less_or_equal", &[3, 21]);
    controls(&mut p).group_starting_with = Some(22);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p1", &["p1"]),
                ("p2", &["p2"]),
                ("p3", &["p3", "p0", "p4", "p5", "p6"]),
            ]
        ))
    );
}

#[test]
fn sort_keys_use_raw_positions_and_parent_bounds_apply_only_to_groups() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).sort_by = Some(3);
    controls(&mut p).sort_descending = true;
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p6", &["p6"]),
                ("p5", &["p5", "p4"]),
                ("p3", &["p3"]),
                ("p2", &["p2", "p1", "p0"]),
            ]
        ))
    );
    controls(&mut p).sort_by = Some(6);
    controls(&mut p).sort_descending = false;
    controls(&mut p).windows = vec![SequenceWindow::First { count: 7 }];
    assert_eq!(
        run(&p),
        Ok(expected("P.root", &[("p1", &["p1"]), ("p2", &["p2"])]))
    );
    controls(&mut p).sort_by = None;
    controls(&mut p).windows = vec![
        SequenceWindow::SkipFirst { count: 10 },
        SequenceWindow::Last { count: 9 },
    ];
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p3", &["p3", "p4"]), ("p5", &["p5", "p6"])]
        ))
    );
    controls(&mut p).windows.reverse();
    assert_eq!(run(&p), Ok(expected("P.root", &[("p5", &["p5", "p6"])])));
    p.graph.nodes.insert(20, constant(Value::Int(3)));
    controls(&mut p).windows = vec![SequenceWindow::FromTo { first: 9, last: 20 }];
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p2", &["p2"]), ("p3", &["p3", "p4"])]
        ))
    );
}

#[test]
fn first_and_empty_group_shapes_do_not_skip_controls_or_reach_excluded_targets() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).iteration_output = IterationOutput::First;
    let mut schema = groups_schema(true);
    schema.repeating = false;
    p.target = SchemaNode::group("Output", vec![schema]);
    assert_eq!(
        run(&p),
        Ok(group(vec![(
            "Groups",
            expected_group(1, "p0", Some("P.root"), &["p0", "p1"])
        )]))
    );
    assert_eq!(
        crate::run(&p, &group(vec![("Rows", Instance::Repeated(vec![]))])),
        Ok(group(vec![("Groups", group(vec![]))]))
    );
    controls(&mut p).windows = vec![SequenceWindow::First { count: 11 }];
    raises(&mut p, 106, "target");
    controls(&mut p).bindings[0].node = 106;
    assert_eq!(run(&p), Ok(group(vec![("Groups", group(vec![]))])));
    raises(&mut p, 101, "boundary");
    controls(&mut p).group_starting_with = Some(101);
    assert_eq!(run(&p), Err(exception(101, "boundary")));
    controls(&mut p).group_starting_with = Some(1);
    raises(&mut p, 105, "post");
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(run(&p), Err(exception(105, "post")));
}

#[test]
fn first_boundary_bool_failure_and_raise_precede_post_with_exact_nodes_and_tags() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 101, "boundary");
    raises(&mut p, 105, "post");
    controls(&mut p).group_starting_with = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(run(&p), Err(exception(101, "boundary")));
    for (value, found) in [
        (Value::String("true".into()), "string"),
        (Value::Null, "null"),
        (Value::JsonNull(ir::JsonNull), "json null"),
        (Value::Int(1), "int"),
        (Value::Float(1.0), "float"),
        (Value::XmlNil(ir::XmlNil), "xml nil"),
    ] {
        p.graph.nodes.insert(101, constant(value));
        assert_eq!(run(&p), Err(EngineError::NotABool { node: 101, found }));
    }
    p.graph.nodes.insert(101, constant(Value::Bool(false)));
    assert_eq!(run(&p), Err(exception(105, "post")));
    p.graph
        .nodes
        .insert(105, constant(Value::String("false".into())));
    assert_eq!(
        run(&p),
        Err(EngineError::NotABool {
            node: 105,
            found: "string"
        })
    );
}

#[test]
fn fused_boundary_post_before_later_filter_and_eager_sorted_phases_keep_error_precedence() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 101, "boundary");
    raises(&mut p, 102, "filter");
    raises(&mut p, 103, "sort");
    raises(&mut p, 104, "bound");
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
    controls(&mut p).group_starting_with = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    controls(&mut p).sort_filter_order = SortFilterOrder::FilterThenSort;
    assert_eq!(run(&p), Err(exception(101, "boundary")));
    controls(&mut p).group_starting_with = Some(1);
    assert_eq!(run(&p), Err(exception(105, "post")));
    controls(&mut p).sort_by = Some(103);
    controls(&mut p).windows = vec![SequenceWindow::First { count: 104 }];
    assert_eq!(run(&p), Err(exception(102, "filter")));
    controls(&mut p).sort_filter_order = SortFilterOrder::SortThenFilter;
    assert_eq!(run(&p), Err(exception(103, "sort")));
    controls(&mut p).sort_by = Some(6);
    assert_eq!(run(&p), Err(exception(104, "bound")));
}

#[test]
fn retained_true_does_not_short_circuit_post_and_empty_windows_do_not_short_circuit_bounds() {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(101, constant(Value::Bool(false)));
    controls(&mut p).group_starting_with = Some(101);
    raises(&mut p, 105, "post");
    call(&mut p, 20, "greater_than", &[3, 10]);
    p.graph.nodes.insert(21, constant(Value::Bool(true)));
    p.graph.nodes.insert(
        22,
        Node::If {
            condition: 20,
            then: 105,
            else_: 21,
        },
    );
    controls(&mut p).post_group_filter = Some(22);
    assert_eq!(run(&p), Err(exception(105, "post")));
    p.graph.nodes.insert(104, constant(Value::Bool(true)));
    controls(&mut p).windows = vec![SequenceWindow::First { count: 104 }];
    assert_eq!(
        crate::run(&p, &group(vec![("Rows", Instance::Repeated(vec![]))])),
        Err(EngineError::NotAnItemCount {
            node: 104,
            found: "bool"
        })
    );
    raises(&mut p, 104, "bound");
    controls(&mut p).windows = vec![
        SequenceWindow::First { count: 11 },
        SequenceWindow::Last { count: 104 },
    ];
    assert_eq!(run(&p), Err(exception(104, "bound")));
    raises(&mut p, 107, "last bound");
    controls(&mut p).windows = vec![SequenceWindow::FromTo {
        first: 104,
        last: 107,
    }];
    assert_eq!(run(&p), Err(exception(104, "bound")));
    controls(&mut p).windows = vec![SequenceWindow::FromTo {
        first: 10,
        last: 107,
    }];
    assert_eq!(run(&p), Err(exception(107, "last bound")));
}

fn root_fixture(
    owner: SourceOwner,
    alias: bool,
    declared_repeating: bool,
) -> (Project, Instance, Vec<(String, Instance)>) {
    let mut p = project(owner.clone());
    let mut schema = row_schema();
    schema.repeating = declared_repeating;
    let rows = Instance::Repeated(vec![
        row(false, "a", true, 0, "A"),
        row(true, "b", true, 0, "A"),
        row(true, "c", true, 0, "A"),
        row(false, "d", true, 0, "A"),
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
        (1, field(owner.clone(), &["Start"], None)),
        (2, field(owner.clone(), &["Label"], None)),
        (3, position(owner.clone(), &[])),
        (8, Node::Position { collection: vec![] }),
        (41, field(owner.clone(), &["Label"], Some(&[]))),
    ]);
    let scope = controls(&mut p);
    scope.iteration = if alias {
        ScopeIteration::Source(path(&["Alias"]))
    } else {
        owned(owner.clone(), &[])
    };
    scope.bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
    scope.children[0].iteration = owned(owner, &[]);
    scope.children[0].bindings[0].node = 41;
    p.target = SchemaNode::group("Output", vec![groups_schema(false)]);
    (p, primary, supplied)
}
fn root_expected() -> Instance {
    output(vec![
        expected_group(1, "a", None, &["a"]),
        expected_group(2, "b", None, &["b"]),
        expected_group(3, "c", None, &["c", "d"]),
    ])
}

#[test]
fn root_members_latest_item_and_actual_legacy_alias_bridge_preserve_exact_identity() {
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
            controls(&mut p).group_starting_with = None;
            controls(&mut p).bindings[0].node = 41;
            controls(&mut p).children.clear();
            assert_eq!(
                crate::run_with_sources(&p, &primary, supplied),
                Ok(output(vec![
                    group(vec![("First", text("a")), ("GroupPosition", int(1))]),
                    group(vec![("First", text("b")), ("GroupPosition", int(2))]),
                    group(vec![("First", text("c")), ("GroupPosition", int(3))]),
                    group(vec![("First", text("d")), ("GroupPosition", int(4))]),
                ]))
            );
        }
    }
}

#[test]
fn synthetic_root_empty_pin_is_immutable_for_both_schema_flags_and_all_three_routes() {
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
                        path: path(&["Label"])
                    },
                })
            );
        }
    }
}

#[test]
fn foreign_root_item_does_not_shadow_primary_group_and_singleton_roots_still_pin() {
    let (mut p, primary, _) = root_fixture(SourceOwner::Primary, false, true);
    p.extra_sources = vec![NamedSource {
        name: "Alias".into(),
        path: "root.json".into(),
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
                Instance::Repeated(vec![row(false, "foreign", true, 0, "A"),])
            )]
        ),
        Err(EngineError::OwnedSourceAccess {
            node: Some(41),
            scope: None,
            source: Access::UniteratedOwnedRepetition {
                path: path(&["Label"])
            },
        })
    );
    for owner in [SourceOwner::Primary, named("Alias")] {
        let (mut p, _, _) = root_fixture(owner.clone(), false, false);
        controls(&mut p).bindings[0].node = 41;
        let singleton = row(false, "single", true, 0, "A");
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
fn nonempty_group_root_pin_and_uniterated_descendants_do_not_gain_representative_permission() {
    let mut p = project(SourceOwner::Primary);
    p.source.repeating = true;
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
    assert_eq!(run(&p), Ok(p4()));
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
            ("Start", scalar(Value::Bool(false))),
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
fn starting_only_or_post_only_owned_demand_in_legacy_scope_is_isolated_and_authenticates_children()
{
    for boundary_only in [true, false] {
        let mut p = project(SourceOwner::Primary);
        p.graph.nodes = BTreeMap::from([
            (
                1,
                if boundary_only {
                    field(SourceOwner::Primary, &["Rows", "Start"], None)
                } else {
                    Node::SourceField {
                        path: path(&["Start"]),
                        frame: Some(path(&["Rows"])),
                    }
                },
            ),
            (
                2,
                Node::SourceField {
                    path: path(&["Label"]),
                    frame: Some(path(&["Rows"])),
                },
            ),
            (
                3,
                Node::Position {
                    collection: path(&["Rows"]),
                },
            ),
        ]);
        if !boundary_only {
            p.graph
                .nodes
                .insert(5, field(SourceOwner::Primary, &["Rows", "Keep"], None));
            call(&mut p, 105, "equal", &[5, 5]);
            controls(&mut p).post_group_filter = Some(105);
        }
        controls(&mut p).iteration = ScopeIteration::Source(path(&["Rows"]));
        controls(&mut p).bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
        controls(&mut p).children[0].iteration = ScopeIteration::Source(vec![]);
        controls(&mut p).children[0].bindings =
            vec![binding("Label", 2), binding("MemberPosition", 3)];
        p.target = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("GroupPosition", ScalarType::Int),
                        SchemaNode::group(
                            "Members",
                            vec![
                                SchemaNode::scalar("Label", ScalarType::String),
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
                (1, "p0", &["p0", "p1"][..]),
                (2, "p2", &["p2"][..]),
                (3, "p3", &["p3", "p4"][..]),
                (4, "p5", &["p5", "p6"][..]),
            ]
            .into_iter()
            .map(|(ordinal, first, labels)| {
                group(vec![
                    ("First", text(first)),
                    ("GroupPosition", int(ordinal)),
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
        let required =
            crate::required_sources_for_target(&p, crate::TargetSelection::Primary).unwrap();
        assert_eq!(
            required
                .static_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Rows"]
        );
        // This child has no owned expression. Only inherited grouped ancestry
        // makes the still-closed Ending operation a scope-associated refusal.
        p.graph.nodes.insert(20, constant(Value::Bool(true)));
        controls(&mut p).children[0].group_ending_with = Some(20);
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
}

#[test]
fn nested_starting_and_keyed_modes_reuse_only_current_group_members() {
    for inner_mode in 0..3 {
        let mut p = project(SourceOwner::Primary);
        p.graph.nodes.insert(20, constant(Value::Bool(false)));
        controls(&mut p).group_starting_with = Some(20);
        controls(&mut p).children.clear();
        let mut inner = project(SourceOwner::Primary).root.children.remove(0);
        inner.target_field = "Inner".into();
        inner.group_starting_with = None;
        let runs: &[(&str, &[&str])] = match inner_mode {
            0 => {
                inner.group_by = Some(12);
                &[
                    ("p0", &["p0", "p2", "p3", "p5", "p6"]),
                    ("p1", &["p1"]),
                    ("p4", &["p4"]),
                ]
            }
            1 => {
                inner.group_adjacent_by = Some(12);
                &[
                    ("p0", &["p0"]),
                    ("p1", &["p1"]),
                    ("p2", &["p2", "p3"]),
                    ("p4", &["p4"]),
                    ("p5", &["p5", "p6"]),
                ]
            }
            _ => {
                inner.group_starting_with = Some(20);
                p.graph.nodes.insert(21, constant(Value::Int(3)));
                controls(&mut p).group_starting_with = Some(1);
                controls(&mut p).windows = vec![SequenceWindow::FromTo {
                    first: 21,
                    last: 21,
                }];
                &[("p3", &["p3", "p4"])]
            }
        };
        let outer_first = if inner_mode == 2 { "p3" } else { "p0" };
        controls(&mut p).bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
        controls(&mut p).children.push(inner);
        let mut inner_schema = groups_schema(true);
        inner_schema.name = "Inner".into();
        p.target = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("GroupPosition", ScalarType::Int),
                        inner_schema,
                    ],
                )
                .repeating(),
            ],
        );
        let inner_expected = expected("P.root", runs).field("Groups").unwrap().clone();
        assert_eq!(
            run(&p),
            Ok(output(vec![group(vec![
                ("First", text(outer_first)),
                ("GroupPosition", int(1)),
                ("Inner", inner_expected),
            ])]))
        );
    }
}

#[test]
fn flattened_groups_preserve_first_batch_prefix_for_cross_batch_members() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Batches",
                vec![
                    SchemaNode::scalar("BatchName", ScalarType::String),
                    row_schema(),
                ],
            )
            .repeating(),
        ],
    );
    controls(&mut p).iteration = owned(SourceOwner::Primary, &["Batches", "Rows"]);
    controls(&mut p).children[0].iteration = owned(SourceOwner::Primary, &["Batches", "Rows"]);
    p.graph.nodes.insert(
        1,
        field(SourceOwner::Primary, &["Batches", "Rows", "Start"], None),
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
    controls(&mut p).bindings[2].target_field = "BatchName".into();
    controls(&mut p).children[0].bindings = vec![
        binding("Label", 2),
        binding("MemberPosition", 3),
        binding("BatchName", 4),
    ];
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![
                    SchemaNode::scalar("First", ScalarType::String),
                    SchemaNode::scalar("GroupPosition", ScalarType::Int),
                    SchemaNode::scalar("BatchName", ScalarType::String),
                    SchemaNode::group(
                        "Members",
                        vec![
                            SchemaNode::scalar("Label", ScalarType::String),
                            SchemaNode::scalar("MemberPosition", ScalarType::Int),
                            SchemaNode::scalar("BatchName", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
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
                    row(false, "x1", true, 0, "A"),
                    row(true, "x2", true, 0, "A"),
                ],
            ),
            batch(
                "Y",
                vec![
                    row(false, "y1", true, 0, "A"),
                    row(true, "y2", true, 0, "A"),
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
                ("BatchName", text(batch)),
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

#[derive(Default)]
struct Events {
    events: RefCell<Vec<crate::TraceEvent>>,
    grouped: Cell<usize>,
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
fn starting_trace_has_no_key_and_preserves_constructed_ordinals_boundary_then_post() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).post_group_filter = Some(5);
    let events = Events::default();
    let execution =
        crate::ExecutionContext::new(Path::new("starting.json")).with_trace_sink(&events);
    assert_eq!(
        crate::run_with_sources_and_context(&p, &input("P"), supplied(), &execution),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0", "p1"]),
                ("p3", &["p3", "p4"]),
                ("p5", &["p5", "p6"])
            ]
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
                key,
                retained,
                ..
            } => {
                assert!(key.is_none());
                Some((*grouping, *group_index, *member_count, *retained))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        groups,
        vec![
            (crate::TraceGrouping::StartingWith { node: 1 }, 1, 2, true),
            (crate::TraceGrouping::StartingWith { node: 1 }, 2, 1, false),
            (crate::TraceGrouping::StartingWith { node: 1 }, 3, 2, true),
            (crate::TraceGrouping::StartingWith { node: 1 }, 4, 2, true),
        ]
    );
    let phases = events
        .events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            crate::TraceEvent::FilterDecision { phase, .. }
                if matches!(
                    phase,
                    crate::TraceFilterPhase::GroupStarting
                        | crate::TraceFilterPhase::PostGroupMember
                ) =>
            {
                Some(*phase)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            crate::TraceFilterPhase::GroupStarting,
            crate::TraceFilterPhase::PostGroupMember
        ]
        .repeat(7)
    );
    assert!(events.grouped.get() > 0);
}

#[test]
fn null_pruning_preserves_raw_boundary_positions_and_first_zero_still_runs_all_controls() {
    let mut p = project(SourceOwner::Primary);
    controls(&mut p).post_group_filter = Some(5);
    let source = group(vec![
        ("Marker", text("P.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                row(false, "a", true, 0, "A"),
                scalar(Value::Null),
                row(false, "b", true, 0, "A"),
                scalar(Value::JsonNull(ir::JsonNull)),
                row(true, "c", true, 0, "A"),
                row(false, "d", true, 0, "A"),
            ]),
        ),
    ]);
    let events = Events::default();
    let execution =
        crate::ExecutionContext::new(Path::new("starting.json")).with_trace_sink(&events);
    assert_eq!(
        crate::run_with_sources_and_context(&p, &source, vec![], &execution),
        Ok(expected(
            "P.root",
            &[("a", &["a", "b"]), ("c", &["c", "d"]),]
        ))
    );
    let positions = events
        .events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            crate::TraceEvent::FilterDecision {
                phase: crate::TraceFilterPhase::GroupStarting,
                positions,
                ..
            } => positions.last().map(|position| position.index),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, vec![1, 3, 5, 6]);
    events.events.borrow_mut().clear();
    p.graph.nodes.insert(11, constant(Value::Int(0)));
    controls(&mut p).windows = vec![SequenceWindow::First { count: 11 }];
    raises(&mut p, 106, "target");
    controls(&mut p).bindings[0].node = 106;
    assert_eq!(
        crate::run_with_sources_and_context(&p, &input("P"), supplied(), &execution),
        Ok(output(vec![]))
    );
    assert_eq!(
        events
            .events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                crate::TraceEvent::FilterDecision {
                    phase: crate::TraceFilterPhase::GroupStarting,
                    ..
                }
            ))
            .count(),
        7
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
        7
    );
}

#[test]
fn boundary_bool_produces_no_native_group_key_text_or_successful_string_frontier() {
    let mut p = project(SourceOwner::Primary);
    let limit = crate::owned_context::OwnedLimits::default().metadata_bytes;
    p.graph
        .nodes
        .insert(100, constant(Value::String("x".repeat(limit + 1))));
    controls(&mut p).group_starting_with = Some(100);
    raises(&mut p, 105, "post");
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(
        crate::run(&p, &input("U")),
        Err(EngineError::NotABool {
            node: 100,
            found: "string"
        })
    );
    call(&mut p, 101, "equal", &[100, 100]);
    controls(&mut p).group_starting_with = Some(101);
    controls(&mut p).post_group_filter = None;
    // Temporary expression strings are not returned grouping keys. The actual
    // boundary is Bool and cannot consume retained-key text admission.
    assert_eq!(
        crate::run(&p, &input("U")),
        Ok(expected("U.root", &[("u0", &["u0"])]))
    );
}

#[test]
fn repeated_scalar_bool_roots_keep_consecutive_true_starts_and_complete_member_payloads() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::scalar("Input", ScalarType::Bool).repeating();
    p.target = SchemaNode::scalar("Output", ScalarType::Bool).repeating();
    p.graph.nodes = BTreeMap::from([
        (1, field(SourceOwner::Primary, &[], None)),
        (3, position(SourceOwner::Primary, &[])),
    ]);
    p.root = Scope {
        iteration: owned(SourceOwner::Primary, &[]),
        group_starting_with: Some(1),
        construction: mapping::ScopeConstruction::Scalar { value: 1 },
        ..Scope::default()
    };
    let source = Instance::Repeated(vec![
        scalar(Value::Bool(false)),
        scalar(Value::Bool(true)),
        scalar(Value::Bool(true)),
        scalar(Value::Bool(false)),
    ]);
    assert_eq!(
        crate::run(&p, &source),
        Ok(Instance::Repeated(vec![
            scalar(Value::Bool(false)),
            scalar(Value::Bool(true)),
            scalar(Value::Bool(true))
        ]))
    );
    p.root.construction = mapping::ScopeConstruction::Constructed;
    p.root.bindings = vec![binding("First", 1), binding("GroupPosition", 3)];
    p.root.children = vec![Scope {
        target_field: "Members".into(),
        iteration: owned(SourceOwner::Primary, &[]),
        construction: mapping::ScopeConstruction::Scalar { value: 1 },
        ..Scope::default()
    }];
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("First", ScalarType::Bool),
            SchemaNode::scalar("GroupPosition", ScalarType::Int),
            SchemaNode::scalar("Members", ScalarType::Bool).repeating(),
        ],
    )
    .repeating();
    assert_eq!(
        crate::run(&p, &source),
        Ok(Instance::Repeated(vec![
            group(vec![
                ("First", scalar(Value::Bool(false))),
                ("GroupPosition", int(1)),
                (
                    "Members",
                    Instance::Repeated(vec![scalar(Value::Bool(false))])
                )
            ]),
            group(vec![
                ("First", scalar(Value::Bool(true))),
                ("GroupPosition", int(2)),
                (
                    "Members",
                    Instance::Repeated(vec![scalar(Value::Bool(true))])
                )
            ]),
            group(vec![
                ("First", scalar(Value::Bool(true))),
                ("GroupPosition", int(3)),
                (
                    "Members",
                    Instance::Repeated(vec![scalar(Value::Bool(true)), scalar(Value::Bool(false))])
                )
            ]),
        ]))
    );
}

#[test]
fn ending_blocks_conflicts_and_specialized_consumers_remain_closed_under_starting_ancestry() {
    for mode in 0..5 {
        let mut p = project(SourceOwner::Primary);
        match mode {
            0 => {
                controls(&mut p).group_starting_with = None;
                controls(&mut p).group_ending_with = Some(1);
            }
            1 => {
                controls(&mut p).group_starting_with = None;
                controls(&mut p).group_into_blocks = Some(9);
            }
            2 => controls(&mut p).group_by = Some(12),
            3 => controls(&mut p).group_adjacent_by = Some(12),
            _ => {
                controls(&mut p).group_starting_with = None;
                controls(&mut p).post_group_filter = Some(5);
            }
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
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(
        40,
        Node::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Key"]),
            matches: 12,
            value: path(&["Label"]),
        },
    );
    controls(&mut p).bindings[0].node = 40;
    assert_eq!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(40),
            function: None,
            scope: None,
            context: Domain::AggregateLookup,
        })
    );
    controls(&mut p).bindings[0].node = 2;
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
fn exact_absent_foreign_and_unknown_owners_are_not_rescued_by_primary_group_payloads() {
    let p = project(named("Rows"));
    assert!(
        matches!(crate::run_with_sources(&p, &input("P"), vec![(" Røws💠 ".into(), input("U"))]),
        Err(EngineError::OwnedSourceAccess { node: None, scope: Some(_), source: Access::SourceUnavailable { owner } })
            if owner == named("Rows"))
    );
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

#[test]
fn item_at_boundary_uses_each_candidate_and_generate_binding_uses_group_or_member_ordinal() {
    for producer in 0..4 {
        let mut p = project(SourceOwner::Primary);
        p.graph
            .nodes
            .insert(30, constant(Value::String("F|F|T|T|F|T|F".into())));
        p.graph
            .nodes
            .insert(31, constant(Value::String("|".into())));
        p.graph
            .nodes
            .insert(32, constant(Value::String("T".into())));
        p.graph.nodes.insert(33, constant(Value::Int(1)));
        p.graph.nodes.insert(34, constant(Value::Int(7)));
        p.graph.nodes.insert(
            90,
            Node::SourceField {
                path: vec![],
                frame: None,
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
                    .insert(30, constant(Value::String("FFTTFTF".into())));
                SequenceExpr::TokenizeByLength {
                    input: 30,
                    length: 33,
                    item: 90,
                }
            }
            2 => {
                p.graph
                    .nodes
                    .insert(31, constant(Value::String("\\|".into())));
                SequenceExpr::TokenizeRegex {
                    input: 30,
                    pattern: 31,
                    flags: None,
                    item: 90,
                }
            }
            _ => SequenceExpr::Generate {
                from: Some(33),
                to: 34,
                item: 90,
            },
        };
        p.graph
            .nodes
            .insert(40, Node::SequenceItemAt { sequence, index: 3 });
        call(
            &mut p,
            101,
            if producer == 3 {
                "greater_than"
            } else {
                "equal"
            },
            if producer == 3 { &[40, 9] } else { &[40, 32] },
        );
        controls(&mut p).group_starting_with = Some(101);
        let expected = if producer == 3 {
            expected(
                "P.root",
                &[
                    ("p0", &["p0", "p1"]),
                    ("p2", &["p2"]),
                    ("p3", &["p3"]),
                    ("p4", &["p4"]),
                    ("p5", &["p5"]),
                    ("p6", &["p6"]),
                ],
            )
        } else {
            p4()
        };
        assert_eq!(run(&p), Ok(expected), "producer {producer}");
    }
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(30, constant(Value::Int(10)));
    p.graph.nodes.insert(31, constant(Value::Int(12)));
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
            sequence: SequenceExpr::Generate {
                from: Some(30),
                to: 31,
                item: 90,
            },
            index: 3,
        },
    );
    controls(&mut p).bindings = vec![binding("Value", 40)];
    controls(&mut p).children.clear();
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Groups", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    assert_eq!(
        run(&p),
        Ok(output(vec![
            group(vec![("Value", int(10))]),
            group(vec![("Value", int(11))]),
            group(vec![("Value", int(12))]),
            group(vec![("Value", scalar(Value::Null))])
        ]))
    );
    controls(&mut p).bindings.clear();
    controls(&mut p).children.push(Scope {
        target_field: "Members".into(),
        iteration: owned(SourceOwner::Primary, &["Rows"]),
        bindings: vec![binding("Value", 40)],
        ..Scope::default()
    });
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![
                    SchemaNode::group(
                        "Members",
                        vec![SchemaNode::scalar("Value", ScalarType::Int)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let one = || group(vec![("Value", int(10))]);
    let two = || group(vec![("Value", int(11))]);
    assert_eq!(
        run(&p),
        Ok(output(vec![
            group(vec![("Members", Instance::Repeated(vec![one(), two()]))]),
            group(vec![("Members", Instance::Repeated(vec![one()]))]),
            group(vec![("Members", Instance::Repeated(vec![one(), two()]))]),
            group(vec![("Members", Instance::Repeated(vec![one(), two()]))]),
        ]))
    );
}

#[test]
fn nested_cross_owner_starting_groups_preserve_outer_frames_and_independent_inner_domains() {
    for (outer_owner, inner_owner, outer_firsts, inner_marker, inner_runs) in [
        (
            SourceOwner::Primary,
            named("Rows"),
            ["p0", "p2"],
            "N.root",
            &[
                ("n0", &["n0", "n1", "n2"][..]),
                ("n3", &["n3", "n4", "n5"][..]),
                ("n6", &["n6"][..]),
            ][..],
        ),
        (
            named("Rows"),
            SourceOwner::Primary,
            ["n0", "n3"],
            "P.root",
            &[
                ("p0", &["p0", "p1"][..]),
                ("p2", &["p2"][..]),
                ("p3", &["p3", "p4"][..]),
                ("p5", &["p5", "p6"][..]),
            ][..],
        ),
    ] {
        let mut p = project(outer_owner);
        controls(&mut p).windows = vec![SequenceWindow::First { count: 9 }];
        p.graph
            .nodes
            .insert(20, field(inner_owner.clone(), &["Rows", "Start"], None));
        p.graph.nodes.insert(
            21,
            field(inner_owner.clone(), &["Rows", "Label"], Some(&["Rows"])),
        );
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
                group_starting_with: Some(20),
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
            outer_firsts
                .into_iter()
                .enumerate()
                .map(|(index, first)| {
                    group(vec![
                        ("OuterFirst", text(first)),
                        ("OuterPosition", int(index as i64 + 1)),
                        ("Inner", expected(inner_marker, inner_runs)),
                    ])
                })
                .collect(),
        );
        assert_eq!(run(&p), Ok(expected));
    }
}

#[test]
fn active_batch_bound_uses_receiving_parent_not_row_or_registered_root() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::group(
                "Batches",
                vec![SchemaNode::scalar("Limit", ScalarType::Int), row_schema()],
            )
            .repeating(),
        ],
    );
    p.graph.nodes.insert(
        1,
        field(SourceOwner::Primary, &["Batches", "Rows", "Start"], None),
    );
    p.graph.nodes.insert(
        2,
        field(SourceOwner::Primary, &["Batches", "Rows", "Label"], None),
    );
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
                        row(false, "x1", true, 0, "A"),
                        row(true, "x2", true, 0, "A"),
                        row(false, "x3", true, 0, "A"),
                    ],
                ),
                batch(
                    2,
                    vec![
                        row(false, "y1", true, 0, "A"),
                        row(true, "y2", true, 0, "A"),
                        row(true, "y3", true, 0, "A"),
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

#[test]
fn primary_row_failures_still_precede_starting_and_do_not_inherit_target_item_at() {
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
    raises(&mut p, 101, "boundary");
    controls(&mut p).group_starting_with = Some(101);
    p.extra_targets.push(mapping::NamedTarget {
        name: "audit".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: p.root.clone(),
    });
    let expected = || EngineError::MappingFailure {
        rule: 1,
        message: Some("primary row".into()),
    };
    let execution = crate::ExecutionContext::new(Path::new("starting.json"));
    assert_eq!(run(&p), Err(expected()));
    assert_eq!(
        crate::run_outputs_with_sources_and_context(&p, &input("P"), supplied(), &execution)
            .map(|_| ()),
        Err(expected())
    );
    for selection in [
        crate::TargetSelection::Primary,
        crate::TargetSelection::Named("audit"),
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
