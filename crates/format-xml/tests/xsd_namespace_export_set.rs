use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::xsd::{self, XsdExportSet};
use ir::{ScalarType, SchemaNode};

const ROOT: &str = "urn:ferrule:namespace-set:root";
const SHARED: &str = "urn:ferrule:namespace-set:shared";
static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct FixtureDir(PathBuf);
impl FixtureDir {
    fn new() -> Self {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_namespace_set_{}_{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write_set(&self, filename: &str, set: &XsdExportSet) -> PathBuf {
        for artifact in &set.dependencies {
            std::fs::write(self.0.join(&artifact.filename), &artifact.contents).unwrap();
        }
        let path = self.0.join(filename);
        std::fs::write(&path, &set.root).unwrap();
        path
    }
}
impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn q(node: SchemaNode, namespace: &str) -> SchemaNode {
    node.xml_qualified(namespace).unwrap()
}

fn root(children: Vec<SchemaNode>) -> SchemaNode {
    q(SchemaNode::group("Root", children), ROOT)
}

fn assert_roundtrip(schema: &SchemaNode, filename: &str, set: &XsdExportSet, xml: &str) {
    assert_eq!(&xsd::export_set(schema, filename).unwrap(), set);
    let dir = FixtureDir::new();
    let path = dir.write_set(filename, set);
    let imported = xsd::import(&path).unwrap();
    assert_eq!(imported, *schema);
    assert_eq!(
        format_xml::from_str(xml, &imported).unwrap(),
        format_xml::from_str(xml, schema).unwrap()
    );
}

#[test]
fn one_import_contains_every_foreign_declaration() {
    let schema = root(vec![
        q(SchemaNode::scalar("First", ScalarType::String), SHARED),
        q(SchemaNode::scalar("Second", ScalarType::Int), SHARED),
    ]);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert_eq!(set.root.matches("<xs:import ").count(), 1);
    let foreign = roxmltree::Document::parse(&set.dependencies[0].contents).unwrap();
    let declarations = foreign
        .root_element()
        .children()
        .filter(|node| node.is_element())
        .map(|node| node.attribute("name").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(declarations, ["First", "Second"]);
    assert_roundtrip(
        &schema,
        "root.xsd",
        &set,
        r#"<Root xmlns="urn:ferrule:namespace-set:root" xmlns:s="urn:ferrule:namespace-set:shared"><s:First>a</s:First><s:Second>7</s:Second></Root>"#,
    );
}

#[test]
fn elements_and_attributes_share_one_namespace_document() {
    let schema = root(vec![
        q(SchemaNode::scalar("Value", ScalarType::Int), SHARED),
        q(
            SchemaNode::scalar("Token", ScalarType::String).attribute(),
            SHARED,
        ),
    ]);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert_eq!(set.root.matches("<xs:import ").count(), 1);
    assert!(
        set.dependencies[0]
            .contents
            .contains("<xs:attribute name=\"Token\"")
    );
    assert!(
        set.dependencies[0]
            .contents
            .contains("<xs:element name=\"Value\"")
    );
    assert_roundtrip(
        &schema,
        "root.xsd",
        &set,
        r#"<Root xmlns="urn:ferrule:namespace-set:root" xmlns:s="urn:ferrule:namespace-set:shared" s:Token="a"><s:Value>7</s:Value></Root>"#,
    );
}

#[test]
fn nested_imports_redirect_to_the_consolidated_artifact() {
    let third = "urn:ferrule:namespace-set:third";
    let shared = |name| {
        q(
            SchemaNode::group(
                name,
                vec![q(SchemaNode::scalar("Code", ScalarType::String), third)],
            ),
            SHARED,
        )
    };
    let schema = root(vec![shared("First"), shared("Second")]);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 2);
    assert_eq!(set.root.matches("<xs:import ").count(), 1);
    assert_eq!(
        set.dependencies[0].contents.matches("<xs:import ").count(),
        1
    );
    assert!(!set.root.contains("root-ns3.xsd"));
    assert_roundtrip(
        &schema,
        "root.xsd",
        &set,
        r#"<Root xmlns="urn:ferrule:namespace-set:root" xmlns:s="urn:ferrule:namespace-set:shared" xmlns:t="urn:ferrule:namespace-set:third"><s:First><t:Code>a</t:Code></s:First><s:Second><t:Code>b</t:Code></s:Second></Root>"#,
    );
}

#[test]
fn merging_preserves_different_meanings_of_the_same_prefix() {
    let third = "urn:ferrule:namespace-set:third";
    let fourth = "urn:ferrule:namespace-set:fourth";
    let schema = root(vec![
        q(
            SchemaNode::group(
                "First",
                vec![q(SchemaNode::scalar("Code", ScalarType::String), third)],
            ),
            SHARED,
        ),
        q(
            SchemaNode::group(
                "Second",
                vec![q(SchemaNode::scalar("Other", ScalarType::String), fourth)],
            ),
            SHARED,
        ),
    ]);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 3);
    let merged = roxmltree::Document::parse(&set.dependencies[0].contents).unwrap();
    let mut saw_component = false;
    for node in merged
        .root_element()
        .children()
        .filter(|node| node.is_element())
    {
        if node.tag_name().name() == "import" {
            assert!(!saw_component, "imports must precede global declarations");
        } else {
            saw_component = true;
        }
    }
    let references = merged
        .descendants()
        .filter_map(|node| node.attribute("ref").map(|name| (node, name)))
        .map(|(node, name)| {
            let (prefix, local) = name.split_once(':').unwrap();
            (node.lookup_namespace_uri(Some(prefix)).unwrap(), local)
        })
        .collect::<Vec<_>>();
    assert_eq!(references, [(third, "Code"), (fourth, "Other")]);
    assert_roundtrip(
        &schema,
        "root.xsd",
        &set,
        r#"<Root xmlns="urn:ferrule:namespace-set:root" xmlns:s="urn:ferrule:namespace-set:shared" xmlns:t="urn:ferrule:namespace-set:third" xmlns:f="urn:ferrule:namespace-set:fourth"><s:First><t:Code>a</t:Code></s:First><s:Second><f:Other>b</f:Other></s:Second></Root>"#,
    );
}

fn recursive_branch(field: &str) -> SchemaNode {
    q(
        SchemaNode::group(
            "Branch",
            vec![
                q(SchemaNode::scalar(field, ScalarType::String), SHARED),
                q(
                    SchemaNode::recursive_group("Leaf", "Branch").repeating(),
                    SHARED,
                ),
            ],
        ),
        SHARED,
    )
}

#[test]
fn equal_recursive_types_deduplicate_across_declaration_documents() {
    let schema = root(vec![
        q(
            SchemaNode::group("First", vec![recursive_branch("Code")]),
            SHARED,
        ),
        q(
            SchemaNode::group("Second", vec![recursive_branch("Code")]),
            SHARED,
        ),
    ]);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert_eq!(
        set.dependencies[0]
            .contents
            .matches("name=\"BranchType\"")
            .count(),
        1
    );
    assert_roundtrip(
        &schema,
        "root.xsd",
        &set,
        r#"<Root xmlns="urn:ferrule:namespace-set:root" xmlns:s="urn:ferrule:namespace-set:shared"><s:First><s:Branch><s:Code>a</s:Code><s:Leaf><s:Code>b</s:Code></s:Leaf></s:Branch></s:First><s:Second><s:Branch><s:Code>c</s:Code></s:Branch></s:Second></Root>"#,
    );
}

#[test]
fn conflicting_recursive_types_reject_atomically() {
    let schema = root(vec![
        q(
            SchemaNode::group("First", vec![recursive_branch("Code")]),
            SHARED,
        ),
        q(
            SchemaNode::group("Second", vec![recursive_branch("Other")]),
            SHARED,
        ),
    ]);
    assert!(matches!(xsd::export_set(&schema, "root.xsd"),
        Err(format_xml::XmlFormatError::UnsupportedRecursiveAnchor { anchor, .. })
        if anchor == "Branch"));
}

#[test]
fn merged_namespace_and_artifact_names_remain_xml_escaped() {
    let shared = "urn:ferrule:namespace-set:shared?a&b";
    let schema = root(vec![
        q(SchemaNode::scalar("First", ScalarType::String), shared),
        q(SchemaNode::scalar("Second", ScalarType::String), shared),
    ]);
    let set = xsd::export_set(&schema, "a&b.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert!(set.root.contains("schemaLocation=\"a&amp;b-ns1.xsd\""));
    let merged = roxmltree::Document::parse(&set.dependencies[0].contents).unwrap();
    assert_eq!(
        merged.root_element().attribute("targetNamespace"),
        Some(shared)
    );
    assert_roundtrip(
        &schema,
        "a&b.xsd",
        &set,
        r#"<Root xmlns="urn:ferrule:namespace-set:root" xmlns:s="urn:ferrule:namespace-set:shared?a&amp;b"><s:First>a</s:First><s:Second>b</s:Second></Root>"#,
    );
}

#[test]
fn consolidation_keeps_the_existing_declaration_limit() {
    let schema = root(
        (0..=64)
            .map(|index| {
                q(
                    SchemaNode::scalar(format!("Value{index}"), ScalarType::String),
                    SHARED,
                )
            })
            .collect(),
    );
    assert!(matches!(
        xsd::export_set(&schema, "root.xsd"),
        Err(format_xml::XmlFormatError::NamespaceArtifactLimit { limit: 64 })
    ));
}

#[test]
fn merged_schema_namespace_keeps_the_fixed_xsd_prefix() {
    let namespace = "http://www.w3.org/2001/XMLSchema";
    let schema = root(vec![
        q(SchemaNode::scalar("First", ScalarType::String), namespace),
        q(SchemaNode::scalar("Second", ScalarType::String), namespace),
    ]);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    let merged = roxmltree::Document::parse(&set.dependencies[0].contents).unwrap();
    assert_eq!(
        merged.root_element().lookup_namespace_uri(Some("xs")),
        Some(namespace)
    );
    assert_roundtrip(
        &schema,
        "root.xsd",
        &set,
        r#"<Root xmlns="urn:ferrule:namespace-set:root" xmlns:s="http://www.w3.org/2001/XMLSchema"><s:First>a</s:First><s:Second>b</s:Second></Root>"#,
    );
}

fn cross_document_type_collision(anchor: &str, alternative: &str) -> SchemaNode {
    let recursive = q(
        SchemaNode::group(
            anchor,
            vec![
                SchemaNode::scalar("Value", ScalarType::String),
                q(
                    SchemaNode::recursive_group("Next", anchor).repeating(),
                    SHARED,
                ),
            ],
        ),
        SHARED,
    );
    let mut typed = q(
        SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
        .with_alternatives(vec![ir::GroupAlternative {
            name: format!("{{{SHARED}}}{alternative}"),
            members: vec!["Value".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }])
        .unwrap(),
        SHARED,
    );
    typed.xml_type_alternatives = true;
    root(vec![recursive, typed])
}

fn assert_cross_document_type_collision(anchor: &str, alternative: &str, allocated: &str) {
    let schema = cross_document_type_collision(anchor, alternative);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    let merged = roxmltree::Document::parse(&set.dependencies[0].contents).unwrap();
    let mut types = std::collections::BTreeSet::new();
    for node in merged
        .root_element()
        .children()
        .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "complexType")))
    {
        assert!(types.insert(node.attribute("name").unwrap()));
    }
    assert!(types.contains(allocated));
    assert!(types.contains(alternative));
    let xml = format!(
        r#"<q:Root xmlns:q="{ROOT}" xmlns:s="{SHARED}" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><s:{anchor}><Value>a</Value><s:Next><Value>b</Value></s:Next></s:{anchor}><s:Item xsi:type="s:{alternative}"><Value>c</Value></s:Item></q:Root>"#
    );
    assert_roundtrip(&schema, "root.xsd", &set, &xml);
}

#[test]
fn foreign_recursive_names_avoid_user_types_in_other_documents() {
    assert_cross_document_type_collision("Branch", "BranchType", "BranchType2");
}

#[test]
fn foreign_recursive_names_avoid_synthetic_bases_in_other_documents() {
    assert_cross_document_type_collision("ABase", "A", "ABaseType2");
}
