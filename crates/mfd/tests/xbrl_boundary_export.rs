use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope, XbrlBoundaryOptions};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xbrl_export_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn project() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["value".to_owned()],
            frame: None,
        },
    );
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        source_path: Some("source.xml".to_owned()),
        target_path: Some("target.xml".to_owned()),
        source_options: mapping::FormatOptions::default(),
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "value".to_owned(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn xbrl_boundary_preflight_preserves_existing_design() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let cases = [
        (
            "source",
            XbrlBoundaryOptions::external_source("source-taxonomy.xsd")?,
        ),
        (
            "target",
            XbrlBoundaryOptions::external_target("target-taxonomy.xsd", Some("table.sps"))?,
        ),
    ];

    for (side, boundary) in cases {
        let mut project = project();
        match side {
            "source" => project.source_options.xbrl = Some(boundary),
            "target" => project.target_options.xbrl = Some(boundary),
            _ => return Err(std::io::Error::other("unknown test side").into()),
        }
        let design = dir.0.join(format!("{side}.mfd"));
        std::fs::write(&design, "preserve")?;

        let result = mfd::export(&project, &design);
        assert!(matches!(
            result,
            Err(mfd::MfdError::Unsupported(message))
                if message.contains("XBRL boundary export is not supported")
        ));
        assert_eq!(std::fs::read_to_string(&design)?, "preserve");
        assert!(!dir.0.join(format!("{side}-source.xsd")).exists());
        assert!(!dir.0.join(format!("{side}-target.xsd")).exists());
    }
    Ok(())
}
