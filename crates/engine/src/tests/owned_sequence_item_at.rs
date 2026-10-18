//! Literal owned ItemAt controls; synthetic items never become source endpoints.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, FunctionId,
    FunctionParameter, FunctionParameterId, Graph, NamedSource, NamedTarget, Node, NodeId,
    OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration, SequenceExpr,
    SourceOwner, UserFunction,
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
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn result(value: Value) -> Instance {
    group(vec![("Result", scalar(value))])
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
fn item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
fn binding(name: &str, node: NodeId) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn iteration(owner: SourceOwner, parts: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        output_path: None,
    }
}
fn schema(index: ScalarType) -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Cfg",
                vec![
                    SchemaNode::scalar("Index", index),
                    SchemaNode::scalar("Text", ScalarType::String),
                    SchemaNode::scalar("Width", ScalarType::Int),
                    SchemaNode::scalar("From", ScalarType::Int),
                    SchemaNode::scalar("To", ScalarType::Int),
                    SchemaNode::scalar("Pattern", ScalarType::String),
                    SchemaNode::scalar("Flags", ScalarType::String),
                ],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Index", ScalarType::Int),
                    SchemaNode::scalar("Text", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}
fn endpoint(index: Value, value: Value) -> Instance {
    group(vec![
        (
            "Cfg",
            group(vec![
                ("Index", scalar(index)),
                ("Text", scalar(value)),
                ("Width", scalar(Value::Int(2))),
                ("From", scalar(Value::Int(10))),
                ("To", scalar(Value::Int(12))),
                ("Pattern", text("[,;]")),
                ("Flags", text("")),
            ]),
        ),
        ("Rows", Instance::Repeated(vec![])),
    ])
}
fn primary() -> Instance {
    endpoint(Value::Int(1), Value::String("P-a|P-b|P-c".into()))
}
fn extras() -> Vec<(String, Instance)> {
    vec![
        (
            "Cfg".into(),
            endpoint(Value::Int(3), Value::String("N-a|N-b|N-c".into())),
        ),
        (
            " Cfg💠 ".into(),
            endpoint(Value::Int(2), Value::String("U-a|U-b|U-c".into())),
        ),
    ]
}
fn tokenize(item: NodeId) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input: 10,
        delimiter: 11,
        item,
    }
}
fn project(sequence: SequenceExpr, output_type: ScalarType) -> Project {
    Project {
        source: schema(ScalarType::Int),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Result", output_type)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Cfg", " Cfg💠 "]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("{name}.json"),
                schema: schema(ScalarType::Int),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (10, constant(Value::String("a|β|終".into()))),
                (11, constant(Value::String("|".into()))),
                (12, constant(Value::String("".into()))),
                (13, constant(Value::Int(2))),
                (14, constant(Value::String("".into()))),
                (15, constant(Value::Int(3))),
                (30, field(named("Cfg"), &["Cfg", "Index"], Some(&[]))),
                (
                    40,
                    Node::SequenceItemAt {
                        sequence,
                        index: 30,
                    },
                ),
                (90, item()),
            ]),
        },
        root: Scope {
            bindings: vec![binding("Result", 40)],
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
#[derive(Default)]
struct Events(RefCell<Vec<crate::TraceEvent>>);
impl crate::TraceSink for Events {
    fn record(&self, event: crate::TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}
impl Events {
    fn inputs(&self, owner: NodeId) -> Vec<(NodeId, usize)> {
        self.0
            .borrow()
            .iter()
            .filter_map(|event| match event {
                crate::TraceEvent::NodeInputValue {
                    consumer,
                    input,
                    input_index,
                    ..
                } if *consumer == owner => Some((*input, *input_index)),
                _ => None,
            })
            .collect()
    }
    fn values(&self, expected: NodeId) -> usize {
        self.0.borrow().iter().filter(|event| matches!(event, crate::TraceEvent::NodeValue { node, .. } if *node == expected)).count()
    }
}

#[test]
fn exact_named_index_and_unicode_owner_do_not_read_colliding_primary_cfg() {
    for (owner, expected) in [(named("Cfg"), "終"), (named(" Cfg💠 "), "β")] {
        let mut p = project(tokenize(90), ScalarType::String);
        p.graph
            .nodes
            .insert(30, field(owner, &["Cfg", "Index"], Some(&[])));
        valid(&p);
        assert_eq!(run(&p), Ok(result(Value::String(expected.into()))));
    }
}

#[test]
fn owned_and_mixed_legacy_parent_arguments_are_evaluated_in_the_receiving_context() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(10, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    valid(&p);
    assert_eq!(run(&p), Ok(result(Value::String("N-c".into()))));
    p.graph.nodes.insert(
        10,
        Node::SourceField {
            path: path(&["Cfg", "Text"]),
            frame: None,
        },
    );
    valid(&p);
    assert_eq!(run(&p), Ok(result(Value::String("P-c".into()))));
}

#[test]
fn generate_owned_from_to_and_index_preserve_int_tag_and_omitted_from_default() {
    let mut p = project(
        SequenceExpr::Generate {
            from: Some(10),
            to: 11,
            item: 90,
        },
        ScalarType::Int,
    );
    p.graph
        .nodes
        .insert(10, field(named("Cfg"), &["Cfg", "From"], Some(&[])));
    p.graph
        .nodes
        .insert(11, field(named("Cfg"), &["Cfg", "To"], Some(&[])));
    valid(&p);
    assert_eq!(run(&p), Ok(result(Value::Int(12))));
    p.graph.nodes.insert(
        40,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 15,
                item: 90,
            },
            index: 30,
        },
    );
    valid(&p);
    assert_eq!(run(&p), Ok(result(Value::Int(3))));
}

#[test]
fn by_length_and_regex_keep_unicode_and_optional_flag_semantics() {
    let mut p = project(
        SequenceExpr::TokenizeByLength {
            input: 10,
            length: 13,
            item: 90,
        },
        ScalarType::String,
    );
    p.graph
        .nodes
        .insert(10, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    p.graph
        .nodes
        .insert(13, field(named("Cfg"), &["Cfg", "Width"], Some(&[])));
    valid(&p);
    assert_eq!(
        crate::run_with_sources(
            &p,
            &primary(),
            vec![(
                "Cfg".into(),
                endpoint(Value::Int(2), Value::String("a💠βzQ".into()))
            )]
        ),
        Ok(result(Value::String("βz".into())))
    );
    for flags in [None, Some(14)] {
        p.graph.nodes.insert(
            40,
            Node::SequenceItemAt {
                sequence: SequenceExpr::TokenizeRegex {
                    input: 10,
                    pattern: 11,
                    flags,
                    item: 90,
                },
                index: 30,
            },
        );
        p.graph
            .nodes
            .insert(11, field(named("Cfg"), &["Cfg", "Pattern"], Some(&[])));
        p.graph
            .nodes
            .insert(14, field(named("Cfg"), &["Cfg", "Flags"], Some(&[])));
        valid(&p);
        assert_eq!(
            crate::run_with_sources(
                &p,
                &primary(),
                vec![(
                    "Cfg".into(),
                    endpoint(Value::Int(2), Value::String("a,bb;終".into()))
                )]
            ),
            Ok(result(Value::String("bb".into())))
        );
    }
}

#[test]
fn nested_eligible_item_at_is_a_parent_scalar_not_a_generated_context() {
    let mut p = project(
        SequenceExpr::Generate {
            from: None,
            to: 15,
            item: 90,
        },
        ScalarType::Int,
    );
    p.graph.nodes.insert(91, item());
    p.graph.nodes.insert(
        31,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 15,
                item: 91,
            },
            index: 30,
        },
    );
    p.graph.nodes.insert(
        40,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 31,
                item: 90,
            },
            index: 31,
        },
    );
    valid(&p);
    assert_eq!(run(&p), Ok(result(Value::Int(3))));
}

#[test]
fn shared_argument_and_index_node_is_evaluated_at_both_edges_without_item_callbacks() {
    let p = project(
        SequenceExpr::Generate {
            from: None,
            to: 30,
            item: 90,
        },
        ScalarType::Int,
    );
    valid(&p);
    let events = Events::default();
    let context = crate::ExecutionContext::new(Path::new("item-at.json")).with_trace_sink(&events);
    assert_eq!(
        crate::run_with_sources_and_context(&p, &primary(), extras(), &context),
        Ok(result(Value::Int(3)))
    );
    assert_eq!(events.inputs(40), vec![(30, 0), (30, 1)]);
    assert_eq!(events.values(30), 2);
    assert_eq!(events.values(90), 0);
}

#[test]
fn index_is_once_after_successful_empty_production_and_null_suppression() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(10, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    raises(&mut p, 11, "LATER");
    let events = Events::default();
    let context = crate::ExecutionContext::new(Path::new("item-at.json")).with_trace_sink(&events);
    assert_eq!(
        crate::run_with_sources_and_context(
            &p,
            &primary(),
            vec![("Cfg".into(), endpoint(Value::Int(3), Value::Null))],
            &context
        ),
        Ok(result(Value::Null))
    );
    assert_eq!(events.inputs(40), vec![(10, 0), (30, 2)]);
    assert_eq!(events.values(11), 0);
    assert_eq!(events.values(30), 1);
}

#[test]
fn empty_and_null_producers_still_raise_the_index_in_original_order() {
    for null in [Value::Null, Value::JsonNull(ir::JsonNull)] {
        for sequence in [
            tokenize(90),
            SequenceExpr::TokenizeByLength {
                input: 10,
                length: 11,
                item: 90,
            },
            SequenceExpr::TokenizeRegex {
                input: 10,
                pattern: 11,
                flags: Some(14),
                item: 90,
            },
            SequenceExpr::Generate {
                from: Some(10),
                to: 11,
                item: 90,
            },
        ] {
            let mut p = project(
                sequence.clone(),
                if matches!(sequence, SequenceExpr::Generate { .. }) {
                    ScalarType::Int
                } else {
                    ScalarType::String
                },
            );
            let leaf = if matches!(sequence, SequenceExpr::Generate { .. }) {
                "Index"
            } else {
                "Text"
            };
            p.graph
                .nodes
                .insert(10, field(named("Cfg"), &["Cfg", leaf], Some(&[])));
            raises(&mut p, 11, "LATER");
            raises(&mut p, 14, "FLAGS");
            raises(&mut p, 30, "INDEX");
            let supplied = endpoint(null.clone(), null.clone());
            assert_eq!(
                crate::run_with_sources(&p, &primary(), vec![("Cfg".into(), supplied)]),
                Err(exception(30, "INDEX"))
            );
        }
    }
    let mut p = project(
        SequenceExpr::Generate {
            from: Some(10),
            to: 11,
            item: 90,
        },
        ScalarType::Int,
    );
    p.root.iteration = iteration(SourceOwner::Primary, &[]);
    p.graph.nodes.insert(10, constant(Value::Int(4)));
    p.graph.nodes.insert(11, constant(Value::Int(3)));
    raises(&mut p, 30, "INDEX");
    assert_eq!(run(&p), Err(exception(30, "INDEX")));
}

#[test]
fn producer_failures_and_limits_precede_even_unavailable_or_raising_index() {
    let mut p = project(tokenize(90), ScalarType::String);
    raises(&mut p, 10, "PRODUCER");
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Err(exception(10, "PRODUCER"))
    );
    raises(&mut p, 30, "INDEX");
    p.root.iteration = iteration(SourceOwner::Primary, &[]);
    assert_eq!(run(&p), Err(exception(10, "PRODUCER")));
    p.graph
        .nodes
        .insert(10, constant(Value::String("text".into())));
    p.graph.nodes.insert(11, constant(Value::String("".into())));
    assert_eq!(
        run(&p),
        Err(EngineError::Function(
            functions::FunctionError::InvalidArgument {
                function: "tokenize",
                message: "requires a non-empty delimiter"
            }
        ))
    );
    let mut p = project(
        SequenceExpr::Generate {
            from: None,
            to: 15,
            item: 90,
        },
        ScalarType::Int,
    );
    p.graph.nodes.insert(15, constant(Value::Int(1_000_001)));
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Err(EngineError::GeneratedSequenceTooLarge {
            requested: 1_000_001,
            max: 1_000_000
        })
    );
}

#[test]
fn xml_nil_is_not_producer_null_and_empty_literal_and_regex_differ() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(10, constant(Value::XmlNil(ir::XmlNil)));
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Err(EngineError::Function(
            functions::FunctionError::TypeMismatch {
                function: "tokenize",
                got: "xml nil"
            }
        ))
    );
    p.graph.nodes.insert(10, constant(Value::String("".into())));
    let supplied = vec![(
        "Cfg".into(),
        endpoint(Value::Int(1), Value::String("unused".into())),
    )];
    assert_eq!(
        crate::run_with_sources(&p, &primary(), supplied.clone()),
        Ok(result(Value::String("".into())))
    );
    p.graph
        .nodes
        .insert(11, constant(Value::String(",".into())));
    p.graph.nodes.insert(
        40,
        Node::SequenceItemAt {
            sequence: SequenceExpr::TokenizeRegex {
                input: 10,
                pattern: 11,
                flags: None,
                item: 90,
            },
            index: 30,
        },
    );
    assert_eq!(
        crate::run_with_sources(&p, &primary(), supplied),
        Ok(result(Value::Null))
    );
}

#[test]
fn index_conversion_is_selection_rounding_not_window_item_count() {
    let cases = [
        (Value::Int(0), Value::Null),
        (Value::Int(-1), Value::Null),
        (Value::Int(4), Value::Null),
        (Value::Int(i64::MAX), Value::Null),
        (Value::Bool(true), Value::Null),
        (Value::Null, Value::Null),
        (Value::JsonNull(ir::JsonNull), Value::Null),
        (Value::XmlNil(ir::XmlNil), Value::Null),
        (Value::String("2.0".into()), Value::Null),
        (Value::String("2e0".into()), Value::Null),
        (Value::String(" 2 ".into()), Value::String("β".into())),
        (Value::Float(2.49), Value::String("β".into())),
        (Value::Float(2.5), Value::String("終".into())),
        (Value::Float(0.5), Value::String("a".into())),
        (Value::Float(-0.5), Value::Null),
        (Value::Float(f64::NAN), Value::Null),
        (Value::Float(f64::INFINITY), Value::Null),
        (Value::Float(f64::NEG_INFINITY), Value::Null),
    ];
    for (index, expected) in cases {
        let mut p = project(tokenize(90), ScalarType::String);
        let ty = match index {
            Value::String(_) => ScalarType::String,
            Value::Float(_) => ScalarType::Float,
            Value::Bool(_) => ScalarType::Bool,
            _ => ScalarType::Int,
        };
        p.extra_sources[0].schema = schema(ty);
        valid(&p);
        assert_eq!(
            crate::run_with_sources(
                &p,
                &primary(),
                vec![(
                    "Cfg".into(),
                    endpoint(index.clone(), Value::String("N".into()))
                )]
            ),
            Ok(result(expected)),
            "index={index:?}"
        );
    }
}

#[test]
fn empty_does_not_hide_exact_owner_unavailability_or_missing_field() {
    let mut p = project(tokenize(90), ScalarType::String);
    let unavailable = || EngineError::OwnedSourceAccess {
        node: Some(30),
        scope: None,
        source: Access::SourceUnavailable {
            owner: named("Cfg"),
        },
    };
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Err(unavailable())
    );
    p.graph.nodes.insert(10, constant(Value::Null));
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Err(unavailable())
    );
    let present_but_missing = group(vec![("Cfg", group(vec![("Text", text("no-index"))]))]);
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![("Cfg".into(), present_but_missing)]),
        Ok(result(Value::Null))
    );
}

#[test]
fn malformed_pinned_index_keeps_node_path_and_original_access_cause() {
    let p = project(tokenize(90), ScalarType::String);
    for (cfg, source) in [
        (
            scalar(Value::Int(7)),
            Access::MalformedOwnedTraversal {
                path: path(&["Cfg", "Index"]),
                found: "a non-null scalar",
            },
        ),
        (
            group(vec![(
                "Index",
                Instance::Repeated(vec![scalar(Value::Int(3))]),
            )]),
            Access::UniteratedOwnedRepetition {
                path: path(&["Cfg", "Index"]),
            },
        ),
    ] {
        assert_eq!(
            crate::run_with_sources(
                &p,
                &primary(),
                vec![("Cfg".into(), group(vec![("Cfg", cfg)]))]
            ),
            Err(EngineError::OwnedSourceAccess {
                node: Some(30),
                scope: None,
                source
            })
        );
    }
}

#[test]
fn inactive_named_frame_is_not_repaired_by_an_active_primary_rows_frame() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.root.iteration = iteration(SourceOwner::Primary, &["Rows"]);
    p.target.repeating = true;
    p.graph
        .nodes
        .insert(30, field(named("Cfg"), &["Rows", "Index"], Some(&["Rows"])));
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![
            ("Key", text("A")),
            ("Index", scalar(Value::Int(1))),
            ("Text", text("P")),
        ])]),
    )]);
    assert_eq!(
        crate::run_with_sources(&p, &primary, extras()),
        Err(EngineError::OwnedSourceAccess {
            node: Some(30),
            scope: None,
            source: Access::InactiveSourceFrame {
                owner: named("Cfg"),
                collection: Some(path(&["Rows"]))
            }
        })
    );
    p.graph.nodes.insert(
        30,
        Node::OwnedPosition(OwnedPosition {
            owner: named("Cfg"),
            collection: Some(path(&["Rows"])),
        }),
    );
    assert_eq!(
        crate::run_with_sources(&p, &primary, extras()),
        Err(EngineError::OwnedSourceAccess {
            node: Some(30),
            scope: None,
            source: Access::InactiveSourceFrame {
                owner: named("Cfg"),
                collection: Some(path(&["Rows"]))
            },
        })
    );
}

#[test]
fn admission_failure_has_no_producer_or_index_observation() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(30, field(named("unknown"), &["Cfg", "Index"], Some(&[])));
    raises(&mut p, 10, "must-not-run");
    let events = Events::default();
    let context = crate::ExecutionContext::new(Path::new("item-at.json")).with_trace_sink(&events);
    assert_eq!(
        crate::run_with_sources_and_context(&p, &primary(), extras(), &context),
        Err(EngineError::OwnedSourceAccess {
            node: Some(30),
            scope: None,
            source: Access::UnknownOwner {
                name: "unknown".into()
            }
        })
    );
    assert!(events.0.borrow().is_empty());
}

fn row(key: &str, index: i64) -> Instance {
    group(vec![
        ("Key", text(key)),
        ("Index", scalar(Value::Int(index))),
        ("Text", text("x|y|z")),
    ])
}
fn grouped_project(root_collection: bool) -> Project {
    let mut p = project(tokenize(90), ScalarType::String);
    let collection: &[&str] = if root_collection { &[] } else { &["Rows"] };
    let index_path: &[&str] = if root_collection {
        &["Index"]
    } else {
        &["Rows", "Index"]
    };
    let key_path: &[&str] = if root_collection {
        &["Key"]
    } else {
        &["Rows", "Key"]
    };
    p.graph
        .nodes
        .insert(10, constant(Value::String("x|y|z".into())));
    p.graph
        .nodes
        .insert(30, field(named("Cfg"), index_path, None));
    p.graph.nodes.insert(
        31,
        Node::OwnedPosition(OwnedPosition {
            owner: named("Cfg"),
            collection: Some(path(collection)),
        }),
    );
    p.graph
        .nodes
        .insert(32, field(named("Cfg"), index_path, Some(collection)));
    p.graph.nodes.insert(
        41,
        Node::SequenceItemAt {
            sequence: tokenize(91),
            index: 31,
        },
    );
    p.graph.nodes.insert(
        42,
        Node::SequenceItemAt {
            sequence: tokenize(92),
            index: 32,
        },
    );
    p.graph
        .nodes
        .insert(50, field(named("Cfg"), key_path, None));
    p.graph.nodes.insert(91, item());
    p.graph.nodes.insert(92, item());
    let groups = SchemaNode::group(
        "Groups",
        vec![
            SchemaNode::scalar("Result", ScalarType::String),
            SchemaNode::scalar("ByPosition", ScalarType::String),
            SchemaNode::group(
                "Members",
                vec![
                    SchemaNode::scalar("Result", ScalarType::String),
                    SchemaNode::scalar("ByPosition", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
    .repeating();
    p.target = SchemaNode::group("Output", vec![groups]);
    p.root = Scope {
        children: vec![Scope {
            target_field: "Groups".into(),
            iteration: iteration(named("Cfg"), collection),
            group_by: Some(50),
            bindings: vec![binding("Result", 40), binding("ByPosition", 41)],
            children: vec![Scope {
                target_field: "Members".into(),
                iteration: iteration(named("Cfg"), collection),
                bindings: vec![binding("Result", 42), binding("ByPosition", 41)],
                ..Scope::default()
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    if root_collection {
        p.extra_sources[0].schema = schema(ScalarType::Int).child("Rows").unwrap().clone();
    }
    p
}
fn grouped_expected() -> Instance {
    group(vec![(
        "Groups",
        Instance::Repeated(vec![
            group(vec![
                ("Result", text("z")),
                ("ByPosition", text("x")),
                (
                    "Members",
                    Instance::Repeated(vec![
                        group(vec![("Result", text("z")), ("ByPosition", text("x"))]),
                        group(vec![("Result", text("x")), ("ByPosition", text("y"))]),
                    ]),
                ),
            ]),
            group(vec![
                ("Result", text("y")),
                ("ByPosition", text("y")),
                (
                    "Members",
                    Instance::Repeated(vec![group(vec![
                        ("Result", text("y")),
                        ("ByPosition", text("x")),
                    ])]),
                ),
            ]),
        ]),
    )])
}

#[test]
fn group_representatives_member_pins_and_distinct_position_spaces_feed_item_at() {
    for root_collection in [false, true] {
        let p = grouped_project(root_collection);
        valid(&p);
        let rows = Instance::Repeated(vec![row("A", 3), row("A", 1), row("B", 2)]);
        let supplied = if root_collection {
            rows
        } else {
            group(vec![("Rows", rows)])
        };
        assert_eq!(
            crate::run_with_sources(&p, &primary(), vec![("Cfg".into(), supplied)]),
            Ok(grouped_expected())
        );
    }
}

#[test]
fn grouped_empty_root_index_pin_uses_immutable_repetition_not_representative() {
    let mut p = grouped_project(true);
    p.graph
        .nodes
        .insert(30, field(named("Cfg"), &["Index"], Some(&[])));
    valid(&p);
    assert_eq!(
        crate::run_with_sources(
            &p,
            &primary(),
            vec![(
                "Cfg".into(),
                Instance::Repeated(vec![row("A", 3), row("A", 1)])
            )]
        ),
        Err(EngineError::OwnedSourceAccess {
            node: Some(30),
            scope: None,
            source: Access::UniteratedOwnedRepetition {
                path: path(&["Index"])
            }
        })
    );
}

#[test]
fn ordinary_rows_keep_parent_index_root_pin_and_position_without_generated_cursors() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.root.iteration = iteration(named("Cfg"), &["Rows"]);
    p.target.repeating = true;
    p.graph
        .nodes
        .insert(10, constant(Value::String("a|b|c".into())));
    p.graph
        .nodes
        .insert(30, field(named("Cfg"), &["Rows", "Index"], None));
    let supplied = group(vec![
        ("Cfg", group(vec![("Index", scalar(Value::Int(3)))])),
        ("Rows", Instance::Repeated(vec![row("A", 3), row("B", 1)])),
    ]);
    valid(&p);
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![("Cfg".into(), supplied.clone())]),
        Ok(Instance::Repeated(vec![
            result(Value::String("c".into())),
            result(Value::String("a".into()))
        ]))
    );
    p.graph
        .nodes
        .insert(30, field(named("Cfg"), &["Cfg", "Index"], Some(&[])));
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![("Cfg".into(), supplied.clone())]),
        Ok(Instance::Repeated(vec![
            result(Value::String("c".into())),
            result(Value::String("c".into()))
        ]))
    );
    p.graph.nodes.insert(
        30,
        Node::OwnedPosition(OwnedPosition {
            owner: named("Cfg"),
            collection: Some(path(&["Rows"])),
        }),
    );
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![("Cfg".into(), supplied)]),
        Ok(Instance::Repeated(vec![
            result(Value::String("a".into())),
            result(Value::String("b".into()))
        ]))
    );
}

#[test]
fn inherited_closed_context_and_context_keyed_reuse_are_not_erased() {
    let p = project(tokenize(90), ScalarType::String);
    for domain in [
        Domain::GeneratedItem,
        Domain::Grouping,
        Domain::AggregateLookup,
        Domain::Join,
        Domain::Document,
        Domain::DynamicTarget,
        Domain::FailureRule,
        Domain::IsolatedFunction,
    ] {
        assert!(
            matches!(crate::source_ownership::check_context_reuse_for_test(&p, 40, domain), Err(EngineError::UnsupportedOwnedContext { node: Some(30), scope: None, context, .. }) if context == domain)
        );
    }
}

#[test]
fn sequence_exists_and_sequence_aggregate_item_at_reducer_stay_closed() {
    for node in [
        Node::SequenceExists {
            sequence: tokenize(90),
            predicate: 30,
        },
        Node::SequenceAggregate {
            function: AggregateOp::ItemAt,
            sequence: tokenize(90),
            predicate: None,
            expression: None,
            arg: Some(30),
        },
    ] {
        let mut p = project(tokenize(90), ScalarType::String);
        p.graph.nodes.insert(40, node);
        assert!(matches!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(30),
                context: Domain::GeneratedItem,
                ..
            })
        ));
    }
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph.nodes.insert(
        60,
        Node::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Key"]),
            matches: 40,
            value: path(&["Text"]),
        },
    );
    p.root.bindings[0].node = 60;
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(30),
            context: Domain::AggregateLookup,
            ..
        })
    ));
}

#[test]
fn recursive_collect_is_not_opened_by_item_at_or_an_owned_index() {
    let p = project(
        SequenceExpr::RecursiveCollect {
            collection: path(&["Rows"]),
            children: vec![],
            descent_value: path(&["Key"]),
            values: vec![],
            value: path(&["Text"]),
            prefix: 12,
            separator: 12,
            item: 90,
        },
        ScalarType::String,
    );
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(30),
            context: Domain::GeneratedItem,
            ..
        })
    ));
}

fn pure_function(value: Value, ty: ScalarType, parameters: Vec<FunctionParameter>) -> UserFunction {
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
fn filter_map_remains_closed_with_a_structurally_valid_pure_stage_pair() {
    let predicate = FunctionId::new(7);
    let mapper = FunctionId::new(8);
    let mut p = project(
        SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
            source: Box::new(SequenceExpr::Generate {
                from: None,
                to: 15,
                item: 91,
            }),
            item: 90,
            predicate,
            mapper,
            output_type: ScalarType::Int,
            captures: vec![],
        }),
        ScalarType::Int,
    );
    p.graph.nodes.insert(91, item());
    let parameters = || {
        vec![
            FunctionParameter {
                id: FunctionParameterId::new(0),
                name: "Value".into(),
                ty: ScalarType::Int,
            },
            FunctionParameter {
                id: FunctionParameterId::new(1),
                name: "Position".into(),
                ty: ScalarType::Int,
            },
        ]
    };
    p.user_functions.insert(
        predicate,
        pure_function(Value::Bool(true), ScalarType::Bool, parameters()),
    );
    p.user_functions.insert(
        mapper,
        pure_function(Value::Int(1), ScalarType::Int, parameters()),
    );
    assert!(p.validate_filter_map_v1().is_ok());
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(30),
            context: Domain::GeneratedItem,
            ..
        })
    ));
}

#[test]
fn udf_and_dynamic_aliases_inside_item_at_do_not_gain_owned_parent_permission() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.root.iteration = iteration(SourceOwner::Primary, &[]);
    let function = FunctionId::new(7);
    p.user_functions.insert(
        function,
        pure_function(Value::Int(3), ScalarType::Int, vec![]),
    );
    p.graph.nodes.insert(
        30,
        Node::UserFunctionCall {
            function,
            args: vec![],
        },
    );
    p.graph
        .nodes
        .insert(10, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(40),
            context: Domain::GeneratedItem,
            ..
        })
    ));
    let mut p = project(tokenize(90), ScalarType::String);
    p.root.iteration = iteration(SourceOwner::Primary, &[]);
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        iteration: vec![],
        node: 12,
    });
    p.graph.nodes.insert(
        10,
        Node::SourceField {
            path: path(&["Cfg", "Cfg", "Text"]),
            frame: None,
        },
    );
    p.graph
        .nodes
        .insert(30, field(named(" Cfg💠 "), &["Cfg", "Index"], Some(&[])));
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(40),
            context: Domain::GeneratedItem,
            ..
        })
    ));
}

#[test]
fn generated_scope_and_document_or_dynamic_receiving_context_remain_closed() {
    for domain in [
        Domain::GeneratedItem,
        Domain::Document,
        Domain::DynamicTarget,
    ] {
        let mut p = project(tokenize(90), ScalarType::String);
        match domain {
            Domain::GeneratedItem => {
                p.graph.nodes.insert(91, item());
                p.root.iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
                    from: None,
                    to: 15,
                    item: 91,
                });
            }
            Domain::Document => {
                p.root.iteration = ScopeIteration::Source(vec![]);
                assert!(p.root.set_output_path(Some(12)));
            }
            Domain::DynamicTarget => {
                p.root.bindings.clear();
                p.root
                    .dynamic_bindings
                    .push(mapping::DynamicBinding { key: 12, value: 40 });
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(run(&p), Err(EngineError::UnsupportedOwnedContext { node: Some(30), context, .. }) if context == domain)
        );
    }
}

#[test]
fn owned_global_root_and_primary_row_failures_do_not_gain_item_at_in_predicates_or_messages() {
    for collection in [vec![], path(&["Rows"])] {
        for in_message in [false, true] {
            let mut p = project(tokenize(90), ScalarType::String);
            p.graph.nodes.insert(
                30,
                field(SourceOwner::Primary, &["Cfg", "Index"], Some(&[])),
            );
            p.graph.nodes.insert(
                51,
                Node::Call {
                    function: "equal".into(),
                    args: vec![40, 10],
                },
            );
            p.failure_rules.push(FailureRule {
                iteration: FailureIteration::Source {
                    collection: collection.clone(),
                },
                selection: if in_message {
                    FailureSelection::All
                } else {
                    FailureSelection::WhenTrue { predicate: 51 }
                },
                message: in_message.then_some(40),
            });
            let events = Events::default();
            let context =
                crate::ExecutionContext::new(Path::new("item-at.json")).with_trace_sink(&events);
            assert!(matches!(
                crate::run_with_sources_and_context(&p, &primary(), extras(), &context),
                Err(EngineError::UnsupportedOwnedContext {
                    node: Some(40),
                    context: Domain::FailureRule,
                    ..
                })
            ));
            assert!(events.0.borrow().is_empty());
        }
    }
}

#[test]
fn existing_global_failure_still_precedes_every_target_item_at_callback() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph.nodes.insert(
        51,
        field(SourceOwner::Primary, &["Cfg", "Index"], Some(&[])),
    );
    p.graph.nodes.insert(52, constant(Value::Int(1)));
    p.graph.nodes.insert(
        53,
        Node::Call {
            function: "equal".into(),
            args: vec![51, 52],
        },
    );
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source { collection: vec![] },
        selection: FailureSelection::WhenTrue { predicate: 53 },
        message: None,
    });
    raises(&mut p, 10, "PRODUCER");
    raises(&mut p, 30, "INDEX");
    let events = Events::default();
    let context = crate::ExecutionContext::new(Path::new("item-at.json")).with_trace_sink(&events);
    assert_eq!(
        crate::run_with_sources_and_context(&p, &primary(), extras(), &context),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
    assert!(events.inputs(40).is_empty());
}

#[test]
fn lexical_item_validation_still_rejects_own_foreign_and_inactive_branch_references() {
    for input_role in [true, false] {
        let mut p = project(tokenize(90), ScalarType::String);
        let node = if input_role { 10 } else { 30 };
        p.graph.nodes.insert(50, constant(Value::Bool(true)));
        p.graph.nodes.insert(
            node,
            Node::If {
                condition: 50,
                then: 15,
                else_: 90,
            },
        );
        assert!(crate::validate(&p).iter().any(|issue| {
            issue
                .message
                .contains("depends on its own sequence item node 90")
        }));
        p.graph.nodes.insert(91, item());
        p.graph.nodes.insert(
            41,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 15,
                    item: 91,
                },
                index: 13,
            },
        );
        p.graph.nodes.insert(
            node,
            Node::If {
                condition: 50,
                then: 15,
                else_: 91,
            },
        );
        assert!(crate::validate(&p).iter().any(|issue| {
            issue
                .message
                .contains("sequence item node 91 owned by another generated context")
        }));
    }
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(90, field(named("Cfg"), &["Cfg", "Text"], None));
    assert!(crate::validate(&p).iter().any(|issue| {
        issue
            .message
            .contains("sequence item must reference an unframed empty-path source field")
    }));
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(30, field(named("Cfg"), &["Cfg", "Missing"], Some(&[])));
    assert!(crate::validate(&p).iter().any(|issue| {
        issue
            .message
            .contains("owned source path `Cfg/Missing` matches no scalar in its owner")
    }));
}

#[test]
fn active_generated_ancestor_is_forbidden_even_on_an_untaken_index_branch() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph.nodes.insert(91, item());
    p.root.iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 15,
        item: 91,
    });
    p.graph.nodes.insert(
        30,
        Node::If {
            condition: 50,
            then: 13,
            else_: 91,
        },
    );
    p.graph.nodes.insert(50, constant(Value::Bool(true)));
    assert!(crate::validate(&p).iter().any(
        |issue| issue.message.contains("sequence item node 91")
            && (issue.message.contains("outside its owning context")
                || issue.message.contains("owned by another generated context"))
    ));
}

#[test]
fn new_item_at_wire_roundtrips_without_new_owner_or_producer_fields() {
    let p = project(tokenize(90), ScalarType::String);
    let bytes = serde_json::to_vec(&p).unwrap();
    let decoded: Project = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    assert_eq!(run(&decoded), Ok(result(Value::String("終".into()))));
    let mut legacy = p;
    legacy.extra_sources.clear();
    legacy.graph.nodes.insert(
        30,
        Node::SourceField {
            path: path(&["Cfg", "Index"]),
            frame: None,
        },
    );
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let decoded: Project = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    assert_eq!(
        crate::run(&decoded, &primary()),
        Ok(result(Value::String("a".into())))
    );
}

#[test]
fn selected_target_keeps_exact_named_owner_demand_and_optional_other_owner() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(31, field(named(" Cfg💠 "), &["Cfg", "Index"], Some(&[])));
    p.graph.nodes.insert(91, item());
    p.graph.nodes.insert(
        41,
        Node::SequenceItemAt {
            sequence: tokenize(91),
            index: 31,
        },
    );
    p.extra_targets.push(NamedTarget {
        name: " Audit ".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: Scope {
            bindings: vec![binding("Result", 41)],
            ..Scope::default()
        },
    });
    valid(&p);
    let context = crate::ExecutionContext::new(Path::new("item-at.json"));
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &primary(),
            vec![(
                " Cfg💠 ".into(),
                endpoint(Value::Int(2), Value::String("U".into()))
            )],
            &context,
            crate::TargetSelection::Named(" Audit ")
        ),
        Ok(crate::SelectedTargetOutput::Named(crate::NamedOutput {
            name: " Audit ".into(),
            instance: result(Value::String("β".into()))
        }))
    );
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &primary(),
            extras(),
            &context,
            crate::TargetSelection::Primary
        ),
        Ok(crate::SelectedTargetOutput::Primary(result(Value::String(
            "終".into()
        ))))
    );
}

#[test]
fn item_at_hidden_only_in_legacy_scope_controls_still_discovers_owned_demand() {
    for role in 0..4 {
        let mut p = project(
            SequenceExpr::Generate {
                from: None,
                to: 15,
                item: 90,
            },
            ScalarType::String,
        );
        p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        p.target.repeating = true;
        p.graph.nodes.insert(
            50,
            Node::SourceField {
                path: path(&["Key"]),
                frame: None,
            },
        );
        p.graph.nodes.insert(
            51,
            Node::Call {
                function: "equal".into(),
                args: vec![40, 15],
            },
        );
        p.graph.nodes.insert(
            60,
            Node::SourceField {
                path: path(&["Text"]),
                frame: None,
            },
        );
        p.root.bindings = vec![binding("Result", 60)];
        match role {
            0 => p.root.filter = Some(51),
            1 => p
                .root
                .windows
                .push(mapping::SequenceWindow::First { count: 40 }),
            2 => p.root.group_by = Some(40),
            _ => {
                p.root.group_by = Some(50);
                p.root.post_group_filter = Some(51);
            }
        }
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                group(vec![
                    ("Key", text("A")),
                    ("Index", scalar(Value::Int(1))),
                    ("Text", text("first")),
                ]),
                group(vec![
                    ("Key", text("B")),
                    ("Index", scalar(Value::Int(1))),
                    ("Text", text("second")),
                ]),
            ]),
        )]);
        valid(&p);
        let expected = if role == 2 {
            Instance::Repeated(vec![result(Value::String("first".into()))])
        } else {
            Instance::Repeated(vec![
                result(Value::String("first".into())),
                result(Value::String("second".into())),
            ])
        };
        assert_eq!(
            crate::run_with_sources(&p, &primary, extras()),
            Ok(expected),
            "control role={role}"
        );
        assert_eq!(
            crate::run_with_sources(&p, &primary, vec![]),
            Err(EngineError::OwnedSourceAccess {
                node: Some(30),
                scope: None,
                source: Access::SourceUnavailable {
                    owner: named("Cfg")
                },
            }),
            "control role={role}"
        );
    }
}

#[test]
fn stored_namespace_and_structural_errors_keep_existing_validation_boundaries() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.extra_sources.push(p.extra_sources[0].clone());
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(30),
            scope: None,
            source: Access::AmbiguousOwner { name: "Cfg".into() }
        })
    );
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph.nodes.remove(&11);
    assert!(
        crate::validate(&p)
            .iter()
            .any(|issue| issue.message.contains("11") && issue.message.contains("missing"))
    );
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph.nodes.insert(
        10,
        Node::Call {
            function: "to_string".into(),
            args: vec![40],
        },
    );
    assert!(
        crate::validate(&p)
            .iter()
            .any(|issue| issue.message.contains("cycle"))
    );
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph.nodes.insert(
        90,
        Node::SourceField {
            path: vec![],
            frame: Some(vec![]),
        },
    );
    assert!(crate::validate(&p).iter().any(|issue| {
        issue
            .message
            .contains("sequence item must reference an unframed empty-path source field")
    }));
}

#[test]
fn a_pure_closed_operator_beside_an_owned_index_does_not_gain_basic_permission() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.graph
        .nodes
        .insert(16, constant(Value::String("A".into())));
    p.graph.nodes.insert(
        10,
        Node::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Key"]),
            matches: 16,
            value: path(&["Text"]),
        },
    );
    // The receiving scope itself has no owned-source ancestry: the only owned
    // input is the sibling index. This must still be a closed ItemAt graph.
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(30),
            context: Domain::GeneratedItem,
            ..
        })
    ));
}

#[test]
fn scalar_scope_construction_keeps_the_same_owned_parent_item_at_context() {
    let mut p = project(tokenize(90), ScalarType::String);
    p.target = SchemaNode::scalar("Output", ScalarType::String);
    p.root.bindings.clear();
    p.root.construction = mapping::ScopeConstruction::Scalar { value: 40 };
    valid(&p);
    assert_eq!(run(&p), Ok(scalar(Value::String("終".into()))));
}

#[test]
fn regex_parent_pattern_or_flag_null_suppresses_later_arguments_but_not_index() {
    for null_pattern in [true, false] {
        let mut p = project(
            SequenceExpr::TokenizeRegex {
                input: 10,
                pattern: 11,
                flags: Some(14),
                item: 90,
            },
            ScalarType::String,
        );
        p.graph
            .nodes
            .insert(10, constant(Value::String("a".into())));
        if null_pattern {
            p.graph
                .nodes
                .insert(11, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
            raises(&mut p, 14, "FLAGS");
        } else {
            p.graph
                .nodes
                .insert(11, constant(Value::String(",".into())));
            p.graph
                .nodes
                .insert(14, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
        }
        let events = Events::default();
        let context =
            crate::ExecutionContext::new(Path::new("item-at.json")).with_trace_sink(&events);
        assert_eq!(
            crate::run_with_sources_and_context(
                &p,
                &primary(),
                vec![("Cfg".into(), endpoint(Value::Int(3), Value::Null))],
                &context
            ),
            Ok(result(Value::Null))
        );
        let expected = if null_pattern {
            vec![(10, 0), (11, 1), (30, 3)]
        } else {
            vec![(10, 0), (11, 1), (14, 2), (30, 3)]
        };
        assert_eq!(events.inputs(40), expected);
        assert_eq!(events.values(30), 1);
    }
}
