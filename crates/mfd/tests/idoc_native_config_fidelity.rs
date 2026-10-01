use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, EdiBoundaryKind, FormatOptions, Graph, IdocNativeCode, IdocNativeConfig,
    IdocNativeField, IdocNativeFieldType, IdocNativeGroup, IdocNativeNode, IdocNativeSegment,
    IdocNativeStatus, Node, Project, Scope,
};
use mfd::MfdError;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-idoc-config-fidelity-{}-{}",
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

fn nonzero(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn project_with_leading_label() -> Project {
    let field = IdocNativeField::new(
        "CODE",
        "  Leading field",
        IdocNativeFieldType::Character,
        nonzero(3),
        nonzero(1),
        nonzero(64),
        nonzero(66),
        vec![IdocNativeCode::new("A", "Active").unwrap()],
    )
    .unwrap();
    let segment = IdocNativeSegment::new(
        "E2ITEM",
        "E1ITEM",
        false,
        nonzero(2),
        IdocNativeStatus::Optional,
        0,
        1,
        vec![field],
    )
    .unwrap();
    let group = IdocNativeGroup::new(
        nonzero(1),
        nonzero(1),
        IdocNativeStatus::Mandatory,
        1,
        1,
        vec![IdocNativeNode::Segment(segment)],
    )
    .unwrap();
    let descriptor = IdocNativeConfig::new("TEST01", vec![IdocNativeNode::Group(group)]).unwrap();
    let (source, layout) = descriptor.project().unwrap();
    Project {
        source,
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("input.idoc".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            edi_kind: Some(EdiBoundaryKind::Idoc),
            idoc: Some(layout),
            idoc_native_config: Some(descriptor),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["SG1".into(), "E2ITEM".into(), "CODE".into()],
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

#[test]
fn unrepresentable_idoc_label_rejects_export_before_creating_artifacts() {
    let temp = TempDir::new();
    let output = temp.0.join("new-parent/mapping.mfd");
    let project = project_with_leading_label();
    for error in [
        mfd::preflight_export(&project, &output).unwrap_err(),
        mfd::export(&project, &output).unwrap_err(),
    ] {
        assert!(matches!(error, MfdError::Unsupported(_)), "{error}");
        assert!(error.to_string().contains("field CODE / TEXT"), "{error}");
    }
    assert!(!output.parent().unwrap().exists());
}
