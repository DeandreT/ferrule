use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope, XbrlBoundaryOptions};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_xbrl_{}_{}",
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
        source_path: None,
        target_path: None,
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

fn write_project(path: &Path, project: &Project) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_vec_pretty(project)?)?;
    Ok(())
}

fn error_message(result: anyhow::Result<usize>) -> Result<String, Box<dyn Error>> {
    match result {
        Ok(_) => Err(std::io::Error::other("XBRL run unexpectedly succeeded").into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn external_xbrl_source_rejects_before_input_extension_dispatch() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut project = project();
    project.source_options.xbrl = Some(XbrlBoundaryOptions::external_source("taxonomy.xsd")?);
    let project_path = dir.0.join("mapping.json");
    let output_path = dir.0.join("output.xml");
    write_project(&project_path, &project)?;

    let message = error_message(cli::run_project(
        &project_path,
        &dir.0.join("missing.unsupported"),
        &output_path,
    ))?;
    assert!(message.contains("XBRL source input is not executable"));
    assert!(!output_path.exists());
    Ok(())
}

#[test]
fn external_xbrl_target_rejects_without_replacing_output() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut project = project();
    project.target_options.xbrl = Some(XbrlBoundaryOptions::external_target(
        "taxonomy.xsd",
        Some("table.sps"),
    )?);
    let project_path = dir.0.join("mapping.json");
    let output_path = dir.0.join("output.xml");
    write_project(&project_path, &project)?;
    std::fs::write(&output_path, "preserve")?;

    let message = error_message(cli::run_project(
        &project_path,
        &dir.0.join("missing.xml"),
        &output_path,
    ))?;
    assert!(message.contains("XBRL target output is not executable"));
    assert_eq!(std::fs::read_to_string(&output_path)?, "preserve");
    Ok(())
}
