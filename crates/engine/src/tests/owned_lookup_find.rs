//! #446 explicitly owned Lookup and CollectionFind.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, FunctionId,
    FunctionParameter, FunctionParameterId, Graph, NamedSource, Node, OwnedCollectionFind,
    OwnedLookup, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration,
    SourceOwner, UserFunction,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError};

fn named() -> SourceOwner {
    SourceOwner::Named {
        name: "Lookup".into(),
    }
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
fn lookup(
    owner: SourceOwner,
    collection: &[&str],
    key: &[&str],
    matches: u32,
    value: &[&str],
) -> Node {
    Node::OwnedLookup(OwnedLookup {
        collection: source(owner, collection),
        key: path(key),
        matches,
        value: path(value),
    })
}
fn find(owner: SourceOwner, collection: &[&str], predicate: u32, value: u32) -> Node {
    Node::OwnedCollectionFind(OwnedCollectionFind {
        collection: source(owner, collection),
        predicate,
        value,
    })
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn constant(value: Value) -> Node {
    Node::Const { value }
}
fn call(function: &str, args: Vec<u32>) -> Node {
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
    group(vec![
        ("Sku", text(sku)),
        ("Qty", Instance::Scalar(Value::Int(qty))),
    ])
}
fn row(code: &str, name: &str, rank: i64, lines: Vec<Instance>) -> Instance {
    group(vec![
        ("Code", text(code)),
        ("Name", text(name)),
        ("Rank", Instance::Scalar(Value::Int(rank))),
        ("Lines", Instance::Repeated(lines)),
    ])
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
                    SchemaNode::group(
                        "Lines",
                        vec![
                            SchemaNode::scalar("Sku", ScalarType::String),
                            SchemaNode::scalar("Qty", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
/// Primary and Lookup share every path but carry disjoint row codes; only
/// a nested Lookup line reuses a Primary SKU.
fn primary_input() -> Instance {
    group(vec![
        ("Note", text("P.note")),
        (
            "Rows",
            Instance::Repeated(vec![
                row("A", "P-A", 1, vec![line("a1", 1), line("a2", 5)]),
                row("B", "P-B", 2, vec![line("b1", 7)]),
                row("A", "P-A2", 3, vec![]),
            ]),
        ),
    ])
}
fn lookup_input() -> Instance {
    group(vec![
        ("Note", text("N.note")),
        (
            "Rows",
            Instance::Repeated(vec![
                row("C", "N-C", 1, vec![line("c1", 3)]),
                row("D", "N-D", 2, vec![line("a1", 9)]),
            ]),
        ),
    ])
}
fn inputs() -> Vec<(String, Instance)> {
    vec![("Lookup".into(), lookup_input())]
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
fn project(nodes: Vec<(u32, Node)>, root: Scope) -> Project {
    Project {
        source: endpoint_schema("Input"),
        target: target_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Lookup".into(),
            path: "lookup.json".into(),
            schema: endpoint_schema("Lookup"),
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
fn bind(field: &str, node: u32) -> Binding {
    Binding {
        target_field: field.into(),
        node,
    }
}
/// The root binds `Value` to node 1.
fn root_value(nodes: Vec<(u32, Node)>) -> Project {
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
fn root_result(project: &Project, extras: Vec<(String, Instance)>) -> Result<Value, EngineError> {
    engine_valid(project);
    crate::run_with_sources(project, &primary_input(), extras).map(|output| {
        output
            .field("Value")
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null)
    })
}
fn string(value: &str) -> Value {
    Value::String(value.into())
}
/// `(Code, Value)` for every constructed item; absent values read as Null.
fn items(
    project: &Project,
    extras: Vec<(String, Instance)>,
) -> Result<Vec<(Value, Value)>, EngineError> {
    engine_valid(project);
    let output = crate::run_with_sources(project, &primary_input(), extras)?;
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
fn items_scope(owner: SourceOwner, bindings: Vec<Binding>) -> Scope {
    Scope {
        children: vec![Scope {
            target_field: "Items".into(),
            iteration: ScopeIteration::OwnedSource {
                source: source(owner, &["Rows"]),
                output_path: None,
            },
            bindings,
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

#[test]
fn owned_lookup_reads_only_the_declared_endpoint_with_first_match_order() {
    for (owner, needle, expected) in [
        (SourceOwner::Primary, "A", string("P-A")),
        (SourceOwner::Primary, "B", string("P-B")),
        // A miss is Null even though the other endpoint has a matching row.
        (SourceOwner::Primary, "C", Value::Null),
        (named(), "C", string("N-C")),
        (named(), "D", string("N-D")),
        (named(), "A", Value::Null),
    ] {
        let p = root_value(vec![
            (1, lookup(owner, &["Rows"], &["Code"], 2, &["Name"])),
            (2, constant(string(needle))),
        ]);
        assert_eq!(root_result(&p, inputs()), Ok(expected), "{needle}");
    }
    // Legacy Lookup keeps primary-first resolution beside the owned form.
    let legacy = |collection: &[&str]| {
        root_value(vec![
            (
                1,
                Node::Lookup {
                    collection: path(collection),
                    key: path(&["Code"]),
                    matches: 2,
                    value: path(&["Name"]),
                },
            ),
            (2, constant(string("C"))),
        ])
    };
    assert_eq!(root_result(&legacy(&["Rows"]), inputs()), Ok(Value::Null));
    assert_eq!(
        root_result(&legacy(&["Lookup", "Rows"]), inputs()),
        Ok(string("N-C"))
    );
}

#[test]
fn owned_find_scans_items_with_owned_and_legacy_item_reads() {
    for (owner, code, expected) in [
        (SourceOwner::Primary, "A", string("P-A:1")),
        (SourceOwner::Primary, "B", string("P-B:2")),
        (SourceOwner::Primary, "D", Value::Null),
        (named(), "D", string("N-D:2")),
        (named(), "A", Value::Null),
    ] {
        let nodes = vec![
            (1, find(owner.clone(), &["Rows"], 3, 4)),
            (2, constant(string(code))),
            (3, call("equal", vec![5, 2])),
            (4, call("concat", vec![6, 7, 8])),
            (5, field(owner.clone(), &["Rows", "Code"], None)),
            (6, field(owner.clone(), &["Rows", "Name"], Some(&["Rows"]))),
            (7, constant(string(":"))),
            (
                8,
                Node::OwnedPosition(OwnedPosition {
                    owner: owner.clone(),
                    collection: Some(path(&["Rows"])),
                }),
            ),
        ];
        assert_eq!(
            root_result(&root_value(nodes.clone()), inputs()),
            Ok(expected.clone()),
            "{code}"
        );
        // Legacy item reads and positions resolve against the selected item.
        let mut legacy = nodes;
        legacy[4] = (
            5,
            Node::SourceField {
                path: path(&["Code"]),
                frame: None,
            },
        );
        legacy[7] = (
            8,
            Node::Position {
                collection: path(&["Rows"]),
            },
        );
        assert_eq!(
            root_result(&root_value(legacy), inputs()),
            Ok(expected),
            "{code}"
        );
    }
}

#[test]
fn absent_empty_null_and_unsupplied_collections_keep_needle_order() {
    let lookup_project = |needle: Node| {
        root_value(vec![
            (1, lookup(named(), &["Rows"], &["Code"], 2, &["Name"])),
            (2, needle),
        ])
    };
    let empty = group(vec![
        ("Note", text("N.note")),
        ("Rows", Instance::Repeated(vec![])),
    ]);
    let absent = group(vec![("Note", text("N.note"))]);
    let null_rows = group(vec![("Rows", Instance::Scalar(Value::Null))]);
    for lookup_root in [empty, absent, null_rows] {
        let extras = vec![("Lookup".into(), lookup_root)];
        assert_eq!(
            root_result(&lookup_project(constant(string("C"))), extras.clone()),
            Ok(Value::Null)
        );
        // Primary rows with the same path never repair a missing named collection.
        let p = root_value(vec![
            (1, find(named(), &["Rows"], 3, 4)),
            (3, constant(Value::Bool(true))),
            (4, field(named(), &["Rows", "Name"], None)),
        ]);
        assert_eq!(root_result(&p, extras.clone()), Ok(Value::Null));
        // The needle still evaluates once in the parent before the walk.
        assert_eq!(
            root_result(&lookup_project(Node::Raise { message: None }), extras),
            Err(EngineError::MappingException {
                node: 2,
                message: None
            })
        );
    }
    // Absent and Null keys never match, so a missing needle misses.
    let keyless = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Name", text("no-key"))]),
            group(vec![
                ("Code", Instance::Scalar(Value::Null)),
                ("Name", text("null-key")),
            ]),
        ]),
    )]);
    assert_eq!(
        root_result(
            &lookup_project(constant(Value::Null)),
            vec![("Lookup".into(), keyless)]
        ),
        Ok(Value::Null)
    );
    // An unsupplied owner fails after the needle; it is never an empty walk.
    assert_eq!(
        root_result(&lookup_project(constant(string("C"))), vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: OwnedSourceError::SourceUnavailable { owner: named() },
        })
    );
}

#[test]
fn active_nested_parent_anchors_same_owner_walks_only() {
    let p = project(
        vec![
            (1, field(SourceOwner::Primary, &["Rows", "Code"], None)),
            (2, call("concat", vec![3, 4, 5])),
            (
                3,
                lookup(
                    SourceOwner::Primary,
                    &["Rows", "Lines"],
                    &["Sku"],
                    6,
                    &["Qty"],
                ),
            ),
            (4, constant(string("/"))),
            (
                5,
                lookup(named(), &["Rows", "Lines"], &["Sku"], 6, &["Qty"]),
            ),
            (6, constant(string("a1"))),
        ],
        items_scope(
            SourceOwner::Primary,
            vec![bind("Code", 1), bind("Value", 2)],
        ),
    );
    // Primary lines come from the current row only; the named walk has no
    // active frame and flattens every Lookup row's lines.
    assert_eq!(
        items(&p, inputs()),
        Ok(vec![
            (string("A"), string("1/9")),
            (string("B"), string("/9")),
            (string("A"), string("/9")),
        ])
    );
    // The same applies from a named parent: Primary walks start at its root.
    let p = project(
        vec![
            (1, field(named(), &["Rows", "Code"], None)),
            (2, call("concat", vec![3, 4, 5])),
            (
                3,
                lookup(
                    SourceOwner::Primary,
                    &["Rows", "Lines"],
                    &["Sku"],
                    6,
                    &["Qty"],
                ),
            ),
            (4, constant(string("/"))),
            (
                5,
                lookup(named(), &["Rows", "Lines"], &["Sku"], 6, &["Qty"]),
            ),
            (6, constant(string("a1"))),
        ],
        items_scope(named(), vec![bind("Code", 1), bind("Value", 2)]),
    );
    assert_eq!(
        items(&p, inputs()),
        Ok(vec![
            (string("C"), string("1/")),
            (string("D"), string("1/9"))
        ])
    );
}

#[test]
fn find_items_read_explicit_owners_and_the_active_parent() {
    let p = project(
        vec![
            (1, field(SourceOwner::Primary, &["Rows", "Code"], None)),
            (2, find(named(), &["Rows"], 3, 6)),
            (3, call("equal", vec![4, 5])),
            (4, field(named(), &["Rows", "Rank"], Some(&["Rows"]))),
            (
                5,
                field(SourceOwner::Primary, &["Rows", "Rank"], Some(&["Rows"])),
            ),
            (6, call("concat", vec![7, 8, 9, 8, 10])),
            (7, field(named(), &["Rows", "Name"], None)),
            (8, constant(string("|"))),
            (9, field(SourceOwner::Primary, &["Rows", "Name"], None)),
            (10, field(named(), &["Note"], Some(&[]))),
        ],
        items_scope(
            SourceOwner::Primary,
            vec![bind("Code", 1), bind("Value", 2)],
        ),
    );
    assert_eq!(
        items(&p, inputs()),
        Ok(vec![
            (string("A"), string("N-C|P-A|N.note")),
            (string("B"), string("N-D|P-B|N.note")),
            (string("A"), Value::Null),
        ])
    );
}

#[test]
fn predicates_skip_null_like_values_and_stop_at_the_first_match() {
    let project_with = |owner: SourceOwner, predicate: Node| {
        root_value(vec![
            (1, find(owner.clone(), &["Rows"], 3, 4)),
            (3, predicate),
            (4, field(owner.clone(), &["Rows", "Name"], None)),
            (5, field(owner, &["Rows", "Code"], None)),
            (6, constant(string("A"))),
            (7, call("equal", vec![5, 6])),
            (8, constant(Value::Bool(true))),
            (9, Node::Raise { message: None }),
            (10, constant(Value::Null)),
            (11, constant(string("yes"))),
        ])
    };
    // The first match returns before a later predicate would raise.
    let guarded = Node::If {
        condition: 7,
        then: 8,
        else_: 9,
    };
    assert_eq!(
        root_result(
            &project_with(SourceOwner::Primary, guarded.clone()),
            inputs()
        ),
        Ok(string("P-A"))
    );
    assert_eq!(
        root_result(&project_with(named(), guarded), inputs()),
        Err(EngineError::MappingException {
            node: 9,
            message: None
        })
    );
    // Null predicates skip every item; a non-Boolean is a typed failure.
    assert_eq!(
        root_result(
            &project_with(SourceOwner::Primary, constant(Value::Null)),
            inputs()
        ),
        Ok(Value::Null)
    );
    assert_eq!(
        root_result(
            &project_with(SourceOwner::Primary, constant(string("yes"))),
            inputs()
        ),
        Err(EngineError::NotABool {
            node: 3,
            found: "string"
        })
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
    let with = |node: Node| root_value(vec![(1, node), (2, constant(string("A")))]);
    for (node, needle) in [
        (
            lookup(
                SourceOwner::Primary,
                &["Rows"],
                &["Lines", "Sku"],
                2,
                &["Name"],
            ),
            "lookup key `Lines/Sku`",
        ),
        (
            lookup(SourceOwner::Primary, &["Rows"], &["Code"], 2, &["Lines"]),
            "lookup value `Lines`",
        ),
        (
            lookup(SourceOwner::Primary, &["Note"], &[], 2, &[]),
            "is not a repeating path",
        ),
        (
            find(SourceOwner::Primary, &["Missing"], 2, 2),
            "matches no path",
        ),
        (
            lookup(
                SourceOwner::Named {
                    name: "Other".into(),
                },
                &["Rows"],
                &["Code"],
                2,
                &["Name"],
            ),
            "unknown source owner `Other`",
        ),
    ] {
        let issue = first_issue(&with(node));
        assert!(issue.contains(needle), "{issue}");
    }
}

#[test]
fn owned_search_stays_closed_outside_basic_contexts() {
    let closed = |p: &Project, node: u32, context: OwnedCapabilityContext| {
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
    let search = |id| lookup(named(), &["Rows"], &["Code"], id, &["Name"]);
    // A legacy aggregate or collection-find item expression stays closed.
    let p = root_value(vec![
        (
            1,
            Node::Aggregate {
                function: AggregateOp::Count,
                collection: path(&["Rows"]),
                value: Vec::new(),
                expression: Some(2),
                arg: None,
            },
        ),
        (2, search(3)),
        (3, constant(string("C"))),
    ]);
    closed(&p, 2, OwnedCapabilityContext::AggregateLookup);
    let p = root_value(vec![
        (
            1,
            Node::CollectionFind {
                collection: path(&["Rows"]),
                predicate: 4,
                value: 2,
            },
        ),
        (2, search(3)),
        (3, constant(string("C"))),
        (4, constant(Value::Bool(true))),
    ]);
    closed(&p, 2, OwnedCapabilityContext::AggregateLookup);
    // Failure predicates keep their closed scalar subsets.
    for iteration in [
        FailureIteration::Source {
            collection: Vec::new(),
        },
        FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        FailureIteration::OwnedSource {
            source: source(SourceOwner::Primary, &["Rows"]),
        },
    ] {
        let mut p = root_value(vec![
            (1, constant(string("x"))),
            (2, call("equal", vec![3, 4])),
            (3, search(4)),
            (4, constant(string("C"))),
        ]);
        p.failure_rules.push(FailureRule {
            iteration,
            selection: FailureSelection::WhenTrue { predicate: 2 },
            message: None,
        });
        closed(&p, 3, OwnedCapabilityContext::FailureRule);
    }
    // A dynamic owner is unavailable before evaluation.
    let mut p = root_value(vec![(1, search(2)), (2, constant(string("C")))]);
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 2,
        iteration: Vec::new(),
    });
    assert_eq!(
        crate::run_with_sources(&p, &primary_input(), inputs()),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(1),
            function: None,
            scope: None,
            context: OwnedCapabilityContext::DynamicOwner,
        })
    );
}

#[test]
fn isolated_functions_refuse_owned_search() {
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
            name: "search".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: parameter,
                name: "code".into(),
                ty: ScalarType::String,
            }],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [
                    (1, Node::FunctionParameter { parameter }),
                    (2, lookup(named(), &["Rows"], &["Code"], 1, &["Name"])),
                ]
                .into(),
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
            kind: "owned_lookup",
        })
    );
}
