use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::RuntimeParameters;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, NamedTarget, Node, Project,
    Scope, ScopeIteration,
};

fn json_options() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_payload_execution_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dynamic_output_project() -> Project {
    let target_schema = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Correlation", ScalarType::String),
        ],
    );
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("File", ScalarType::String),
                        SchemaNode::scalar("Value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: target_schema,
        source_path: None,
        target_path: None,
        source_options: json_options(),
        target_options: json_options(),
        extra_sources: vec![NamedSource {
            name: "catalog".into(),
            path: "catalog.json".into(),
            schema: SchemaNode::group(
                "Catalog",
                vec![SchemaNode::scalar("Label", ScalarType::String)],
            ),
            options: json_options(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: Some("audit.json".into()),
            schema: SchemaNode::group(
                "Audit",
                vec![
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("Correlation", ScalarType::String),
                ],
            ),
            options: json_options(),
            root: Scope {
                bindings: vec![
                    Binding {
                        target_field: "Label".into(),
                        node: 2,
                    },
                    Binding {
                        target_field: "Correlation".into(),
                        node: 3,
                    },
                ],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["Rows".into()]),
                        path: vec!["File".into()],
                    },
                ),
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
                    Node::RuntimeParameter {
                        name: "correlation_id".into(),
                        ty: ScalarType::String,
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["Rows".into()],
                output_path: 0,
            },
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Label".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Correlation".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        },
    }
}

#[test]
fn payload_run_returns_dynamic_primary_then_extra_artifacts() -> anyhow::Result<()> {
    let source = br#"{"Rows":[{"File":"a.json","Value":"A"},{"File":"b.json","Value":"B"}]}"#;
    let catalog = br#"{"Label":"shared"}"#;
    let primary = cli::PayloadDocument::new(Path::new("input.json"), source)?;
    let catalog = cli::PayloadDocument::new(Path::new("catalog.json"), catalog)?;
    let named = [cli::NamedPayloadInput::new("catalog", catalog)?];
    let mut parameters = RuntimeParameters::new();
    parameters.insert("correlation_id", Value::String("txn-42".into()))?;

    let outcome = cli::run_project_value_payloads(
        &dynamic_output_project(),
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(primary)
            .with_extra_sources(&named)
            .with_output_path(Path::new("payload-out"))
            .with_runtime_parameters(&parameters),
    )?;

    assert_eq!(outcome.records_written, 2);
    assert_eq!(
        outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.target.as_str(), artifact.path.as_path()))
            .collect::<Vec<_>>(),
        vec![
            ("Result", Path::new("payload-out/a.json")),
            ("Result", Path::new("payload-out/b.json")),
            ("audit", Path::new("/virtual/audit.json")),
        ]
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&outcome.artifacts[0].bytes)?,
        serde_json::json!({
            "Value": "A",
            "Label": "shared",
            "Correlation": "txn-42"
        })
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&outcome.artifacts[2].bytes)?,
        serde_json::json!({
            "Label": "shared",
            "Correlation": "txn-42"
        })
    );
    Ok(())
}

fn dynamic_source_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Files",
            vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Path", ScalarType::String),
                        SchemaNode::scalar("Value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: Some("output.json".into()),
        source_options: json_options(),
        target_options: json_options(),
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: SchemaNode::group(
                "Document",
                vec![
                    SchemaNode::group(
                        "Item",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            ),
            options: json_options(),
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: vec!["File".into()],
            }),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["File".into()]),
                        path: Vec::new(),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["document".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Path".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Value".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn dynamic_source_paths_resolve_only_from_supplied_payloads() -> anyhow::Result<()> {
    let source = cli::PayloadDocument::new(
        Path::new("files.json"),
        br#"{"File":["first.json","second.json"]}"#,
    )?;
    let first =
        cli::PayloadDocument::new(Path::new("first.json"), br#"{"Item":[{"Value":"alpha"}]}"#)?;
    let second =
        cli::PayloadDocument::new(Path::new("second.json"), br#"{"Item":[{"Value":"beta"}]}"#)?;
    let extras = [
        cli::NamedPayloadInput::new("document", first)?,
        cli::NamedPayloadInput::new("document", second)?,
    ];

    let outcome = cli::run_project_value_payloads(
        &dynamic_source_project(),
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(source).with_extra_sources(&extras),
    )?;
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&outcome.artifacts[0].bytes)?,
        serde_json::json!({
            "Row": [
                {"Path": "first.json", "Value": "alpha"},
                {"Path": "second.json", "Value": "beta"}
            ]
        })
    );

    let missing = [cli::NamedPayloadInput::new("document", first)?];
    let error = cli::run_project_value_payloads(
        &dynamic_source_project(),
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(source).with_extra_sources(&missing),
    )
    .expect_err("an unsupplied dynamic document must fail");
    assert!(
        error
            .to_string()
            .contains("host did not supply payload source `document`")
    );
    Ok(())
}

#[test]
fn stateful_output_formats_reject_without_creating_files() -> anyhow::Result<()> {
    let mut project = dynamic_source_project();
    project.extra_sources.clear();
    project.target_path = Some("output.sqlite".into());
    project.root = Scope::default();
    project.source = SchemaNode::group("Source", Vec::new());
    project.target = SchemaNode::group("Target", Vec::new());
    project.target_options = FormatOptions::default();
    project.graph.nodes.clear();
    let source = cli::PayloadDocument::new(Path::new("source.json"), br#"{}"#)?;

    let error = cli::run_project_value_payloads(
        &project,
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(source),
    )
    .expect_err("SQLite output must require persistent state");
    let message = format!("{error:#}");
    assert!(
        message.contains("SQLite output requires a persistent database"),
        "{message}"
    );
    assert!(!Path::new("/virtual/output.sqlite").exists());
    Ok(())
}

#[test]
fn payload_routing_metadata_is_bounded() -> anyhow::Result<()> {
    let long_path = "x".repeat(cli::MAX_PAYLOAD_PATH_BYTES + 1);
    let error = cli::PayloadDocument::new(Path::new(&long_path), b"{}")
        .expect_err("oversized logical paths must fail");
    assert!(error.to_string().contains("path exceeds"));

    let document = cli::PayloadDocument::new(Path::new("input.json"), b"{}")?;
    let long_name = "x".repeat(cli::MAX_PAYLOAD_NAME_BYTES + 1);
    let error = cli::NamedPayloadInput::new(&long_name, document)
        .expect_err("oversized source names must fail");
    assert!(error.to_string().contains("source name exceeds"));
    Ok(())
}

#[test]
fn payload_and_filesystem_runners_produce_identical_csv() -> anyhow::Result<()> {
    let project = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar("Quantity", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Code", ScalarType::String),
                SchemaNode::scalar("Quantity", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: Some("output.csv".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["Row".into()]),
                        path: vec!["Code".into()],
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["Row".into()]),
                        path: vec!["Quantity".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::Source(vec!["Row".into()]),
            bindings: vec![
                Binding {
                    target_field: "Code".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Quantity".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    };
    let xml = b"<Source><Row><Code>A</Code><Quantity>2</Quantity></Row><Row><Code>B</Code><Quantity>5</Quantity></Row></Source>";
    let directory = TempDir::new()?;
    let project_path = directory.0.join("project.json");
    let input_path = directory.0.join("input.xml");
    let file_output = directory.0.join("file.csv");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    std::fs::write(&input_path, xml)?;
    cli::run_project(&project_path, &input_path, &file_output)?;

    let payload = cli::PayloadDocument::new(Path::new("input.xml"), xml)?;
    let payload_output = directory.0.join("payload.csv");
    let outcome = cli::run_project_value_payloads(
        &project,
        &project_path,
        &cli::PayloadRunOptions::new(payload).with_output_path(&payload_output),
    )?;

    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(outcome.artifacts[0].bytes, std::fs::read(file_output)?);
    assert_eq!(outcome.artifacts[0].records_written, 2);
    assert!(!payload_output.exists());
    Ok(())
}
