use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding as MappingBinding, Graph, NamedTarget, Node, Project, Scope, ScopeConstruction,
    ScopeIteration,
};

use crate::{
    Diagnostic, Expression, GeneratedSequence, IterationPlan, SUPPORTED_SCALAR_CALLS,
    ScalarFunction, ScopeFeature, UnsupportedNodeKind, UnsupportedSequenceKind, lower,
};

mod artifacts;
mod collection_find;
mod extra_sources;
mod failures;
mod joins;
mod sequences;

fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn typed_scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}

fn supported_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![scalar("First"), scalar("Second"), scalar("NestedValue")],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                typed_scalar("SecondOut", ScalarType::Int).repeating(),
                scalar("FirstOut"),
                SchemaNode::group("Details", vec![scalar("Value")]).repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    30,
                    Node::SourceField {
                        path: vec!["NestedValue".into()],
                        frame: None,
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        path: vec!["First".into()],
                        frame: None,
                    },
                ),
                (
                    10,
                    Node::Const {
                        value: Value::Int(7),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                MappingBinding {
                    target_field: "SecondOut".into(),
                    node: 10,
                },
                MappingBinding {
                    target_field: "FirstOut".into(),
                    node: 20,
                },
            ],
            children: vec![Scope {
                target_field: "Details".into(),
                bindings: vec![MappingBinding {
                    target_field: "Value".into(),
                    node: 30,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn lowers_static_constructed_scopes_in_declaration_order() {
    let project = supported_project();

    let program = lower(&project).expect("the supported subset lowers");

    assert_eq!(program.source, project.source);
    assert_eq!(program.target, project.target);
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
    assert_eq!(
        program
            .root
            .bindings
            .iter()
            .map(|binding| (binding.target_field.as_str(), binding.expression))
            .collect::<Vec<_>>(),
        vec![("SecondOut", 10), ("FirstOut", 20)]
    );
    assert_eq!(program.root.bindings[0].target_type, ScalarType::Int);
    assert!(program.root.bindings[0].repeating);
    assert_eq!(program.root.bindings[1].target_type, ScalarType::String);
    assert!(!program.root.bindings[1].repeating);
    assert_eq!(program.root.children[0].target_field, "Details");
    assert!(!program.root.repeating);
    assert!(program.root.children[0].repeating);
    assert_eq!(program.root.children[0].bindings[0].expression, 30);
    assert!(matches!(
        program.expressions[0].expression,
        Expression::Const {
            value: Value::Int(7)
        }
    ));
}

#[test]
fn lowers_named_targets_in_order_with_one_reachable_expression_set() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (
            40,
            Node::Const {
                value: Value::String(" suffix".into()),
            },
        ),
        (
            50,
            Node::Call {
                function: "concat".into(),
                args: vec![20, 40],
            },
        ),
        (
            90,
            Node::Const {
                value: Value::String("unreachable".into()),
            },
        ),
    ]);
    project.extra_targets = vec![
        NamedTarget {
            name: "Audit".into(),
            path: None,
            schema: SchemaNode::group("Audit", vec![scalar("Label")]),
            options: Default::default(),
            root: Scope {
                bindings: vec![MappingBinding {
                    target_field: "Label".into(),
                    node: 50,
                }],
                ..Scope::default()
            },
        },
        NamedTarget {
            name: "Summary".into(),
            path: None,
            schema: SchemaNode::group("Summary", vec![typed_scalar("Count", ScalarType::Int)]),
            options: Default::default(),
            root: Scope {
                bindings: vec![MappingBinding {
                    target_field: "Count".into(),
                    node: 10,
                }],
                ..Scope::default()
            },
        },
    ];

    let program = lower(&project).expect("named targets are part of the neutral program");

    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 30, 40, 50]
    );
    assert_eq!(
        program
            .extra_targets
            .iter()
            .map(|target| target.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Audit", "Summary"]
    );
    assert_eq!(
        program.extra_targets[0].target,
        project.extra_targets[0].schema
    );
    assert_eq!(program.extra_targets[0].root.bindings[0].expression, 50);
    assert_eq!(program.extra_targets[1].root.bindings[0].expression, 10);
}

#[test]
fn lowers_exact_whole_current_source_group_copy() {
    let source = SchemaNode::group(
        "Source",
        vec![
            typed_scalar("Id", ScalarType::Int),
            SchemaNode::group("Items", vec![scalar("Name")]).repeating(),
        ],
    );
    let mut target = source.clone();
    target.name = "Target".into();
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    };

    let program = lower(&project).expect("whole current source group copy lowers");
    assert_eq!(
        program.root.construction,
        crate::TargetConstruction::CopyCurrentSource
    );
    assert!(program.expressions.is_empty());
}

#[test]
fn rejects_copy_current_source_with_dynamic_target_paths() {
    let source = SchemaNode::group("Source", vec![scalar("Value")]).repeating();
    let mut target = source.clone();
    target.name = "Target".into();
    let mut root = Scope {
        iteration: ScopeIteration::Source(Vec::new()),
        construction: ScopeConstruction::CopyCurrentSource,
        ..Scope::default()
    };
    assert!(root.set_output_path(Some(1)));
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                1,
                Node::Const {
                    value: Value::String("item.xml".into()),
                },
            )]),
        },
        root,
    };

    let diagnostics = lower(&project)
        .expect_err("dynamic target documents remain outside code generation")
        .into_diagnostics();
    assert!(diagnostics.contains(&Diagnostic::UnsupportedScope {
        target_path: Vec::new(),
        feature: ScopeFeature::Iteration,
    }));
}

#[test]
fn unused_unsupported_nodes_do_not_block_lowering() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (
            90,
            Node::Const {
                value: Value::String("unused".into()),
            },
        ),
        (
            99,
            Node::Call {
                function: "upper".into(),
                args: vec![90],
            },
        ),
    ]);

    let program = lower(&project).expect("unreachable nodes are outside the generated program");

    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
}

#[test]
fn reports_each_reachable_unsupported_function_by_name() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::Call {
            function: "upper".into(),
            args: vec![10, 20],
        },
    );
    project.root.bindings[0].node = 40;

    let diagnostics = lower(&project)
        .expect_err("upper is outside the portable call whitelist")
        .into_diagnostics();

    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedFunction {
            node: 40,
            function: "upper".into(),
        }]
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "graph node 40: code generation does not support function `upper`"
    );
}

#[test]
fn scalar_call_whitelist_is_closed_and_name_addressable() {
    let expected = [
        "and",
        "or",
        "not",
        "exists",
        "is_empty",
        "starts_with",
        "contains",
        "concat",
        "normalize_space",
        "left_trim",
        "right_trim",
        "length",
        "substring_before",
        "substring_after",
        "string",
        "substitute_missing",
        "is_xml_nil",
        "get_folder",
        "remove_folder",
        "resolve_filepath",
        "substring",
        "sql_like",
        "pad_string_left",
        "pad_string_right",
        "isbn10_to_isbn13",
        "round",
        "date_from_datetime",
        "add",
        "subtract",
        "multiply",
        "divide",
        "equal",
        "not_equal",
        "less_than",
        "greater_than",
        "less_or_equal",
        "greater_or_equal",
    ];

    assert_eq!(
        SUPPORTED_SCALAR_CALLS
            .iter()
            .copied()
            .map(ScalarFunction::as_str)
            .collect::<Vec<_>>(),
        expected
    );
    for (name, function) in expected.into_iter().zip(SUPPORTED_SCALAR_CALLS) {
        assert_eq!(ScalarFunction::from_name(name), Some(*function));
    }
    assert_eq!(ScalarFunction::from_name("upper"), None);
}

#[test]
fn lowers_direct_calls_with_ordered_arguments() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::Call {
            function: "multiply".into(),
            args: vec![20, 10, 20],
        },
    );
    project.root.bindings[0].node = 40;

    let program = lower(&project).expect("whitelisted calls lower");

    assert_eq!(
        program.expressions,
        vec![
            crate::ExpressionNode {
                id: 10,
                expression: Expression::Const {
                    value: Value::Int(7),
                },
            },
            crate::ExpressionNode {
                id: 20,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["First".into()],
                },
            },
            crate::ExpressionNode {
                id: 30,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["NestedValue".into()],
                },
            },
            crate::ExpressionNode {
                id: 40,
                expression: Expression::Call {
                    function: ScalarFunction::Multiply,
                    args: vec![20, 10, 20],
                },
            },
        ]
    );
}

#[test]
fn lowers_all_runtime_values_as_dependency_free_expressions() {
    let mut project = supported_project();
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::MappingFilePath,
            },
        ),
        (
            2,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::MainMappingFilePath,
            },
        ),
        (
            3,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::CurrentDateTime,
            },
        ),
    ]);
    project.root.bindings[0].node = 1;
    project.root.bindings[1].node = 2;
    project.root.children[0].bindings[0].node = 3;

    let program = lower(&project).expect("runtime values are portable host inputs");

    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| (&node.expression, node.id))
            .collect::<Vec<_>>(),
        vec![
            (
                &Expression::RuntimeValue {
                    value: crate::RuntimeValue::MappingFilePath,
                },
                1,
            ),
            (
                &Expression::RuntimeValue {
                    value: crate::RuntimeValue::MainMappingFilePath,
                },
                2,
            ),
            (
                &Expression::RuntimeValue {
                    value: crate::RuntimeValue::CurrentDateTime,
                },
                3,
            ),
        ]
    );
}

#[test]
fn nested_calls_and_if_retain_every_dependency_deterministically() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (
            40,
            Node::Const {
                value: Value::Int(5),
            },
        ),
        (
            50,
            Node::Call {
                function: "add".into(),
                args: vec![10, 40],
            },
        ),
        (
            60,
            Node::Call {
                function: "greater_than".into(),
                args: vec![10, 40],
            },
        ),
        (
            70,
            Node::If {
                condition: 60,
                then: 50,
                else_: 10,
            },
        ),
    ]);
    project.root.bindings[0].node = 70;

    let first = lower(&project).expect("nested supported expressions lower");
    let second = lower(&project).expect("lowering is deterministic");

    assert_eq!(first, second);
    assert_eq!(
        first
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 30, 40, 50, 60, 70]
    );
    assert!(matches!(
        first.expressions[4].expression,
        Expression::Call {
            function: ScalarFunction::Add,
            ref args,
        } if args == &[10, 40]
    ));
    assert!(matches!(
        first.expressions[5].expression,
        Expression::Call {
            function: ScalarFunction::GreaterThan,
            ref args,
        } if args == &[10, 40]
    ));
    assert!(matches!(
        first.expressions[6].expression,
        Expression::If {
            condition: 60,
            then: 50,
            else_: 10,
        }
    ));
}

#[test]
fn lowers_ordered_value_maps_without_normalizing_rows_or_defaults() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::ValueMap {
            input: 10,
            input_type: Some(ScalarType::Float),
            table: vec![
                (Value::Float(7.0), Value::String("first".into())),
                (Value::Float(7.0), Value::String("second".into())),
                (Value::Null, Value::Float(f64::INFINITY)),
            ],
            default: Some(Value::xml_nil()),
        },
    );
    project.root.bindings[0].node = 40;

    let program = lower(&project).expect("value maps are portable expressions");

    assert!(matches!(
        program.expressions.last().map(|node| &node.expression),
        Some(Expression::ValueMap {
            input: 10,
            input_type: Some(ScalarType::Float),
            table,
            default: Some(Value::XmlNil(_)),
        }) if table.len() == 3
            && table[0] == (Value::Float(7.0), Value::String("first".into()))
            && table[1] == (Value::Float(7.0), Value::String("second".into()))
            && matches!(table[2].1, Value::Float(value) if value.is_infinite())
    ));
}

#[test]
fn lowers_primary_source_lookups_with_only_the_match_dependency() {
    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            scalar("First"),
            scalar("NestedValue"),
            scalar("Needle"),
            SchemaNode::group("Catalog", vec![scalar("Code"), scalar("Label")]).repeating(),
        ],
    );
    project.graph.nodes.insert(
        10,
        Node::SourceField {
            path: vec!["Needle".into()],
            frame: None,
        },
    );
    project.graph.nodes.insert(
        40,
        Node::Lookup {
            collection: vec!["Catalog".into()],
            key: vec!["Code".into()],
            matches: 10,
            value: vec!["Label".into()],
        },
    );
    project.root.bindings[1].node = 40;

    let program = lower(&project).expect("primary-source lookups are portable expressions");

    assert!(matches!(
        program.expressions.last().map(|node| &node.expression),
        Some(Expression::Lookup {
            collection,
            key,
            matches: 10,
            value,
        }) if collection == &["Catalog"] && key == &["Code"] && value == &["Label"]
    ));
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 30, 40]
    );
}

#[test]
fn preserves_non_finite_constant_bits_during_shared_lowering() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::Const {
            value: Value::Float(f64::INFINITY),
        },
    );
    project.root.bindings[0].node = 40;

    let program = lower(&project).expect("IEEE-754 literals are portable by exact bits");

    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|node| node.id == 40)
            .map(|node| &node.expression),
        Some(Expression::Const {
            value: Value::Float(value)
        }) if value.to_bits() == f64::INFINITY.to_bits()
    ));
}

#[test]
fn converts_engine_validation_failures_before_subset_analysis() {
    let mut project = supported_project();
    project.root.bindings[0].node = 404;

    let diagnostics = lower(&project)
        .expect_err("missing graph references fail validation")
        .into_diagnostics();

    assert!(diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        Diagnostic::Validation { message, .. } if message.contains("404")
    )));
}

#[test]
fn lowers_source_iteration_at_the_static_target_path() {
    let mut project = supported_project();
    project.root.children[0].iteration = ScopeIteration::Source(Vec::new());

    let program = lower(&project).expect("source iteration is supported");

    assert_eq!(
        program.root.children[0].iteration,
        Some(IterationPlan::source(Vec::new()))
    );
}

#[test]
fn lowers_complete_source_iteration_controls_and_reachability() {
    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            scalar("First"),
            scalar("Second"),
            scalar("NestedValue"),
            SchemaNode::group(
                "Rows",
                vec![
                    typed_scalar("Keep", ScalarType::Bool),
                    typed_scalar("Score", ScalarType::Int),
                    scalar("Tie"),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes.extend([
        (
            40,
            Node::SourceField {
                path: vec!["Keep".into()],
                frame: None,
            },
        ),
        (
            41,
            Node::SourceField {
                path: vec!["Score".into()],
                frame: None,
            },
        ),
        (
            42,
            Node::SourceField {
                path: vec!["Tie".into()],
                frame: None,
            },
        ),
        (
            43,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            44,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            45,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            46,
            Node::Const {
                value: Value::Int(4),
            },
        ),
        (
            47,
            Node::Const {
                value: Value::Int(5),
            },
        ),
    ]);
    project.root.children[0] = Scope {
        target_field: "Details".into(),
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        filter: Some(40),
        sort_by: Some(41),
        sort_descending: true,
        sort_then_by: vec![mapping::SortKey {
            node: 42,
            descending: false,
        }],
        sort_filter_order: mapping::SortFilterOrder::FilterThenSort,
        windows: vec![
            mapping::SequenceWindow::SkipFirst { count: 43 },
            mapping::SequenceWindow::First { count: 44 },
            mapping::SequenceWindow::From { position: 45 },
            mapping::SequenceWindow::FromTo {
                first: 46,
                last: 47,
            },
            mapping::SequenceWindow::Last { count: 43 },
        ],
        bindings: vec![MappingBinding {
            target_field: "Value".into(),
            node: 30,
        }],
        ..Scope::default()
    };

    let program = lower(&project).expect("all source iteration controls lower together");
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 30, 40, 41, 42, 43, 44, 45, 46, 47]
    );
    assert_eq!(
        program.root.children[0].iteration,
        Some(crate::IterationPlan::new(
            crate::SourceIteration::new(vec!["Rows".into()]),
            Some(40),
            Some(crate::SortPlan::new(
                crate::SortKey {
                    expression: 41,
                    descending: true,
                },
                vec![crate::SortKey {
                    expression: 42,
                    descending: false,
                }],
                crate::SortFilterOrder::FilterThenSort,
            )),
            vec![
                crate::SequenceWindow::SkipFirst { count: 43 },
                crate::SequenceWindow::First { count: 44 },
                crate::SequenceWindow::From { position: 45 },
                crate::SequenceWindow::FromTo {
                    first: 46,
                    last: 47,
                },
                crate::SequenceWindow::Last { count: 43 },
            ],
            crate::IterationOutput::Repeated,
        ))
    );
}

#[test]
fn lowers_framed_fields_positions_and_filter_dependencies() {
    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            scalar("First"),
            scalar("Second"),
            scalar("NestedValue"),
            SchemaNode::group(
                "Rows",
                vec![scalar("Name"), typed_scalar("Keep", ScalarType::Bool)],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![
            typed_scalar("SecondOut", ScalarType::Int).repeating(),
            scalar("FirstOut"),
            SchemaNode::group(
                "Details",
                vec![
                    scalar("Value"),
                    typed_scalar("Position", ScalarType::Int),
                    typed_scalar("InnerPosition", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes.extend([
        (
            40,
            Node::SourceField {
                path: vec!["Name".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            41,
            Node::Position {
                collection: vec!["Rows".into()],
            },
        ),
        (
            42,
            Node::SourceField {
                path: vec!["Keep".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            43,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            44,
            Node::Call {
                function: "equal".into(),
                args: vec![42, 43],
            },
        ),
        (
            45,
            Node::Position {
                collection: Vec::new(),
            },
        ),
    ]);
    project.root.children[0] = Scope {
        target_field: "Details".into(),
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        filter: Some(44),
        bindings: vec![
            MappingBinding {
                target_field: "Value".into(),
                node: 40,
            },
            MappingBinding {
                target_field: "Position".into(),
                node: 41,
            },
            MappingBinding {
                target_field: "InnerPosition".into(),
                node: 45,
            },
        ],
        ..Scope::default()
    };

    let program = lower(&project).expect("framed fields, positions, and source filters lower");
    let details = &program.root.children[0];

    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 40, 41, 42, 43, 44, 45]
    );
    assert_eq!(
        details.iteration,
        Some(IterationPlan::new(
            crate::SourceIteration::new(vec!["Rows".into()]),
            Some(44),
            None,
            Vec::new(),
            crate::IterationOutput::Repeated,
        ))
    );
    assert_eq!(
        program.expressions[2],
        crate::ExpressionNode {
            id: 40,
            expression: Expression::SourceField {
                frame: Some(vec!["Rows".into()]),
                path: vec!["Name".into()],
            },
        }
    );
    assert_eq!(
        program.expressions[3],
        crate::ExpressionNode {
            id: 41,
            expression: Expression::Position {
                collection: vec!["Rows".into()],
            },
        }
    );
    assert_eq!(
        program.expressions[6],
        crate::ExpressionNode {
            id: 44,
            expression: Expression::Call {
                function: ScalarFunction::Equal,
                args: vec![42, 43],
            },
        }
    );
    assert_eq!(
        program.expressions[7],
        crate::ExpressionNode {
            id: 45,
            expression: Expression::Position {
                collection: Vec::new(),
            },
        }
    );
}

#[test]
fn lowers_all_ordinary_aggregate_inputs_without_ignored_state() {
    use mapping::AggregateOp;

    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![typed_scalar("Amount", ScalarType::Int), scalar("Label")],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![
            typed_scalar("Direct", ScalarType::Int),
            typed_scalar("Computed", ScalarType::Int),
            scalar("Joined"),
        ],
    );
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Rows".into()],
                value: vec!["Amount".into()],
                expression: None,
                arg: None,
            },
        ),
        (
            2,
            Node::SourceField {
                frame: Some(vec!["Rows".into()]),
                path: vec!["Amount".into()],
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            4,
            Node::Call {
                function: "multiply".into(),
                args: vec![2, 3],
            },
        ),
        (
            5,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Rows".into()],
                value: vec!["ignored".into()],
                expression: Some(4),
                arg: None,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::String(" | ".into()),
            },
        ),
        (
            7,
            Node::Aggregate {
                function: AggregateOp::Join,
                collection: vec!["Rows".into()],
                value: vec!["Label".into()],
                expression: None,
                arg: Some(6),
            },
        ),
    ]);
    project.root = Scope {
        bindings: vec![
            MappingBinding {
                target_field: "Direct".into(),
                node: 1,
            },
            MappingBinding {
                target_field: "Computed".into(),
                node: 5,
            },
            MappingBinding {
                target_field: "Joined".into(),
                node: 7,
            },
        ],
        ..Scope::default()
    };

    let program = lower(&project).expect("ordinary aggregates lower");

    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(
        program.expressions[0].expression,
        Expression::Aggregate {
            function: crate::AggregateFunction::Sum,
            collection: vec!["Rows".into()],
            value: crate::AggregateValue::Path(vec!["Amount".into()]),
            arg: None,
        }
    );
    assert_eq!(
        program.expressions[4].expression,
        Expression::Aggregate {
            function: crate::AggregateFunction::Sum,
            collection: vec!["Rows".into()],
            value: crate::AggregateValue::Expression(4),
            arg: None,
        }
    );
    assert_eq!(
        program.expressions[6].expression,
        Expression::Aggregate {
            function: crate::AggregateFunction::Join,
            collection: vec!["Rows".into()],
            value: crate::AggregateValue::Path(vec!["Label".into()]),
            arg: Some(6),
        }
    );
}

#[test]
fn rejects_filters_without_iteration_before_subset_lowering() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    project.root.filter = Some(40);

    let diagnostics = lower(&project)
        .expect_err("filters require engine-valid iteration")
        .into_diagnostics();

    assert_eq!(
        diagnostics,
        vec![Diagnostic::Validation {
            location: "root scope".into(),
            message: "filter has no iterated source".into(),
        }]
    );
}
