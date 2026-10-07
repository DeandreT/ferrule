use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

struct Directory {
    path: PathBuf,
    passed: Cell<bool>,
}

impl Directory {
    fn new() -> anyhow::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-payload-deferred-csv-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            passed: Cell::new(false),
        })
    }

    fn capture(
        &self,
        name: &str,
        result: &anyhow::Result<cli::PayloadRunOutcome>,
    ) -> anyhow::Result<()> {
        let directory = self.path.join(name);
        std::fs::create_dir(&directory)?;
        match result {
            Ok(outcome) => {
                for (index, artifact) in outcome.artifacts.iter().enumerate() {
                    std::fs::write(
                        directory.join(format!("artifact-{index}.bin")),
                        &artifact.bytes,
                    )?;
                }
                std::fs::write(
                    directory.join("outcome.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "records": outcome.records_written,
                        "artifacts": outcome.artifacts.iter().map(|artifact| serde_json::json!({
                            "target": artifact.target, "path": artifact.path,
                            "bytes": artifact.bytes.len(), "records": artifact.records_written,
                        })).collect::<Vec<_>>(),
                    }))?,
                )?;
            }
            Err(error) => {
                std::fs::write(directory.join("error-debug.txt"), format!("{error:?}"))?;
                std::fs::write(
                    directory.join("error-chain.json"),
                    serde_json::to_vec_pretty(
                        &error.chain().map(ToString::to_string).collect::<Vec<_>>(),
                    )?,
                )?;
            }
        }
        Ok(())
    }

    fn fixture(&self, name: &str, project: &Project, input: &[u8]) -> anyhow::Result<()> {
        std::fs::write(
            self.path.join(format!("{name}-project.json")),
            mapping::project_file::encode_pretty(project)?,
        )?;
        std::fs::write(self.path.join(format!("{name}-input.json")), input)?;
        let issues = engine::validate(project);
        std::fs::write(
            self.path.join(format!("{name}-validation.txt")),
            format!("{issues:?}"),
        )?;
        assert!(issues.is_empty(), "{issues:?}");
        Ok(())
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if !self.passed.get()
            || std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!("retained payload CSV originals: {}", self.path.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn csv_options() -> FormatOptions {
    FormatOptions {
        has_header_row: Some(false),
        ..FormatOptions::default()
    }
}

fn rows(node: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Value", ScalarType::String),
                        SchemaNode::scalar("File", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        source_path: None,
        target_path: Some("first.csv".into()),
        source_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        target_options: csv_options(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["Rows".into()]),
                        path: vec!["Value".into()],
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("x".into()),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        frame: Some(vec!["Rows".into()]),
                        path: vec!["File".into()],
                    },
                ),
            ]),
        },
        root: rows(0),
    }
}

fn execute(
    project: &Project,
    directory: &Directory,
    input: &[u8],
    target: Option<engine::TargetSelection<'_>>,
) -> anyhow::Result<cli::PayloadRunOutcome> {
    let document = cli::PayloadDocument::new(Path::new("input.json"), input)?;
    let options = cli::PayloadRunOptions::new(document);
    let options = if let Some(target) = target {
        options.with_target(target)
    } else {
        options
    };
    cli::run_project_value_payloads(project, &directory.path.join("project.json"), &options)
}

#[test]
fn payload_csv_literals_and_dynamic_named_order_return_without_publication() -> anyhow::Result<()> {
    let directory = Directory::new()?;
    let mut project = project();
    project.target_options = FormatOptions {
        delimiter: Some(';'),
        csv_quote: Some('\''),
        csv_utf8_bom: true,
        has_header_row: Some(true),
        ..FormatOptions::default()
    };
    project.extra_targets = vec![
        NamedTarget {
            name: "members".into(),
            path: None,
            schema: project.target.clone(),
            options: FormatOptions {
                json_document: true,
                ..FormatOptions::default()
            },
            root: Scope {
                iteration: ScopeIteration::DynamicDocuments {
                    source: vec!["Rows".into()],
                    output_path: 2,
                },
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        },
        NamedTarget {
            name: "plain".into(),
            path: Some("plain.csv".into()),
            schema: project.target.clone(),
            options: FormatOptions::default(),
            root: rows(0),
        },
    ];
    let input =
        r#"{"Rows":[{"Value":"O'Neil;\r\n😀","File":"b.json"},{"Value":"","File":"a.json"}]}"#
            .as_bytes();
    directory.fixture("literal", &project, input)?;
    std::fs::write(directory.path.join("first.csv"), b"existing output")?;
    let result = execute(&project, &directory, input, None);
    directory.capture("literal", &result)?;
    let outcome = result?;
    assert_eq!(outcome.records_written, 2);
    assert_eq!(
        outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.target.as_str(), artifact.records_written))
            .collect::<Vec<_>>(),
        [("Row", 2), ("members", 1), ("members", 1), ("plain", 2)]
    );
    assert_eq!(
        outcome
            .artifacts
            .iter()
            .map(|artifact| artifact.path.file_name().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        ["first.csv", "b.json", "a.json", "plain.csv"]
    );
    let expected = [
        "\u{feff}Value\n'O''Neil;\r\n😀'\n''\n",
        "{\n  \"Value\": \"O'Neil;\\r\\n😀\"\n}\n",
        "{\n  \"Value\": \"\"\n}\n",
        "Value\n\"O'Neil;\r\n😀\"\n\"\"\n",
    ];
    for (artifact, expected) in outcome.artifacts.iter().zip(expected) {
        assert_eq!(artifact.bytes, expected.as_bytes());
    }
    assert_eq!(
        std::fs::read(directory.path.join("first.csv"))?,
        b"existing output"
    );
    for path in ["a.json", "b.json", "plain.csv"] {
        assert!(!directory.path.join(path).exists());
    }
    directory.passed.set(true);
    Ok(())
}

#[test]
fn selected_named_csv_does_not_evaluate_an_engine_valid_unselected_format_error()
-> anyhow::Result<()> {
    let directory = Directory::new()?;
    let mut project = project();
    project.target_options.csv_quote_disabled = true;
    project.extra_targets.push(NamedTarget {
        name: "plain".into(),
        path: Some("plain.csv".into()),
        schema: project.target.clone(),
        options: csv_options(),
        root: rows(0),
    });
    let input = br#"{"Rows":[{"Value":"bad,field","File":"unused.json"}]}"#;
    directory.fixture("selection", &project, input)?;
    let all = execute(&project, &directory, input, None);
    directory.capture("all", &all)?;
    let error = all.unwrap_err();
    assert!(
        matches!(error.downcast_ref::<cli::CsvFormatError>(), Some(cli::CsvFormatError::UnquotedFieldBoundary { row: 0, field }) if field == "Value")
    );
    let selected = execute(
        &project,
        &directory,
        input,
        Some(engine::TargetSelection::Named("plain")),
    );
    directory.capture("selected", &selected)?;
    let outcome = selected?;
    assert_eq!(outcome.records_written, 1);
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(outcome.artifacts[0].target, "plain");
    assert_eq!(outcome.artifacts[0].bytes, b"\"bad,field\"\n");
    assert!(!directory.path.join("plain.csv").exists());
    directory.passed.set(true);
    Ok(())
}

#[test]
fn genuine_public_csv_64_mib_and_plus_one_use_below_cap_json_input() -> anyhow::Result<()> {
    let directory = Directory::new()?;
    let mut project = project();
    project.graph.nodes.insert(
        3,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 0, 1],
        },
    );
    project.root = rows(3);
    let characters = 33_554_431;
    let mut input = Vec::with_capacity(characters + 64);
    input.extend_from_slice(br#"{"Rows":[{"Value":""#);
    input.resize(input.len() + characters, b'x');
    input.extend_from_slice(br#"","File":"unused.json"}]}"#);
    directory.fixture("maximum", &project, &input)?;
    assert!(input.len() < cli::MAX_PAYLOAD_DOCUMENT_BYTES);
    let accepted = execute(&project, &directory, &input, None);
    directory.capture("maximum", &accepted)?;
    let outcome = accepted?;
    assert_eq!(outcome.records_written, 1);
    assert_eq!(outcome.artifacts.len(), 1);
    let bytes = &outcome.artifacts[0].bytes;
    assert_eq!(bytes.len(), 67_108_864);
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert!(bytes[..bytes.len() - 1].iter().all(|byte| *byte == b'x'));
    drop(outcome);
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("xx".into()),
        },
    );
    directory.fixture("excess", &project, &input)?;
    let excess = execute(&project, &directory, &input, None);
    directory.capture("excess", &excess)?;
    let error = excess.unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "output artifact `{}` exceeds the 64 MiB per-document limit",
            directory.path.join("first.csv").display(),
        )
    );
    assert_eq!(error.chain().count(), 1);
    assert!(
        error
            .downcast_ref::<format_csv::CsvBoundedError>()
            .is_none()
    );
    assert!(!directory.path.join("first.csv").exists());
    directory.passed.set(true);
    Ok(())
}
