use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};
use mfd::{ExportCompatibility, ExportProfile};
use serde_json::Value;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_export_compatibility_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn project_path(&self, project: &Project) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join("project.json");
        std::fs::write(&path, serde_json::to_vec_pretty(project)?)?;
        Ok(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn extension_project() -> Project {
    let mut project = project();
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "isbn10_to_isbn13".into(),
            args: vec![0],
        },
    );
    project.root.bindings[0].node = 1;
    project
}

fn ferrule(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(args)
        .output()
        .unwrap()
}

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn check_is_read_only_and_json_report_is_versioned() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let project_path = temp.project_path(&project())?;
    let out = temp.0.join("not-created/mapping.mfd");
    let report = cli::preflight_mfd_export(&project_path, &out)?;
    assert_eq!(report.compatibility, ExportCompatibility::NativeMfd);
    assert!(!out.parent().unwrap().exists());

    let output = ferrule(&[
        "export-mfd",
        "--project",
        project_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--profile",
        "native-mfd",
        "--check",
        "--report-json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!out.parent().unwrap().exists());
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["command"], "export-mfd");
    assert_eq!(value["mode"], "check");
    assert_eq!(value["profile"], "native_mfd");
    assert_eq!(value["accepted"], true);
    assert_eq!(value["report"], serde_json::to_value(report)?);
    let repeated = ferrule(&[
        "export-mfd",
        "--project",
        project_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--profile",
        "native-mfd",
        "--check",
        "--report-json",
    ]);
    assert!(repeated.status.success());
    assert_eq!(output.stdout, repeated.stdout);
    assert!(!out.parent().unwrap().exists());
    Ok(())
}

#[test]
fn native_profile_rejects_extension_and_emits_owned_json_findings() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let project_path = temp.project_path(&extension_project())?;
    let out = temp.0.join("not-created/mapping.mfd");

    for check in [true, false] {
        let mut args = vec![
            "--diagnostics",
            "json",
            "export-mfd",
            "--project",
            project_path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--profile",
            "native-mfd",
            "--report-json",
        ];
        if check {
            args.push("--check");
        }
        let output = ferrule(&args);
        assert!(!output.status.success());
        assert!(!out.parent().unwrap().exists());
        let value: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(value["accepted"], false);
        assert_eq!(value["report"]["compatibility"], "ferrule_extensions");
        assert_eq!(value["report"]["issues"].as_array().unwrap().len(), 1);
        assert_eq!(value["report"]["issues"][0]["feature"], "ferrule_component");
        let diagnostics = json_lines(&output.stderr);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0]["severity"], "error");
        assert_eq!(diagnostics[0]["feature"], "ferrule_component");
        assert_eq!(diagnostics[0]["component"], "isbn10_to_isbn13");
        assert!(diagnostics[0]["component_uid"].is_number());
        assert!(diagnostics[0]["message"].is_string());
    }
    Ok(())
}

#[test]
fn default_export_keeps_legacy_behavior_and_native_profile_writes_clean_design()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let extension = temp.project_path(&extension_project())?;
    let out = temp.0.join("extension.mfd");
    let output = ferrule(&[
        "export-mfd",
        "--project",
        extension.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Ferrule's component library"));
    assert!(std::fs::read_to_string(&out)?.contains("library=\"ferrule\""));
    assert!(cli::export_mfd(&extension, &out)?.is_empty());
    let report = cli::export_mfd_with_profile(&extension, &out, ExportProfile::FerruleExtensions)?;
    assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);

    let clean = temp.project_path(&project())?;
    let native = temp.0.join("native.mfd");
    let output = ferrule(&[
        "export-mfd",
        "--project",
        clean.to_str().unwrap(),
        "--out",
        native.to_str().unwrap(),
        "--profile",
        "native-mfd",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(native.exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("native MFD"));
    Ok(())
}
