use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xml_namespace_{label}_{}_{}",
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

fn qualified(node: SchemaNode, namespace: &str) -> Result<SchemaNode, Box<dyn Error>> {
    node.xml_qualified(namespace)
        .ok_or_else(|| "test namespace must be non-empty".into())
}

fn project_with_source(source: SchemaNode) -> Project {
    Project {
        source,
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Result", ScalarType::String)],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["ForeignValue".into()],
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
    }
}

#[test]
fn cross_namespace_schema_artifacts_export_and_reimport() -> Result<(), Box<dyn Error>> {
    let source = qualified(
        SchemaNode::group(
            "Input",
            vec![qualified(
                SchemaNode::scalar("ForeignValue", ScalarType::String),
                "urn:ferrule:foreign",
            )?],
        ),
        "urn:ferrule:document",
    )?;
    let project = project_with_source(source.clone());
    let temp = TempDir::new("roundtrip")?;
    let design = temp.0.join("namespaces.mfd");

    assert!(mfd::export(&project, &design)?.is_empty());
    let source_xsd = std::fs::read_to_string(temp.0.join("namespaces-source.xsd"))?;
    let dependency = std::fs::read_to_string(temp.0.join("namespaces-source-ns1.xsd"))?;
    assert!(source_xsd.contains(r#"schemaLocation="namespaces-source-ns1.xsd""#));
    assert!(source_xsd.contains(r#"ref="ns1:ForeignValue""#));
    assert!(dependency.contains(r#"targetNamespace="urn:ferrule:foreign""#));

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, source);
    let input = format_xml::from_str(
        r#"<Input xmlns="urn:ferrule:document"><f:ForeignValue xmlns:f="urn:ferrule:foreign">exact</f:ForeignValue></Input>"#,
        &project.source,
    )?;
    assert_eq!(
        engine::run(&imported.project, &input)?,
        engine::run(&project, &input)?
    );
    Ok(())
}

#[test]
fn namespace_dependency_failure_publishes_no_artifacts() -> Result<(), Box<dyn Error>> {
    let source = qualified(
        SchemaNode::group(
            "Input",
            vec![qualified(
                SchemaNode::group(
                    "Foreign",
                    vec![qualified(
                        SchemaNode::scalar("Input", ScalarType::String),
                        "urn:ferrule:document",
                    )?],
                ),
                "urn:ferrule:foreign",
            )?],
        ),
        "urn:ferrule:document",
    )?;
    let mut project = project_with_source(source);
    project.graph.nodes.clear();
    project.root.bindings.clear();
    let temp = TempDir::new("atomic")?;
    let design = temp.0.join("rejected.mfd");

    assert!(mfd::export(&project, &design).is_err());
    assert!(!design.exists());
    assert!(!temp.0.join("rejected-source.xsd").exists());
    assert!(!temp.0.join("rejected-source-ns1.xsd").exists());
    assert!(!temp.0.join("rejected-target.xsd").exists());
    Ok(())
}
