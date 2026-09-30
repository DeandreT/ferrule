use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{FormatOptions, Graph, Node, Pipeline, PipelineInput, PipelineStage, Project, Scope};
use mfd::{MAX_MFD_DESIGN_BYTES, MfdError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_design_limits_{}_{}",
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

fn constant_project(value: String) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::Const {
                    value: Value::String(value),
                },
            )]),
        },
        root: Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn assert_export_limit(error: MfdError) {
    assert!(
        matches!(error, MfdError::Unsupported(ref reason) if reason.contains("64 MiB byte limit")),
        "{error}"
    );
}

#[test]
fn oversized_sparse_design_rejects_before_xml_parsing_in_both_import_paths() {
    let dir = TempDir::new();
    let path = dir.0.join("oversized.mfd");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_MFD_DESIGN_BYTES as u64 + 1)
        .unwrap();
    for result in [
        mfd::import(&path).map(|_| ()),
        mfd::import_pipeline(&path).map(|_| ()),
    ] {
        let error = result.err().unwrap();
        assert!(
            matches!(error, MfdError::UnsupportedImport(ref reason) if reason.contains("64 MiB byte limit")),
            "{error}"
        );
    }
}

#[test]
fn xml_escaping_counts_toward_export_limit_before_existing_artifacts_change() {
    let dir = TempDir::new();
    let path = dir.0.join("mapping.mfd");
    let project = constant_project("\"".repeat(MAX_MFD_DESIGN_BYTES / 6 + 1));
    std::fs::write(&path, "existing design").unwrap();
    let sibling = dir.0.join("mapping-source.xsd");
    std::fs::write(&sibling, "existing schema").unwrap();
    assert_export_limit(mfd::preflight_export(&project, &path).unwrap_err());
    assert_export_limit(mfd::export(&project, &path).unwrap_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing design");
    assert_eq!(
        std::fs::read_to_string(&sibling).unwrap(),
        "existing schema"
    );
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 2);
}

#[test]
fn connected_pipeline_obeys_one_combined_design_budget() {
    let dir = TempDir::new();
    let mut first = constant_project("\"".repeat(MAX_MFD_DESIGN_BYTES / 10));
    first.target_path = None;
    let mut second = first.clone();
    second.source = first.target.clone();
    second.source_path = None;
    second.target.name = "Final".into();
    second.target_path = Some("output.xml".into());
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![
            PipelineStage {
                id: "first".into(),
                mapping_path: None,
                project: first,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "second".into(),
                mapping_path: None,
                project: second,
                source: PipelineInput::StageTarget {
                    stage: "first".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    // Each independently valid stage fits; only the connected design exceeds
    // the budget. Rejection must happen before the output directory exists.
    for (index, stage) in pipeline.stages.iter().enumerate() {
        mfd::preflight_export(&stage.project, &dir.0.join(format!("stage-{index}.mfd"))).unwrap();
    }
    let output = dir.0.join("uncreated").join("pipeline.mfd");
    assert_export_limit(mfd::export_pipeline(&pipeline, &output).unwrap_err());
    assert!(!output.parent().unwrap().exists());
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}
