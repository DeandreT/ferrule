use std::cell::RefCell;
use std::error::Error;
use std::path::Path;

use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration, SortFilterOrder};

use crate::trace::TraceSourceRow;
use crate::{ExecutionContext, TraceEvent, TraceOutputKind, TraceSink, run_with_context};

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn project(source: SchemaNode, target: SchemaNode, graph: Graph, root: Scope) -> Project {
    Project {
        source,
        target,
        graph,
        root,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    }
}

#[test]
fn source_row_previews_are_bounded_and_preserve_scalar_states() {
    let source = Instance::Group(vec![
        (
            "é".repeat(100),
            Instance::Scalar(Value::String("🦀".repeat(200))),
        ),
        ("Missing".into(), Instance::Scalar(Value::Null)),
        ("JsonNull".into(), Instance::Scalar(Value::json_null())),
        ("XmlNil".into(), Instance::Scalar(Value::xml_nil())),
        ("Children".into(), Instance::Repeated(Vec::new())),
        ("Nested".into(), Instance::Group(Vec::new())),
        ("Flag".into(), Instance::Scalar(Value::Bool(true))),
        ("Count".into(), Instance::Scalar(Value::Int(3))),
        ("Omitted".into(), Instance::Scalar(Value::Int(4))),
    ]);

    let row = TraceSourceRow::new(&source);

    assert_eq!(row.kind, TraceOutputKind::Group);
    assert_eq!(row.fields.len(), 8);
    assert_eq!(row.omitted_fields, 1);
    assert!(row.fields[0].name_truncated);
    assert!(
        row.fields[0]
            .value
            .as_ref()
            .is_some_and(|value| value.truncated)
    );
    assert_eq!(
        row.fields[1].value.as_ref().map(|value| value.value_type),
        Some("null")
    );
    assert_eq!(
        row.fields[2].value.as_ref().map(|value| value.value_type),
        Some("json null")
    );
    assert_eq!(
        row.fields[3].value.as_ref().map(|value| value.value_type),
        Some("xml nil")
    );
    assert_eq!(row.fields[4].kind, TraceOutputKind::Repeated);
    assert!(row.fields[4].value.is_none());
    assert_eq!(row.fields[5].kind, TraceOutputKind::Group);
    assert!(row.fields[5].value.is_none());
    let bytes = row
        .fields
        .iter()
        .map(|field| field.name.len() + field.value.as_ref().map_or(0, |value| value.preview.len()))
        .sum::<usize>();
    assert!(bytes <= 512, "row retained {bytes} text bytes");

    let scalar = TraceSourceRow::new(&Instance::Scalar(Value::String("🦀".repeat(200))));
    let scalar = scalar.value.expect("scalar preview");
    assert!(scalar.truncated);
    assert_eq!(scalar.preview.len(), 512);
    assert_eq!(scalar.preview.chars().count(), 128);
}

#[test]
fn source_candidates_include_rows_before_filters_and_sorting() -> Result<(), Box<dyn Error>> {
    let project = project(
        SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Keep", ScalarType::Bool),
                        SchemaNode::group("Nested", Vec::new()),
                    ],
                )
                .repeating(),
            ],
        ),
        SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Name", ScalarType::String)])
                    .repeating(),
            ],
        ),
        Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Keep".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                filter: Some(1),
                sort_by: Some(0),
                sort_filter_order: SortFilterOrder::FilterThenSort,
                bindings: vec![Binding {
                    target_field: "Name".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    let row = |name: &str, keep| {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
            ("Nested".into(), Instance::Group(Vec::new())),
        ])
    };
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![row("dropped", false), row("kept", true)]),
    )]);
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    let output = run_with_context(&project, &source, &execution)?;

    assert_eq!(
        output
            .field("Row")
            .and_then(Instance::as_repeated)
            .map(<[_]>::len),
        Some(1)
    );
    let events = collector.0.into_inner();
    let candidates = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            TraceEvent::IterationCandidate {
                ordinal,
                positions,
                source_row,
                ..
            } => Some((index, *ordinal, positions, source_row)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 3);
    assert!(candidates[0].3.is_none(), "once scope has no source row");
    for (candidate, expected_name) in candidates[1..].iter().zip(["dropped", "kept"]) {
        let row = candidate.3.as_ref().expect("source row");
        assert_eq!(row.fields[0].name, "Name");
        assert_eq!(
            row.fields[0]
                .value
                .as_ref()
                .map(|value| value.preview.as_str()),
            Some(expected_name)
        );
        assert_eq!(row.fields[2].kind, TraceOutputKind::Group);
    }
    assert_eq!(candidates[1].1, 1);
    assert_eq!(
        candidates[1].2.last().map(|position| position.index),
        Some(1)
    );
    assert_eq!(candidates[2].1, 2);
    assert_eq!(
        candidates[2].2.last().map(|position| position.index),
        Some(2)
    );
    let first_filter = events
        .iter()
        .position(|event| matches!(event, TraceEvent::FilterDecision { .. }))
        .expect("filter decision");
    assert!(
        candidates[2].0 < first_filter,
        "both raw candidates precede filtering"
    );
    Ok(())
}

#[test]
fn dynamic_document_candidates_preview_each_member_with_its_path() -> Result<(), Box<dyn Error>> {
    let mut project = project(
        SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        Graph {
            nodes: [
                (0, Node::SourceDocumentPath),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path: 0,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    );
    project.source_path = Some("records-*.xml".into());
    project.source_options = mapping::FormatOptions {
        xml_document: true,
        local_xml_file_set: true,
        ..Default::default()
    };
    project.target_options = mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    let member = |path: &str, value: &str| {
        DocumentMember::new(
            path,
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            )]),
        )
        .expect("valid document member")
    };
    let source = Instance::DocumentSet(vec![member("a.xml", "A"), member("b.xml", "B")]);
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &source, &execution)?;

    let candidates = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::IterationCandidate {
                positions,
                source_row: Some(row),
                ..
            } => Some((positions, row)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        candidates[0]
            .0
            .last()
            .and_then(|position| position.document_path.as_deref()),
        Some("a.xml")
    );
    assert_eq!(
        candidates[1]
            .0
            .last()
            .and_then(|position| position.document_path.as_deref()),
        Some("b.xml")
    );
    assert_eq!(
        candidates[0].1.fields[0]
            .value
            .as_ref()
            .map(|value| value.preview.as_str()),
        Some("A")
    );
    assert_eq!(
        candidates[1].1.fields[0]
            .value
            .as_ref()
            .map(|value| value.preview.as_str()),
        Some("B")
    );
    Ok(())
}
