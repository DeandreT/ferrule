use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use format_csv::CsvFormatError;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

struct TestDirectory {
    path: PathBuf,
    completed: bool,
}

impl TestDirectory {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-csv-row-release-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            completed: false,
        })
    }

    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.completed && std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() != Ok("1") {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn row_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Number", ScalarType::Int),
        ],
    )
}

fn rows(number: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(Vec::new()),
        bindings: vec![
            Binding {
                target_field: "Text".into(),
                node: 0,
            },
            Binding {
                target_field: "Number".into(),
                node: number,
            },
        ],
        ..Scope::default()
    }
}

fn project() -> Project {
    Project {
        source: row_schema("Input"),
        target: row_schema("Output"),
        source_path: Some("input.csv".into()),
        target_path: Some("primary.csv".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions {
            csv_utf8_bom: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "report".into(),
            path: Some("named.csv".into()),
            schema: row_schema("Report"),
            options: FormatOptions {
                delimiter: Some(';'),
                csv_quote: Some('\''),
                ..FormatOptions::default()
            },
            root: rows(1),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Number".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: rows(1),
    }
}

fn save(project: &Project, directory: &TestDirectory) -> anyhow::Result<PathBuf> {
    let issues = engine::validate(project);
    assert!(issues.is_empty(), "{issues:?}");
    let path = directory.path.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(project)?)?;
    Ok(path)
}

#[test]
fn file_and_payload_csv_keep_complete_primary_named_literals_and_order() -> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let project = project();
    let saved = save(&project, &directory)?;
    let input = "Text,Number\n\"a,b;é\"\"q'\",1\n\"line\n🙂\",2\n,3\n".as_bytes();
    let primary = "\u{feff}Text,Number\n\"a,b;é\"\"q'\",1\n\"line\n🙂\",2\n,3\n".as_bytes();
    let named = "Text;Number\n'a,b;é\"q''';1\n'line\n🙂';2\n;3\n".as_bytes();
    let input_path = directory.path.join("input.csv");
    let primary_path = directory.path.join("primary.csv");
    let named_path = directory.path.join("named.csv");
    std::fs::write(&input_path, input)?;
    std::fs::write(directory.path.join("expected-primary.csv"), primary)?;
    std::fs::write(directory.path.join("expected-named.csv"), named)?;
    assert_eq!(cli::run_project(&saved, &input_path, &primary_path)?, 3);
    assert_eq!(std::fs::read(&primary_path)?, primary);
    assert_eq!(std::fs::read(&named_path)?, named);

    let payload_path = directory.path.join("payload.csv");
    let outcome = cli::run_project_value_payloads(
        &project,
        &saved,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(Path::new("input.csv"), input)?)
            .with_output_path(&payload_path),
    )?;
    for (index, artifact) in outcome.artifacts.iter().enumerate() {
        std::fs::write(
            directory.path.join(format!("actual-payload-{index}.csv")),
            &artifact.bytes,
        )?;
    }
    assert_eq!(outcome.records_written, 3);
    assert_eq!(outcome.artifacts.len(), 2);
    assert_eq!(outcome.artifacts[0].target, "Output");
    assert_eq!(outcome.artifacts[0].path, payload_path);
    assert_eq!(outcome.artifacts[0].records_written, 3);
    assert_eq!(outcome.artifacts[0].bytes, primary);
    assert_eq!(outcome.artifacts[1].target, "report");
    assert_eq!(outcome.artifacts[1].path, named_path);
    assert_eq!(outcome.artifacts[1].records_written, 3);
    assert_eq!(outcome.artifacts[1].bytes, named);
    assert!(!payload_path.exists());
    assert_eq!(std::fs::read(&primary_path)?, primary);
    assert_eq!(std::fs::read(&named_path)?, named);
    directory.complete();
    Ok(())
}

fn assert_late_number_error(error: &anyhow::Error) {
    let original = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<CsvFormatError>())
        .expect("native CSV error remains available through its error chain");
    assert!(
        matches!(original, CsvFormatError::ValueType {
        row: 1, field, expected: ScalarType::Int, got: "string"
    } if field == "Number"),
        "{error:#}"
    );
    assert_eq!(
        original.to_string(),
        "row 1: column `Number` expected Int, got string"
    );
}

#[test]
fn late_named_csv_type_failure_preserves_all_existing_files_and_no_payload_result()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut project = project();
    project.graph.nodes.extend([
        (
            2,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            3,
            Node::Call {
                function: "equal".into(),
                args: vec![1, 2],
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String("not an integer".into()),
            },
        ),
        (
            5,
            Node::If {
                condition: 3,
                then: 4,
                else_: 1,
            },
        ),
    ]);
    project.extra_targets[0].root = rows(5);
    let saved = save(&project, &directory)?;
    let input = b"Text,Number\nfirst,1\nsecond,2\nthird,3\n";
    let input_path = directory.path.join("input.csv");
    let primary_path = directory.path.join("primary.csv");
    let named_path = directory.path.join("named.csv");
    std::fs::write(&input_path, input)?;
    std::fs::write(&primary_path, b"primary sentinel")?;
    std::fs::write(&named_path, b"named sentinel")?;
    let file_error = cli::run_project(&saved, &input_path, &primary_path).unwrap_err();
    std::fs::write(
        directory.path.join("file.error.txt"),
        format!("{file_error:?}\n{file_error:#}\n"),
    )?;
    assert_late_number_error(&file_error);
    assert_eq!(std::fs::read(&primary_path)?, b"primary sentinel");
    assert_eq!(std::fs::read(&named_path)?, b"named sentinel");

    let payload_path = directory.path.join("absent-payload.csv");
    let payload_error = cli::run_project_value_payloads(
        &project,
        &saved,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(Path::new("input.csv"), input)?)
            .with_output_path(&payload_path),
    )
    .expect_err("late named serialization failure discards the complete payload outcome");
    std::fs::write(
        directory.path.join("payload.error.txt"),
        format!("{payload_error:?}\n{payload_error:#}\n"),
    )?;
    assert_late_number_error(&payload_error);
    assert!(!payload_path.exists());
    assert_eq!(std::fs::read(&primary_path)?, b"primary sentinel");
    assert_eq!(std::fs::read(&named_path)?, b"named sentinel");
    directory.complete();
    Ok(())
}

#[test]
fn ordinary_file_output_above_64_mib_succeeds_while_payload_keeps_its_limit() -> anyhow::Result<()>
{
    const INPUT_TEXT_BYTES: usize = 32 * 1024 * 1024;
    const OUTPUT_BYTES: usize = 2 * INPUT_TEXT_BYTES + 1;
    let mut directory = TestDirectory::new()?;
    let schema = SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]);
    let project = Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("input.csv".into()),
        target_path: Some("ordinary.csv".into()),
        source_options: FormatOptions {
            has_header_row: Some(false),
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            has_header_row: Some(false),
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 0],
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    let saved = save(&project, &directory)?;
    let input_path = directory.path.join("input.csv");
    {
        let mut input = std::fs::File::create(&input_path)?;
        let block = [b'x'; 64 * 1024];
        for _ in 0..INPUT_TEXT_BYTES / block.len() {
            input.write_all(&block)?;
        }
        input.write_all(b"\n")?;
    }
    let output_path = directory.path.join("ordinary.csv");
    assert_eq!(cli::run_project(&saved, &input_path, &output_path)?, 1);
    assert_eq!(std::fs::metadata(&output_path)?.len(), OUTPUT_BYTES as u64);
    {
        let mut output = std::fs::File::open(&output_path)?;
        let mut buffer = [0; 8192];
        let mut observed = 0;
        loop {
            let count = output.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            observed += count;
            assert!(observed <= OUTPUT_BYTES);
            let text = if observed == OUTPUT_BYTES {
                assert_eq!(buffer[count - 1], b'\n');
                &buffer[..count - 1]
            } else {
                &buffer[..count]
            };
            assert!(text.iter().all(|byte| *byte == b'x'));
        }
        assert_eq!(observed, OUTPUT_BYTES);
    }
    let input_bytes = std::fs::read(&input_path)?;
    assert!(input_bytes.len() < cli::MAX_PAYLOAD_DOCUMENT_BYTES);
    let payload_path = directory.path.join("absent-payload.csv");
    let error = cli::run_project_value_payloads(
        &project,
        &saved,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(
            Path::new("input.csv"),
            &input_bytes,
        )?)
        .with_output_path(&payload_path),
    )
    .expect_err("payload output still refuses its existing per-document byte excess");
    std::fs::write(
        directory.path.join("payload-limit.error.txt"),
        format!("{error:?}\n{error:#}\n"),
    )?;
    assert_eq!(
        error.to_string(),
        format!(
            "output artifact `{}` exceeds the 64 MiB per-document limit",
            payload_path.display(),
        )
    );
    assert!(
        error
            .chain()
            .all(|cause| cause.downcast_ref::<CsvFormatError>().is_none())
    );
    assert!(!payload_path.exists());
    assert_eq!(std::fs::metadata(&output_path)?.len(), OUTPUT_BYTES as u64);
    directory.complete();
    Ok(())
}
