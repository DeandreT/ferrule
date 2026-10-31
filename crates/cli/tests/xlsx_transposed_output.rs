//! Transposed XLSX targets through the CLI file, payload, and standard-stream
//! routes.

use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-xlsx-transposed-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const ROWS: [u32; 2] = [2, 4];
const INPUT: &[u8] = br#"{"Item":[
    {"Name":"Alpha","Amount":10,"Col":2},
    {"Name":"Delta","Col":6},
    {"Name":"Echo","Amount":50,"Col":9}
]}"#;

fn field(path: &str) -> Node {
    Node::SourceField {
        frame: Some(vec!["Item".into()]),
        path: vec![path.into()],
    }
}

/// Items map to columns through `n`; a missing Amount leaves a gap.
fn project() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(0, field("Name"));
    graph.nodes.insert(1, field("Amount"));
    graph.nodes.insert(2, field("Col"));
    let binding = |target_field: &str, node| Binding {
        target_field: target_field.into(),
        node,
    };
    Project {
        source: SchemaNode::group(
            "Data",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Amount", ScalarType::Int),
                        SchemaNode::scalar("Col", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Pivot",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Amount", ScalarType::Int),
                SchemaNode::scalar("n", ScalarType::Int),
            ],
        ),
        source_path: Some("input.json".into()),
        target_path: Some("pivot.xlsx".into()),
        source_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
            xlsx_sheet: Some("Pivot".into()),
            xlsx_rows: ROWS.to_vec(),
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            iteration: ScopeIteration::Source(vec!["Item".into()]),
            bindings: vec![binding("Name", 0), binding("Amount", 1), binding("n", 2)],
            ..Scope::default()
        },
    }
}

fn expected() -> Vec<Instance> {
    let record = |name: &str, amount: Value, column: i64| {
        Instance::Group(
            (vec![
                (
                    "Name".to_string(),
                    Instance::Scalar(Value::String(name.into())),
                ),
                ("Amount".to_string(), Instance::Scalar(amount)),
                ("n".to_string(), Instance::Scalar(Value::Int(column))),
            ])
            .into(),
        )
    };
    vec![
        record("Alpha", Value::Int(10), 2),
        record("Delta", Value::Null, 6),
        record("Echo", Value::Int(50), 9),
    ]
}

fn write_project(scratch: &Scratch, project: &Project) -> PathBuf {
    assert!(
        engine::validate(project).is_empty(),
        "{:?}",
        engine::validate(project)
    );
    let path = scratch.0.join("project.json");
    std::fs::write(&path, serde_json::to_vec(project).unwrap()).unwrap();
    std::fs::write(scratch.0.join("input.json"), INPUT).unwrap();
    path
}

#[test]
fn transposed_target_writes_through_the_file_route() {
    let scratch = Scratch::new("file");
    let project = project();
    let project_path = write_project(&scratch, &project);
    let output = scratch.0.join("pivot.xlsx");

    let written = cli::run_project(&project_path, &scratch.0.join("input.json"), &output).unwrap();

    assert_eq!(written, 3);
    assert_eq!(
        format_xlsx::read_transposed(&output, &project.target, Some("Pivot"), &ROWS).unwrap(),
        expected()
    );
}

#[test]
fn transposed_target_renders_through_the_payload_route() {
    let scratch = Scratch::new("payload");
    let project = project();
    let project_path = write_project(&scratch, &project);

    let input = cli::PayloadDocument::new(Path::new("input.json"), INPUT).unwrap();
    let outcome = cli::run_project_value_payloads(
        &project,
        &project_path,
        &cli::PayloadRunOptions::new(input),
    )
    .unwrap();

    assert_eq!(outcome.records_written, 3);
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(
        format_xlsx::from_bytes_transposed(
            &outcome.artifacts[0].bytes,
            &project.target,
            Some("Pivot"),
            &ROWS
        )
        .unwrap(),
        expected()
    );
}

#[test]
fn invalid_records_refuse_before_any_output_is_written() {
    let scratch = Scratch::new("invalid");
    let project = project();
    let project_path = write_project(&scratch, &project);
    std::fs::write(
        scratch.0.join("input.json"),
        br#"{"Item":[{"Name":"Alpha","Col":3},{"Name":"Bravo","Col":3}]}"#,
    )
    .unwrap();
    let output = scratch.0.join("pivot.xlsx");

    let error =
        cli::run_project(&project_path, &scratch.0.join("input.json"), &output).unwrap_err();

    assert!(
        format!("{error:#}").contains("record 1: column 3 is already written by record 0"),
        "{error:#}"
    );
    assert!(!output.exists());
}

#[test]
fn transposed_target_refuses_streams_update_existing_and_bounded_profiles() {
    let scratch = Scratch::new("refusals");
    let project = project();
    let project_path = write_project(&scratch, &project);

    let mut options = cli::StandardIoRunOptions::new();
    options.input_path = Some(Path::new("-"));
    options.output_path = Some(Path::new("-"));
    let mut stdin: &[u8] = INPUT;
    let error = cli::run_project_with_standard_streams(&project_path, &options, &mut stdin)
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("transposed XLSX output is unavailable on standard streams"),
        "{error:#}"
    );

    let input = cli::PayloadDocument::new(Path::new("input.json"), INPUT).unwrap();
    let error = cli::run_project_value_payloads(
        &project,
        &project_path,
        &cli::PayloadRunOptions::new(input).with_bounded_documents(),
    )
    .err()
    .unwrap();
    assert!(
        format!("{error:#}").contains("bounded document profile supports only new flat XLSX"),
        "{error:#}"
    );

    let mut update = project.clone();
    update.target_options.xlsx_update_existing = true;
    assert!(engine::validate(&update).iter().any(|issue| {
        issue.message
            == "`xlsx_update_existing` cannot update a transposed XLSX layout; only flat tables and hierarchical layouts update existing workbooks"
    }));
    std::fs::write(&project_path, serde_json::to_vec(&update).unwrap()).unwrap();
    let output = scratch.0.join("pivot.xlsx");
    let error =
        cli::run_project(&project_path, &scratch.0.join("input.json"), &output).unwrap_err();
    assert!(
        format!("{error:#}").contains("cannot update a transposed XLSX layout"),
        "{error:#}"
    );
    assert!(!output.exists());
}
