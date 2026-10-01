use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cli::{PayloadDocument, PayloadRunOptions};
use engine::ExecutionPurpose;
use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, CsvTextRepairCause as Cause, CsvTextRepairDependency, FormatOptions, Graph, Node,
    Project, Scope, ScopeIteration, TabularBoundaryKind,
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_csv_repair_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn schema() -> SchemaNode {
    SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
}
fn options() -> FormatOptions {
    FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        has_header_row: Some(false),
        ..Default::default()
    }
}
fn project() -> Project {
    Project {
        source: schema(),
        target: schema(),
        source_path: Some("input.csv".into()),
        target_path: Some("output.csv".into()),
        source_options: options(),
        target_options: options(),
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
            iteration: ScopeIteration::Source(Vec::new()),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    }
}
fn assert_dependency(error: &anyhow::Error, cause: Cause) {
    assert!(error.chain().any(|error| matches!(error.downcast_ref::<format_csv::CsvFormatError>(), Some(format_csv::CsvFormatError::RepairDependency(dependency)) if *dependency == CsvTextRepairDependency::new(cause))), "{error:#}");
}

#[test]
fn new_settings_repairs_survive_file_and_payload_identity_overrides() {
    let temp = TempDir::new();
    let input = temp.0.join("input.csv");
    std::fs::write(&input, b"data\n").unwrap();
    let output = temp.0.join("sentinel.csv");
    for cause in [
        Cause::EmptyPolicy,
        Cause::Separator,
        Cause::Quote,
        Cause::HeaderRow,
        Cause::TypedEmptyCells,
    ] {
        for source in [true, false] {
            if !source && cause == Cause::TypedEmptyCells {
                continue;
            }
            let mut project = project();
            let boundary = if source {
                &mut project.source_options
            } else {
                &mut project.target_options
            };
            boundary.csv_text_repair_dependency = Some(CsvTextRepairDependency::new(cause));
            let saved = temp.0.join(format!("{cause:?}-{source}.json"));
            std::fs::write(
                &saved,
                mapping::project_file::encode_pretty(&project).unwrap(),
            )
            .unwrap();
            let project =
                mapping::project_file::decode_str(&std::fs::read_to_string(&saved).unwrap())
                    .unwrap();
            std::fs::write(&output, b"sentinel").unwrap();
            let actual_input = if source {
                temp.0.join("missing-renamed.xml")
            } else {
                input.clone()
            };
            assert_dependency(
                &cli::run_project_with_paths(&saved, Some(&actual_input), Some(&output))
                    .unwrap_err(),
                cause,
            );
            for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                let primary = PayloadDocument::new(
                    if source {
                        Path::new("renamed.json")
                    } else {
                        Path::new("input.csv")
                    },
                    b"data\n",
                )
                .unwrap();
                let options = PayloadRunOptions::new(primary)
                    .with_output_path(&output)
                    .with_execution_purpose(purpose);
                assert_dependency(
                    &cli::run_project_value_payloads(&project, &saved, &options).unwrap_err(),
                    cause,
                );
            }
            assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
        }
    }
}
