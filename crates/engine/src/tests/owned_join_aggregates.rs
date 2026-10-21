//! #450 owned joined aggregates: every reducer, active-source correlation by
//! owner and collection, grouped members, tuple and parent phases, and closed
//! contexts.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, FunctionId,
    FunctionParameter, FunctionParameterId, Graph, JoinConditions, JoinId, JoinKey, JoinPlan,
    JoinSource, NamedSource, Node, NodeId, OwnedJoinAggregate, OwnedJoinConditions, OwnedJoinField,
    OwnedJoinKey, OwnedJoinPlan, OwnedJoinSource, OwnedJoinSourcePosition, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner, UserFunction,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError, run_with_sources, validate};

const JOIN: JoinId = JoinId::new(6);

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
            SchemaNode::group("Rows", vec![int("id"), string("code"), int("qty")]).repeating(),
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

fn constant(value: Value) -> Node {
    Node::Const { value }
}

fn text_value(value: &str) -> Value {
    Value::String(value.into())
}

fn call(function: &str, args: Vec<NodeId>) -> Node {
    Node::Call {
        function: function.into(),
        args,
    }
}

fn owned_field(source: OwnedSourceRef) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source,
        frame: None,
    })
}

/// Primary `Rows` joined to the same-path named `Lookup` `Rows` by id.
fn rows_plan() -> OwnedJoinPlan {
    OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Rows"])),
        OwnedJoinSource::new(lookup(&["Rows"])),
        key(primary(&["Rows"]), &["id"], &["id"]),
    )
    .unwrap()
}

fn aggregate(
    function: AggregateOp,
    plan: OwnedJoinPlan,
    expression: Option<NodeId>,
    arg: Option<NodeId>,
) -> Node {
    Node::OwnedJoinAggregate(OwnedJoinAggregate {
        function,
        join: JOIN,
        plan,
        expression,
        arg,
    })
}

fn bind(target_field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn project(target: Vec<SchemaNode>, nodes: Vec<(NodeId, Node)>, root: Scope) -> Project {
    Project {
        source: endpoint("Input"),
        target: SchemaNode::group("Output", target),
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
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}

/// One root `Value` binding of node 1.
fn root_value(nodes: Vec<(NodeId, Node)>) -> Project {
    project(
        vec![string("Value")],
        nodes,
        Scope {
            bindings: vec![bind("Value", 1)],
            ..Scope::default()
        },
    )
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
    scalar(text_value(value))
}

/// `(id, code, qty)` rows.
fn rows(rows: &[(Value, &str, i64)]) -> Instance {
    Instance::Repeated(
        rows.iter()
            .map(|(id, code, qty)| {
                group(vec![
                    ("id", scalar(id.clone())),
                    ("code", text(code)),
                    ("qty", scalar(Value::Int(*qty))),
                ])
            })
            .collect(),
    )
}

fn document(rows: Instance) -> Instance {
    group(vec![
        ("Header", group(vec![("code", text("H"))])),
        ("Rows", rows),
    ])
}

/// Primary ids 1,2,2,3 coded a,b,c,d.
fn primary_rows() -> Instance {
    document(rows(&[
        (Value::Int(1), "a", 1),
        (Value::Int(2), "b", 2),
        (Value::Int(2), "c", 3),
        (Value::Int(3), "d", 4),
    ]))
}

/// Lookup ids 2,1,2,4 with quantities 10-40. Tuples by id are (a,y) (b,x)
/// (b,z) (c,x) (c,z), whose Lookup quantities are 20,10,30,10,30.
fn lookup_rows() -> Instance {
    document(rows(&[
        (Value::Int(2), "x", 10),
        (Value::Int(1), "y", 20),
        (Value::Int(2), "z", 30),
        (Value::Int(4), "w", 40),
    ]))
}

fn run(project: &Project, primary: Instance, named: Instance) -> Result<Instance, EngineError> {
    assert_eq!(validate(project), vec![], "fixture validates");
    run_with_sources(project, &primary, vec![("Lookup".into(), named)])
}

fn value(project: &Project, primary: Instance, named: Instance) -> Result<Value, EngineError> {
    run(project, primary, named).map(|output| match output.field("Value") {
        Some(Instance::Scalar(value)) => value.clone(),
        other => panic!("scalar output: {other:?}"),
    })
}

const REDUCERS: [(&str, AggregateOp); 7] = [
    ("count", AggregateOp::Count),
    ("sum", AggregateOp::Sum),
    ("avg", AggregateOp::Avg),
    ("min", AggregateOp::Min),
    ("max", AggregateOp::Max),
    ("join", AggregateOp::Join),
    ("item", AggregateOp::ItemAt),
];

#[test]
fn decimal_sum_stays_closed_for_owned_joined_aggregates() {
    for expression in [None, Some(2)] {
        let p = root_value(vec![
            (
                1,
                aggregate(AggregateOp::DecimalSum, rows_plan(), expression, Some(3)),
            ),
            (2, Node::Raise { message: Some(4) }),
            (3, Node::Raise { message: Some(5) }),
            (4, constant(text_value("projection must not run"))),
            (5, constant(text_value("argument must not run"))),
        ]);
        for (primary, named) in [
            (primary_rows(), lookup_rows()),
            (document(Instance::Repeated(vec![])), lookup_rows()),
        ] {
            assert_eq!(
                value(&p, primary, named),
                Err(functions::FunctionError::InvalidArgument {
                    function: "owned_join_aggregate",
                    message: "this reducer is not supported for owned joined collections",
                }
                .into())
            );
        }
    }
}

fn reducer_target() -> Vec<SchemaNode> {
    vec![
        int("count"),
        int("sum"),
        SchemaNode::scalar("avg", ScalarType::Float),
        int("min"),
        int("max"),
        string("join"),
        int("item"),
    ]
}

/// Nodes 1-7 reduce Lookup `qty` (node 20) with `,` (21) and index 2 (22).
fn reducers(aggregate_node: impl Fn(AggregateOp, Option<NodeId>) -> Node) -> Project {
    let mut nodes = vec![
        (21, constant(text_value(","))),
        (22, constant(Value::Int(2))),
    ];
    for (index, (_, function)) in REDUCERS.iter().enumerate() {
        let arg = match function {
            AggregateOp::Join => Some(21),
            AggregateOp::ItemAt => Some(22),
            _ => None,
        };
        nodes.push((index as NodeId + 1, aggregate_node(*function, arg)));
    }
    project(
        reducer_target(),
        nodes,
        Scope {
            bindings: REDUCERS
                .iter()
                .zip(1..)
                .map(|((field, _), node)| bind(field, node))
                .collect(),
            ..Scope::default()
        },
    )
}

fn owned_reducers() -> Project {
    let mut project = reducers(|function, arg| aggregate(function, rows_plan(), Some(20), arg));
    project
        .graph
        .nodes
        .insert(20, join_field(lookup(&["Rows"]), &["qty"]));
    project
}

fn legacy_reducers() -> Project {
    let plan = JoinPlan::new(
        JoinSource::new(path(&["Rows"])),
        JoinSource::new(path(&["Lookup", "Rows"])),
        JoinConditions::new(JoinKey::new(path(&["Rows"]), path(&["id"]), path(&["id"]))),
    )
    .unwrap();
    let mut project = reducers(|function, arg| Node::JoinAggregate {
        function,
        join: JOIN,
        plan: plan.clone(),
        expression: Some(20),
        arg,
    });
    project.graph.nodes.insert(
        20,
        Node::JoinField {
            join: JOIN,
            collection: path(&["Lookup", "Rows"]),
            path: path(&["qty"]),
        },
    );
    project
}

fn totals(values: [Value; 7]) -> Instance {
    group(
        REDUCERS
            .iter()
            .zip(values)
            .map(|((field, _), value)| (*field, scalar(value)))
            .collect(),
    )
}

#[test]
fn every_reducer_matches_the_legacy_joined_aggregate() {
    let owned = owned_reducers();
    let legacy = legacy_reducers();
    let expected = totals([
        Value::Int(5),
        Value::Int(100),
        Value::Float(20.0),
        Value::Int(10),
        Value::Int(30),
        text_value("20,10,30,10,30"),
        Value::Int(10),
    ]);
    assert_eq!(
        run(&owned, primary_rows(), lookup_rows()),
        Ok(expected.clone())
    );
    assert_eq!(run(&legacy, primary_rows(), lookup_rows()), Ok(expected));
    // Empty sides and Null-like keys reduce as empty under the legacy policy.
    let empty = totals([
        Value::Int(0),
        Value::Int(0),
        Value::Null,
        Value::Null,
        Value::Null,
        text_value(""),
        Value::Null,
    ]);
    for (primary, named) in [
        (document(Instance::Repeated(vec![])), lookup_rows()),
        (primary_rows(), document(Instance::Repeated(vec![]))),
        (
            primary_rows(),
            group(vec![("Header", group(vec![("code", text("H"))]))]),
        ),
        (
            document(rows(&[(Value::Null, "a", 1)])),
            document(rows(&[(Value::Null, "x", 10)])),
        ),
    ] {
        assert_eq!(
            run(&owned, primary.clone(), named.clone()),
            Ok(empty.clone())
        );
        assert_eq!(run(&legacy, primary, named), Ok(empty.clone()));
    }
}

#[test]
fn missing_expressions_contribute_null_and_type_failures_stay_typed() {
    // Without an expression each tuple counts and value reducers see Null.
    let mut project = reducers(|function, arg| aggregate(function, rows_plan(), None, arg));
    project.graph.nodes.insert(20, constant(Value::Null));
    assert_eq!(
        run(&project, primary_rows(), lookup_rows()),
        Ok(totals([
            Value::Int(5),
            Value::Int(0),
            Value::Null,
            Value::Null,
            Value::Null,
            text_value(""),
            Value::Null,
        ]))
    );
    // Non-numeric text is skipped, while non-finite and overflowing tuple
    // values keep the legacy typed failures.
    let sum = |value: Value| {
        let project = root_value(vec![
            (1, aggregate(AggregateOp::Sum, rows_plan(), Some(2), None)),
            (2, call("concat", vec![3])),
            (3, constant(value)),
        ]);
        let mut project = project;
        project.target = SchemaNode::group("Output", vec![int("Value")]);
        run(&project, primary_rows(), lookup_rows())
    };
    assert_eq!(
        sum(text_value("code")),
        Ok(group(vec![("Value", scalar(Value::Int(0)))]))
    );
    assert_eq!(
        sum(text_value("inf")),
        Err(EngineError::AggregateNonFinite {
            function: AggregateOp::Sum
        })
    );
    assert_eq!(
        sum(Value::Int(i64::MAX)),
        Err(EngineError::AggregateIntegerOverflow {
            function: AggregateOp::Sum
        })
    );
}

#[test]
fn tuple_expressions_see_raw_and_flattened_positions_in_left_major_order() {
    // `left-raw:right-raw@tuple` per tuple, joined in tuple order.
    let project = root_value(vec![
        (
            1,
            aggregate(AggregateOp::Join, rows_plan(), Some(2), Some(9)),
        ),
        (2, call("concat", vec![3, 7, 4, 8, 5])),
        (3, source_position(primary(&["Rows"]))),
        (4, source_position(lookup(&["Rows"]))),
        (5, Node::JoinPosition { join: JOIN }),
        (7, constant(text_value(":"))),
        (8, constant(text_value("@"))),
        (9, constant(text_value(" "))),
    ]);
    assert_eq!(
        value(&project, primary_rows(), lookup_rows()),
        Ok(text_value("1:2@1 2:1@2 2:3@3 3:1@4 3:3@5"))
    );
    // A third, singleton stage keeps stage order and multiplicity.
    let plan = rows_plan()
        .then(
            OwnedJoinSource::singleton(lookup(&["Header", "code"])),
            key(primary(&["Rows"]), &["code"], &[]),
        )
        .unwrap();
    let mut project = project;
    project
        .graph
        .nodes
        .insert(1, aggregate(AggregateOp::Join, plan, Some(2), Some(9)));
    let header = |code: &str| {
        group(vec![
            ("Header", group(vec![("code", text(code))])),
            ("Rows", lookup_rows().field("Rows").cloned().unwrap()),
        ])
    };
    assert_eq!(
        value(&project, primary_rows(), header("b")),
        Ok(text_value("2:1@1 2:3@2"))
    );
    assert_eq!(
        value(&project, primary_rows(), header("q")),
        Ok(text_value(""))
    );
}

#[test]
fn the_argument_evaluates_once_in_the_parent_after_every_tuple_value() {
    // The separator reads the Primary header in the parent context.
    let nodes = |expression: Node, arg: Node| {
        vec![
            (
                1,
                aggregate(AggregateOp::Join, rows_plan(), Some(2), Some(3)),
            ),
            (2, expression),
            (3, arg),
        ]
    };
    let project = root_value(nodes(
        join_field(primary(&["Rows"]), &["code"]),
        owned_field(primary(&["Header", "code"])),
    ));
    assert_eq!(
        value(&project, primary_rows(), lookup_rows()),
        Ok(text_value("aHbHbHcHc"))
    );
    // A failing tuple value precedes the argument; with no tuples the
    // argument still evaluates once.
    let mut project = root_value(nodes(
        Node::Raise { message: Some(4) },
        Node::Raise { message: Some(5) },
    ));
    project.graph.nodes.insert(4, constant(text_value("tuple")));
    project
        .graph
        .nodes
        .insert(5, constant(text_value("argument")));
    let raised = |result: Result<Value, EngineError>| match result {
        Err(EngineError::MappingException { message, .. }) => message,
        other => panic!("raised: {other:?}"),
    };
    assert_eq!(
        raised(value(&project, primary_rows(), lookup_rows())),
        Some("tuple".into())
    );
    assert_eq!(
        raised(value(
            &project,
            primary_rows(),
            document(Instance::Repeated(vec![]))
        )),
        Some("argument".into())
    );
}

fn order_lines() -> Instance {
    let line = |order: i64, sku: &str| {
        group(vec![
            ("order", scalar(Value::Int(order))),
            ("sku", text(sku)),
        ])
    };
    let order = |id: i64, lines: Vec<Instance>| {
        group(vec![
            ("id", scalar(Value::Int(id))),
            ("Lines", Instance::Repeated(lines)),
        ])
    };
    Instance::Repeated(vec![
        order(7, vec![line(7, "x"), line(9, "y"), line(7, "y")]),
        order(9, vec![line(9, "x"), line(9, "q")]),
    ])
}

#[test]
fn an_active_owned_row_correlates_only_same_owner_sources() {
    // Inside an owned Primary order, the Primary `Orders` source is that
    // order and its lines; the same-path Lookup `Orders` is never correlated
    // and contributes every Lookup order with an equal id.
    let lines = OwnedJoinPlan::new(
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
    let orders = OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Orders"])),
        OwnedJoinSource::new(lookup(&["Orders"])),
        key(primary(&["Orders"]), &["id"], &["id"]),
    )
    .unwrap();
    let nodes = vec![
        (1, owned_field(primary(&["Orders", "id"]))),
        (2, aggregate(AggregateOp::Join, lines, Some(3), Some(4))),
        (3, call("concat", vec![5, 6, 7])),
        (5, join_field(primary(&["Orders", "Lines"]), &["sku"])),
        (6, source_position(primary(&["Orders"]))),
        (7, source_position(primary(&["Orders", "Lines"]))),
        (4, constant(text_value("|"))),
        (8, aggregate(AggregateOp::Count, orders, None, None)),
    ];
    let project = project(
        vec![
            SchemaNode::group("Order", vec![int("id"), string("lines"), int("lookups")])
                .repeating(),
        ],
        nodes,
        Scope {
            children: vec![Scope {
                target_field: "Order".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: primary(&["Orders"]),
                    output_path: None,
                },
                bindings: vec![bind("id", 1), bind("lines", 2), bind("lookups", 8)],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    let primary = group(vec![("Orders", order_lines())]);
    // Two Lookup orders share id 9; none has id 7.
    let named = group(vec![
        ("Header", group(vec![("code", text("H"))])),
        ("Rows", lookup_rows().field("Rows").cloned().unwrap()),
        (
            "Orders",
            Instance::Repeated(vec![
                group(vec![("id", scalar(Value::Int(9)))]),
                group(vec![("id", scalar(Value::Int(9)))]),
                group(vec![("id", scalar(Value::Int(8)))]),
            ]),
        ),
    ]);
    let order = |id: i64, lines: &str, lookups: i64| {
        group(vec![
            ("id", scalar(Value::Int(id))),
            ("lines", text(lines)),
            ("lookups", scalar(Value::Int(lookups))),
        ])
    };
    // Order 7 keeps lines 1 (x) and 3 (y); order 9 keeps line 1 (x), since
    // Lookup has no `q` row. Raw order positions are the active row's.
    assert_eq!(
        run(&project, primary, named),
        Ok(group(vec![(
            "Order",
            Instance::Repeated(vec![order(7, "x11|y13", 0), order(9, "x21", 2)]),
        )]))
    );
}

#[test]
fn a_grouped_source_joins_only_its_members() {
    // Primary rows grouped by id; the joined Lookup codes per group.
    let project = project(
        vec![
            SchemaNode::group("Group", vec![int("id"), string("codes"), int("count")]).repeating(),
        ],
        vec![
            (1, owned_field(primary(&["Rows", "id"]))),
            (
                2,
                aggregate(AggregateOp::Join, rows_plan(), Some(3), Some(4)),
            ),
            (3, call("concat", vec![5, 6])),
            (5, join_field(primary(&["Rows"]), &["code"])),
            (6, join_field(lookup(&["Rows"]), &["code"])),
            (4, constant(text_value(","))),
            (7, aggregate(AggregateOp::Count, rows_plan(), None, None)),
        ],
        Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: primary(&["Rows"]),
                    output_path: None,
                },
                group_by: Some(1),
                bindings: vec![bind("id", 1), bind("codes", 2), bind("count", 7)],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    let group_of = |id: i64, codes: &str, count: i64| {
        group(vec![
            ("id", scalar(Value::Int(id))),
            ("codes", text(codes)),
            ("count", scalar(Value::Int(count))),
        ])
    };
    assert_eq!(
        run(&project, primary_rows(), lookup_rows()),
        Ok(group(vec![(
            "Group",
            Instance::Repeated(vec![
                group_of(1, "ay", 1),
                group_of(2, "bx,bz,cx,cz", 4),
                group_of(3, "", 0),
            ]),
        )]))
    );
}

#[test]
fn an_unsupplied_named_owner_is_a_typed_node_failure() {
    let project = root_value(vec![(
        1,
        aggregate(AggregateOp::Count, rows_plan(), None, None),
    )]);
    assert_eq!(validate(&project), vec![]);
    assert_eq!(
        run_with_sources(&project, &primary_rows(), vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: OwnedSourceError::SourceUnavailable {
                owner: SourceOwner::Named {
                    name: "Lookup".into()
                },
            },
        })
    );
}

fn issues(project: &Project) -> Vec<String> {
    validate(project)
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

#[test]
fn wrong_inactive_and_unresolved_reads_are_typed_validation_errors() {
    let with_expression = |node: Node| {
        root_value(vec![
            (1, aggregate(AggregateOp::Join, rows_plan(), Some(2), None)),
            (2, node),
        ])
    };
    assert_eq!(
        issues(&with_expression(join_field(lookup(&["Orders"]), &["id"]))),
        ["owned join node 2 collection named `Lookup` `Orders` does not belong to join 6"]
    );
    assert_eq!(
        issues(&with_expression(Node::OwnedJoinField(OwnedJoinField {
            join: JoinId::new(9),
            collection: primary(&["Rows"]),
            path: path(&["code"]),
        }))),
        ["owned join node 2 references inactive join 9"]
    );
    assert_eq!(
        issues(&with_expression(join_field(
            primary(&["Rows"]),
            &["missing"]
        ))),
        ["owned join field `missing` is not a nonrepeating scalar under owned collection `Rows`"]
    );
    assert_eq!(
        issues(&with_expression(Node::JoinField {
            join: JOIN,
            collection: path(&["Rows"]),
            path: path(&["code"]),
        })),
        ["join field node 2 references owned join 6; read its members with owned join fields"]
    );
    // A tuple read outside its aggregate is inactive, as is the aggregate's
    // own join position in the argument.
    let mut outside = with_expression(join_field(primary(&["Rows"]), &["code"]));
    outside
        .graph
        .nodes
        .insert(1, aggregate(AggregateOp::Join, rows_plan(), None, Some(2)));
    assert_eq!(
        issues(&outside),
        ["owned join node 2 references inactive join 6"]
    );
    // Each source resolves against its own endpoint.
    let missing = OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Rows"])),
        OwnedJoinSource::new(lookup(&["Missing"])),
        key(primary(&["Rows"]), &["id"], &["id"]),
    )
    .unwrap();
    let project = root_value(vec![(
        1,
        aggregate(AggregateOp::Count, missing, None, None),
    )]);
    assert_eq!(
        issues(&project),
        ["join 6 collection named `Lookup` `Missing` is missing or not repeating"]
    );
}

fn closed(project: &Project, node: NodeId, context: OwnedCapabilityContext) {
    assert_eq!(
        run_with_sources(
            project,
            &primary_rows(),
            vec![("Lookup".into(), lookup_rows())]
        ),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(node),
            function: None,
            scope: None,
            context,
        })
    );
}

#[test]
fn owned_joined_aggregates_stay_closed_outside_basic_contexts() {
    let count = || aggregate(AggregateOp::Count, rows_plan(), None, None);
    // Ordinary and owned source reads stay closed in the tuple expression.
    for read in [
        owned_field(primary(&["Header", "code"])),
        Node::SourceField {
            path: path(&["Header", "code"]),
            frame: None,
        },
        count(),
    ] {
        let project = root_value(vec![
            (1, aggregate(AggregateOp::Join, rows_plan(), Some(2), None)),
            (2, call("concat", vec![3])),
            (3, read),
        ]);
        closed(&project, 3, OwnedCapabilityContext::Join);
    }
    // A legacy aggregate item expression stays closed.
    let legacy = root_value(vec![
        (
            1,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: path(&["Rows"]),
                value: Vec::new(),
                expression: Some(2),
                arg: None,
            },
        ),
        (2, count()),
    ]);
    closed(&legacy, 2, OwnedCapabilityContext::AggregateLookup);
    // An owned join tuple admits no joined aggregate.
    let tuple_join = JoinId::new(5);
    let in_tuple = project(
        vec![SchemaNode::group("Row", vec![int("count")]).repeating()],
        vec![(
            1,
            Node::OwnedJoinAggregate(OwnedJoinAggregate {
                function: AggregateOp::Count,
                join: JOIN,
                plan: rows_plan(),
                expression: None,
                arg: None,
            }),
        )],
        Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::OwnedInnerJoin {
                    id: tuple_join,
                    plan: rows_plan(),
                },
                bindings: vec![bind("count", 1)],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    closed(&in_tuple, 1, OwnedCapabilityContext::Join);
    // Failure predicates keep their closed scalar subsets.
    let mut project = root_value(vec![
        (1, constant(text_value("x"))),
        (2, call("equal", vec![3, 4])),
        (3, count()),
        (4, constant(Value::Int(2))),
    ]);
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: Vec::new(),
        },
        selection: FailureSelection::WhenTrue { predicate: 2 },
        message: None,
    });
    closed(&project, 3, OwnedCapabilityContext::FailureRule);
    // A dynamic owner is unavailable before evaluation.
    let mut project = root_value(vec![(1, count()), (2, constant(text_value("ref.json")))]);
    project.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 2,
        iteration: Vec::new(),
    });
    closed(&project, 1, OwnedCapabilityContext::DynamicOwner);
    // Isolated functions refuse the owned kind.
    let function = FunctionId::new(1);
    let parameter = FunctionParameterId::new(1);
    let mut project = root_value(vec![
        (
            1,
            Node::UserFunctionCall {
                function,
                args: vec![2],
            },
        ),
        (2, constant(text_value("C"))),
    ]);
    project.user_functions.insert(
        function,
        UserFunction {
            library: "test".into(),
            name: "total".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: parameter,
                name: "code".into(),
                ty: ScalarType::String,
            }],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(1, Node::FunctionParameter { parameter }), (2, count())].into(),
            },
            output: 2,
        },
    );
    assert_eq!(
        run_with_sources(
            &project,
            &primary_rows(),
            vec![("Lookup".into(), lookup_rows())]
        ),
        Err(EngineError::UnsupportedSourceOwnership {
            node: 2,
            function: Some(function),
            kind: "owned_join_aggregate",
        })
    );
}
