use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xml_boundary_identity_{}_{}",
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

#[test]
fn pathless_xml_boundaries_roundtrip_without_format_inference() -> Result<(), Box<dyn Error>> {
    let source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    let project = Project {
        source: source.clone(),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
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
                target_field: "Result".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    };
    let temp = TempDir::new()?;
    let design = temp.0.join("pathless.mfd");

    assert!(mfd::export(&project, &design)?.is_empty());
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, source);
    assert_eq!(imported.project.target, target);
    assert!(imported.project.source_options.xml_document);
    assert!(imported.project.target_options.xml_document);
    assert!(imported.project.source_path.is_none());
    assert!(imported.project.target_path.is_none());

    let input = format_xml::from_str("<Source><Value>retained</Value></Source>", &project.source)?;
    assert_eq!(
        engine::run(&imported.project, &input)?,
        engine::run(&project, &input)?
    );
    Ok(())
}
