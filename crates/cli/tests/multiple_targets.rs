use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope};

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

#[test]
fn explicit_primary_output_does_not_replace_stored_extra_target_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("project.json");
    let input_path = dir.0.join("input.xml");
    let override_path = dir.0.join("override.xml");
    let stored_primary = dir.0.join("stored-primary.xml");
    let stored_secondary = dir.0.join("stored-secondary.xml");
    let project = Project {
        source: document("Source"),
        target: document("First"),
        source_path: None,
        target_path: Some("stored-primary.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "second".into(),
            path: Some("stored-secondary.xml".into()),
            schema: document("Second"),
            options: Default::default(),
            root: output_scope(),
        }],
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
    };
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    std::fs::write(&input_path, "<Source><Value>shared</Value></Source>")?;

    let outcome =
        cli::run_project_with_paths(&project_path, Some(&input_path), Some(&override_path))?;
    assert_eq!(outcome.output_path, override_path);
    assert_eq!(outcome.extra_outputs.len(), 1);
    assert_eq!(outcome.extra_outputs[0].name, "second");
    assert_eq!(outcome.extra_outputs[0].path, stored_secondary);
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
