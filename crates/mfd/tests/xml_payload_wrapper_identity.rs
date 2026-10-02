use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

type TestResult = Result<(), Box<dyn Error>>;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xml_entry_identity_{}_{}",
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

fn qualified(node: SchemaNode, namespace: &str) -> SchemaNode {
    node.xml_qualified(namespace)
        .expect("test namespace is nonempty")
}
fn project(source: SchemaNode, target: SchemaNode, bindings: &[(&[&str], &str)]) -> Project {
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: bindings
                .iter()
                .enumerate()
                .map(|(index, (path, _))| {
                    (
                        index as u32,
                        Node::SourceField {
                            path: path.iter().map(|value| (*value).to_string()).collect(),
                            frame: None,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        },
        root: Scope {
            bindings: bindings
                .iter()
                .enumerate()
                .map(|(index, (_, field))| Binding {
                    target_field: (*field).to_string(),
                    node: index as u32,
                })
                .collect(),
            ..Default::default()
        },
    }
}

fn check_payloads(namespace: Option<Option<&str>>) -> TestResult {
    let name = |node: SchemaNode| match namespace {
        None => node,
        Some(None) => node.xml_unqualified(),
        Some(Some(namespace)) => qualified(node, namespace),
    };
    for (source_root, target_root) in [
        ("document", "FileInstance"),
        ("FileInstance", "document"),
        ("document", "document"),
        ("FileInstance", "FileInstance"),
    ] {
        let source = name(SchemaNode::group(
            source_root,
            vec![name(SchemaNode::scalar("FileInstance", ScalarType::String))],
        ));
        let target = name(SchemaNode::group(
            target_root,
            vec![name(SchemaNode::scalar("document", ScalarType::String))],
        ));
        let project = project(source, target, &[(&["FileInstance"], "document")]);
        let directory = TempDir::new()?;
        let design = directory.0.join("mapping.mfd");
        assert!(mfd::export(&project, &design)?.is_empty());
        let imported = mfd::import(&design)?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(imported.project.source.name, source_root);
        assert_eq!(imported.project.target.name, target_root);
        assert_eq!(
            imported.project.source.child("FileInstance").unwrap().kind,
            project.source.child("FileInstance").unwrap().kind
        );
        assert_eq!(
            imported.project.target.child("document").unwrap().kind,
            project.target.child("document").unwrap().kind
        );
        // This regression owns payload selection. The existing XSD boundary
        // normalizes explicit unqualified metadata to its legacy representation.
        if namespace != Some(None) {
            assert_eq!(imported.project.source, project.source);
            assert_eq!(imported.project.target, project.target);
        }
        let xmlns = namespace
            .flatten()
            .map_or_else(String::new, |namespace| format!(" xmlns=\"{namespace}\""));
        let input =
            format!("<{source_root}{xmlns}><FileInstance>payload</FileInstance></{source_root}>");
        let source = format_xml::from_str(&input, &project.source)?;
        let expected = ir::Instance::Group(
            (vec![(
                "document".into(),
                ir::Instance::Scalar(ir::Value::String("payload".into())),
            )])
            .into(),
        );
        assert_eq!(engine::run(&project, &source)?, expected);
        assert_eq!(engine::run(&imported.project, &source)?, expected);
    }
    Ok(())
}

#[test]
fn legacy_reserved_payload_names_are_not_additional_wrappers() -> TestResult {
    check_payloads(None)
}
#[test]
fn explicitly_unqualified_reserved_payload_names_are_retained() -> TestResult {
    check_payloads(Some(None))
}
#[test]
fn qualified_reserved_payload_names_are_retained() -> TestResult {
    check_payloads(Some(Some("urn:ferrule:payload")))
}
#[test]
fn payloads_can_intentionally_use_reserved_wrapper_namespace() -> TestResult {
    check_payloads(Some(Some("http://www.altova.com/mapforce")))
}
