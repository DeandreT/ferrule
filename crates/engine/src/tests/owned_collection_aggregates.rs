//! #447 explicitly owned collection aggregates.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, FunctionId,
    FunctionParameter, FunctionParameterId, Graph, NamedSource, Node, NodeId, OwnedAggregate,
    OwnedAggregateValue, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope,
    ScopeIteration, SourceOwner, UserFunction,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError};

fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn source(owner: SourceOwner, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path(parts),
    }
}
fn field(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: source(owner, parts),
        frame: frame.map(path),
    })
}
fn aggregate(
    function: AggregateOp,
    owner: SourceOwner,
    collection: &[&str],
    value: &[&str],
    arg: Option<NodeId>,
) -> Node {
    Node::OwnedAggregate(OwnedAggregate {
        function,
        collection: source(owner, collection),
        value: OwnedAggregateValue::Path(path(value)),
        arg,
    })
}
fn computed(
    function: AggregateOp,
    owner: SourceOwner,
    collection: &[&str],
    expression: NodeId,
    arg: Option<NodeId>,
) -> Node {
    Node::OwnedAggregate(OwnedAggregate {
        function,
        collection: source(owner, collection),
        value: OwnedAggregateValue::Expression(expression),
        arg,
    })
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn int(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}
fn constant(value: Value) -> Node {
    Node::Const { value }
}
fn string(value: &str) -> Value {
    Value::String(value.into())
}
fn call(function: &str, args: Vec<NodeId>) -> Node {
    Node::Call {
        function: function.into(),
        args,
    }
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
fn line(sku: &str, qty: i64) -> Instance {
    group(vec![("Sku", text(sku)), ("Qty", int(qty))])
}
fn row(
    code: &str,
    name: &str,
    rank: i64,
    price: Option<&str>,
    lines: Vec<Instance>,
    tags: &[&str],
) -> Instance {
    let mut fields = vec![
        ("Code", text(code)),
        ("Name", text(name)),
        ("Rank", int(rank)),
    ];
    if let Some(price) = price {
        fields.push(("Price", text(price)));
    }
    fields.push(("Lines", Instance::Repeated(lines)));
    fields.push((
        "Tags",
        Instance::Repeated(tags.iter().map(|tag| text(tag)).collect()),
    ));
    group(fields)
}
fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Price", ScalarType::String),
                    SchemaNode::group(
                        "Lines",
                        vec![
                            SchemaNode::scalar("Sku", ScalarType::String),
                            SchemaNode::scalar("Qty", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                    SchemaNode::scalar("Tags", ScalarType::String).repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
/// Primary and Ref share every path but carry disjoint values.
fn primary_input() -> Instance {
    group(vec![
        ("Note", text("P.note")),
        (
            "Rows",
            Instance::Repeated(vec![
                row(
                    "A",
                    "P-A",
                    1,
                    Some("1.5"),
                    vec![line("a1", 1), line("a2", 5)],
                    &["x", "y"],
                ),
                row("B", "P-B", 2, Some("2"), vec![line("b1", 7)], &[]),
                row("A", "P-A2", 3, Some("oops"), vec![], &["z"]),
            ]),
        ),
    ])
}
fn ref_input() -> Instance {
    group(vec![
        ("Note", text("R.note")),
        (
            "Rows",
            Instance::Repeated(vec![
                row("C", "R-C", 4, Some("10"), vec![line("c1", 3)], &["n1"]),
                row("D", "R-D", 5, None, vec![line("d1", 9), line("d2", 2)], &[]),
            ]),
        ),
    ])
}
fn inputs() -> Vec<(String, Instance)> {
    vec![("Ref".into(), ref_input())]
}
fn target_schema() -> SchemaNode {
    SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Items",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Value", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}
fn project(nodes: Vec<(NodeId, Node)>, root: Scope) -> Project {
    Project {
        source: endpoint_schema("Input"),
        target: target_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: endpoint_schema("Ref"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}
fn bind(field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: field.into(),
        node,
    }
}
/// The root binds `Value` to node 1.
fn root_value(nodes: Vec<(NodeId, Node)>) -> Project {
    project(
        nodes,
        Scope {
            bindings: vec![bind("Value", 1)],
            ..Scope::default()
        },
    )
}
fn engine_valid(project: &Project) {
    let issues = crate::validate(project);
    assert!(issues.is_empty(), "{issues:?}");
}
/// The raw root value, read before target adaptation.
fn evaluate(
    project: &Project,
    primary: &Instance,
    extras: Vec<(String, Instance)>,
) -> Result<Value, EngineError> {
    engine_valid(project);
    crate::run_with_sources(project, primary, extras).map(|output| {
        output
            .field("Value")
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null)
    })
}
fn root_result(project: &Project, extras: Vec<(String, Instance)>) -> Result<Value, EngineError> {
    evaluate(project, &primary_input(), extras)
}
/// `(Code, Value)` for every constructed item.
fn items(project: &Project) -> Result<Vec<(Value, Value)>, EngineError> {
    engine_valid(project);
    let output = crate::run_with_sources(project, &primary_input(), inputs())?;
    let read = |item: &Instance, name: &str| {
        item.field(name)
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null)
    };
    Ok(match output.field("Items") {
        Some(Instance::Repeated(items)) => items
            .iter()
            .map(|item| (read(item, "Code"), read(item, "Value")))
            .collect(),
        _ => Vec::new(),
    })
}
fn items_scope(owner: SourceOwner, group_by: Option<NodeId>, bindings: Vec<Binding>) -> Scope {
    Scope {
        children: vec![Scope {
            target_field: "Items".into(),
            iteration: ScopeIteration::OwnedSource {
                source: source(owner, &["Rows"]),
                output_path: None,
            },
            group_by,
            bindings,
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

/// Function, collection, value path, argument, Primary and Ref results.
type RootCase = (
    AggregateOp,
    &'static [&'static str],
    &'static [&'static str],
    Option<Value>,
    Value,
    Value,
);

/// Every reducer over the entire root of one endpoint. String targets render
/// the raw reduction, so integer and float results stay distinguishable.
#[test]
fn every_operator_reduces_only_the_declared_endpoint_root() {
    use AggregateOp::*;
    let cases: &[RootCase] = &[
        (Count, &["Rows"], &[], None, Value::Int(3), Value::Int(2)),
        (
            Sum,
            &["Rows"],
            &["Rank"],
            None,
            Value::Int(6),
            Value::Int(9),
        ),
        // Numeric strings parse, non-numeric and absent values are skipped.
        (
            Sum,
            &["Rows"],
            &["Price"],
            None,
            Value::Float(3.5),
            Value::Int(10),
        ),
        (
            Avg,
            &["Rows"],
            &["Rank"],
            None,
            Value::Float(2.0),
            Value::Float(4.5),
        ),
        (
            Min,
            &["Rows"],
            &["Rank"],
            None,
            Value::Int(1),
            Value::Int(4),
        ),
        (
            Max,
            &["Rows"],
            &["Rank"],
            None,
            Value::Int(3),
            Value::Int(5),
        ),
        (
            Join,
            &["Rows"],
            &["Name"],
            Some(string(",")),
            string("P-A,P-B,P-A2"),
            string("R-C,R-D"),
        ),
        (
            Join,
            &["Rows"],
            &["Name"],
            None,
            string("P-AP-BP-A2"),
            string("R-CR-D"),
        ),
        (
            ItemAt,
            &["Rows"],
            &["Name"],
            Some(Value::Int(2)),
            string("P-B"),
            string("R-D"),
        ),
        (
            ItemAt,
            &["Rows"],
            &["Name"],
            Some(Value::Int(3)),
            string("P-A2"),
            Value::Null,
        ),
        // Without an active frame a nested walk flattens every crossed row.
        (
            Count,
            &["Rows", "Lines"],
            &[],
            None,
            Value::Int(3),
            Value::Int(3),
        ),
        (
            Sum,
            &["Rows", "Lines"],
            &["Qty"],
            None,
            Value::Int(13),
            Value::Int(14),
        ),
        // A scalar collection's empty value path reads the item itself.
        (
            Join,
            &["Rows", "Tags"],
            &[],
            Some(string("+")),
            string("x+y+z"),
            string("n1"),
        ),
    ];
    for (function, collection, value, arg, primary, reference) in cases {
        for (owner, expected) in [(SourceOwner::Primary, primary), (named(), reference)] {
            let node = aggregate(*function, owner, collection, value, arg.as_ref().map(|_| 2));
            let mut nodes = vec![(1, node)];
            nodes.extend(arg.clone().map(|arg| (2, constant(arg))));
            assert_eq!(
                root_result(&root_value(nodes), inputs()),
                Ok(expected.clone()),
                "{function:?} {collection:?} {value:?}"
            );
        }
    }
    // The legacy aggregate keeps primary-first resolution beside the owned form.
    let legacy = |collection: &[&str]| {
        root_value(vec![(
            1,
            Node::Aggregate {
                function: Sum,
                collection: path(collection),
                value: path(&["Rank"]),
                expression: None,
                arg: None,
            },
        )])
    };
    assert_eq!(root_result(&legacy(&["Rows"]), inputs()), Ok(Value::Int(6)));
    assert_eq!(
        root_result(&legacy(&["Ref", "Rows"]), inputs()),
        Ok(Value::Int(9))
    );
}

#[test]
fn grouped_and_row_frames_select_a_subset_while_other_owners_see_their_root() {
    let concat = |args: Vec<NodeId>| call("concat", args);
    let nodes = vec![
        (1, field(SourceOwner::Primary, &["Rows", "Code"], None)),
        (2, concat(vec![3, 9, 4, 9, 5, 9, 6])),
        (
            3,
            aggregate(
                AggregateOp::Count,
                SourceOwner::Primary,
                &["Rows"],
                &[],
                None,
            ),
        ),
        (
            4,
            aggregate(AggregateOp::Count, named(), &["Rows"], &[], None),
        ),
        (
            5,
            aggregate(
                AggregateOp::Sum,
                SourceOwner::Primary,
                &["Rows", "Lines"],
                &["Qty"],
                None,
            ),
        ),
        (
            6,
            aggregate(
                AggregateOp::Join,
                SourceOwner::Primary,
                &["Rows"],
                &["Name"],
                Some(7),
            ),
        ),
        (7, constant(string("+"))),
        (9, constant(string("/"))),
    ];
    // A By group exposes only its members; the named walk keeps its root.
    let grouped = project(
        nodes.clone(),
        items_scope(
            SourceOwner::Primary,
            Some(1),
            vec![bind("Code", 1), bind("Value", 2)],
        ),
    );
    assert_eq!(
        items(&grouped),
        Ok(vec![
            (string("A"), string("2/2/6/P-A+P-A2")),
            (string("B"), string("1/2/7/P-B")),
        ])
    );
    // An ordinary owned row anchors same-owner walks at that row.
    let rows = project(
        nodes,
        items_scope(
            SourceOwner::Primary,
            None,
            vec![bind("Code", 1), bind("Value", 2)],
        ),
    );
    assert_eq!(
        items(&rows),
        Ok(vec![
            (string("A"), string("1/2/6/P-A")),
            (string("B"), string("1/2/7/P-B")),
            (string("A"), string("1/2/0/P-A2")),
        ])
    );
}

#[test]
fn absent_empty_and_null_collections_reduce_as_empty_without_foreign_items() {
    use AggregateOp::*;
    let empty_results = [
        (Count, &[][..], Value::Int(0)),
        (Sum, &["Rank"][..], Value::Int(0)),
        (Avg, &["Rank"][..], Value::Null),
        (Min, &["Rank"][..], Value::Null),
        (Max, &["Rank"][..], Value::Null),
        (Join, &["Name"][..], string("")),
        (ItemAt, &["Name"][..], Value::Null),
    ];
    let empty = group(vec![
        ("Note", text("R.note")),
        ("Rows", Instance::Repeated(vec![])),
    ]);
    let absent = group(vec![("Note", text("R.note"))]);
    let null_rows = group(vec![("Rows", Instance::Scalar(Value::Null))]);
    for reference in [empty, absent, null_rows] {
        for (function, value, expected) in &empty_results {
            // Primary rows with the same path never repair the named collection.
            let p = root_value(vec![
                (1, aggregate(*function, named(), &["Rows"], value, Some(2))),
                (2, constant(Value::Int(1))),
            ]);
            assert_eq!(
                root_result(&p, vec![("Ref".into(), reference.clone())]),
                Ok(expected.clone()),
                "{function:?}"
            );
        }
    }
    // An unsupplied owner fails before item values and the argument.
    let p = root_value(vec![
        (1, computed(Join, named(), &["Rows"], 2, Some(3))),
        (2, Node::Raise { message: None }),
        (3, Node::Raise { message: None }),
    ]);
    assert_eq!(
        root_result(&p, vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: OwnedSourceError::SourceUnavailable { owner: named() },
        })
    );
    // Item values precede the argument, which still runs for an empty walk.
    assert_eq!(
        root_result(&p, inputs()),
        Err(EngineError::MappingException {
            node: 2,
            message: None
        })
    );
    assert_eq!(
        root_result(&p, vec![("Ref".into(), group(vec![]))]),
        Err(EngineError::MappingException {
            node: 3,
            message: None
        })
    );
}

#[test]
fn item_values_run_per_owned_item_while_the_argument_stays_in_the_parent() {
    let nodes = vec![
        (1, field(SourceOwner::Primary, &["Rows", "Code"], None)),
        (2, call("concat", vec![3, 9, 4, 9, 5])),
        // The index is the parent's Primary rank, never the named item's.
        (
            3,
            aggregate(AggregateOp::ItemAt, named(), &["Rows"], &["Name"], Some(6)),
        ),
        // The separator is the parent's Primary code.
        (
            4,
            aggregate(AggregateOp::Join, named(), &["Rows"], &["Code"], Some(1)),
        ),
        // Each named item sees its own fields and position plus the pinned
        // Primary parent row.
        (
            5,
            computed(AggregateOp::Join, named(), &["Rows"], 7, Some(8)),
        ),
        (6, field(SourceOwner::Primary, &["Rows", "Rank"], None)),
        (7, call("concat", vec![10, 11, 12, 13])),
        (8, constant(string(","))),
        (9, constant(string("|"))),
        (10, field(named(), &["Rows", "Code"], None)),
        (
            11,
            field(SourceOwner::Primary, &["Rows", "Code"], Some(&["Rows"])),
        ),
        (
            12,
            Node::OwnedPosition(OwnedPosition {
                owner: named(),
                collection: Some(path(&["Rows"])),
            }),
        ),
        // Legacy reads resolve the item outward, as inside an owned scope.
        (
            13,
            Node::SourceField {
                path: path(&["Name"]),
                frame: None,
            },
        ),
    ];
    let p = project(
        nodes,
        items_scope(
            SourceOwner::Primary,
            None,
            vec![bind("Code", 1), bind("Value", 2)],
        ),
    );
    assert_eq!(
        items(&p),
        Ok(vec![
            (string("A"), string("R-C|CAD|CA1R-C,DA2R-D")),
            (string("B"), string("R-D|CBD|CB1R-C,DB2R-D")),
            (string("A"), string("|CAD|CA1R-C,DA2R-D")),
        ])
    );
}

#[test]
fn reducer_null_and_type_policies_match_the_legacy_aggregate() {
    use AggregateOp::*;
    let reference = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![
                ("Code", text("n")),
                ("Price", Instance::Scalar(Value::Null)),
            ]),
            group(vec![
                ("Code", Instance::Scalar(Value::Null)),
                ("Price", text(" 4 ")),
            ]),
            group(vec![("Code", text("m")), ("Price", text("2.5"))]),
        ]),
    )]);
    for (function, value, arg, expected) in [
        // Count counts items, not non-null values.
        (Count, &["Price"][..], None, Value::Int(3)),
        (Sum, &["Price"][..], None, Value::Float(6.5)),
        (Min, &["Price"][..], None, Value::Float(2.5)),
        (Max, &["Price"][..], None, Value::Int(4)),
        // Join skips Null values; ItemAt indexes every item.
        (Join, &["Code"][..], Some(string("-")), string("n-m")),
        (ItemAt, &["Code"][..], Some(string(" 2 ")), Value::Null),
        (ItemAt, &["Code"][..], Some(Value::Float(2.6)), string("m")),
        (ItemAt, &["Code"][..], Some(Value::Int(0)), Value::Null),
    ] {
        let node = aggregate(function, named(), &["Rows"], value, arg.as_ref().map(|_| 2));
        let mut nodes = vec![(1, node)];
        nodes.extend(arg.map(|arg| (2, constant(arg))));
        let p = root_value(nodes);
        assert_eq!(
            root_result(&p, vec![("Ref".into(), reference.clone())]),
            Ok(expected),
            "{function:?}"
        );
    }
    let overflow = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Rank", int(i64::MAX))]),
            group(vec![("Rank", int(1))]),
        ]),
    )]);
    let p = root_value(vec![(
        1,
        aggregate(Sum, named(), &["Rows"], &["Rank"], None),
    )]);
    assert_eq!(
        root_result(&p, vec![("Ref".into(), overflow)]),
        Err(EngineError::AggregateIntegerOverflow { function: Sum })
    );
    let infinite = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![("Price", text("1e999"))])]),
    )]);
    let p = root_value(vec![(
        1,
        aggregate(Sum, named(), &["Rows"], &["Price"], None),
    )]);
    assert_eq!(
        root_result(&p, vec![("Ref".into(), infinite)]),
        Err(EngineError::AggregateNonFinite { function: Sum })
    );
}

fn first_issue(project: &Project) -> String {
    crate::validate(project)
        .into_iter()
        .map(|issue| issue.to_string())
        .next()
        .unwrap_or_default()
}

#[test]
fn validation_requires_static_collections_and_nonrepeating_item_scalars() {
    use AggregateOp::*;
    let with = |node: Node| root_value(vec![(1, node)]);
    for (node, needle) in [
        (
            aggregate(
                Sum,
                SourceOwner::Primary,
                &["Rows"],
                &["Lines", "Qty"],
                None,
            ),
            "owned aggregate value `Lines/Qty`",
        ),
        (
            aggregate(Count, SourceOwner::Primary, &["Rows"], &["Lines"], None),
            "owned aggregate value `Lines`",
        ),
        // Only Count accepts the empty path over group items.
        (
            aggregate(Join, SourceOwner::Primary, &["Rows"], &[], None),
            "owned aggregate value ``",
        ),
        (
            aggregate(Count, SourceOwner::Primary, &["Note"], &[], None),
            "is not a repeating path",
        ),
        (
            aggregate(Count, SourceOwner::Primary, &["Missing"], &[], None),
            "matches no path",
        ),
        (
            aggregate(
                Count,
                SourceOwner::Named {
                    name: "Other".into(),
                },
                &["Rows"],
                &[],
                None,
            ),
            "unknown source owner `Other`",
        ),
    ] {
        let issue = first_issue(&with(node));
        assert!(issue.contains(needle), "{issue}");
    }
    engine_valid(&with(aggregate(Count, named(), &["Rows"], &[], None)));
    engine_valid(&with(aggregate(Count, named(), &["Rows"], &["Code"], None)));
}

#[test]
fn owned_aggregates_stay_closed_outside_basic_contexts() {
    let closed = |p: &Project, node: NodeId, context: OwnedCapabilityContext| {
        assert_eq!(
            crate::run_with_sources(p, &primary_input(), inputs()),
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(node),
                function: None,
                scope: None,
                context,
            })
        );
    };
    let count = || aggregate(AggregateOp::Count, named(), &["Rows"], &[], None);
    // A legacy aggregate item expression stays closed.
    let p = root_value(vec![
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
    closed(&p, 2, OwnedCapabilityContext::AggregateLookup);
    // Failure predicates keep their closed scalar subsets.
    for iteration in [
        FailureIteration::Source {
            collection: Vec::new(),
        },
        FailureIteration::OwnedSource {
            source: source(SourceOwner::Primary, &["Rows"]),
        },
    ] {
        let mut p = root_value(vec![
            (1, constant(string("x"))),
            (2, call("equal", vec![3, 4])),
            (3, count()),
            (4, constant(Value::Int(2))),
        ]);
        p.failure_rules.push(FailureRule {
            iteration,
            selection: FailureSelection::WhenTrue { predicate: 2 },
            message: None,
        });
        closed(&p, 3, OwnedCapabilityContext::FailureRule);
    }
    // A dynamic owner is unavailable before evaluation.
    let mut p = root_value(vec![(1, count()), (2, constant(string("ref.json")))]);
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(2, Vec::new()));
    assert_eq!(
        crate::run_with_sources(&p, &primary_input(), inputs()),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(1),
            function: None,
            scope: None,
            context: OwnedCapabilityContext::DynamicOwner,
        })
    );
    // Isolated functions refuse the owned kind.
    let function = FunctionId::new(1);
    let parameter = FunctionParameterId::new(1);
    let mut p = root_value(vec![
        (
            1,
            Node::UserFunctionCall {
                function,
                args: vec![2],
            },
        ),
        (2, constant(string("C"))),
    ]);
    p.user_functions.insert(
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
    assert!(!crate::validate(&p).is_empty());
    assert_eq!(
        crate::run_with_sources(&p, &primary_input(), inputs()),
        Err(EngineError::UnsupportedSourceOwnership {
            node: 2,
            function: Some(function),
            kind: "owned_aggregate",
        })
    );
}
