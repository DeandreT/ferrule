use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use engine::{EngineError, ExecutionPurpose, RuntimeParameters, TraceEvent, TraceSink};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, EdiAutocomplete, EdiBoundaryKind, EdifactAutocomplete, FormatOptions, Graph,
    NamedSource, NamedTarget, Node, Project, RuntimeValue, Scope, ScopeIteration, X12Autocomplete,
};

const FIRST: &str = "2026-10-13T07:08:09.123456789-07:00";
const SECOND: &str = "2031-02-03T18:19:20+05:30";

fn json_options() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    }
}

fn binding(field: &str, node: u32) -> Binding {
    Binding {
        target_field: field.into(),
        node,
    }
}

fn clock_project() -> Project {
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("ObservedAt", ScalarType::String)],
        ),
        source_path: None,
        target_path: Some("primary.json".into()),
        source_options: json_options(),
        target_options: json_options(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime,
                },
            )]),
        },
        root: Scope {
            bindings: vec![binding("ObservedAt", 0)],
            ..Scope::default()
        },
    }
}

fn row_project() -> Project {
    let mut project = clock_project();
    project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("ObservedAt", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Choice", ScalarType::Int),
        ],
    )
    .repeating();
    project.extra_sources.push(NamedSource {
        name: "catalog".into(),
        path: "catalog.json".into(),
        schema: SchemaNode::group(
            "Catalog",
            vec![SchemaNode::scalar("Label", ScalarType::String)],
        ),
        options: json_options(),
        dynamic_path: None,
    });
    project.graph.nodes.extend([
        (
            1,
            Node::SourceField {
                frame: Some(vec!["Rows".into()]),
                path: vec!["Value".into()],
            },
        ),
        (
            2,
            Node::SourceField {
                frame: None,
                path: vec!["catalog".into(), "Label".into()],
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(7),
            },
        ),
        (
            4,
            Node::RuntimeParameterDefault {
                name: "choice".into(),
                ty: ScalarType::Int,
                default: 3,
                preview: Some("11".into()),
            },
        ),
    ]);
    project.root = Scope {
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        bindings: vec![
            binding("Value", 1),
            binding("ObservedAt", 0),
            binding("Label", 2),
            binding("Choice", 4),
        ],
        ..Scope::default()
    };
    project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: Some("audit.json".into()),
        schema: SchemaNode::group(
            "Audit",
            vec![
                SchemaNode::scalar("ObservedAt", ScalarType::String),
                SchemaNode::scalar("Label", ScalarType::String),
                SchemaNode::scalar("Choice", ScalarType::Int),
            ],
        ),
        options: json_options(),
        root: Scope {
            bindings: vec![
                binding("ObservedAt", 0),
                binding("Label", 2),
                binding("Choice", 4),
            ],
            ..Scope::default()
        },
    });
    project
}

fn expected_rows(clock: &str, choice: Option<i64>) -> serde_json::Value {
    let mut rows = serde_json::json!([
        {"Value": "alpha", "ObservedAt": clock, "Label": "shared-雪"},
        {"Value": "beta", "ObservedAt": clock, "Label": "shared-雪"}
    ]);
    if let Some(choice) = choice {
        for row in rows.as_array_mut().unwrap() {
            row["Choice"] = choice.into();
        }
    }
    rows
}

fn expected_audit(clock: &str, choice: Option<i64>) -> serde_json::Value {
    let mut audit = serde_json::json!({"ObservedAt": clock, "Label": "shared-雪"});
    if let Some(choice) = choice {
        audit["Choice"] = choice.into();
    }
    audit
}

fn assert_json_artifact(
    artifact: &cli::PayloadArtifact,
    target: &str,
    path: &str,
    records: usize,
    expected: &serde_json::Value,
) -> anyhow::Result<()> {
    assert_eq!(artifact.target, target);
    assert_eq!(artifact.path, Path::new(path));
    assert_eq!(artifact.records_written, records);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&artifact.bytes)?,
        *expected
    );
    Ok(())
}

fn output_files(paths: &[&str]) -> std::io::Result<Vec<Option<Vec<u8>>>> {
    paths
        .iter()
        .map(|path| match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        })
        .collect()
}

#[test]
fn explicit_clocks_replay_complete_json_outputs_and_preserve_host_data() -> anyhow::Result<()> {
    let project = row_project();
    let original_project = serde_json::to_vec(&project)?;
    let primary_bytes = br#"{"Rows":[{"Value":"alpha"},{"Value":"beta"}]}"#.to_vec();
    let catalog_bytes = "{\"Label\":\"shared-雪\"}".as_bytes().to_vec();
    let original_primary = primary_bytes.clone();
    let original_catalog = catalog_bytes.clone();
    let primary = cli::PayloadDocument::new(Path::new("input.json"), &primary_bytes)?;
    let catalog = cli::PayloadDocument::new(Path::new("catalog.json"), &catalog_bytes)?;
    let extras = [cli::NamedPayloadInput::new("catalog", catalog)?];
    let mut parameters = RuntimeParameters::new();
    parameters.insert("choice", Value::String("5".into()))?;
    let original_parameters = format!("{parameters:?}");
    let output_paths = ["/virtual/primary.json", "/virtual/audit.json"];
    let original_outputs = output_files(&output_paths)?;

    for clock in [FIRST, SECOND] {
        let options = cli::PayloadRunOptions::new(primary)
            .with_current_datetime(clock)?
            .with_extra_sources(&extras)
            .with_runtime_parameters(&parameters);
        let all = cli::run_project_value_payloads(
            &project,
            Path::new("/virtual/project.json"),
            &options,
        )?;
        assert_eq!(all.records_written, 2);
        assert_eq!(all.artifacts.len(), 2);
        assert_json_artifact(
            &all.artifacts[0],
            "Result",
            "/virtual/primary.json",
            2,
            &expected_rows(clock, Some(5)),
        )?;
        assert_json_artifact(
            &all.artifacts[1],
            "audit",
            "/virtual/audit.json",
            1,
            &expected_audit(clock, Some(5)),
        )?;
        assert_eq!(
            cli::run_project_value_payloads(
                &project,
                Path::new("/virtual/project.json"),
                &options
            )?,
            all
        );

        for (selection, index, records) in [
            (cli::TargetSelection::Primary, 0, 2),
            (cli::TargetSelection::Named("audit"), 1, 1),
        ] {
            let selected = cli::run_project_value_payloads(
                &project,
                Path::new("/virtual/project.json"),
                &cli::PayloadRunOptions::new(primary)
                    .with_current_datetime(clock)?
                    .with_extra_sources(&extras)
                    .with_runtime_parameters(&parameters)
                    .with_target(selection),
            )?;
            assert_eq!(selected.records_written, records);
            assert_eq!(selected.artifacts, vec![all.artifacts[index].clone()]);
        }
        assert_eq!(serde_json::to_vec(&project)?, original_project);
        assert_eq!(primary_bytes, original_primary);
        assert_eq!(catalog_bytes, original_catalog);
        assert_eq!(format!("{parameters:?}"), original_parameters);
        assert_eq!(output_files(&output_paths)?, original_outputs);
    }
    Ok(())
}

#[test]
fn valid_clock_boundaries_preserve_the_complete_lexical_value() -> anyhow::Result<()> {
    let project = clock_project();
    let original_project = serde_json::to_vec(&project)?;
    let input = cli::PayloadDocument::new(Path::new("input.json"), b"{}")?;
    for clock in [
        "2024-02-29T00:00:00Z",
        "2000-02-29T23:59:59.0+00:00",
        "2026-10-13T07:08:09.000000000-00:00",
        "2026-10-13T07:08:09.123456789+05:30",
        "2026-10-13T07:08:09-07:45",
        "0001-01-01T00:00:00+14:00",
        "9999-12-31T23:59:59.999999999-14:00",
        "0001-01-01T00:00:00-14:00",
        "9999-12-31T23:59:59+14:00",
    ] {
        let options = cli::PayloadRunOptions::new(input).with_current_datetime(clock)?;
        let outcome = cli::run_project_value_payloads(
            &project,
            Path::new("/virtual/project.json"),
            &options,
        )?;
        assert_eq!(outcome.records_written, 1);
        assert_eq!(outcome.artifacts.len(), 1);
        assert_json_artifact(
            &outcome.artifacts[0],
            "Result",
            "/virtual/primary.json",
            1,
            &serde_json::json!({"ObservedAt": clock}),
        )?;
        assert_eq!(serde_json::to_vec(&project)?, original_project);
    }
    Ok(())
}

#[test]
fn invalid_clocks_fail_at_the_public_builder_without_ambient_fallback() -> anyhow::Result<()> {
    let input = cli::PayloadDocument::new(Path::new("input.json"), b"not JSON")?;
    for clock in [
        "",
        "2026-10-13T07:08:09",
        "2026-10-13 07:08:09Z",
        "2026-10-13t07:08:09Z",
        "2026-10-13T07:08:09z",
        "2026/10/13T07:08:09Z",
        "0000-01-01T00:00:00Z",
        "-0001-01-01T00:00:00Z",
        "10000-01-01T00:00:00Z",
        "1900-02-29T00:00:00Z",
        "2023-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:60:00Z",
        "2026-01-01T00:00:60Z",
        "2026-10-13T07:08:09.Z",
        "2026-10-13T07:08:09.1234567890Z",
        "2026-10-13T07:08:09,1Z",
        "2026-10-13T07:08:09.雪Z",
        "2026-10-13T07:08:09+14:01",
        "2026-10-13T07:08:09-14:01",
        "2026-10-13T07:08:09+15:00",
        "2026-10-13T07:08:09-15:00",
        "2026-10-13T07:08:09+00:60",
        "2026-10-13T07:08:09+0000",
        "2026-10-13T07:08:09+00",
        "2026-10-13T07:08:09+00:00:00",
        "2026-10-13T07:08:09Z[UTC]",
        "2026-10-13T07:08:09Zextra",
        " 2026-10-13T07:08:09Z",
        "2026-10-13T07:08:09Z ",
        "2026-10-13T07:08:09Z\0",
        "2026-10-13T07:08:09Z\n",
        "2026-10-13T07:08:09.123456789+00:00x",
    ] {
        let error = cli::PayloadRunOptions::new(input)
            .with_current_datetime(clock)
            .err()
            .expect("invalid supplied clock must fail before malformed input is read");
        assert!(
            error.to_string().contains("payload current dateTime"),
            "{clock:?}: {error:#}"
        );
    }
    let oversized = "x".repeat(4096);
    assert!(
        cli::PayloadRunOptions::new(input)
            .with_current_datetime(&oversized)
            .is_err()
    );
    assert!(
        cli::PayloadRunOptions::new(input)
            .with_current_datetime(FIRST)?
            .with_current_datetime("invalid")
            .is_err()
    );
    Ok(())
}

#[test]
fn a_later_valid_clock_builder_replaces_the_previous_value() -> anyhow::Result<()> {
    let input = cli::PayloadDocument::new(Path::new("input.json"), b"{}")?;
    let options = cli::PayloadRunOptions::new(input)
        .with_current_datetime(FIRST)?
        .with_current_datetime(SECOND)?;
    let outcome = cli::run_project_value_payloads(
        &clock_project(),
        Path::new("/virtual/project.json"),
        &options,
    )?;
    assert_eq!(outcome.artifacts.len(), 1);
    assert_json_artifact(
        &outcome.artifacts[0],
        "Result",
        "/virtual/primary.json",
        1,
        &serde_json::json!({"ObservedAt": SECOND}),
    )?;
    Ok(())
}

#[test]
fn omitted_clock_keeps_one_ambient_sample_for_all_rows_and_targets() -> anyhow::Result<()> {
    let project = row_project();
    let primary = cli::PayloadDocument::new(
        Path::new("input.json"),
        br#"{"Rows":[{"Value":"alpha"},{"Value":"beta"}]}"#,
    )?;
    let catalog_bytes = "{\"Label\":\"shared-雪\"}";
    let catalog = cli::PayloadDocument::new(Path::new("catalog.json"), catalog_bytes.as_bytes())?;
    let extras = [cli::NamedPayloadInput::new("catalog", catalog)?];
    let before = jiff::Timestamp::now();
    let outcome = cli::run_project_value_payloads(
        &project,
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(primary).with_extra_sources(&extras),
    )?;
    let after = jiff::Timestamp::now();
    let rows: serde_json::Value = serde_json::from_slice(&outcome.artifacts[0].bytes)?;
    let clock = rows[0]["ObservedAt"].as_str().expect("ambient clock value");
    let instant: jiff::Timestamp = clock.parse()?;
    assert!(before <= instant && instant <= after);
    let offset = &clock.as_bytes()[clock.len() - 6..];
    assert!(matches!(offset[0], b'+' | b'-'));
    assert_eq!(offset[3], b':');
    assert_eq!(clock.as_bytes()[10], b'T');
    assert_eq!(outcome.records_written, 2);
    assert_eq!(outcome.artifacts.len(), 2);
    assert_json_artifact(
        &outcome.artifacts[0],
        "Result",
        "/virtual/primary.json",
        2,
        &expected_rows(clock, Some(7)),
    )?;
    assert_json_artifact(
        &outcome.artifacts[1],
        "audit",
        "/virtual/audit.json",
        1,
        &expected_audit(clock, Some(7)),
    )?;
    Ok(())
}

#[test]
fn supplied_clock_does_not_change_preview_defaults_or_host_precedence() -> anyhow::Result<()> {
    let project = row_project();
    let primary = cli::PayloadDocument::new(
        Path::new("input.json"),
        br#"{"Rows":[{"Value":"alpha"},{"Value":"beta"}]}"#,
    )?;
    let catalog_bytes = "{\"Label\":\"shared-雪\"}";
    let catalog = cli::PayloadDocument::new(Path::new("catalog.json"), catalog_bytes.as_bytes())?;
    let extras = [cli::NamedPayloadInput::new("catalog", catalog)?];
    let mut supplied = RuntimeParameters::new();
    supplied.insert("choice", Value::String("5".into()))?;
    let mut null = RuntimeParameters::new();
    null.insert("choice", Value::Null)?;
    for (purpose, parameters, expected_choice) in [
        (ExecutionPurpose::Run, None, Some(7)),
        (ExecutionPurpose::Preview, None, Some(11)),
        (ExecutionPurpose::Run, Some(&supplied), Some(5)),
        (ExecutionPurpose::Preview, Some(&supplied), Some(5)),
        (ExecutionPurpose::Run, Some(&null), None),
        (ExecutionPurpose::Preview, Some(&null), None),
    ] {
        let mut options = cli::PayloadRunOptions::new(primary)
            .with_current_datetime(FIRST)?
            .with_extra_sources(&extras)
            .with_execution_purpose(purpose);
        if let Some(parameters) = parameters {
            options = options.with_runtime_parameters(parameters);
        }
        let outcome = cli::run_project_value_payloads(
            &project,
            Path::new("/virtual/project.json"),
            &options,
        )?;
        assert_eq!(outcome.records_written, 2);
        assert_eq!(outcome.artifacts.len(), 2);
        assert_json_artifact(
            &outcome.artifacts[0],
            "Result",
            "/virtual/primary.json",
            2,
            &expected_rows(FIRST, expected_choice),
        )?;
        assert_json_artifact(
            &outcome.artifacts[1],
            "audit",
            "/virtual/audit.json",
            1,
            &expected_audit(FIRST, expected_choice),
        )?;
    }
    Ok(())
}

#[test]
fn clock_override_preserves_missing_parameter_errors_and_unselected_target_isolation()
-> anyhow::Result<()> {
    let mut project = clock_project();
    project.target = SchemaNode::group(
        "Result",
        vec![SchemaNode::scalar("Required", ScalarType::Int)],
    );
    project.root.bindings = vec![binding("Required", 1)];
    project.graph.nodes.insert(
        1,
        Node::RuntimeParameter {
            name: "required".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    );
    project.extra_targets.push(NamedTarget {
        name: "clock-only".into(),
        path: Some("clock-only.json".into()),
        schema: SchemaNode::group(
            "Clock",
            vec![SchemaNode::scalar("ObservedAt", ScalarType::String)],
        ),
        options: json_options(),
        root: Scope {
            bindings: vec![binding("ObservedAt", 0)],
            ..Scope::default()
        },
    });
    let input = cli::PayloadDocument::new(Path::new("input.json"), b"{}")?;
    let error = cli::run_project_value_payloads(
        &project,
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(input).with_current_datetime(FIRST)?,
    )
    .unwrap_err();
    assert!(
        matches!(error.downcast_ref::<EngineError>(), Some(EngineError::MissingRuntimeParameter { node: 1, name }) if name == "required")
    );
    let mut invalid = RuntimeParameters::new();
    invalid.insert("required", Value::String("not an integer".into()))?;
    let error = cli::run_project_value_payloads(
        &project,
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(input)
            .with_current_datetime(FIRST)?
            .with_runtime_parameters(&invalid),
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<EngineError>(),
        Some(EngineError::RuntimeParameterType { node: 1, .. })
    ));
    let selected = cli::run_project_value_payloads(
        &project,
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(input)
            .with_current_datetime(FIRST)?
            .with_target(cli::TargetSelection::Named("clock-only")),
    )?;
    assert_eq!(selected.records_written, 1);
    assert_eq!(selected.artifacts.len(), 1);
    assert_json_artifact(
        &selected.artifacts[0],
        "clock-only",
        "/virtual/clock-only.json",
        1,
        &serde_json::json!({"ObservedAt": FIRST}),
    )?;
    Ok(())
}

#[derive(Default)]
struct ClockTrace(RefCell<Vec<TraceEvent>>);

impl TraceSink for ClockTrace {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

#[test]
fn tracing_observes_the_same_admitted_clock_as_output_serialization() -> anyhow::Result<()> {
    let trace = ClockTrace::default();
    let input = cli::PayloadDocument::new(Path::new("input.json"), b"{}")?;
    let outcome = cli::run_project_value_payloads(
        &clock_project(),
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(input)
            .with_current_datetime(FIRST)?
            .with_trace_sink(&trace),
    )?;
    assert_json_artifact(
        &outcome.artifacts[0],
        "Result",
        "/virtual/primary.json",
        1,
        &serde_json::json!({"ObservedAt": FIRST}),
    )?;
    let events = trace.0.borrow();
    let clocks = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeValue { node: 0, value, .. } => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!clocks.is_empty());
    for value in clocks {
        assert_eq!(value.value_type, "string");
        assert_eq!(value.preview, FIRST);
        assert!(!value.truncated);
    }
    Ok(())
}

enum Field<'a> {
    Scalar(&'a str, &'a str),
    Group(&'a str, Vec<Field<'a>>),
}

fn constant_record(name: &str, fields: &[Field<'_>], graph: &mut Graph) -> (SchemaNode, Scope) {
    let mut schema_fields = Vec::new();
    let mut bindings = Vec::new();
    let mut children = Vec::new();
    for field in fields {
        match field {
            Field::Scalar(name, value) => {
                let node = graph.nodes.len() as u32;
                graph.nodes.insert(
                    node,
                    Node::Const {
                        value: Value::String((*value).into()),
                    },
                );
                schema_fields.push(SchemaNode::scalar(*name, ScalarType::String));
                bindings.push(binding(name, node));
            }
            Field::Group(name, fields) => {
                let (schema, scope) = constant_record(name, fields, graph);
                schema_fields.push(schema);
                children.push(scope);
            }
        }
    }
    (
        SchemaNode::group(name, schema_fields),
        Scope {
            target_field: name.into(),
            bindings,
            children,
            ..Scope::default()
        },
    )
}

fn edifact_target(graph: &mut Graph) -> NamedTarget {
    use Field::{Group, Scalar};
    let (schema, mut root) = constant_record(
        "Envelope",
        &[
            Group(
                "UNB",
                vec![
                    Group("S001", vec![Scalar("F0001", ""), Scalar("F0002", "")]),
                    Group("S002", vec![Scalar("F0004", "CLOCK-SENDER")]),
                    Group("S003", vec![Scalar("F0010", "CLOCK-RECEIVER")]),
                    Group("S004", vec![Scalar("F0017", ""), Scalar("F0019", "")]),
                    Scalar("F0020", "42"),
                ],
            ),
            Group(
                "UNH",
                vec![
                    Scalar("F0062", "21"),
                    Group(
                        "S009",
                        vec![
                            Scalar("F0065", ""),
                            Scalar("F0052", "D"),
                            Scalar("F0054", "24A"),
                            Scalar("F0051", ""),
                        ],
                    ),
                ],
            ),
            Group("BGM", vec![Scalar("F1001", "order-1")]),
        ],
        graph,
    );
    root.target_field.clear();
    NamedTarget {
        name: "edifact".into(),
        path: Some("clock.edifact".into()),
        schema,
        options: FormatOptions {
            edi_kind: Some(EdiBoundaryKind::Edifact),
            edi_autocomplete: Some(EdiAutocomplete::Edifact(EdifactAutocomplete {
                syntax_level: Some("A".into()),
                syntax_version: Some("4".into()),
                controlling_agency: Some("UNO".into()),
                message_type: Some("ORDERS".into()),
            })),
            ..FormatOptions::default()
        },
        root,
    }
}

fn x12_target(graph: &mut Graph) -> NamedTarget {
    use Field::{Group, Scalar};
    let (schema, mut root) = constant_record(
        "Interchange",
        &[
            Group(
                "ISA",
                vec![
                    Scalar("01", ""),
                    Scalar("02", ""),
                    Scalar("03", ""),
                    Scalar("04", ""),
                    Scalar("05", ""),
                    Scalar("06", "CLOCK-SENDER"),
                    Scalar("07", ""),
                    Scalar("08", "CLOCK-RECEIVER"),
                    Scalar("09", ""),
                    Scalar("10", ""),
                    Scalar("11", "U"),
                    Scalar("12", "00401"),
                    Scalar("13", "42"),
                    Scalar("14", ""),
                    Scalar("15", ""),
                    Scalar("16", ":"),
                ],
            ),
            Group(
                "GS",
                vec![
                    Scalar("01", "PO"),
                    Scalar("02", "CLOCK-SENDER"),
                    Scalar("03", "CLOCK-RECEIVER"),
                    Scalar("04", ""),
                    Scalar("05", ""),
                    Scalar("06", "7"),
                    Scalar("07", "X"),
                    Scalar("08", "004010"),
                ],
            ),
            Group("ST", vec![Scalar("01", "850"), Scalar("02", "0021")]),
            Group(
                "BEG",
                vec![
                    Scalar("01", "00"),
                    Scalar("02", "SA"),
                    Scalar("03", "order-1"),
                ],
            ),
        ],
        graph,
    );
    root.target_field.clear();
    NamedTarget {
        name: "x12".into(),
        path: Some("clock.x12".into()),
        schema,
        options: FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            edi_autocomplete: Some(EdiAutocomplete::X12(X12Autocomplete {
                request_acknowledgement: false,
                transaction_set: Some("850".into()),
            })),
            ..FormatOptions::default()
        },
        root,
    }
}

#[test]
fn all_and_selected_edi_outputs_share_the_exact_engine_clock() -> anyhow::Result<()> {
    let mut project = clock_project();
    project.extra_targets = vec![
        edifact_target(&mut project.graph),
        x12_target(&mut project.graph),
    ];
    let original_project = serde_json::to_vec(&project)?;
    let input_bytes = b"{}".to_vec();
    let original_input = input_bytes.clone();
    let input = cli::PayloadDocument::new(Path::new("input.json"), &input_bytes)?;
    let output_paths = [
        "/virtual/primary.json",
        "/virtual/clock.edifact",
        "/virtual/clock.x12",
    ];
    let original_outputs = output_files(&output_paths)?;
    let cases = [
        (
            FIRST,
            "UNB+UNOA:4+CLOCK-SENDER+CLOCK-RECEIVER+20261013:0708+42'\nUNH+21+ORDERS:D:24A:UN'\nBGM+order-1'\nUNT+3+21'\nUNZ+1+42'\n",
            "ISA*00*          *00*          *ZZ*CLOCK-SENDER   *ZZ*CLOCK-RECEIVER *261013*0708*U*00401*000000042*0*P*:~\nGS*PO*CLOCK-SENDER*CLOCK-RECEIVER*20261013*070809*7*X*004010~\nST*850*0021~\nBEG*00*SA*order-1~\nSE*3*0021~\nGE*1*7~\nIEA*1*000000042~\n",
        ),
        (
            SECOND,
            "UNB+UNOA:4+CLOCK-SENDER+CLOCK-RECEIVER+20310203:1819+42'\nUNH+21+ORDERS:D:24A:UN'\nBGM+order-1'\nUNT+3+21'\nUNZ+1+42'\n",
            "ISA*00*          *00*          *ZZ*CLOCK-SENDER   *ZZ*CLOCK-RECEIVER *310203*1819*U*00401*000000042*0*P*:~\nGS*PO*CLOCK-SENDER*CLOCK-RECEIVER*20310203*181920*7*X*004010~\nST*850*0021~\nBEG*00*SA*order-1~\nSE*3*0021~\nGE*1*7~\nIEA*1*000000042~\n",
        ),
    ];
    for (clock, expected_edifact, expected_x12) in cases {
        let options = cli::PayloadRunOptions::new(input).with_current_datetime(clock)?;
        let outcome = cli::run_project_value_payloads(
            &project,
            Path::new("/virtual/project.json"),
            &options,
        )?;
        assert_eq!(outcome.records_written, 1);
        assert_eq!(outcome.artifacts.len(), 3);
        assert_json_artifact(
            &outcome.artifacts[0],
            "Result",
            "/virtual/primary.json",
            1,
            &serde_json::json!({"ObservedAt": clock}),
        )?;
        for (index, target, path, expected) in [
            (1, "edifact", "/virtual/clock.edifact", expected_edifact),
            (2, "x12", "/virtual/clock.x12", expected_x12),
        ] {
            let artifact = &outcome.artifacts[index];
            assert_eq!(artifact.target, target);
            assert_eq!(artifact.path, Path::new(path));
            assert_eq!(artifact.records_written, 1);
            assert_eq!(artifact.bytes, expected.as_bytes());
            let selected = cli::run_project_value_payloads(
                &project,
                Path::new("/virtual/project.json"),
                &cli::PayloadRunOptions::new(input)
                    .with_current_datetime(clock)?
                    .with_target(cli::TargetSelection::Named(target)),
            )?;
            assert_eq!(selected.records_written, 1);
            assert_eq!(selected.artifacts, vec![artifact.clone()]);
        }
        assert_eq!(
            cli::run_project_value_payloads(
                &project,
                Path::new("/virtual/project.json"),
                &options
            )?,
            outcome
        );
        assert_eq!(serde_json::to_vec(&project)?, original_project);
        assert_eq!(input_bytes, original_input);
        assert_eq!(output_files(&output_paths)?, original_outputs);
    }
    Ok(())
}
