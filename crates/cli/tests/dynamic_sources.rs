use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_dynamic_sources_{}_{}",
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
fn loads_relative_dynamic_xml_sources_per_primary_item() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("mapping.json");
    let source_path = dir.0.join("files.xml");
    let output_path = dir.0.join("output.xml");
    let project = Project {
        source: SchemaNode::group(
            "Files",
            vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Path", ScalarType::String),
                        SchemaNode::scalar("Value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: SchemaNode::group(
                "Document",
                vec![
                    SchemaNode::group(
                        "Item",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            ),
            options: Default::default(),
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: vec!["File".into()],
            }),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["File".into()]),
                        path: Vec::new(),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["document".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Path".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Value".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    std::fs::write(
        &source_path,
        "<Files><File>first.xml</File><File>second.xml</File></Files>",
    )?;
    std::fs::write(
        dir.0.join("first.xml"),
        "<Document><Item><Value>alpha</Value></Item></Document>",
    )?;
    std::fs::write(
        dir.0.join("second.xml"),
        "<Document><Item><Value>beta</Value></Item></Document>",
    )?;

    cli::run_project(&project_path, &source_path, &output_path)?;
    assert_eq!(
        std::fs::read_to_string(output_path)?,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Path>first.xml</Path>\n    <Value>alpha</Value>\n  </Row>\n  <Row>\n    <Path>second.xml</Path>\n    <Value>beta</Value>\n  </Row>\n</Output>"
    );
    Ok(())
}
