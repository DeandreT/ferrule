use std::ops::ControlFlow;
use std::path::Path;

use engine::{
    DocumentVisit, EngineError, ExecutionContext, InvocationCounterLimits, TargetSelection,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration,
};

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Order", ScalarType::String),
                        SchemaNode::scalar("Item", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Item", ScalarType::String),
                SchemaNode::scalar("Line", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::SourceField {
                        path: vec!["Order".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Item".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::InvocationCounter {
                        key: "rows".into(),
                        start: 3,
                        step: 3,
                        restart: None,
                    },
                ),
                (
                    5,
                    Node::Aggregate {
                        function: AggregateOp::LastPresentText,
                        collection: vec!["Rows".into()],
                        value: vec!["Order".into()],
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::String(".txt".into()),
                    },
                ),
                (
                    7,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![5, 6],
                    },
                ),
                (
                    8,
                    Node::Aggregate {
                        function: AggregateOp::LastDistinctText,
                        collection: vec![],
                        value: vec!["Item".into()],
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    9,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![2],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["Rows".into()],
                output_path: 7,
            },
            construction: ScopeConstruction::DocumentRows,
            group_by: Some(1),
            children: vec![Scope {
                iteration: ScopeIteration::Source(vec![]),
                filter: Some(9),
                group_by: Some(2),
                bindings: vec![
                    Binding {
                        target_field: "Item".into(),
                        node: 8,
                    },
                    Binding {
                        target_field: "Line".into(),
                        node: 4,
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

fn input(rows: &[(&str, Option<&str>)]) -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                rows.iter()
                    .map(|(order, item)| {
                        let mut fields = vec![(
                            "Order".into(),
                            Instance::Scalar(Value::String((*order).into())),
                        )];
                        if let Some(item) = item {
                            fields.push((
                                "Item".into(),
                                Instance::Scalar(Value::String((*item).into())),
                            ));
                        }
                        Instance::Group(fields.into())
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn context() -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("rows.ferrule")).with_invocation_counter_limits(
        InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 16,
        },
    )
}

fn csv(schema: &SchemaNode, instance: &Instance) -> String {
    let Instance::Repeated(rows) = instance else {
        panic!("document body is not a row set")
    };
    format_csv::to_string_with_options(
        schema,
        rows,
        &format_csv::CsvWriteOptions {
            delimiter: Some('|'),
            quote_disabled: true,
            raw_delimited_crlf: true,
            has_headers: false,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn interleaved_orders_emit_complete_row_sets_with_one_counter() {
    let project = project();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let source = input(&[
        ("A", Some("x")),
        ("B", Some("z")),
        ("A", Some("y")),
        ("A", Some("x")),
    ]);
    let mut actual = vec![];
    let result = engine::visit_target_documents(
        &project,
        &source,
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            actual.push((
                document.path().to_owned(),
                csv(&project.target, document.value()),
            ));
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 2 });
    assert_eq!(
        actual,
        [
            ("A.txt".into(), "x|1\r\ny|2\r\n".into()),
            ("B.txt".into(), "z|3\r\n".into())
        ]
    );
}

#[test]
fn an_empty_row_set_still_emits_an_empty_document() {
    let project = project();
    let mut actual = vec![];
    let result = engine::visit_target_documents(
        &project,
        &input(&[("A", None)]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            actual.push((
                document.path().to_owned(),
                csv(&project.target, document.value()),
            ));
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
    assert_eq!(actual, [("A.txt".into(), String::new())]);
}

#[test]
fn missing_inner_keys_do_not_hide_later_valid_rows_or_create_extra_rows() {
    let project = project();
    let mut actual = vec![];
    let result = engine::visit_target_documents(
        &project,
        &input(&[("A", None), ("A", Some("x")), ("A", None)]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            actual.push((
                document.path().to_owned(),
                csv(&project.target, document.value()),
            ));
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
    assert_eq!(actual, [("A.txt".into(), "x|1\r\n".into())]);
}

#[test]
fn invalid_row_shape_is_refused_before_callbacks() {
    let mut project = project();
    project.root.children[0].target_field = "invented_wrapper".into();
    let mut called = false;
    let error = engine::visit_target_documents(
        &project,
        &input(&[("A", Some("x"))]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |_| {
            called = true;
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap_err();
    assert_eq!(error.accepted, 0);
    assert!(!called);
    assert!(matches!(
        error.source,
        EngineError::DocumentRowsConstruction { .. }
    ));
    assert!(!engine::validate(&project).is_empty());
}

#[test]
fn document_row_wire_and_pruning_keep_body_and_filename_dependencies() {
    let mut project = project();
    project.graph.nodes.insert(
        99,
        Node::Const {
            value: Value::String("unused".into()),
        },
    );
    let text = serde_json::to_string(&project).unwrap();
    assert!(text.contains("document_rows"));
    let mut restored: Project = serde_json::from_str(&text).unwrap();
    restored.prune_unreachable_nodes();
    assert!(!restored.graph.nodes.contains_key(&99));
    for node in 1..=9 {
        assert!(
            restored.graph.nodes.contains_key(&node),
            "missing dependency {node}"
        );
    }
    assert!(engine::validate(&restored).is_empty());
}

#[test]
fn flat_csv_row_roots_keep_nested_grouping_and_missing_field_presence() {
    let mut project = project();
    let mut row = project.source.child("Rows").unwrap().clone();
    row.name = "Input".into();
    row.repeating = false;
    project.source = row;
    project.root.set_source(Some(vec![]));
    let Node::SourceField { frame, .. } = project.graph.nodes.get_mut(&2).unwrap() else {
        unreachable!()
    };
    *frame = Some(vec![]);
    let Node::Aggregate { collection, .. } = project.graph.nodes.get_mut(&5).unwrap() else {
        unreachable!()
    };
    collection.clear();
    let wrapped = input(&[
        ("A", Some("x")),
        ("B", Some("z")),
        ("A", None),
        ("A", Some("y")),
    ]);
    let source = wrapped.field("Rows").unwrap();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let mut actual = vec![];
    let result = engine::visit_target_documents(
        &project,
        source,
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            actual.push((
                document.path().to_owned(),
                csv(&project.target, document.value()),
            ));
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 2 });
    assert_eq!(
        actual,
        [
            ("A.txt".into(), "x|1\r\ny|2\r\n".into()),
            ("B.txt".into(), "z|3\r\n".into())
        ]
    );
}
