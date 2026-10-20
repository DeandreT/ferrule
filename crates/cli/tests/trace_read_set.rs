//! `run --trace-json` refuses destinations that name the design or a consumed
//! local input, on both the filesystem and stdout routes.

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::symlink as directory_symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_dir as directory_symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, Node, Project, Scope,
    ScopeIteration,
};

type TestResult = Result<(), Box<dyn Error>>;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_trace_read_set_{label}_{}_{}",
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

fn ferrule(dir: &Path, args: &[&str], stdin: &[u8]) -> std::io::Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(stdin)?;
    }
    child.wait_with_output()
}

fn utf8(path: &Path) -> Result<&str, Box<dyn Error>> {
    Ok(path.to_str().ok_or("non-UTF-8 test path")?)
}

fn value_schema(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
}

/// Copies `Value` from `input.json` to `output.json`, optionally declaring a
/// static named `lookup.json` source that every unselected run loads.
fn scalar_project(lookup: bool) -> Project {
    let scope = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    Project {
        source: value_schema("Input"),
        target: value_schema("Output"),
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: lookup
            .then(|| NamedSource {
                name: "lookup".into(),
                path: "lookup.json".into(),
                schema: value_schema("Lookup"),
                options: FormatOptions::default(),
                dynamic_path: None,
            })
            .into_iter()
            .collect(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: scope,
    }
}

/// Lists dynamically loaded documents in `files.xml` and copies their values.
fn dynamic_project() -> Project {
    let mut project = scalar_project(false);
    project.source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    project.target = SchemaNode::group("Output", vec![value_schema("Row").repeating()]);
    project.source_path = Some("files.xml".into());
    project.target_path = Some("output.xml".into());
    project.extra_sources = vec![NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group("Document", vec![value_schema("Item").repeating()]),
        options: FormatOptions::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 0,
            iteration: vec!["File".into()],
        }),
    }];
    project.graph = Graph {
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
    };
    project.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

/// Reads every `records-*.xml` member of a local XML file set.
fn file_set_project() -> Project {
    let xml = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    let mut project = scalar_project(false);
    project.source = SchemaNode::group("Source", vec![value_schema("Item").repeating()]);
    project.target = SchemaNode::group("Target", vec![value_schema("Item").repeating()]);
    project.source_path = Some("records-*.xml".into());
    project.target_path = Some("output.xml".into());
    project.source_options = FormatOptions {
        local_xml_file_set: true,
        ..xml.clone()
    };
    project.target_options = xml;
    project.graph = Graph {
        nodes: BTreeMap::from([(
            0,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: Some(vec!["Item".into()]),
            },
        )]),
    };
    project.root = Scope {
        children: vec![Scope {
            target_field: "Item".into(),
            iteration: ScopeIteration::Source(vec!["Item".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

/// Files whose complete original bytes a refusal must preserve.
struct Fixture {
    dir: TempDir,
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl Fixture {
    fn new(label: &str, project: &Project, files: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let dir = TempDir::new(label)?;
        let mut written = vec![(
            dir.0.join("project.json"),
            serde_json::to_vec_pretty(project)?,
        )];
        for (name, text) in files {
            written.push((dir.0.join(name), text.as_bytes().to_vec()));
        }
        for (path, bytes) in &written {
            std::fs::write(path, bytes)?;
        }
        Ok(Self {
            dir,
            files: written,
        })
    }

    fn assert_refused(&self, output: &Output, published: &[&str]) -> TestResult {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains("would replace consumed input"), "{stderr}");
        assert!(output.stdout.is_empty(), "{stderr}");
        for (path, bytes) in &self.files {
            assert_eq!(&std::fs::read(path)?, bytes, "{}", path.display());
        }
        for name in published {
            assert!(!self.dir.0.join(name).exists(), "{name} was published");
        }
        for entry in std::fs::read_dir(&self.dir.0)? {
            let name = entry?.file_name();
            assert!(!name.to_string_lossy().contains("trace-stage"), "{name:?}");
        }
        Ok(())
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn trace_cannot_replace_the_design_through_path_aliases() -> TestResult {
    let fixture = Fixture::new(
        "design",
        &scalar_project(false),
        &[("input.json", r#"{"Value":"a"}"#)],
    )?;
    let dir = &fixture.dir.0;
    std::fs::create_dir(dir.join("sub"))?;
    directory_symlink(dir, dir.join("link"))?;
    let absolute = dir.join("project.json");
    for alias in [
        "project.json",
        "./sub/../project.json",
        "link/project.json",
        utf8(&absolute)?,
    ] {
        let output = ferrule(
            dir,
            &["run", "--project", "project.json", "--trace-json", alias],
            b"",
        )?;
        fixture.assert_refused(&output, &["output.json"])?;
    }
    // The design may also be named absolutely while the trace is relative.
    let output = ferrule(
        dir,
        &[
            "run",
            "--project",
            utf8(&absolute)?,
            "--trace-json",
            "project.json",
        ],
        b"",
    )?;
    fixture.assert_refused(&output, &["output.json"])
}

#[test]
fn hard_linked_trace_destination_leaves_the_design_intact() -> TestResult {
    let fixture = Fixture::new(
        "hard_link",
        &scalar_project(false),
        &[("input.json", r#"{"Value":"a"}"#)],
    )?;
    let dir = &fixture.dir.0;
    std::fs::hard_link(dir.join("project.json"), dir.join("linked.jsonl"))?;

    let output = ferrule(
        dir,
        &[
            "run",
            "--project",
            "project.json",
            "--trace-json",
            "linked.jsonl",
        ],
        b"",
    )?;

    // Publication renames over the link's directory entry, so the design inode
    // keeps its bytes while the link now names the completed trace.
    assert_success(&output);
    let (design, bytes) = &fixture.files[0];
    assert_eq!(&std::fs::read(design)?, bytes);
    let trace = std::fs::read_to_string(dir.join("linked.jsonl"))?;
    assert!(!trace.is_empty());
    assert!(
        trace
            .lines()
            .all(|line| line.contains("\"schema_version\""))
    );
    Ok(())
}

#[test]
fn trace_cannot_replace_primary_or_named_inputs() -> TestResult {
    let fixture = Fixture::new(
        "inputs",
        &scalar_project(true),
        &[
            ("input.json", r#"{"Value":"a"}"#),
            ("lookup.json", r#"{"Value":"b"}"#),
        ],
    )?;
    let dir = &fixture.dir.0;
    let absolute_input = dir.join("input.json");
    for args in [
        vec![
            "run",
            "--project",
            "project.json",
            "--trace-json",
            "input.json",
        ],
        vec![
            "run",
            "--project",
            "project.json",
            "--input",
            "./input.json",
            "--trace-json",
            utf8(&absolute_input)?,
        ],
        vec![
            "run",
            "--project",
            "project.json",
            "--trace-json",
            "lookup.json",
        ],
    ] {
        fixture.assert_refused(&ferrule(dir, &args, b"")?, &["output.json"])?;
    }

    // An unrelated earlier trace is still replaced.
    std::fs::write(dir.join("old.jsonl"), "old trace\n")?;
    let output = ferrule(
        dir,
        &[
            "run",
            "--project",
            "project.json",
            "--trace-json",
            "old.jsonl",
        ],
        b"",
    )?;
    assert_success(&output);
    assert_ne!(
        std::fs::read_to_string(dir.join("old.jsonl"))?,
        "old trace\n"
    );
    Ok(())
}

#[test]
fn trace_cannot_replace_a_dynamically_loaded_document() -> TestResult {
    let fixture = Fixture::new(
        "dynamic",
        &dynamic_project(),
        &[
            ("files.xml", "<Files><File>first.xml</File></Files>"),
            (
                "first.xml",
                "<Document><Item><Value>alpha</Value></Item></Document>",
            ),
        ],
    )?;
    let dir = &fixture.dir.0;
    let output = ferrule(
        dir,
        &[
            "run",
            "--project",
            "project.json",
            "--trace-json",
            "first.xml",
        ],
        b"",
    )?;
    fixture.assert_refused(&output, &["output.xml"])?;

    let output = ferrule(
        dir,
        &[
            "run",
            "--project",
            "project.json",
            "--trace-json",
            "run.jsonl",
        ],
        b"",
    )?;
    assert_success(&output);
    assert!(std::fs::read_to_string(dir.join("output.xml"))?.contains("alpha"));
    Ok(())
}

#[test]
fn trace_cannot_replace_a_file_set_member() -> TestResult {
    let fixture = Fixture::new(
        "file_set",
        &file_set_project(),
        &[
            (
                "records-a.xml",
                "<Source><Item><Value>a</Value></Item></Source>",
            ),
            (
                "records-b.xml",
                "<Source><Item><Value>b</Value></Item></Source>",
            ),
        ],
    )?;
    let dir = &fixture.dir.0;
    directory_symlink(dir, dir.join("link"))?;
    for alias in ["records-b.xml", "link/records-a.xml"] {
        let output = ferrule(
            dir,
            &["run", "--project", "project.json", "--trace-json", alias],
            b"",
        )?;
        fixture.assert_refused(&output, &["output.xml"])?;
    }
    Ok(())
}

#[test]
fn stdout_trace_cannot_replace_the_design_or_consumed_inputs() -> TestResult {
    let fixture = Fixture::new(
        "stdout",
        &scalar_project(true),
        &[
            ("input.json", r#"{"Value":"a"}"#),
            ("lookup.json", r#"{"Value":"b"}"#),
        ],
    )?;
    let dir = &fixture.dir.0;
    let absolute_lookup = dir.join("lookup.json");
    for (args, stdin) in [
        (
            vec![
                "run",
                "project.json",
                "-",
                "-",
                "--trace-json",
                "project.json",
            ],
            br#"{"Value":"s"}"#.as_slice(),
        ),
        (
            vec![
                "run",
                "project.json",
                "input.json",
                "-",
                "--trace-json",
                "./input.json",
            ],
            b"".as_slice(),
        ),
        (
            vec![
                "run",
                "project.json",
                "-",
                "-",
                "--trace-json",
                utf8(&absolute_lookup)?,
            ],
            br#"{"Value":"s"}"#.as_slice(),
        ),
    ] {
        fixture.assert_refused(&ferrule(dir, &args, stdin)?, &[])?;
    }

    // Stdin is not a filesystem read, so its configured identity remains a
    // legitimate trace destination, as does an unrelated earlier trace.
    for trace in ["input.json", "old.jsonl"] {
        let output = ferrule(
            dir,
            &["run", "project.json", "-", "-", "--trace-json", trace],
            br#"{"Value":"s"}"#,
        )?;
        assert_success(&output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("\"s\""));
    }
    Ok(())
}
