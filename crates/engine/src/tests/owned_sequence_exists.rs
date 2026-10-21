//! Owned endpoint reads inside a SequenceExists predicate. The predicate's
//! private generated item stays a lexical scalar frame, never an endpoint.
use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph,
    NamedSource, Node, NodeId, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope,
    ScopeIteration, SequenceExpr, SourceOwner, UserFunction,
};

use crate::{EngineError, OwnedCapabilityContext as Domain, OwnedSourceError as Access};

const UNICODE: &str = " Cfg💠 ";

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
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn found(value: bool) -> Instance {
    group(vec![("Found", scalar(Value::Bool(value)))])
}
fn constant(value: Value) -> Node {
    Node::Const { value }
}
fn string(value: &str) -> Node {
    constant(Value::String(value.into()))
}
fn call(function: &str, args: &[NodeId]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
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
fn item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
fn tokenize(input: NodeId) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input,
        delimiter: 11,
        item: 90,
    }
}
fn exists(sequence: SequenceExpr, predicate: NodeId) -> Node {
    Node::SequenceExists {
        sequence,
        predicate,
    }
}
fn raise(p: &mut Project, node: NodeId, message: &str) {
    p.graph.nodes.insert(node + 1000, string(message));
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
fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Cfg",
                vec![
                    SchemaNode::scalar("Text", ScalarType::String),
                    SchemaNode::scalar("Code", ScalarType::String),
                ],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}
fn endpoint(list: &str, code: &str, rows: &[(&str, &str)]) -> Instance {
    group(vec![
        (
            "Cfg",
            group(vec![("Text", text(list)), ("Code", text(code))]),
        ),
        (
            "Rows",
            Instance::Repeated(
                rows.iter()
                    .map(|(code, label)| group(vec![("Code", text(code)), ("Label", text(label))]))
                    .collect(),
            ),
        ),
    ])
}
fn primary() -> Instance {
    endpoint("P|Q", "Q", &[("Q", "p1"), ("Z", "p2"), ("M", "p3")])
}
fn extras() -> Vec<(String, Instance)> {
    vec![
        ("Cfg".into(), endpoint("N|M", "M", &[("M", "n1")])),
        (UNICODE.into(), endpoint("U|V", "U", &[("U", "u1")])),
    ]
}
/// `Found = exists(tokenize(10, "|"), equal(item, owned Cfg.Code))`.
fn project(owner: SourceOwner) -> Project {
    Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Found", ScalarType::Bool)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Cfg", UNICODE]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("{name}.json"),
                schema: schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (10, string("x|M|y")),
                (11, string("|")),
                (30, field(owner, &["Cfg", "Code"], Some(&[]))),
                (31, call("equal", &[90, 30])),
                (40, exists(tokenize(10), 31)),
                (90, item()),
                (901, constant(Value::Bool(true))),
                (902, constant(Value::Bool(false))),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Found".into(),
                node: 40,
            }],
            ..Scope::default()
        },
    }
}
fn run(p: &Project) -> Result<Instance, EngineError> {
    crate::run_with_sources(p, &primary(), extras())
}
fn valid(p: &Project) {
    assert!(crate::validate(p).is_empty(), "{:?}", crate::validate(p));
}

#[test]
fn predicate_reads_item_and_exact_owner_without_colliding_primary_or_alias_fallback() {
    for (owner, expected) in [
        (SourceOwner::Primary, false),
        (named("Cfg"), true),
        (named(UNICODE), false),
    ] {
        let p = project(owner.clone());
        valid(&p);
        assert_eq!(run(&p), Ok(found(expected)), "{owner:?}");
    }
    // The ordinary path through the same spelling stays the primary field.
    let mut p = project(named("Cfg"));
    p.graph.nodes.insert(
        30,
        Node::SourceField {
            path: path(&["Cfg", "Code"]),
            frame: None,
        },
    );
    assert_eq!(run(&p), Ok(found(false)));
    p.graph.nodes.insert(10, string("Q"));
    assert_eq!(run(&p), Ok(found(true)));
}

#[test]
fn producer_inputs_read_exact_owners_in_the_receiving_context() {
    let mut p = project(named("Cfg"));
    p.graph
        .nodes
        .insert(10, field(named(UNICODE), &["Cfg", "Text"], Some(&[])));
    // Unicode endpoint items U|V never equal the named Cfg code M.
    assert_eq!(run(&p), Ok(found(false)));
    p.graph
        .nodes
        .insert(10, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    assert_eq!(run(&p), Ok(found(true)));
}

#[test]
fn first_true_short_circuits_later_predicate_errors_in_source_order() {
    let mut p = project(named("Cfg"));
    // equal(item, owned) ? true : (item == "boom" ? raise : false)
    p.graph.nodes.insert(50, string("boom"));
    p.graph.nodes.insert(51, call("equal", &[90, 50]));
    raise(&mut p, 52, "late");
    p.graph.nodes.insert(
        53,
        Node::If {
            condition: 51,
            then: 52,
            else_: 902,
        },
    );
    p.graph.nodes.insert(
        54,
        Node::If {
            condition: 31,
            then: 901,
            else_: 53,
        },
    );
    p.graph.nodes.insert(40, exists(tokenize(10), 54));
    for (items, expected) in [
        ("x|M|boom", Ok(found(true))),
        ("x|boom|M", Err(exception(52, "late"))),
        ("x|y", Ok(found(false))),
    ] {
        p.graph.nodes.insert(10, string(items));
        assert_eq!(run(&p), expected, "{items}");
    }
}

#[test]
fn empty_and_null_suppressed_sequences_are_false_without_predicate_reads() {
    let mut p = project(named("Cfg"));
    for input in [constant(Value::Null), constant(Value::json_null())] {
        p.graph.nodes.insert(10, input);
        raise(&mut p, 11, "delimiter");
        // The predicate's missing selected owner is never reached.
        assert_eq!(
            crate::run_with_sources(&p, &primary(), vec![]),
            Ok(found(false))
        );
    }
    p.graph.nodes.insert(12, constant(Value::Int(3)));
    p.graph.nodes.insert(13, constant(Value::Int(1)));
    p.graph.nodes.insert(
        40,
        exists(
            SequenceExpr::Generate {
                from: Some(12),
                to: 13,
                item: 90,
            },
            31,
        ),
    );
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Ok(found(false))
    );
    raise(&mut p, 31, "predicate");
    assert_eq!(run(&p), Ok(found(false)));
}

#[test]
fn producer_failures_precede_every_predicate_evaluation() {
    let mut p = project(named("Cfg"));
    raise(&mut p, 31, "predicate");
    raise(&mut p, 10, "producer");
    assert_eq!(run(&p), Err(exception(10, "producer")));
    p.graph.nodes.insert(10, constant(Value::Int(7)));
    assert!(matches!(
        run(&p),
        Err(EngineError::Function(
            functions::FunctionError::TypeMismatch {
                function: "tokenize",
                ..
            }
        ))
    ));
    p.graph.nodes.insert(10, string("a"));
    p.graph.nodes.insert(11, string(""));
    assert!(matches!(
        run(&p),
        Err(EngineError::Function(
            functions::FunctionError::InvalidArgument {
                function: "tokenize",
                ..
            }
        ))
    ));
}

#[test]
fn non_boolean_predicates_and_missing_selected_owners_fail_when_reached() {
    let mut p = project(named("Cfg"));
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(30),
            scope: None,
            source: Access::SourceUnavailable {
                owner: named("Cfg")
            },
        })
    );
    p.graph.nodes.insert(40, exists(tokenize(10), 30));
    assert_eq!(
        run(&p),
        Err(EngineError::NotABool {
            node: 30,
            found: "string",
        })
    );
    // A true predicate before the non-boolean item short-circuits it.
    p.graph.nodes.insert(55, call("equal", &[90, 30]));
    p.graph.nodes.insert(
        56,
        Node::If {
            condition: 55,
            then: 901,
            else_: 90,
        },
    );
    p.graph.nodes.insert(40, exists(tokenize(10), 56));
    p.graph.nodes.insert(10, string("M|x"));
    assert_eq!(run(&p), Ok(found(true)));
    p.graph.nodes.insert(10, string("x|M"));
    assert_eq!(
        run(&p),
        Err(EngineError::NotABool {
            node: 56,
            found: "string",
        })
    );
}

/// Each row compares generated items against its own pinned owned row field,
/// its owned row position and the item's own generated position.
#[test]
fn nested_row_frames_keep_owned_positions_beside_generated_positions() {
    let mut p = project(named("Cfg"));
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Found", ScalarType::Bool)])
                .repeating(),
        ],
    );
    p.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            bindings: vec![Binding {
                target_field: "Found".into(),
                node: 40,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    p.graph.nodes.insert(10, string("M|Q"));
    p.graph.nodes.insert(
        30,
        field(SourceOwner::Primary, &["Rows", "Code"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(
        32,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(path(&["Rows"])),
        }),
    );
    p.graph
        .nodes
        .insert(33, Node::Position { collection: vec![] });
    // Row position equals the generated item's own position.
    p.graph.nodes.insert(34, call("equal", &[32, 33]));
    p.graph.nodes.insert(35, call("and", &[31, 34]));
    p.graph.nodes.insert(40, exists(tokenize(10), 35));
    valid(&p);
    let rows = |values: &[bool]| {
        group(vec![(
            "Rows",
            Instance::Repeated(values.iter().map(|value| found(*value)).collect()),
        )])
    };
    // Row 1 (Q) matches item 2, row 3 (M) only item 1: positions differ.
    assert_eq!(run(&p), Ok(rows(&[false, false, false])));
    p.graph.nodes.insert(10, string("Z|Z|M"));
    assert_eq!(run(&p), Ok(rows(&[false, true, true])));
    p.graph.nodes.insert(10, string("Q|Z"));
    assert_eq!(run(&p), Ok(rows(&[true, true, false])));
    p.graph.nodes.insert(10, string("Q"));
    assert_eq!(run(&p), Ok(rows(&[true, false, false])));
}

#[test]
fn nested_exists_predicates_keep_their_own_item_and_the_receiving_owner() {
    let mut p = project(named("Cfg"));
    // Outer item must be "go"; inner exists over the named Cfg list reads the
    // named Cfg code. The inner predicate only sees its own item.
    p.graph.nodes.remove(&31);
    p.graph.nodes.insert(10, string("stop|go"));
    p.graph
        .nodes
        .insert(20, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    p.graph.nodes.insert(91, item());
    p.graph.nodes.insert(36, call("equal", &[91, 30]));
    p.graph.nodes.insert(
        37,
        exists(
            SequenceExpr::Tokenize {
                input: 20,
                delimiter: 11,
                item: 91,
            },
            36,
        ),
    );
    p.graph.nodes.insert(38, string("go"));
    p.graph.nodes.insert(39, call("equal", &[90, 38]));
    p.graph.nodes.insert(41, call("and", &[39, 37]));
    p.graph.nodes.insert(40, exists(tokenize(10), 41));
    valid(&p);
    assert_eq!(run(&p), Ok(found(true)));
    p.graph
        .nodes
        .insert(30, field(SourceOwner::Primary, &["Cfg", "Code"], Some(&[])));
    assert_eq!(run(&p), Ok(found(false)));
}

#[test]
fn grouped_representatives_are_the_receiving_owned_context() {
    let mut p = project(named("Cfg"));
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![SchemaNode::scalar("Found", ScalarType::Bool)],
            )
            .repeating(),
        ],
    );
    p.graph.nodes.insert(
        30,
        field(SourceOwner::Primary, &["Rows", "Code"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(
        45,
        field(SourceOwner::Primary, &["Rows", "Label"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(10, string("Z|Q"));
    p.root = Scope {
        children: vec![Scope {
            target_field: "Groups".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            group_by: Some(45),
            bindings: vec![Binding {
                target_field: "Found".into(),
                node: 40,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    valid(&p);
    assert_eq!(
        run(&p),
        Ok(group(vec![(
            "Groups",
            Instance::Repeated(vec![found(true), found(true), found(false)])
        )]))
    );
}

fn closed(p: &Project, node: NodeId, domain: Domain) {
    assert_eq!(
        run(p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(node),
            function: None,
            scope: None,
            context: domain,
        })
    );
}

#[test]
fn recursive_collect_and_filter_map_producers_stay_closed() {
    let mut p = project(named("Cfg"));
    p.graph.nodes.insert(12, string(""));
    p.graph.nodes.insert(
        40,
        exists(
            SequenceExpr::RecursiveCollect {
                collection: path(&["Rows"]),
                children: vec![],
                descent_value: path(&["Code"]),
                values: vec![],
                value: path(&["Label"]),
                prefix: 12,
                separator: 12,
                item: 90,
            },
            31,
        ),
    );
    closed(&p, 30, Domain::GeneratedItem);

    // The owned walk is qualified only for parent-context ItemAt.
    let mut p = project(named("Cfg"));
    p.graph.nodes.insert(12, string(""));
    p.graph.nodes.insert(
        40,
        exists(
            SequenceExpr::OwnedRecursiveCollect(mapping::OwnedRecursiveCollect {
                collection: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                children: vec![],
                descent_value: path(&["Code"]),
                values: vec![],
                value: path(&["Label"]),
                prefix: 12,
                separator: 12,
                item: 90,
            }),
            31,
        ),
    );
    closed(&p, 40, Domain::GeneratedItem);

    let mut p = project(named("Cfg"));
    let (predicate, mapper) = (FunctionId::new(7), FunctionId::new(8));
    p.graph.nodes.insert(92, item());
    p.graph.nodes.insert(15, constant(Value::Int(2)));
    p.graph.nodes.insert(
        40,
        exists(
            SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
                source: Box::new(SequenceExpr::Generate {
                    from: None,
                    to: 15,
                    item: 92,
                }),
                item: 90,
                predicate,
                mapper,
                output_type: ScalarType::String,
                captures: vec![],
            }),
            31,
        ),
    );
    let parameters = || {
        vec![
            mapping::FunctionParameter {
                id: mapping::FunctionParameterId::new(0),
                name: "Value".into(),
                ty: ScalarType::Int,
            },
            mapping::FunctionParameter {
                id: mapping::FunctionParameterId::new(1),
                name: "Position".into(),
                ty: ScalarType::Int,
            },
        ]
    };
    for (id, value, ty) in [
        (predicate, Value::Bool(true), ScalarType::Bool),
        (mapper, Value::String("M".into()), ScalarType::String),
    ] {
        p.user_functions
            .insert(id, pure_function(value, ty, parameters()));
    }
    assert!(p.validate_filter_map_v1().is_ok());
    closed(&p, 30, Domain::GeneratedItem);
}

fn pure_function(
    value: Value,
    ty: ScalarType,
    parameters: Vec<mapping::FunctionParameter>,
) -> UserFunction {
    UserFunction {
        library: "test".into(),
        name: "pure".into(),
        description: None,
        parameters,
        output_name: "Result".into(),
        output_type: ty,
        body: Graph {
            nodes: BTreeMap::from([(1, constant(value))]),
        },
        output: 1,
    }
}

#[test]
fn closed_operators_and_functions_in_the_predicate_keep_their_refusals() {
    // A closed reducer beside the owned read closes the whole consumer. A
    // pure-producer SequenceAggregate is qualified separately (#474).
    let mut p = project(named("Cfg"));
    p.graph.nodes.insert(93, item());
    p.graph.nodes.insert(12, string(""));
    p.graph.nodes.insert(
        60,
        Node::SequenceAggregate {
            function: AggregateOp::Count,
            sequence: SequenceExpr::RecursiveCollect {
                collection: path(&["Rows"]),
                children: vec![],
                descent_value: path(&["Code"]),
                values: vec![],
                value: path(&["Label"]),
                prefix: 12,
                separator: 12,
                item: 93,
            },
            predicate: None,
            expression: None,
            arg: None,
        },
    );
    p.graph.nodes.insert(61, call("equal", &[60, 31]));
    p.graph.nodes.insert(40, exists(tokenize(10), 61));
    closed(&p, 30, Domain::GeneratedItem);

    let mut p = project(named("Cfg"));
    p.graph.nodes.insert(
        60,
        Node::Aggregate {
            function: AggregateOp::Count,
            collection: path(&["Rows"]),
            value: vec![],
            expression: None,
            arg: None,
        },
    );
    p.graph.nodes.insert(61, call("equal", &[60, 31]));
    p.graph.nodes.insert(40, exists(tokenize(10), 61));
    closed(&p, 30, Domain::GeneratedItem);

    let mut p = project(named("Cfg"));
    let function = FunctionId::new(7);
    p.user_functions.insert(
        function,
        pure_function(Value::Bool(true), ScalarType::Bool, vec![]),
    );
    p.graph.nodes.insert(
        60,
        Node::UserFunctionCall {
            function,
            args: vec![],
        },
    );
    p.graph.nodes.insert(61, call("and", &[60, 31]));
    p.graph.nodes.insert(40, exists(tokenize(10), 61));
    closed(&p, 30, Domain::GeneratedItem);
}

#[test]
fn failure_rules_do_not_gain_the_exists_permission() {
    let mut p = project(named("Cfg"));
    p.failure_rules = vec![FailureRule {
        iteration: FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: FailureSelection::WhenTrue { predicate: 40 },
        message: None,
    }];
    assert_eq!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(40),
            function: None,
            scope: None,
            context: Domain::FailureRule,
        })
    );
}
