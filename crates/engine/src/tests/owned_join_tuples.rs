//! #449 owned join tuples: exact member identity, raw and flattened
//! positions, stage order, correlation and closed tuple contexts.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, JoinId, NamedSource, Node, NodeId, OwnedJoinConditions, OwnedJoinField,
    OwnedJoinKey, OwnedJoinPlan, OwnedJoinSource, OwnedJoinSourcePosition, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SequenceWindow, SortFilterOrder, SourceOwner,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError, run_with_sources, validate};

const JOIN: JoinId = JoinId::new(5);

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}

fn endpoint(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::group("Header", vec![string("code")]),
            SchemaNode::group("Rows", vec![int("id"), string("code")]).repeating(),
            SchemaNode::group(
                "Orders",
                vec![
                    int("id"),
                    SchemaNode::group("Lines", vec![int("order"), string("sku")]).repeating(),
                ],
            )
            .repeating(),
        ],
    )
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn primary(parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(parts),
    }
}

fn lookup(parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: SourceOwner::Named {
            name: "Lookup".into(),
        },
        path: path(parts),
    }
}

fn key(left: OwnedSourceRef, left_path: &[&str], right_path: &[&str]) -> OwnedJoinConditions {
    OwnedJoinConditions::new(OwnedJoinKey::new(left, path(left_path), path(right_path)))
}

fn join_field(collection: OwnedSourceRef, parts: &[&str]) -> Node {
    Node::OwnedJoinField(OwnedJoinField {
        join: JOIN,
        collection,
        path: path(parts),
    })
}

fn source_position(collection: OwnedSourceRef) -> Node {
    Node::OwnedJoinSourcePosition(OwnedJoinSourcePosition {
        join: JOIN,
        collection,
    })
}

/// Primary `Rows` joined to the same-path named `Lookup` `Rows` by id.
fn same_path_plan() -> OwnedJoinPlan {
    OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Rows"])),
        OwnedJoinSource::new(lookup(&["Rows"])),
        key(primary(&["Rows"]), &["id"], &["id"]),
    )
    .unwrap()
}

fn row_target() -> SchemaNode {
    SchemaNode::group(
        "Row",
        vec![
            string("left"),
            string("right"),
            int("left_at"),
            int("right_at"),
            int("tuple"),
        ],
    )
    .repeating()
}

/// Nodes 1-5 read the left code, right code, both raw positions and the
/// flattened position; tests add their own nodes from 10.
fn project(plan: OwnedJoinPlan) -> Project {
    let mut graph = Graph::default();
    for (id, node) in [
        (1, join_field(primary(&["Rows"]), &["code"])),
        (2, join_field(lookup(&["Rows"]), &["code"])),
        (3, source_position(primary(&["Rows"]))),
        (4, source_position(lookup(&["Rows"]))),
        (5, Node::JoinPosition { join: JOIN }),
    ] {
        graph.nodes.insert(id, node);
    }
    Project {
        source: endpoint("Input"),
        target: SchemaNode::group("Output", vec![row_target()]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Lookup".into(),
            path: "lookup.json".into(),
            schema: endpoint("Lookup"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::OwnedInnerJoin { id: JOIN, plan },
                bindings: ["left", "right", "left_at", "right_at", "tuple"]
                    .into_iter()
                    .zip(1..)
                    .map(|(target_field, node)| Binding {
                        target_field: target_field.into(),
                        node,
                    })
                    .collect(),
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn text(value: &str) -> Instance {
    scalar(Value::String(value.into()))
}

fn rows(rows: &[(Value, &str)]) -> Instance {
    Instance::Repeated(
        rows.iter()
            .map(|(id, code)| group(vec![("id", scalar(id.clone())), ("code", text(code))]))
            .collect(),
    )
}

fn document(header: &str, rows: Instance) -> Instance {
    group(vec![
        ("Header", group(vec![("code", text(header))])),
        ("Rows", rows),
    ])
}

fn ids(values: &[i64], prefix: &str) -> Instance {
    let rows: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(index, id)| (Value::Int(*id), format!("{prefix}{}", index + 1)))
        .collect();
    let borrowed: Vec<_> = rows
        .iter()
        .map(|(id, code)| (id.clone(), code.as_str()))
        .collect();
    self::rows(&borrowed)
}

fn output(rows: &[(&str, &str, i64, i64, i64)]) -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            rows.iter()
                .map(|(left, right, left_at, right_at, tuple)| {
                    group(vec![
                        ("left", text(left)),
                        ("right", text(right)),
                        ("left_at", scalar(Value::Int(*left_at))),
                        ("right_at", scalar(Value::Int(*right_at))),
                        ("tuple", scalar(Value::Int(*tuple))),
                    ])
                })
                .collect(),
        ),
    )])
}

fn run(project: &Project, primary: Instance, named: Instance) -> Result<Instance, EngineError> {
    assert_eq!(validate(project), vec![], "fixture validates");
    run_with_sources(project, &primary, vec![("Lookup".into(), named)])
}

#[test]
fn same_path_sources_join_by_identity_with_duplicates_in_left_major_order() {
    // Primary ids 1,2,2,3 against Lookup ids 2,1,2,4: Lookup never supplies
    // primary rows and primary never supplies Lookup rows.
    let result = run(
        &project(same_path_plan()),
        document("H", ids(&[1, 2, 2, 3], "p")),
        document("H", ids(&[2, 1, 2, 4], "l")),
    )
    .unwrap();
    assert_eq!(
        result,
        output(&[
            ("p1", "l2", 1, 2, 1),
            ("p2", "l1", 2, 1, 2),
            ("p2", "l3", 2, 3, 3),
            ("p3", "l1", 3, 1, 4),
            ("p3", "l3", 3, 3, 5),
        ])
    );
}

#[test]
fn empty_sides_and_null_like_keys_produce_no_tuples() {
    let project = project(same_path_plan());
    for (primary, named) in [
        (
            document("H", Instance::Repeated(vec![])),
            document("H", ids(&[1], "l")),
        ),
        (
            document("H", ids(&[1], "p")),
            document("H", Instance::Repeated(vec![])),
        ),
        // An absent Lookup collection is empty; primary rows never stand in.
        (
            document("H", ids(&[1], "p")),
            group(vec![("Header", group(vec![("code", text("H"))]))]),
        ),
        (
            document(
                "H",
                rows(&[(Value::Null, "p1"), (Value::JsonNull(ir::JsonNull), "p2")]),
            ),
            document("H", rows(&[(Value::Null, "l1"), (Value::Int(1), "l2")])),
        ),
    ] {
        assert_eq!(
            run(&project, primary, named).unwrap(),
            group(vec![("Row", Instance::Repeated(vec![]))])
        );
    }
}

#[test]
fn equality_coerces_like_legacy_joins() {
    // String "2" equals Int 2 under ordinary scalar coercion.
    let result = run(
        &project(same_path_plan()),
        document("H", rows(&[(Value::String("2".into()), "p1")])),
        document("H", ids(&[2], "l")),
    )
    .unwrap();
    assert_eq!(result, output(&[("p1", "l1", 1, 1, 1)]));
}

#[test]
fn multi_stage_joins_keep_stage_order_and_singleton_members() {
    // Stage 3 keys a named singleton header on the Lookup row code.
    let plan = same_path_plan()
        .then(
            OwnedJoinSource::singleton(lookup(&["Header", "code"])),
            key(lookup(&["Rows"]), &["code"], &[]),
        )
        .unwrap();
    let mut project = project(plan);
    project
        .graph
        .nodes
        .insert(10, join_field(lookup(&["Header", "code"]), &[]));
    project
        .graph
        .nodes
        .insert(11, source_position(lookup(&["Header", "code"])));
    let row = &mut project.root.children[0];
    row.bindings[0].node = 10;
    row.bindings[2].node = 11;
    let result = run(
        &project,
        document("p", ids(&[1, 2], "p")),
        document("l2", ids(&[2, 1, 2], "l")),
    )
    .unwrap();
    // Only Lookup row 2 has code `l2`, the header's code; it joined primary row 1.
    assert_eq!(result, output(&[("l2", "l2", 1, 2, 1)]));
}

#[test]
fn controls_see_original_sorted_and_compacted_tuple_positions() {
    let mut project = project(same_path_plan());
    let nodes = &mut project.graph.nodes;
    // Filter: tuple position != 2. Sort: Lookup raw position descending.
    nodes.insert(
        10,
        Node::Const {
            value: Value::Int(2),
        },
    );
    nodes.insert(
        11,
        Node::Call {
            function: "not_equal".into(),
            args: vec![5, 10],
        },
    );
    nodes.insert(
        12,
        Node::Const {
            value: Value::Int(3),
        },
    );
    let row = &mut project.root.children[0];
    row.filter = Some(11);
    row.sort_by = Some(4);
    row.sort_descending = true;
    let primary = document("H", ids(&[1, 2, 2], "p"));
    let named = document("H", ids(&[2, 1, 2], "l"));
    // Tuples: (p1,l2) (p2,l1) (p2,l3) (p3,l1) (p3,l3). Sort-then-filter sees
    // sorted positions: (p2,l3) (p3,l3) (p1,l2) (p2,l1) (p3,l1); the second
    // is dropped and outputs compact.
    assert_eq!(
        run(&project, primary.clone(), named.clone()).unwrap(),
        output(&[
            ("p2", "l3", 2, 3, 1),
            ("p1", "l2", 1, 2, 2),
            ("p2", "l1", 2, 1, 3),
            ("p3", "l1", 3, 1, 4),
        ])
    );
    // Filter-then-sort sees the original tuple positions, dropping (p2,l1).
    let row = &mut project.root.children[0];
    row.sort_filter_order = SortFilterOrder::FilterThenSort;
    assert_eq!(
        run(&project, primary.clone(), named.clone()).unwrap(),
        output(&[
            ("p2", "l3", 2, 3, 1),
            ("p3", "l3", 3, 3, 2),
            ("p1", "l2", 1, 2, 3),
            ("p3", "l1", 3, 1, 4),
        ])
    );
    // A parent-context window bound applies after the filter.
    let row = &mut project.root.children[0];
    row.windows = vec![SequenceWindow::First { count: 12 }];
    assert_eq!(
        run(&project, primary, named).unwrap(),
        output(&[
            ("p2", "l3", 2, 3, 1),
            ("p3", "l3", 3, 3, 2),
            ("p1", "l2", 1, 2, 3),
        ])
    );
}

fn orders(orders: &[(i64, &[(i64, &str)])]) -> Instance {
    Instance::Repeated(
        orders
            .iter()
            .map(|(id, lines)| {
                group(vec![
                    ("id", scalar(Value::Int(*id))),
                    (
                        "Lines",
                        Instance::Repeated(
                            lines
                                .iter()
                                .map(|(order, sku)| {
                                    group(vec![
                                        ("order", scalar(Value::Int(*order))),
                                        ("sku", text(sku)),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    )
}

#[test]
fn an_active_owned_row_correlates_its_sources() {
    // Inside an owned Primary `Orders` row, a Primary `Orders` source is that
    // active order at its position, and `Orders/Lines` walks only its lines.
    let plan = OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Orders"])),
        OwnedJoinSource::new(primary(&["Orders", "Lines"])),
        key(primary(&["Orders"]), &["id"], &["order"]),
    )
    .unwrap()
    .then(
        OwnedJoinSource::new(lookup(&["Rows"])),
        key(primary(&["Orders", "Lines"]), &["sku"], &["code"]),
    )
    .unwrap();
    let mut project = project(plan);
    let nodes = &mut project.graph.nodes;
    nodes.insert(1, join_field(primary(&["Orders", "Lines"]), &["sku"]));
    nodes.insert(3, source_position(primary(&["Orders"])));
    nodes.insert(
        10,
        Node::OwnedSourceField(OwnedSourceField {
            source: primary(&["Orders", "id"]),
            frame: None,
        }),
    );
    nodes.insert(11, source_position(primary(&["Orders", "Lines"])));
    let mut join = project.root.children.remove(0);
    join.bindings[3].node = 11;
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Order", vec![int("id"), row_target()]).repeating()],
    );
    project.root.children = vec![Scope {
        target_field: "Order".into(),
        iteration: ScopeIteration::OwnedSource {
            source: primary(&["Orders"]),
            output_path: None,
        },
        bindings: vec![Binding {
            target_field: "id".into(),
            node: 10,
        }],
        children: vec![join],
        ..Scope::default()
    }];
    // Order 7 keeps lines 1 and 3; order 9 keeps line 1; Lookup rows match by sku.
    let primary = group(vec![(
        "Orders",
        orders(&[
            (7, &[(7, "a"), (9, "b"), (7, "b")]),
            (9, &[(9, "a"), (9, "c")]),
        ]),
    )]);
    let named = document("H", rows(&[(Value::Int(1), "a"), (Value::Int(2), "b")]));
    let order = |id, rows: Vec<Instance>| {
        group(vec![
            ("id", scalar(Value::Int(id))),
            ("Row", Instance::Repeated(rows)),
        ])
    };
    let result = run(&project, primary, named).unwrap();
    let row = |sku: &str, order_at, line_at, tuple| {
        group(vec![
            ("left", text(sku)),
            ("right", text(sku)),
            ("left_at", scalar(Value::Int(order_at))),
            ("right_at", scalar(Value::Int(line_at))),
            ("tuple", scalar(Value::Int(tuple))),
        ])
    };
    assert_eq!(
        result,
        group(vec![(
            "Order",
            Instance::Repeated(vec![
                order(7, vec![row("a", 1, 1, 1), row("b", 1, 3, 2)]),
                order(9, vec![row("a", 2, 1, 1)]),
            ])
        )])
    );
}

#[test]
fn an_unsupplied_named_owner_is_a_typed_scope_failure() {
    let project = project(same_path_plan());
    let error = run_with_sources(&project, &document("H", ids(&[1], "p")), vec![]).unwrap_err();
    assert!(
        matches!(
            &error,
            EngineError::OwnedSourceAccess {
                node: None,
                scope: Some(_),
                source: OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named { name },
                },
            } if name == "Lookup"
        ),
        "{error:?}"
    );
}

fn issues(project: &Project) -> Vec<String> {
    validate(project)
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

#[test]
fn wrong_and_inactive_owner_reads_are_typed_validation_errors() {
    let refused = |id: NodeId, node: Node| {
        let mut project = project(same_path_plan());
        project.graph.nodes.insert(id, node);
        issues(&project)
    };
    // Same collection text under the other owner is not a member.
    let mut plan = OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Rows"])),
        OwnedJoinSource::new(primary(&["Orders"])),
        key(primary(&["Rows"]), &["id"], &["id"]),
    )
    .unwrap();
    let mut members = project(plan.clone());
    members.root.children[0].bindings.truncate(1);
    members.root.children[0].bindings[0].node = 2;
    assert_eq!(
        issues(&members),
        ["owned join node 2 collection named `Lookup` `Rows` does not belong to join 5"]
    );
    assert_eq!(
        refused(
            1,
            Node::OwnedJoinField(OwnedJoinField {
                join: JoinId::new(9),
                collection: primary(&["Rows"]),
                path: path(&["code"]),
            })
        ),
        ["owned join node 1 references inactive join 9"]
    );
    assert_eq!(
        refused(1, join_field(primary(&["Rows"]), &["missing"])),
        ["owned join field `missing` is not a nonrepeating scalar under owned collection `Rows`"]
    );
    assert_eq!(
        refused(
            1,
            Node::JoinField {
                join: JOIN,
                collection: path(&["Rows"]),
                path: path(&["code"]),
            }
        ),
        ["join field node 1 references owned join 5; read its members with owned join fields"]
    );
    // An owned read outside every join, and a singleton read below its value.
    plan = plan
        .then(
            OwnedJoinSource::singleton(primary(&["Header", "code"])),
            key(primary(&["Rows"]), &["code"], &[]),
        )
        .unwrap();
    let mut singleton = project(plan);
    singleton.root.children[0].bindings.truncate(1);
    singleton
        .graph
        .nodes
        .insert(1, join_field(primary(&["Header", "code"]), &["x"]));
    singleton.root.bindings = vec![Binding {
        target_field: "Row".into(),
        node: 5,
    }];
    let found = issues(&singleton);
    assert!(
        found.contains(&"owned join field node 1 reads `x` below singleton collection primary `Header/code`; a singleton member is read with an empty path".to_string()),
        "{found:?}"
    );
    assert!(
        found.contains(&"join position node 5 references inactive join 5".to_string()),
        "{found:?}"
    );
}

#[test]
fn closed_tuple_contexts_refuse_other_source_reads_and_nested_iteration() {
    let input = document("H", ids(&[1], "p"));
    let named = document("H", ids(&[1], "l"));
    // An ordinary owned read in a tuple stays closed.
    let mut owned_read = project(same_path_plan());
    owned_read.graph.nodes.insert(
        1,
        Node::OwnedSourceField(OwnedSourceField {
            source: primary(&["Header", "code"]),
            frame: None,
        }),
    );
    let error = run(&owned_read, input.clone(), named.clone()).unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::UnsupportedOwnedContext {
                node: Some(1),
                context: OwnedCapabilityContext::Join,
                ..
            }
        ),
        "{error:?}"
    );
    let mut legacy_read = project(same_path_plan());
    legacy_read.graph.nodes.insert(
        1,
        Node::SourceField {
            path: path(&["Header", "code"]),
            frame: None,
        },
    );
    let error = run(&legacy_read, input.clone(), named.clone()).unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::UnsupportedOwnedContext {
                node: Some(1),
                context: OwnedCapabilityContext::Join,
                ..
            }
        ),
        "{error:?}"
    );
    // Owned join reads outside their tuple are refused before execution even
    // when validation is bypassed by the caller.
    let mut outside = project(same_path_plan());
    outside.root.bindings = vec![];
    outside.root.children[0].iteration = ScopeIteration::None;
    assert!(matches!(
        crate::source_ownership::check_project(&outside, None),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(_),
            context: OwnedCapabilityContext::Join,
            ..
        })
    ));
}

#[derive(Default)]
struct Collector(std::cell::RefCell<Vec<crate::TraceEvent>>);

impl crate::TraceSink for Collector {
    fn record(&self, event: crate::TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

#[test]
fn traced_owned_joins_report_the_join_and_its_tuples() {
    let project = project(same_path_plan());
    let collector = Collector::default();
    let execution = crate::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_trace_sink(&collector);
    let outputs = crate::run_outputs_with_sources_and_context(
        &project,
        &document("H", ids(&[1, 2], "p")),
        vec![("Lookup".into(), document("H", ids(&[2, 2], "l")))],
        &execution,
    )
    .unwrap();
    assert_eq!(
        outputs.primary,
        output(&[("p2", "l1", 2, 1, 1), ("p2", "l2", 2, 2, 2)])
    );
    let events = collector.0.into_inner();
    assert!(events.iter().any(|event| matches!(event,
        crate::TraceEvent::ScopeStarted { iteration: crate::TraceIteration::Join { join }, .. }
            if *join == JOIN)));
    // The root identity candidate and one candidate per tuple.
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, crate::TraceEvent::IterationCandidate { .. }))
            .count(),
        3
    );
    assert!(events.iter().any(|event| matches!(
        event,
        crate::TraceEvent::ScopeFinished {
            candidates: 2,
            produced: 2,
            ..
        }
    )));
}
