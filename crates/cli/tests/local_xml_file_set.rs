use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-local-xml-file-set-{}-{}",
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

fn source_row(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)]).repeating()
}

fn target_row(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("FileName", ScalarType::String),
        ],
    )
    .repeating()
}

#[test]
fn runs_a_sorted_bounded_local_xml_file_set() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let project_path = directory.0.join("mapping.json");
    std::fs::write(
        directory.0.join("records-b.xml"),
        "<Source><Item><Value>b</Value></Item></Source>",
    )?;
    std::fs::write(
        directory.0.join("records-a.xml"),
        "<Source><Item><Value>a</Value></Item></Source>",
    )?;
    let project = Project {
        source: SchemaNode::group("Source", vec![source_row("Item")]),
        target: SchemaNode::group("Target", vec![target_row("Item")]),
        source_path: Some("records-*.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (1, Node::SourceDocumentPath),
                (
                    2,
                    Node::Call {
                        function: "remove_folder".into(),
                        args: vec![1],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Item".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Value".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "FileName".into(),
                        node: 2,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let outcome = cli::run_project_with_paths(&project_path, None, None)?;

    assert_eq!(outcome.input_path, directory.0.join("records-*.xml"));
    let output = format_xml::read(&directory.0.join("output.xml"), &project.target)?;
    let items = output
        .field("Item")
        .and_then(ir::Instance::as_repeated)
        .ok_or("missing output items")?;
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].field("Value").and_then(ir::Instance::as_scalar),
        Some(&Value::String("a".into()))
    );
    assert_eq!(
        items[1].field("Value").and_then(ir::Instance::as_scalar),
        Some(&Value::String("b".into()))
    );
    assert_eq!(
        items[0].field("FileName").and_then(ir::Instance::as_scalar),
        Some(&Value::String("records-a.xml".into()))
    );
    assert_eq!(
        items[1].field("FileName").and_then(ir::Instance::as_scalar),
        Some(&Value::String("records-b.xml".into()))
    );
    Ok(())
}
