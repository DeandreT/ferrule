use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cli::{
    PayloadArtifact, PayloadDocument, PayloadRunOptions, PayloadRunOutcome, TargetSelection,
};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

fn json_options() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    }
}

fn binding(node: mapping::NodeId) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}

fn project(primary_path: &str, named_path: &str) -> Project {
    let target = SchemaNode::group(
        "Result",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    Project {
        execution_contract: None,
        source: SchemaNode::group("Input", Vec::new()),
        target: target.clone(),
        source_path: None,
        target_path: Some(primary_path.into()),
        source_options: json_options(),
        target_options: json_options(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "named".into(),
            path: Some(named_path.into()),
            schema: target,
            options: json_options(),
            root: binding(1),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::String("primary".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("named".into()),
                    },
                ),
            ]),
        },
        root: binding(0),
    }
}

fn options() -> PayloadRunOptions<'static> {
    PayloadRunOptions::new(PayloadDocument::new(Path::new("input.json"), b"{}").unwrap())
}

fn assert_outcome(outcome: PayloadRunOutcome, expected: &[(&str, &Path)]) {
    let artifacts = expected
        .iter()
        .map(|(target, path)| PayloadArtifact {
            target: (*target).into(),
            records_written: 1,
            path: path.to_path_buf(),
            bytes: match *target {
                "Result" => b"{\n  \"Value\": \"primary\"\n}\n".to_vec(),
                "named" => b"{\n  \"Value\": \"named\"\n}\n".to_vec(),
                _ => unreachable!("the fixture has only primary and named targets"),
            },
        })
        .collect::<Vec<_>>();
    // Path equality alone hides interior `.` and repeated separators.
    for (actual, expected) in outcome.artifacts.iter().zip(&artifacts) {
        assert_eq!(actual.path.as_os_str(), expected.path.as_os_str());
    }
    assert_eq!(
        outcome,
        PayloadRunOutcome {
            records_written: 1,
            artifacts,
        }
    );
}

#[test]
fn leading_parent_paths_remain_distinct_for_all_targets() -> anyhow::Result<()> {
    for (project_path, primary, named, directory) in [
        ("project.json", "../result.json", "result.json", "."),
        ("./project.json", "../../result.json", "../result.json", "."),
        ("../project.json", "../result.json", "result.json", ".."),
        (
            "../../project.json",
            "../result.json",
            "result.json",
            "../..",
        ),
        ("project.json", ".././../result.json", "../result.json", "."),
        (
            "project.json",
            "dir/../../../result.json",
            "../result.json",
            ".",
        ),
        (
            "project.json",
            "../result.json",
            "result.json/child.json",
            ".",
        ),
        (
            "project.json",
            "../../result.json/child.json",
            "../result.json",
            ".",
        ),
        (
            "nested/project.json",
            "../result.json",
            "result.json",
            "nested",
        ),
        (
            "nested/deep/project.json",
            "../../../result.json",
            "../../result.json",
            "nested/deep",
        ),
    ] {
        let outcome = cli::run_project_value_payloads(
            &project(primary, named),
            Path::new(project_path),
            &options(),
        )?;
        assert_outcome(
            outcome,
            &[
                ("Result", &Path::new(directory).join(primary)),
                ("named", &Path::new(directory).join(named)),
            ],
        );
    }
    Ok(())
}

#[test]
fn explicit_primary_override_preserves_relative_parent_identity() -> anyhow::Result<()> {
    for (override_path, named) in [
        ("../result.json", "result.json"),
        ("../../result.json", "../result.json"),
        ("../dir/../../result.json", "../result.json"),
    ] {
        let outcome = cli::run_project_value_payloads(
            &project("unused.json", named),
            Path::new("project.json"),
            &options().with_output_path(Path::new(override_path)),
        )?;
        assert_outcome(
            outcome,
            &[
                ("Result", Path::new(override_path)),
                ("named", &Path::new(".").join(named)),
            ],
        );
    }
    Ok(())
}

#[test]
fn selected_targets_preserve_stored_and_explicit_parent_path_spellings() -> anyhow::Result<()> {
    for (project_path, directory) in [
        ("project.json", "."),
        ("./project.json", "."),
        ("nested/project.json", "nested"),
    ] {
        let project = project("../../result.json", "../result.json");
        for (selection, target, stored) in [
            (
                TargetSelection::Primary,
                "Result",
                Path::new(directory).join("../../result.json"),
            ),
            (
                TargetSelection::Named("named"),
                "named",
                Path::new(directory).join("../result.json"),
            ),
        ] {
            assert_outcome(
                cli::run_project_value_payloads(
                    &project,
                    Path::new(project_path),
                    &options().with_target(selection),
                )?,
                &[(target, &stored)],
            );
            let override_path = Path::new("../explicit/./dir/../../result.json");
            assert_outcome(
                cli::run_project_value_payloads(
                    &project,
                    Path::new(project_path),
                    &options()
                        .with_target(selection)
                        .with_output_path(override_path),
                )?,
                &[(target, override_path)],
            );
        }
    }
    Ok(())
}

fn assert_collision(project: &Project, project_path: &Path, expected: &str) {
    let error = cli::run_project_value_payloads(project, project_path, &options())
        .expect_err("a collision must discard the complete payload artifact set");
    assert_eq!(error.to_string(), expected);
    assert_eq!(error.chain().count(), 1);
}

#[test]
fn genuine_parent_aliases_discard_all_artifacts_but_allow_selected_targets() -> anyhow::Result<()> {
    for (primary, named) in [
        ("../x/../result.json", "../result.json"),
        ("../../x/../result.json", "../../result.json"),
        ("dir/../result.json", "result.json"),
        ("../x/../../result.json", "../../result.json"),
        ("../x/./y/../../result.json", "../result.json"),
    ] {
        let project = project(primary, named);
        let named_path = Path::new(".").join(named);
        assert_collision(
            &project,
            Path::new("project.json"),
            &format!(
                "multiple output artifacts use path `{}`",
                named_path.display()
            ),
        );
        for (selection, target, path) in [
            (
                TargetSelection::Primary,
                "Result",
                Path::new(".").join(primary),
            ),
            (TargetSelection::Named("named"), "named", named_path),
        ] {
            assert_outcome(
                cli::run_project_value_payloads(
                    &project,
                    Path::new("project.json"),
                    &options().with_target(selection),
                )?,
                &[(target, &path)],
            );
        }
    }
    let project = project("unused.json", "../result.json");
    let error = cli::run_project_value_payloads(
        &project,
        Path::new("project.json"),
        &options().with_output_path(Path::new("../x/../result.json")),
    )
    .expect_err("an explicit override must not bypass collision checks");
    assert_eq!(
        error.to_string(),
        format!(
            "multiple output artifacts use path `{}`",
            Path::new(".").join("../result.json").display()
        )
    );
    Ok(())
}

#[test]
fn leading_parent_file_directory_overlaps_are_still_rejected() {
    for (primary, named) in [
        ("../result.json", "../result.json/child.json"),
        ("../../x/../result.json/child.json", "../../result.json"),
        ("../x/../../result.json", "../../result.json/child.json"),
    ] {
        let primary_path = Path::new(".").join(primary);
        let named_path = Path::new(".").join(named);
        assert_collision(
            &project(primary, named),
            Path::new("project.json"),
            &format!(
                "output artifact paths `{}` and `{}` overlap as file and directory",
                primary_path.display(),
                named_path.display()
            ),
        );
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_payload_parent_paths_{}_{}",
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

#[test]
fn file_loaded_payload_runs_preserve_paths_and_never_publish_outputs() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let nested = directory.0.join("nested");
    std::fs::create_dir(&nested)?;
    let project_path = nested.join("project.json");
    let primary_path = nested.join("../result.json");
    let named_path = nested.join("result.json");
    let override_path = nested.join("../unused/result.json");
    let sentinel_path = directory.0.join("result.json");
    std::fs::write(&sentinel_path, b"existing output must remain unchanged")?;
    let mut project = project("../result.json", "result.json");
    let saved = serde_json::to_vec_pretty(&project)?;
    std::fs::write(&project_path, &saved)?;
    assert_outcome(
        cli::run_project_value_payloads(&project, &project_path, &options())?,
        &[("Result", &primary_path), ("named", &named_path)],
    );
    assert_outcome(
        cli::run_project_payloads(&project_path, &options())?,
        &[("Result", &primary_path), ("named", &named_path)],
    );
    for (selection, target, stored) in [
        (TargetSelection::Primary, "Result", &primary_path),
        (TargetSelection::Named("named"), "named", &named_path),
    ] {
        assert_outcome(
            cli::run_project_payloads(&project_path, &options().with_target(selection))?,
            &[(target, stored)],
        );
        assert_outcome(
            cli::run_project_payloads(
                &project_path,
                &options()
                    .with_target(selection)
                    .with_output_path(&override_path),
            )?,
            &[(target, &override_path)],
        );
    }
    project.extra_targets[0].path = Some("../missing/../result.json".into());
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let error = cli::run_project_payloads(&project_path, &options())
        .expect_err("a genuine alias returns no primary or named artifact");
    assert_eq!(
        error.to_string(),
        format!(
            "multiple output artifacts use path `{}`",
            nested.join("../missing/../result.json").display()
        )
    );
    assert_eq!(
        std::fs::read(&sentinel_path)?,
        b"existing output must remain unchanged"
    );
    assert!(!named_path.exists());
    assert!(!override_path.exists());
    assert!(!directory.0.join("missing").exists());
    assert_eq!(std::fs::read_dir(&directory.0)?.count(), 2);
    assert_eq!(std::fs::read_dir(&nested)?.count(), 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn rooted_parents_still_clamp_without_changing_returned_paths() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let primary = format!(
        "/../../{}",
        directory.0.strip_prefix("/")?.join("result.json").display()
    );
    let named = directory.0.join("named.json");
    let project_path = directory.0.join("project.json");
    assert_outcome(
        cli::run_project_value_payloads(
            &project(&primary, named.to_str().unwrap()),
            &project_path,
            &options(),
        )?,
        &[("Result", Path::new(&primary)), ("named", &named)],
    );
    let alias = directory.0.join("result.json");
    assert_collision(
        &project(&primary, alias.to_str().unwrap()),
        &project_path,
        &format!("multiple output artifacts use path `{}`", alias.display()),
    );
    assert_eq!(std::fs::read_dir(&directory.0)?.count(), 0);
    Ok(())
}

#[cfg(windows)]
#[test]
fn drive_relative_prefixes_keep_unmatched_parents() -> anyhow::Result<()> {
    let project = project("unused.json", r"C:result.json");
    // A rootless drive prefix is not a root and cannot be cancelled by `..`.
    assert_outcome(
        cli::run_project_value_payloads(
            &project,
            Path::new("project.json"),
            &options().with_output_path(Path::new(r"C:..\result.json")),
        )?,
        &[
            ("Result", Path::new(r"C:..\result.json")),
            ("named", Path::new(r"C:result.json")),
        ],
    );
    Ok(())
}

#[test]
fn dynamic_parent_traversal_is_still_refused() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    for path in ["../result.json", "dir/../result.json"] {
        let mut project = project("unused.json", "named.json");
        project.target_path = None;
        project.graph.nodes.insert(
            2,
            Node::Const {
                value: Value::String(path.into()),
            },
        );
        project.root.iteration = ScopeIteration::DynamicDocuments {
            source: Vec::new(),
            output_path: 2,
        };
        let error = cli::run_project_value_payloads(
            &project,
            &directory.0.join("project.json"),
            &options(),
        )
        .expect_err("static parent admission must not admit dynamic traversal");
        assert!(
            format!("{error:#}").contains("dynamic output path cannot contain `..` segments"),
            "{error:#}"
        );
    }
    assert_eq!(std::fs::read_dir(&directory.0)?.count(), 0);
    Ok(())
}
