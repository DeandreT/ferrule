use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{GroupAlternative, ScalarType, SchemaNode};
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

fn entry_namespace<'a>(
    root: roxmltree::Node<'a, 'a>,
    entry: roxmltree::Node<'a, 'a>,
) -> Option<&'a str> {
    let slot = entry
        .attribute("ns")
        .unwrap_or("0")
        .parse::<usize>()
        .expect("namespace slot");
    let namespaces = root
        .children()
        .find(|node| node.has_tag_name("header"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("namespaces"))
        .unwrap();
    namespaces
        .children()
        .filter(|node| node.has_tag_name("namespace"))
        .nth(slot)
        .unwrap()
        .attribute("uid")
}
fn component_root<'a>(
    document: &'a roxmltree::Document<'a>,
    name: &str,
) -> roxmltree::Node<'a, 'a> {
    document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("data"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("root"))
        .unwrap()
}
fn document_entry<'a>(root: roxmltree::Node<'a, 'a>) -> roxmltree::Node<'a, 'a> {
    let file = root
        .children()
        .find(|node| node.has_tag_name("entry"))
        .unwrap();
    let document = file
        .children()
        .find(|node| node.has_tag_name("entry"))
        .unwrap();
    assert_eq!(file.attribute("name"), Some("FileInstance"));
    assert_eq!(document.attribute("name"), Some("document"));
    for wrapper in [file, document] {
        assert_eq!(
            entry_namespace(root, wrapper),
            Some("http://www.altova.com/mapforce")
        );
    }
    document
        .children()
        .find(|node| node.has_tag_name("entry"))
        .unwrap()
}
fn exported(project: &Project, directory: &TempDir) -> Result<String, Box<dyn Error>> {
    let design = directory.0.join("mapping.mfd");
    assert!(mfd::export(project, &design)?.is_empty());
    Ok(std::fs::read_to_string(design)?)
}
fn roundtrip(project: &Project, directory: &TempDir, input: &str) -> TestResult {
    let imported = mfd::import(&directory.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, project.source);
    assert_eq!(imported.project.target, project.target);
    let source = format_xml::from_str(input, &project.source)?;
    assert_eq!(
        engine::run(&imported.project, &source)?,
        engine::run(project, &source)?
    );
    Ok(())
}

#[test]
fn entry_slots_preserve_qualified_elements_and_attributes() -> TestResult {
    let source = qualified(
        SchemaNode::group(
            "Input",
            vec![
                qualified(
                    SchemaNode::scalar("ForeignValue", ScalarType::String),
                    "urn:ferrule:foreign",
                ),
                SchemaNode::scalar("LocalValue", ScalarType::String).xml_unqualified(),
                SchemaNode::scalar("BareId", ScalarType::String).attribute(),
                qualified(
                    SchemaNode::scalar("Tag", ScalarType::String).attribute(),
                    "urn:ferrule:attributes",
                ),
            ],
        ),
        "urn:ferrule:input",
    );
    let target = qualified(
        SchemaNode::group(
            "Output",
            vec![
                qualified(
                    SchemaNode::scalar("ForeignResult", ScalarType::String),
                    "urn:ferrule:foreign",
                ),
                SchemaNode::scalar("LocalResult", ScalarType::String).xml_unqualified(),
                SchemaNode::scalar("BareId", ScalarType::String).attribute(),
                qualified(
                    SchemaNode::scalar("Tag", ScalarType::String).attribute(),
                    "urn:ferrule:attributes",
                ),
            ],
        ),
        "urn:ferrule:output",
    );
    let project = project(
        source,
        target,
        &[
            (&["ForeignValue"], "ForeignResult"),
            (&["LocalValue"], "LocalResult"),
            (&["BareId"], "BareId"),
            (&["Tag"], "Tag"),
        ],
    );
    let directory = TempDir::new()?;
    let text = exported(&project, &directory)?;
    let document = roxmltree::Document::parse(&text)?;
    for (name, namespace, foreign, local) in [
        ("Input", "urn:ferrule:input", "ForeignValue", "LocalValue"),
        (
            "Output",
            "urn:ferrule:output",
            "ForeignResult",
            "LocalResult",
        ),
    ] {
        let root = component_root(&document, name);
        let element = document_entry(root);
        assert_eq!(entry_namespace(root, element), Some(namespace));
        for (name, expected, attribute) in [
            (foreign, Some("urn:ferrule:foreign"), false),
            (local, None, false),
            ("BareId", None, true),
            ("Tag", Some("urn:ferrule:attributes"), true),
        ] {
            let entry = element
                .children()
                .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some(name))
                .unwrap();
            assert_eq!(entry_namespace(root, entry), expected);
            assert_eq!(entry.attribute("type") == Some("attribute"), attribute);
        }
    }
    roundtrip(
        &project,
        &directory,
        r#"<Input xmlns="urn:ferrule:input" xmlns:f="urn:ferrule:foreign" xmlns:a="urn:ferrule:attributes" BareId="plain" a:Tag="qualified"><f:ForeignValue>foreign</f:ForeignValue><LocalValue xmlns="">local</LocalValue></Input>"#,
    )
}

#[test]
fn user_document_names_and_namespace_can_match_reserved_wrappers() -> TestResult {
    let namespace = "http://www.altova.com/mapforce";
    let source = qualified(
        SchemaNode::group(
            "document",
            vec![qualified(
                SchemaNode::scalar("FileInstance", ScalarType::String),
                namespace,
            )],
        ),
        namespace,
    );
    let target = qualified(
        SchemaNode::group(
            "FileInstance",
            vec![qualified(
                SchemaNode::scalar("document", ScalarType::String),
                namespace,
            )],
        ),
        namespace,
    );
    let project = project(source, target, &[(&["FileInstance"], "document")]);
    let directory = TempDir::new()?;
    let text = exported(&project, &directory)?;
    let document = roxmltree::Document::parse(&text)?;
    for name in ["document", "FileInstance"] {
        let root = component_root(&document, name);
        assert_eq!(
            root.descendants()
                .filter(|node| node.has_tag_name("namespace")
                    && node.attribute("uid") == Some(namespace))
                .count(),
            1
        );
        let element = document_entry(root);
        assert_eq!(entry_namespace(root, element), Some(namespace));
        let child = element
            .children()
            .find(|node| node.has_tag_name("entry"))
            .unwrap();
        assert_eq!(entry_namespace(root, child), Some(namespace));
    }
    let input = format_xml::from_str(
        r#"<document xmlns="http://www.altova.com/mapforce"><FileInstance>payload</FileInstance></document>"#,
        &project.source,
    )?;
    assert_eq!(
        engine::run(&project, &input)?,
        ir::Instance::Group(
            (vec![(
                "document".into(),
                ir::Instance::Scalar(ir::Value::String("payload".into()))
            )])
            .into()
        )
    );
    Ok(())
}

#[test]
fn recursive_entry_occurrences_keep_qualified_anchor_descendants() -> TestResult {
    let source = qualified(
        SchemaNode::group(
            "Tree",
            vec![
                qualified(
                    SchemaNode::scalar("Value", ScalarType::String),
                    "urn:ferrule:foreign",
                ),
                qualified(
                    SchemaNode::recursive_group("Tree", "Tree"),
                    "urn:ferrule:tree",
                ),
            ],
        ),
        "urn:ferrule:tree",
    );
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    let project = project(source, target, &[(&["Tree", "Value"], "Result")]);
    let directory = TempDir::new()?;
    let text = exported(&project, &directory)?;
    let document = roxmltree::Document::parse(&text)?;
    let root = component_root(&document, "Tree");
    let element = document_entry(root);
    let branch = element
        .children()
        .find(|node| node.attribute("name") == Some("Tree"))
        .unwrap();
    assert_eq!(entry_namespace(root, branch), Some("urn:ferrule:tree"));
    let value = branch
        .children()
        .find(|node| node.attribute("name") == Some("Value"))
        .unwrap();
    assert_eq!(entry_namespace(root, value), Some("urn:ferrule:foreign"));
    roundtrip(
        &project,
        &directory,
        r#"<Tree xmlns="urn:ferrule:tree" xmlns:f="urn:ferrule:foreign"><Tree><f:Value>nested</f:Value></Tree></Tree>"#,
    )
}

#[test]
fn xsi_type_clone_entries_keep_the_element_namespace() -> TestResult {
    let address = qualified(
        SchemaNode::group(
            "Address",
            vec![qualified(
                SchemaNode::scalar("Value", ScalarType::String),
                "urn:ferrule:types",
            )],
        )
        .with_alternatives(vec![
            GroupAlternative {
                name: "{urn:ferrule:types}First".into(),
                members: vec!["Value".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            },
            GroupAlternative {
                name: "{urn:ferrule:types}Second".into(),
                members: Vec::new(),
                required: Vec::new(),
                constraints: Vec::new(),
            },
        ])
        .expect("compatible alternatives"),
        "urn:ferrule:types",
    );
    let mut address = address;
    address.xml_type_alternatives = true;
    let source = qualified(
        SchemaNode::group("Input", vec![address]),
        "urn:ferrule:types",
    );
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    let project = project(source, target, &[(&["Address", "Value"], "Result")]);
    let directory = TempDir::new()?;
    let text = exported(&project, &directory)?;
    let document = roxmltree::Document::parse(&text)?;
    let root = component_root(&document, "Input");
    let element = document_entry(root);
    let addresses = element
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Address"))
        .collect::<Vec<_>>();
    assert_eq!(addresses.len(), 3);
    for address in addresses {
        assert_eq!(entry_namespace(root, address), Some("urn:ferrule:types"));
    }
    roundtrip(
        &project,
        &directory,
        r#"<Input xmlns="urn:ferrule:types" xmlns:t="urn:ferrule:types" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Address xsi:type="t:First"><Value>chosen</Value></Address></Input>"#,
    )
}

#[test]
fn legacy_root_uses_exported_type_namespace_without_qualifying_children() -> TestResult {
    let mut address = SchemaNode::group(
        "Address",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "{urn:ferrule:types}First".into(),
            members: vec!["Value".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
        GroupAlternative {
            name: "{urn:ferrule:types}Second".into(),
            members: Vec::new(),
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ])
    .expect("compatible alternatives");
    address.xml_type_alternatives = true;
    let source = SchemaNode::group("Input", vec![address]);
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    let project = project(source, target, &[(&["Address", "Value"], "Result")]);
    let directory = TempDir::new()?;
    let text = exported(&project, &directory)?;
    let document = roxmltree::Document::parse(&text)?;
    let root = component_root(&document, "Input");
    let element = document_entry(root);
    assert_eq!(entry_namespace(root, element), Some("urn:ferrule:types"));
    for address in element.children().filter(|node| node.has_tag_name("entry")) {
        assert_eq!(entry_namespace(root, address), None);
    }
    roundtrip(
        &project,
        &directory,
        r#"<Input xmlns="urn:ferrule:types" xmlns:t="urn:ferrule:types" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Address xmlns="" xsi:type="t:First"><Value>chosen</Value></Address></Input>"#,
    )
}
