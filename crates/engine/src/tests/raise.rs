use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, JsonNull, ScalarType, SchemaNode, Value, XmlNil};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, FunctionParameter,
    FunctionParameterId, Graph, NamedSource, Node, Project, Scope, ScopeIteration, UserFunction,
};

use crate::{
    EngineError, ExecutionContext, TargetSelection, TraceEvent, TraceSink,
    required_sources_for_target, run, run_with_context, run_with_sources, validate,
};

#[derive(Default)]
struct Trace(RefCell<Vec<TraceEvent>>);

impl TraceSink for Trace {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn scalar_project(output: u32, nodes: impl IntoIterator<Item = (u32, Node)>) -> Project {
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Result", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: output,
            }],
            ..Scope::default()
        },
    }
}

fn rows_project() -> Project {
    let mut project = scalar_project(4, []);
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::scalar("Selected", ScalarType::Bool),
                    SchemaNode::scalar("Message", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("Result", ScalarType::Int)])
                .repeating(),
        ],
    );
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Selected".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec!["Message".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (4, Node::Raise { message: Some(2) }),
        (
            5,
            Node::If {
                condition: 1,
                then: 4,
                else_: 3,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            7,
            Node::Call {
                function: "divide".into(),
                args: vec![0, 6],
            },
        ),
    ]);
    project.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            filter: Some(5),
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: 0,
            }],
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

fn row(value: i64, selected: Value, message: &str) -> Instance {
    group(vec![
        ("Value", Instance::Scalar(Value::Int(value))),
        ("Selected", Instance::Scalar(selected)),
        ("Message", Instance::Scalar(Value::String(message.into()))),
    ])
}

fn rows(items: Vec<Instance>) -> Instance {
    group(vec![("Rows", Instance::Repeated(items))])
}

fn assert_valid(project: &Project) {
    let issues = validate(project);
    eprintln!("raise project validation: {issues:#?}");
    assert!(issues.is_empty(), "{issues:#?}");
}

#[test]
fn raise_wire_dependencies_and_pruning_preserve_reached_messages() {
    let absent: Node = serde_json::from_value(serde_json::json!({"kind":"raise"})).unwrap();
    assert!(matches!(absent, Node::Raise { message: None }));
    assert!(absent.dependencies().is_empty());
    assert_eq!(
        serde_json::to_value(&absent).unwrap(),
        serde_json::json!({"kind":"raise"})
    );
    let present = Node::Raise { message: Some(9) };
    assert_eq!(present.dependencies(), vec![9]);
    assert_eq!(
        serde_json::to_value(&present).unwrap(),
        serde_json::json!({"kind":"raise","message":9})
    );
    let mut project = scalar_project(
        4,
        [
            (4, present),
            (
                9,
                Node::Const {
                    value: Value::String("kept".into()),
                },
            ),
            (
                10,
                Node::Const {
                    value: Value::String("unreachable".into()),
                },
            ),
        ],
    );
    project.prune_unreachable_nodes();
    assert_eq!(
        project.graph.nodes.keys().copied().collect::<Vec<_>>(),
        vec![4, 9]
    );
    let bytes = serde_json::to_vec(&project).unwrap();
    let reopened: Project = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
    assert_valid(&reopened);
    assert_eq!(
        run(&reopened, &group(Vec::new())),
        Err(EngineError::MappingException {
            node: 4,
            message: Some("kept".into())
        })
    );
    let legacy = Node::If {
        condition: 1,
        then: 2,
        else_: 3,
    };
    assert_eq!(
        serde_json::to_value(legacy).unwrap(),
        serde_json::json!({"kind":"if","condition":1,"then":2,"else":3})
    );
}

#[test]
fn absent_message_and_every_scalar_message_keep_exact_node_and_text() {
    let absent = scalar_project(4, [(4, Node::Raise { message: None })]);
    assert_valid(&absent);
    let error = run(&absent, &group(Vec::new()));
    eprintln!("absent raise: {error:#?}");
    assert_eq!(
        error,
        Err(EngineError::MappingException {
            node: 4,
            message: None
        })
    );
    assert_eq!(
        error.unwrap_err().to_string(),
        "node 4: mapping exception: mapping exception was raised"
    );
    for (value, expected) in [
        (Value::Null, ""),
        (Value::JsonNull(JsonNull), ""),
        (Value::XmlNil(XmlNil), ""),
        (Value::Bool(false), "false"),
        (Value::Int(-7), "-7"),
        (Value::Float(-0.0), "-0"),
        (Value::String("message 😀\n".into()), "message 😀\n"),
    ] {
        let project = scalar_project(
            4,
            [
                (4, Node::Raise { message: Some(9) }),
                (9, Node::Const { value }),
            ],
        );
        assert_valid(&project);
        let error = run(&project, &group(Vec::new()));
        eprintln!("scalar raise: {error:#?}");
        assert_eq!(
            error,
            Err(EngineError::MappingException {
                node: 4,
                message: Some(expected.into())
            })
        );
    }
}

#[test]
fn unselected_raise_message_is_lazy_and_selected_message_cause_is_not_wrapped() {
    let mut project = rows_project();
    project
        .graph
        .nodes
        .insert(4, Node::Raise { message: Some(7) });
    assert_valid(&project);
    let source = rows(vec![
        row(10, Value::Bool(false), "a"),
        row(20, Value::Bool(false), "b"),
    ]);
    let trace = Trace::default();
    let execution = ExecutionContext::new(Path::new("raise.json")).with_trace_sink(&trace);
    let outcome = run_with_context(&project, &source, &execution);
    eprintln!("lazy output: {outcome:#?}; trace={:#?}", trace.0.borrow());
    assert_eq!(
        outcome,
        Ok(group(vec![(
            "Row",
            Instance::Repeated(vec![
                group(vec![("Result", Instance::Scalar(Value::Int(10)))]),
                group(vec![("Result", Instance::Scalar(Value::Int(20)))]),
            ])
        )]))
    );
    assert!(!trace.0.borrow().iter().any(|event| matches!(
        event,
        TraceEvent::NodeValue { node: 7, .. } | TraceEvent::NodeInputValue { consumer: 4, .. }
    )));
    let selected = rows(vec![row(10, Value::Bool(true), "selected")]);
    let outcome = run(&project, &selected);
    eprintln!("selected throwing message: {outcome:#?}");
    assert_eq!(
        outcome,
        Err(EngineError::Function(
            functions::FunctionError::DivideByZero
        ))
    );
}

#[test]
fn original_predicate_not_a_bool_identity_precedes_raise_and_message() {
    let mut project = rows_project();
    project
        .graph
        .nodes
        .insert(4, Node::Raise { message: Some(7) });
    assert_valid(&project);
    for (predicate, found) in [
        (Value::Null, "null"),
        (Value::Int(1), "int"),
        (Value::XmlNil(XmlNil), "xml nil"),
    ] {
        let result = run(&project, &rows(vec![row(10, predicate, "unused")]));
        eprintln!("typed predicate refusal: {result:#?}");
        assert_eq!(result, Err(EngineError::NotABool { node: 1, found }));
    }
    project.graph.nodes.insert(
        5,
        Node::If {
            condition: 1,
            then: 3,
            else_: 4,
        },
    );
    project
        .graph
        .nodes
        .insert(4, Node::Raise { message: Some(2) });
    assert_eq!(
        run(
            &project,
            &rows(vec![row(10, Value::Bool(false), "false-picked")])
        ),
        Err(EngineError::MappingException {
            node: 4,
            message: Some("false-picked".into())
        })
    );
}

#[test]
fn plain_item_order_and_legacy_global_rule_scan_remain_distinct() {
    let mut per_item = rows_project();
    per_item.root.children[0].bindings[0].node = 7;
    assert_valid(&per_item);
    let earlier_target = row(10, Value::Bool(false), "earlier-unselected");
    let later_failure = row(20, Value::Bool(true), "late-selected");
    let source = rows(vec![earlier_target.clone(), later_failure.clone()]);
    let item_error = run(&per_item, &source);
    eprintln!("per-item earlier target: {item_error:#?}");
    assert_eq!(
        item_error,
        Err(EngineError::Function(
            functions::FunctionError::DivideByZero
        ))
    );
    assert_eq!(
        run(&per_item, &rows(vec![later_failure, earlier_target])),
        Err(EngineError::MappingException {
            node: 4,
            message: Some("late-selected".into())
        })
    );
    let mut global = per_item.clone();
    global.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Rows".into()],
        },
        selection: FailureSelection::WhenTrue { predicate: 1 },
        message: Some(2),
    });
    assert_valid(&global);
    let global_error = run(&global, &source);
    eprintln!("legacy global before all targets: {global_error:#?}");
    assert_eq!(
        global_error,
        Err(EngineError::MappingFailure {
            rule: 1,
            message: Some("late-selected".into())
        })
    );
}

#[test]
fn first_selected_message_reads_raw_item_position_after_earlier_construction() {
    let mut project = rows_project();
    project.graph.nodes.insert(
        8,
        Node::Position {
            collection: vec!["Rows".into()],
        },
    );
    project
        .graph
        .nodes
        .insert(4, Node::Raise { message: Some(8) });
    assert_valid(&project);
    let source = rows(vec![
        row(10, Value::Bool(false), "unused"),
        row(20, Value::Bool(true), "picked"),
        row(30, Value::Bool(true), "not-reached"),
    ]);
    let trace = Trace::default();
    let execution = ExecutionContext::new(Path::new("raise.json")).with_trace_sink(&trace);
    let result = run_with_context(&project, &source, &execution);
    eprintln!(
        "raw position outcome: {result:#?}; trace={:#?}",
        trace.0.borrow()
    );
    assert_eq!(
        result,
        Err(EngineError::MappingException {
            node: 4,
            message: Some("2".into())
        })
    );
    assert_eq!(trace.0.borrow().iter().filter(|event| matches!(event, TraceEvent::TargetFieldWritten { field, .. } if field == "Result")).count(), 1);
    let delivered = trace
        .0
        .borrow()
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeInputValue {
                consumer: 4,
                input: 8,
                input_index: 0,
                positions,
                ..
            } => Some(positions.last().unwrap().index),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(delivered, vec![2]);
    let empty = run(&project, &rows(Vec::new()));
    assert_eq!(
        empty,
        Ok(group(vec![("Row", Instance::Repeated(Vec::new()))]))
    );
}

#[test]
fn message_dependency_validation_cycles_and_named_source_reachability_are_complete() {
    let missing = scalar_project(4, [(4, Node::Raise { message: Some(999) })]);
    assert!(validate(&missing).iter().any(|issue| {
        issue
            .message
            .contains("message references missing node 999")
    }));
    assert_eq!(
        run(&missing, &group(Vec::new())),
        Err(EngineError::MissingNode(999))
    );
    let cycle = scalar_project(4, [(4, Node::Raise { message: Some(4) })]);
    assert!(
        validate(&cycle)
            .iter()
            .any(|issue| issue.message.contains("cycle"))
    );
    assert_eq!(run(&cycle, &group(Vec::new())), Err(EngineError::Cycle(4)));
    let mut named = scalar_project(
        4,
        [
            (4, Node::Raise { message: Some(9) }),
            (
                9,
                Node::SourceField {
                    path: vec!["Other".into(), "Message".into()],
                    frame: None,
                },
            ),
        ],
    );
    named.extra_sources.push(NamedSource {
        name: "Other".into(),
        path: "other.json".into(),
        schema: SchemaNode::group(
            "Other",
            vec![SchemaNode::scalar("Message", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    assert_valid(&named);
    let required = required_sources_for_target(&named, TargetSelection::Primary).unwrap();
    assert_eq!(
        required
            .static_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Other"]
    );
    assert!(required.dynamic_sources.is_empty());
    let result = run_with_sources(
        &named,
        &group(Vec::new()),
        vec![(
            "Other".into(),
            group(vec![(
                "Message",
                Instance::Scalar(Value::String("named message".into())),
            )]),
        )],
    );
    eprintln!("named message: {result:#?}");
    assert_eq!(
        result,
        Err(EngineError::MappingException {
            node: 4,
            message: Some("named message".into())
        })
    );
}

#[test]
fn isolated_function_raise_is_lazy_and_preserves_function_message_failures() {
    let id = FunctionId::new(7);
    let parameter = FunctionParameterId::new(1);
    let mut project = scalar_project(
        20,
        [
            (
                1,
                Node::Const {
                    value: Value::Bool(false),
                },
            ),
            (
                20,
                Node::UserFunctionCall {
                    function: id,
                    args: vec![1],
                },
            ),
        ],
    );
    project.user_functions.insert(
        id,
        UserFunction {
            library: "tests".into(),
            name: "guarded_raise".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: parameter,
                name: "selected".into(),
                ty: ScalarType::Bool,
            }],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: BTreeMap::from([
                    (0, Node::FunctionParameter { parameter }),
                    (
                        1,
                        Node::Const {
                            value: Value::String("function message".into()),
                        },
                    ),
                    (2, Node::Raise { message: Some(1) }),
                    (
                        3,
                        Node::Const {
                            value: Value::String("safe".into()),
                        },
                    ),
                    (
                        4,
                        Node::If {
                            condition: 0,
                            then: 2,
                            else_: 3,
                        },
                    ),
                    (
                        5,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        6,
                        Node::Const {
                            value: Value::Int(0),
                        },
                    ),
                    (
                        7,
                        Node::Call {
                            function: "divide".into(),
                            args: vec![5, 6],
                        },
                    ),
                ]),
            },
            output: 4,
        },
    );
    assert_valid(&project);
    assert_eq!(
        run(&project, &group(Vec::new())),
        Ok(group(vec![(
            "Result",
            Instance::Scalar(Value::String("safe".into()))
        )]))
    );
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    let selected = run(&project, &group(Vec::new()));
    eprintln!("function raise: {selected:#?}");
    assert_eq!(
        selected,
        Err(EngineError::MappingException {
            node: 2,
            message: Some("function message".into())
        })
    );
    project
        .user_functions
        .get_mut(&id)
        .unwrap()
        .body
        .nodes
        .insert(2, Node::Raise { message: Some(7) });
    assert_valid(&project);
    assert_eq!(
        run(&project, &group(Vec::new())),
        Err(EngineError::UserFunctionBuiltin {
            function: id,
            node: 7,
            source: functions::FunctionError::DivideByZero
        })
    );
}

#[test]
fn primary_root_message_keeps_immutable_owner_and_required_field_cause() {
    let mut project = scalar_project(
        4,
        [
            (4, Node::Raise { message: Some(9) }),
            (
                9,
                Node::SourceRootField {
                    path: vec!["Code".into()],
                    required: true,
                },
            ),
        ],
    );
    project.source = serde_json::from_str(r#"{"name":"Root","xml_namespace":{"kind":"unqualified"},"xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code"]},{"name":"Derived","members":["Code","Extra"]}]}}"#).unwrap();
    project.source_options = mapping::FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: true,
        ..Default::default()
    };
    assert_valid(&project);
    let result = run(
        &project,
        &group(vec![(
            "Code",
            Instance::Scalar(Value::String("primary owner 😀".into())),
        )]),
    );
    eprintln!("primary-root raise: {result:#?}");
    assert_eq!(
        result,
        Err(EngineError::MappingException {
            node: 4,
            message: Some("primary owner 😀".into())
        })
    );
    let missing = run(&project, &group(Vec::new()));
    eprintln!("primary-root message cause: {missing:#?}");
    assert_eq!(
        missing,
        Err(EngineError::PrimaryRoot {
            node: 9,
            source: ir::PrimaryRootError::MissingRequiredField {
                path: vec!["Code".into()]
            },
        })
    );
}
