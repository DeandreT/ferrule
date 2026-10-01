use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::xsd::{self, XsdExportSet};
use ir::{GroupAlternative, ScalarType, SchemaNode, XML_TEXT_FIELD, XmlRepeatingChoice};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
struct FixtureDir(PathBuf);
impl FixtureDir {
    fn new(name: &str) -> Self {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ferrule_xsd_recursive_{name}_{}_{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn schema(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
    fn set(&self, set: &XsdExportSet) -> PathBuf {
        for dependency in &set.dependencies {
            self.schema(&dependency.filename, &dependency.contents);
        }
        self.schema("root.xsd", &set.root)
    }
}
impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn qualified(node: SchemaNode, namespace: &str) -> SchemaNode {
    node.xml_qualified(namespace).unwrap()
}
fn assert_declared_type_qnames(contents: &str) {
    let doc = roxmltree::Document::parse(contents).unwrap();
    let schema = doc.root_element();
    for element in doc.descendants().filter(|node| node.is_element()) {
        let Some(qname) = element.attribute("type") else {
            continue;
        };
        let (namespace, local) = qname
            .split_once(':')
            .map_or((None, qname), |(prefix, local)| {
                (element.lookup_namespace_uri(Some(prefix)), local)
            });
        if namespace == Some("http://www.w3.org/2001/XMLSchema") {
            continue;
        }
        assert_eq!(namespace, schema.attribute("targetNamespace"), "{qname}");
        assert!(
            schema.children().any(|node| node.is_element()
                && matches!(node.tag_name().name(), "complexType" | "simpleType")
                && node.attribute("name") == Some(local)),
            "{qname}"
        );
    }
}
fn assert_recursive_xml_reimport(path: &Path, original: &SchemaNode, xml: &str) {
    let reimported = xsd::import(path).unwrap();
    let expected = format_xml::from_str(xml, original).unwrap();
    assert_eq!(format_xml::from_str(xml, &reimported).unwrap(), expected);
}
fn mixed_root(namespace: Option<&str>) -> SchemaNode {
    let exact = |node: SchemaNode| match namespace {
        Some(ns) => qualified(node, ns),
        None => node,
    };
    let mut root = exact(SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            exact(SchemaNode::recursive_group("Alpha", "Root").repeating()),
            exact(SchemaNode::recursive_group("Beta", "Root").repeating()),
            exact(SchemaNode::recursive_group("Gamma", "Root").repeating()),
        ],
    ));
    root.xml_repeating_choices = vec![XmlRepeatingChoice {
        required: false,
        repeating: true,
        members: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
    }];
    root
}

#[test]
fn recursive_named_type_qnames_retain_target_namespace() {
    let namespace = "urn:ferrule:recursive:tree";
    let branch = qualified(
        SchemaNode::group(
            "Branch",
            vec![
                qualified(SchemaNode::scalar("Label", ScalarType::String), namespace),
                qualified(
                    SchemaNode::recursive_group("Leaf", "Branch").repeating(),
                    namespace,
                ),
            ],
        ),
        namespace,
    );
    let schema = qualified(SchemaNode::group("Root", vec![branch]), namespace);
    let text = xsd::export(&schema).unwrap();
    assert_declared_type_qnames(&text);
    assert_eq!(text.matches("type=\"tns:BranchType\"").count(), 2);
    let dir = FixtureDir::new("namespace");
    let path = dir.schema("root.xsd", &text);
    assert_recursive_xml_reimport(
        &path,
        &schema,
        "<Root xmlns=\"urn:ferrule:recursive:tree\"><Branch><Label>a</Label><Leaf><Label>b</Label><Leaf><Label>c</Label></Leaf></Leaf></Branch></Root>",
    );
}

#[test]
fn differently_named_root_recursion_retains_distinct_choice_particles() {
    for namespace in [None, Some("urn:ferrule:recursive:mixed")] {
        let schema = mixed_root(namespace);
        let text = xsd::export(&schema).unwrap();
        assert_declared_type_qnames(&text);
        let doc = roxmltree::Document::parse(&text).unwrap();
        for choice in doc
            .descendants()
            .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "choice")))
        {
            let names: Vec<_> = choice
                .children()
                .filter(|node| node.is_element())
                .map(|node| node.attribute("name").unwrap())
                .collect();
            assert_eq!(names, ["Alpha", "Beta", "Gamma"]);
        }
        let dir = FixtureDir::new("root_names");
        let path = dir.schema("root.xsd", &text);
        let xml = if namespace.is_some() {
            "<Root xmlns=\"urn:ferrule:recursive:mixed\">a<Alpha>b<Beta>c</Beta>d</Alpha><Gamma/></Root>"
        } else {
            "<Root>a<Alpha>b<Beta>c</Beta>d</Alpha><Gamma/></Root>"
        };
        assert_recursive_xml_reimport(&path, &schema, xml);
    }
}

#[test]
fn foreign_recursive_declaration_preserves_root_and_aliased_child_names() {
    let document = "urn:ferrule:recursive:document";
    let markup = "urn:ferrule:recursive:markup";
    let mut paragraph = qualified(
        SchemaNode::group(
            "Paragraph",
            vec![
                SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                qualified(
                    SchemaNode::recursive_group("Emphasis", "Paragraph").repeating(),
                    markup,
                ),
                SchemaNode::recursive_group("Aside", "Paragraph")
                    .xml_unqualified()
                    .repeating(),
            ],
        ),
        document,
    );
    paragraph.xml_repeating_choices = vec![XmlRepeatingChoice {
        required: false,
        repeating: true,
        members: vec!["Emphasis".into(), "Aside".into()],
    }];
    let schema = qualified(SchemaNode::group("Document", vec![paragraph]), document);
    let set = xsd::export_set(&schema, "root.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert_declared_type_qnames(&set.root);
    assert_declared_type_qnames(&set.dependencies[0].contents);
    assert!(
        set.dependencies[0]
            .contents
            .contains("name=\"Aside\" type=\"tns:EmphasisType\"")
    );
    assert!(
        set.dependencies[0]
            .contents
            .contains("name=\"Emphasis\" type=\"tns:EmphasisType\"")
    );
    let dir = FixtureDir::new("foreign");
    let path = dir.set(&set);
    assert_recursive_xml_reimport(
        &path,
        &schema,
        "<Document xmlns=\"urn:ferrule:recursive:document\" xmlns:m=\"urn:ferrule:recursive:markup\"><Paragraph>a<m:Emphasis>b<Aside xmlns=\"\">c<m:Emphasis>d</m:Emphasis></Aside></m:Emphasis></Paragraph></Document>",
    );
}

#[test]
fn recursive_root_type_preserves_same_name_unqualified_occurrences() {
    let namespace = "urn:ferrule:recursive:qualified";
    let schema = qualified(
        SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Value", ScalarType::String).xml_unqualified(),
                SchemaNode::recursive_group("Root", "Root")
                    .xml_unqualified()
                    .repeating(),
            ],
        ),
        namespace,
    );
    let text = xsd::export(&schema).unwrap();
    assert_declared_type_qnames(&text);
    assert!(
        text.contains(r#"name="Root" type="tns:RootType" minOccurs="0" maxOccurs="unbounded""#)
    );
    assert!(!text.contains(r#"ref="Root""#));
    let dir = FixtureDir::new("unqualified_same_name");
    let path = dir.schema("root.xsd", &text);
    assert_recursive_xml_reimport(
        &path,
        &schema,
        r#"<q:Root xmlns:q="urn:ferrule:recursive:qualified"><Value>a</Value><Root><Value>b</Value><Root><Value>c</Value></Root></Root></q:Root>"#,
    );
}

#[test]
fn recursive_root_type_preserves_repeating_nil_occurrence() {
    let mut branch = SchemaNode::recursive_group("Branch", "Root").repeating();
    branch.nillable = true;
    let schema = SchemaNode::group("Root", vec![branch]);
    let text = xsd::export(&schema).unwrap();
    assert!(text.contains(
        r#"name="Branch" type="RootType" minOccurs="0" maxOccurs="unbounded" nillable="true""#
    ));
    let dir = FixtureDir::new("nillable");
    let path = dir.schema("root.xsd", &text);
    let reimported = xsd::import(&path).unwrap();
    assert!(reimported.child("Branch").unwrap().nillable);
    let nil = r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Branch xsi:nil="true"/></Root>"#;
    for boundary in [&schema, &reimported] {
        assert!(
            matches!(format_xml::from_str(nil, boundary), Err(format_xml::XmlFormatError::UnsupportedXmlNilGroup { name }) if name == "Branch")
        );
    }
    assert_recursive_xml_reimport(&path, &schema, "<Root><Branch><Branch/></Branch></Root>");
}

#[test]
fn missing_and_conflicting_recursive_anchors_still_reject() {
    let missing = SchemaNode::group(
        "Root",
        vec![SchemaNode::recursive_group("Child", "Missing")],
    );
    assert!(
        matches!(xsd::export(&missing), Err(format_xml::XmlFormatError::UnsupportedRecursiveAnchor { anchor, .. }) if anchor == "Missing")
    );
    let branch = |field| {
        SchemaNode::group(
            "Branch",
            vec![
                SchemaNode::scalar(field, ScalarType::String),
                SchemaNode::recursive_group("Leaf", "Branch"),
            ],
        )
    };
    let conflicting = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group("First", vec![branch("Code")]),
            SchemaNode::group("Second", vec![branch("Name")]),
        ],
    );
    assert!(
        matches!(xsd::export(&conflicting), Err(format_xml::XmlFormatError::UnsupportedRecursiveAnchor { anchor, .. }) if anchor == "Branch")
    );
}

fn typed_group(name: &str, identity: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
        .with_alternatives(vec![GroupAlternative {
            name: identity.into(),
            members: vec!["Value".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }])
        .unwrap()
}

fn assert_unique_types(text: &str) {
    let doc = roxmltree::Document::parse(text).unwrap();
    let mut names = std::collections::BTreeSet::new();
    for node in doc.root_element().children().filter(|node| {
        node.is_element() && matches!(node.tag_name().name(), "complexType" | "simpleType")
    }) {
        assert!(
            names.insert(node.attribute("name").unwrap()),
            "duplicate type"
        );
    }
    assert_declared_type_qnames(text);
}

#[test]
fn recursive_type_names_avoid_user_alternatives_and_reserved_suffixes() {
    let namespace = "urn:ferrule:recursive:collision";
    let schema = qualified(
        SchemaNode::group(
            "Root",
            vec![
                typed_group("Item", "{urn:ferrule:recursive:collision}RootType"),
                typed_group("Second", "{urn:ferrule:recursive:collision}RootType2"),
                SchemaNode::recursive_group("Again", "Root").repeating(),
            ],
        ),
        namespace,
    );
    let text = xsd::export(&schema).unwrap();
    assert_eq!(text, xsd::export(&schema).unwrap());
    assert_unique_types(&text);
    assert!(text.contains(r#"name="RootType3""#));
    assert_eq!(text.matches(r#"type="tns:RootType3""#).count(), 2);
    let dir = FixtureDir::new("type_collision");
    let path = dir.schema("root.xsd", &text);
    assert_recursive_xml_reimport(
        &path,
        &schema,
        r#"<q:Root xmlns:q="urn:ferrule:recursive:collision" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Item xsi:type="q:RootType"><Value>a</Value></Item><Second xsi:type="q:RootType2"><Value>b</Value></Second><Again><Item xsi:type="q:RootType"><Value>c</Value></Item><Second xsi:type="q:RootType2"><Value>d</Value></Second></Again></q:Root>"#,
    );
}

#[test]
fn recursive_type_names_avoid_synthetic_alternative_bases() {
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::group(
            "ABase",
            vec![
                typed_group("Item", "A"),
                SchemaNode::recursive_group("Next", "ABase").repeating(),
            ],
        )],
    );
    let text = xsd::export(&schema).unwrap();
    assert_unique_types(&text);
    assert!(text.contains(r#"name="ABaseType2""#));
    assert_eq!(text.matches(r#"type="ABaseType2""#).count(), 2);
    let dir = FixtureDir::new("synthetic_collision");
    let path = dir.schema("root.xsd", &text);
    assert_recursive_xml_reimport(
        &path,
        &schema,
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><ABase><Item xsi:type="A"><Value>a</Value></Item><Next><Item xsi:type="A"><Value>b</Value></Item></Next></ABase></Root>"#,
    );
}

#[test]
fn recursive_alternative_anchor_reuses_its_declared_base_type() {
    for namespace in [None, Some("urn:ferrule:recursive:alternative")] {
        let identity = namespace.map_or_else(|| "A".into(), |ns| format!("{{{ns}}}A"));
        let mut schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Value", ScalarType::String),
                SchemaNode::recursive_group("Again", "Root").repeating(),
            ],
        )
        .with_alternatives(vec![GroupAlternative {
            name: identity,
            members: vec!["Value".into(), "Again".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }])
        .unwrap();
        if let Some(ns) = namespace {
            schema = qualified(schema, ns);
        }
        let text = xsd::export(&schema).unwrap();
        assert_unique_types(&text);
        assert!(!text.contains(r#"name="RootType""#));
        let base = if namespace.is_some() {
            "tns:ABaseType"
        } else {
            "ABaseType"
        };
        assert_eq!(text.matches(&format!(r#"type="{base}""#)).count(), 2);
        let dir = FixtureDir::new("alternative_anchor");
        let path = dir.schema("root.xsd", &text);
        let xml = if namespace.is_some() {
            r#"<q:Root xmlns:q="urn:ferrule:recursive:alternative" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="q:A"><Value>a</Value></q:Root>"#
        } else {
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="A"><Value>a</Value></Root>"#
        };
        assert_recursive_xml_reimport(&path, &schema, xml);
    }
}

#[test]
fn recursive_alternative_occurrences_retain_restricted_views() {
    let view = |name: &str, derived: &str, extra: &str| {
        SchemaNode::group(
            name,
            vec![
                SchemaNode::scalar("Value", ScalarType::String),
                SchemaNode::recursive_group("Again", "Root").repeating(),
                SchemaNode::scalar(extra, ScalarType::String),
            ],
        )
        .with_alternatives(vec![
            GroupAlternative {
                name: "Base".into(),
                members: vec!["Value".into(), "Again".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            },
            GroupAlternative {
                name: derived.into(),
                members: vec!["Value".into(), "Again".into(), extra.into()],
                required: Vec::new(),
                constraints: Vec::new(),
            },
        ])
        .unwrap()
    };
    let mut schema = view("Root", "A", "AOnly");
    if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
        children.push(view("Other", "B", "BOnly"));
    }
    let text = xsd::export(&schema).unwrap();
    assert_unique_types(&text);
    let doc = roxmltree::Document::parse(&text).unwrap();
    let occurrences: Vec<_> = doc
        .descendants()
        .filter(|node| {
            node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "element"))
                && matches!(node.attribute("name"), Some("Root" | "Again"))
        })
        .collect();
    assert_eq!(occurrences.len(), 2);
    for occurrence in occurrences {
        assert_eq!(occurrence.attribute("type"), Some("Base"));
        let identities: Vec<_> = occurrence
            .descendants()
            .filter(|node| node.has_tag_name(("urn:ferrule:xsd:group-alternatives", "type")))
            .map(|node| node.attribute("name").unwrap())
            .collect();
        assert_eq!(identities, ["Base", "A"]);
    }
    let dir = FixtureDir::new("recursive_view");
    let path = dir.schema("root.xsd", &text);
    assert_recursive_xml_reimport(
        &path,
        &schema,
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="A"><Value>a</Value><Again xsi:type="A"><Value>b</Value><AOnly>y</AOnly></Again><AOnly>x</AOnly></Root>"#,
    );
}
