//! `.mfd` export refuses JSON tuple frames before writing anything, because
//! the generated JSON Schema siblings cannot represent them yet (#1227).

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_json::JsonFormatError;
use ir::{ItemCountRange, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode};
use mapping::{Graph, NamedSource, Project, Scope};
use mfd::{ExportProfile, MfdError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-json-tuple-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn is_empty(&self) -> std::io::Result<bool> {
        Ok(std::fs::read_dir(&self.0)?.next().is_none())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A closed `[string, integer]` tuple of length 0..2.
fn frame() -> SchemaNode {
    SchemaNode::group(
        "Point",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    )
    .with_json_tuple(JsonTupleLayout {
        length: ItemCountRange::new(0, Some(2)),
        tail: JsonTupleTail::Closed,
    })
    .unwrap()
}

fn document(name: &str, framed: bool) -> SchemaNode {
    let mut point = frame();
    if !framed {
        point.json_tuple = None;
    }
    SchemaNode::group(
        name,
        vec![SchemaNode::scalar("Head", ScalarType::String), point],
    )
}

fn project(source: SchemaNode, target: SchemaNode) -> Project {
    Project {
        source,
        target,
        source_path: Some("source.json".into()),
        target_path: Some("target.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn refused(result: Result<impl std::fmt::Debug, MfdError>) -> bool {
    matches!(
        result,
        Err(MfdError::JsonSchemaExport(
            JsonFormatError::UnsupportedJsonTupleFrameExport { .. }
        ))
    )
}

#[test]
fn tuple_frames_are_refused_before_any_artifact_is_written() -> Result<(), Box<dyn Error>> {
    let mut cases = vec![
        project(document("Source", true), document("Target", false)),
        project(document("Source", false), document("Target", true)),
    ];
    let mut named = project(document("Source", false), document("Target", false));
    named.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "lookup.json".into(),
        schema: document("Lookup", true),
        options: Default::default(),
        dynamic_path: None,
    });
    cases.push(named);

    for (index, project) in cases.iter().enumerate() {
        let directory = TempDir::new()?;
        let design = directory.0.join("mapping.mfd");
        assert!(refused(mfd::export(project, &design)), "case {index}");
        assert!(
            refused(mfd::export_with_profile(
                project,
                &design,
                ExportProfile::NativeMfd
            )),
            "case {index}"
        );
        assert!(
            refused(
                mfd::export_package(project, "mapping.mfd", ExportProfile::default())
                    .map(|package| package.report)
            ),
            "case {index}"
        );
        assert!(directory.is_empty()?, "case {index} wrote artifacts");
    }

    // The same designs without frames export their JSON Schema siblings.
    let control = project(document("Source", false), document("Target", false));
    let directory = TempDir::new()?;
    mfd::export(&control, &directory.0.join("mapping.mfd"))?;
    assert!(directory.0.join("mapping.mfd").exists());
    Ok(())
}
