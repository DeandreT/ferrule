use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use cli::{PayloadDocument, PayloadRunOptions, TargetSelection};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

const INPUT: &[u8] = br#"{"Rows":[{"Primary":"primary/z.json","Named":"named/z.json","Value":"first"},{"Primary":"primary/a.json","Named":"named/a.json","Value":"second"}]}"#;
const FIRST: &[u8] = b"{\n  \"Value\": \"first\"\n}\n";
const SECOND: &[u8] = b"{\n  \"Value\": \"second\"\n}\n";
const SUMMARY: &[u8] = b"{\n  \"Value\": \"summary\"\n}\n";
const PAYLOAD_CHILD: &str = "FERRULE_DYNAMIC_OUTPUT_PATH_CHILD";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-output-paths-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    let options = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    let target = SchemaNode::group(
        "Result",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    ["Primary", "Named", "Value"]
                        .map(|name| SchemaNode::scalar(name, ScalarType::String))
                        .to_vec(),
                )
                .repeating(),
            ],
        ),
        target: target.clone(),
        source_path: Some("input.json".into()),
        target_path: None,
        source_options: options.clone(),
        target_options: options.clone(),
        extra_sources: Vec::new(),
        extra_targets: vec![
            NamedTarget {
                name: "details".into(),
                path: None,
                schema: target.clone(),
                options: options.clone(),
                root: dynamic_scope(1),
            },
            NamedTarget {
                name: "summary".into(),
                path: Some("summary.json".into()),
                schema: target,
                options,
                root: Scope {
                    bindings: vec![Binding {
                        target_field: "Value".into(),
                        node: 3,
                    }],
                    ..Scope::default()
                },
            },
        ],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: ["Primary", "Named", "Value"]
                .into_iter()
                .zip(0..)
                .map(|(name, id)| {
                    (
                        id,
                        Node::SourceField {
                            path: vec![name.into()],
                            frame: Some(vec!["Rows".into()]),
                        },
                    )
                })
                .chain([(
                    3,
                    Node::Const {
                        value: Value::String("summary".into()),
                    },
                )])
                .collect(),
        },
        root: dynamic_scope(0),
    }
}

fn dynamic_scope(output_path: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec!["Rows".into()],
            output_path,
        },
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 2,
        }],
        ..Scope::default()
    }
}

fn prepare(directory: &Path, project_path: &Path, project: &Project) -> anyhow::Result<()> {
    let path = directory.join(project_path);
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent)?;
    std::fs::write(&path, mapping::project_file::encode_pretty(project)?)?;
    std::fs::write(parent.join("input.json"), INPUT)?;
    Ok(())
}

fn project_paths(directory: &Path) -> [PathBuf; 4] {
    [
        "project.json".into(),
        "./project.json".into(),
        "nested/project.json".into(),
        directory.join("absolute/project.json"),
    ]
}

fn bases(directory: &Path) -> [Option<PathBuf>; 3] {
    [
        None,
        Some("chosen/./out".into()),
        Some(directory.join("absolute-output")),
    ]
}

fn selection(name: &str) -> Option<TargetSelection<'_>> {
    match name {
        "all" => None,
        "primary" => Some(TargetSelection::Primary),
        _ => Some(TargetSelection::Named(name)),
    }
}

fn expected(
    project_path: &Path,
    explicit: Option<&Path>,
    selected: &str,
) -> Vec<(&'static str, PathBuf, &'static [u8])> {
    let directory = match project_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let base = explicit.unwrap_or(directory);
    let mut artifacts = Vec::new();
    if selected == "all" || selected == "primary" {
        artifacts.extend([
            ("Result", base.join("primary/z.json"), FIRST),
            ("Result", base.join("primary/a.json"), SECOND),
        ]);
    }
    if selected == "all" || selected == "details" {
        // Without selection, an override belongs only to the primary target.
        let base = if selected == "all" { directory } else { base };
        artifacts.extend([
            ("details", base.join("named/z.json"), FIRST),
            ("details", base.join("named/a.json"), SECOND),
        ]);
    }
    if selected == "all" {
        artifacts.push(("summary", directory.join("summary.json"), SUMMARY));
    }
    artifacts
}

fn run_file(
    directory: &Path,
    project_path: &Path,
    explicit: Option<&Path>,
    selected: &str,
) -> anyhow::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command.current_dir(directory).arg("run").arg(project_path);
    if let Some(base) = explicit {
        command.arg("--output").arg(base);
    }
    if selected != "all" {
        command.arg("--target").arg(selected);
    }
    Ok(command.output()?)
}

fn check_file_paths(selected: &str) -> anyhow::Result<()> {
    for index in 0..4 {
        for base_index in 0..3 {
            let directory = TempDir::new()?;
            let project_path = &project_paths(&directory.0)[index];
            let explicit = &bases(&directory.0)[base_index];
            prepare(&directory.0, project_path, &project())?;
            let before = snapshot(&directory.0)?;
            let output = run_file(&directory.0, project_path, explicit.as_deref(), selected)?;
            assert!(
                output.status.success(),
                "{project_path:?}, {explicit:?}, {selected}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let artifacts = expected(project_path, explicit.as_deref(), selected);
            let report = artifacts
                .iter()
                .map(|(target, path, _)| {
                    format!("wrote 1 record(s) for {target} to {}\n", path.display())
                })
                .collect::<String>();
            assert!(String::from_utf8(output.stdout)?.ends_with(&report));
            for (_, path, bytes) in &artifacts {
                assert_eq!(std::fs::read(directory.0.join(path))?, *bytes);
            }
            // No unselected outputs, staging debris or changes to the input/project.
            let mut after = snapshot(&directory.0)?;
            for (_, path, _) in artifacts {
                let absolute = directory.0.join(path);
                let relative = absolute.strip_prefix(&directory.0)?;
                after.remove(relative);
                let mut parent = relative.parent();
                while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
                    if !before.contains_key(path) {
                        after.remove(path);
                    }
                    parent = path.parent();
                }
            }
            assert_eq!(after, before);
        }
    }
    Ok(())
}

#[test]
fn file_primary_dynamic_bases() -> anyhow::Result<()> {
    check_file_paths("primary")
}

#[test]
fn file_named_dynamic_bases() -> anyhow::Result<()> {
    check_file_paths("details")
}

#[test]
fn file_all_target_dynamic_bases() -> anyhow::Result<()> {
    check_file_paths("all")
}

fn check_payload_child(selected: &str) -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let output = Command::new(std::env::current_exe()?)
        .current_dir(&directory.0)
        .env(PAYLOAD_CHILD, selected)
        .args(["--exact", "payload_path_child", "--nocapture"])
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn payload_primary_dynamic_bases() -> anyhow::Result<()> {
    check_payload_child("primary")
}

#[test]
fn payload_named_dynamic_bases() -> anyhow::Result<()> {
    check_payload_child("details")
}

#[test]
fn payload_all_target_dynamic_bases() -> anyhow::Result<()> {
    check_payload_child("all")
}

// Each payload probe owns its process cwd; tests never mutate the test runner's cwd.
#[test]
fn payload_path_child() -> anyhow::Result<()> {
    let Ok(selected) = std::env::var(PAYLOAD_CHILD) else {
        return Ok(());
    };
    let directory = std::env::current_dir()?;
    let project = project();
    let input = PayloadDocument::new(Path::new("input.json"), INPUT)?;
    for project_path in project_paths(&directory) {
        prepare(&directory, &project_path, &project)?;
        for explicit in bases(&directory) {
            let mut options = PayloadRunOptions::new(input);
            if let Some(base) = &explicit {
                options = options.with_output_path(base);
            }
            if let Some(selection) = selection(&selected) {
                options = options.with_target(selection);
            }
            let before = snapshot(&directory)?;
            let value = cli::run_project_value_payloads(&project, &project_path, &options)?;
            let loaded = cli::run_project_payloads(&project_path, &options)?;
            assert_eq!(value, loaded);
            assert_eq!(value.records_written, 2);
            let artifacts = expected(&project_path, explicit.as_deref(), &selected);
            assert_eq!(value.artifacts.len(), artifacts.len());
            for (actual, (target, path, bytes)) in value.artifacts.iter().zip(artifacts) {
                assert_eq!(actual.target, target);
                // Compare the spelling as well, so explicit bases are not normalized.
                assert_eq!(actual.path.as_os_str(), path.as_os_str());
                assert_eq!(actual.records_written, 1);
                assert_eq!(actual.bytes, bytes);
            }
            assert_eq!(snapshot(&directory)?, before, "payload run wrote files");
        }
        let mut options = PayloadRunOptions::new(input).with_output_path(Path::new(""));
        if let Some(selection) = selection(&selected) {
            options = options.with_target(selection);
        }
        let before = snapshot(&directory)?;
        let error = cli::run_project_value_payloads(&project, &project_path, &options).unwrap_err();
        assert!(
            error.to_string().contains("path cannot be empty"),
            "{error:#}"
        );
        assert_eq!(snapshot(&directory)?, before);
    }
    Ok(())
}

#[test]
fn basename_refusals_do_not_publish_or_replace_any_outputs() -> anyhow::Result<()> {
    for input in [
        br#"{"Rows":[{"Primary":"primary/z.json","Named":"named/z.json","Value":"first"},{"Primary":"primary/a.json","Named":"../escape.json","Value":"second"}]}"#.as_slice(),
        br#"{"Rows":[{"Primary":"primary/z.json","Named":"named/z.json","Value":"first"},{"Primary":"primary/a.json","Named":"primary/z.json","Value":"second"}]}"#.as_slice(),
    ] {
        let directory = TempDir::new()?;
        prepare(&directory.0, Path::new("project.json"), &project())?;
        std::fs::write(directory.0.join("input.json"), input)?;
        std::fs::create_dir(directory.0.join("primary"))?;
        std::fs::write(directory.0.join("primary/z.json"), b"keep me")?;
        let before = snapshot(&directory.0)?;
        let output = run_file(&directory.0, Path::new("project.json"), None, "all")?;
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr)?;
        assert!(error.contains("cannot contain `..`") || error.contains("resolve to the same path"), "{error}");
        assert_eq!(snapshot(&directory.0)?, before);
        let payload = PayloadDocument::new(Path::new("input.json"), input)?;
        assert!(cli::run_project_value_payloads(
            &project(),
            &directory.0.join("project.json"),
            &PayloadRunOptions::new(payload),
        ).is_err());
        assert_eq!(snapshot(&directory.0)?, before);
    }
    Ok(())
}

#[test]
fn explicit_empty_file_bases_are_not_replaced_with_defaults() -> anyhow::Result<()> {
    for selected in ["primary", "details"] {
        let directory = TempDir::new()?;
        prepare(&directory.0, Path::new("project.json"), &project())?;
        let before = snapshot(&directory.0)?;
        let output = run_file(
            &directory.0,
            Path::new("project.json"),
            Some(Path::new("")),
            selected,
        )?;
        assert!(!output.status.success());
        assert_eq!(snapshot(&directory.0)?, before);
    }
    Ok(())
}

fn snapshot(directory: &Path) -> std::io::Result<BTreeMap<PathBuf, Option<Vec<u8>>>> {
    fn visit(
        root: &Path,
        directory: &Path,
        entries: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let is_directory = entry.file_type()?.is_dir();
            entries.insert(
                path.strip_prefix(root).unwrap().to_owned(),
                if is_directory {
                    None
                } else {
                    Some(std::fs::read(&path)?)
                },
            );
            if is_directory {
                visit(root, &path, entries)?;
            }
        }
        Ok(())
    }
    let mut entries = BTreeMap::new();
    visit(directory, directory, &mut entries)?;
    Ok(entries)
}
