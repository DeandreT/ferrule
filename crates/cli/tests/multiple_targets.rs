use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedSource, NamedTarget, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_multiple_targets_{}_{}",
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

fn document(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
}

fn output_scope() -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    }
}

fn project(extra_path: &str) -> Project {
    Project {
        source: document("Source"),
        target: document("First"),
        source_path: None,
        target_path: Some("stored-primary.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "second".into(),
            path: Some(extra_path.into()),
            schema: document("Second"),
            options: Default::default(),
            root: output_scope(),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]
            .into(),
        },
        root: output_scope(),
    }
}

fn colliding_project() -> Project {
    let mut project = project("shared.xml");
    project.target_path = Some("shared.xml".into());
    project
}

fn project_with_unavailable_unselected_source() -> Project {
    Project {
        source: document("Source"),
        target: document("Primary"),
        source_path: None,
        target_path: Some("selected.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "unselected_data".into(),
            path: "unavailable.xml".into(),
            schema: document("Lookup"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "unselected".into(),
            path: Some("unselected.xml".into()),
            schema: document("Unselected"),
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("selected".into()),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["unselected_data".into(), "Value".into()],
                        frame: None,
                    },
                ),
            ]
            .into(),
        },
        root: output_scope(),
    }
}

#[test]
fn explicit_primary_output_does_not_replace_stored_extra_target_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("project.json");
    let input_path = dir.0.join("input.xml");
    let override_path = dir.0.join("override.xml");
    let stored_primary = dir.0.join("stored-primary.xml");
    let stored_secondary = dir.0.join("stored-secondary.xml");
    let project = project("stored-secondary.xml");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    std::fs::write(&input_path, "<Source><Value>shared</Value></Source>")?;

    let outcome =
        cli::run_project_with_paths(&project_path, Some(&input_path), Some(&override_path))?;
    assert_eq!(outcome.output_path, override_path);
    assert_eq!(outcome.extra_outputs.len(), 1);
    assert_eq!(outcome.extra_outputs[0].name, "second");
    assert_eq!(outcome.extra_outputs[0].path, stored_secondary);
    assert_eq!(
        outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.name.as_str(), artifact.path.as_path()))
            .collect::<Vec<_>>(),
        vec![
            ("First", override_path.as_path()),
            ("second", stored_secondary.as_path()),
        ]
    );
    assert!(!stored_primary.exists());
    let expected = Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::String("shared".into())),
    )]);
    assert_eq!(
        format_xml::read(&outcome.output_path, &document("First"))?,
        expected
    );
    assert_eq!(
        format_xml::read(&outcome.extra_outputs[0].path, &document("Second"))?,
        expected
    );
    Ok(())
}

#[test]
fn late_extra_render_failure_leaves_existing_primary_untouched()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("project.json");
    let input_path = dir.0.join("input.xml");
    let primary_path = dir.0.join("primary.xml");
    let late_path = dir.0.join("late.pdf");
    std::fs::write(
        &project_path,
        serde_json::to_vec_pretty(&project("late.pdf"))?,
    )?;
    std::fs::write(&input_path, "<Source><Value>new</Value></Source>")?;
    std::fs::write(&primary_path, "keep primary")?;

    let error = cli::run_project_with_paths(&project_path, Some(&input_path), Some(&primary_path))
        .expect_err("the unsupported late target must fail the batch");

    let message = format!("{error:#}");
    assert!(
        message.contains("writing extra target `second`"),
        "{message}"
    );
    assert!(message.contains("PDF output is not supported"), "{message}");
    assert_eq!(std::fs::read_to_string(primary_path)?, "keep primary");
    assert!(!late_path.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_parent_aliases_are_preflighted_as_one_output_path()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new()?;
    let project_path = dir.0.join("project.json");
    let input_path = dir.0.join("input.xml");
    let real = dir.0.join("real");
    let alias = dir.0.join("alias");
    std::fs::create_dir(&real)?;
    symlink(&real, &alias)?;
    std::fs::write(
        &project_path,
        serde_json::to_vec_pretty(&project("real/output.xml"))?,
    )?;
    std::fs::write(&input_path, "<Source><Value>value</Value></Source>")?;

    let error = cli::run_project_with_paths(
        &project_path,
        Some(&input_path),
        Some(&alias.join("output.xml")),
    )
    .expect_err("symlink aliases must collide before staging");

    assert!(
        format!("{error:#}").contains("resolve to the same path"),
        "{error:#}"
    );
    assert!(!real.join("output.xml").exists());
    Ok(())
}

#[test]
fn selected_target_avoids_colliding_unselected_destination()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("project.json");
    let input_path = dir.0.join("input.xml");
    let shared_path = dir.0.join("shared.xml");
    std::fs::write(
        &project_path,
        serde_json::to_vec_pretty(&colliding_project())?,
    )?;
    std::fs::write(&input_path, "<Source><Value>selected</Value></Source>")?;
    std::fs::write(&shared_path, "keep")?;

    let error = cli::run_project_with_paths(&project_path, Some(&input_path), None)
        .expect_err("all-target mode must reject duplicate destinations");
    assert!(
        format!("{error:#}").contains("resolve to the same path"),
        "{error:#}"
    );
    assert_eq!(std::fs::read_to_string(&shared_path)?, "keep");

    let outcome = cli::run_project_with_options(
        &project_path,
        &cli::RunOptions::new()
            .with_input_path(&input_path)
            .with_target(cli::TargetSelection::Named("second")),
    )?;
    assert_eq!(outcome.output_path, shared_path);
    assert!(outcome.primary_outputs.is_empty());
    assert_eq!(outcome.extra_outputs.len(), 1);
    assert_eq!(outcome.extra_outputs[0].name, "second");
    assert_eq!(outcome.artifacts, outcome.extra_outputs);
    assert_eq!(
        format_xml::read(&shared_path, &document("Second"))?,
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("selected".into())),
        )])
    );
    Ok(())
}

#[test]
fn run_subcommand_accepts_primary_target_selector() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("project.json");
    let input_path = dir.0.join("input.xml");
    let shared_path = dir.0.join("shared.xml");
    std::fs::write(
        &project_path,
        serde_json::to_vec_pretty(&colliding_project())?,
    )?;
    std::fs::write(&input_path, "<Source><Value>primary</Value></Source>")?;

    let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .arg("run")
        .arg("--project")
        .arg(&project_path)
        .arg("--input")
        .arg(&input_path)
        .arg("--target")
        .arg("primary")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        format_xml::read(&shared_path, &document("First"))?,
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("primary".into())),
        )])
    );
    Ok(())
}

#[test]
fn selected_filesystem_target_does_not_load_unselected_static_source()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("project.json");
    let input_path = dir.0.join("input.xml");
    let selected_path = dir.0.join("selected.xml");
    std::fs::write(
        &project_path,
        serde_json::to_vec_pretty(&project_with_unavailable_unselected_source())?,
    )?;
    std::fs::write(&input_path, "<Source><Value>input</Value></Source>")?;

    let selected = cli::run_project_with_options(
        &project_path,
        &cli::RunOptions::new()
            .with_input_path(&input_path)
            .with_target(cli::TargetSelection::Primary),
    )?;
    assert_eq!(selected.output_path, selected_path);
    assert_eq!(
        format_xml::read(&selected_path, &document("Primary"))?,
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("selected".into())),
        )])
    );

    let error = cli::run_project_with_paths(&project_path, Some(&input_path), None)
        .expect_err("all-target execution must still load every static source");
    assert!(
        format!("{error:#}").contains("loading extra source `unselected_data`"),
        "{error:#}"
    );
    Ok(())
}
