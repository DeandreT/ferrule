//! Migration controls for writing XSD occurrences from the effective XML view.
//!
//! The pre-migration flag reading is retained as an oracle and compared with
//! the view-derived declarations over every role-flag combination and every
//! XML Schema fixture in the workspace.

use std::path::{Path, PathBuf};

use super::super::super::export::{XmlDeclaration, declaration};
use super::super::super::*;

/// The hand-derived element and attribute readings replaced by the view.
fn legacy_matches(node: &SchemaNode, declared: XmlDeclaration) -> bool {
    declared.repeated == node.repeating
        && declared.nil == node.nillable
        && if node.attribute {
            (!declared.repeated && !declared.optional) == node.xml_attribute_required
        } else {
            declared.optional == node.xml_optional
        }
}

fn shapes() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("Value", ScalarType::String),
        SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String),
        SchemaNode::group("Value", vec![]),
        SchemaNode::group("Value", vec![SchemaNode::scalar("Leaf", ScalarType::Int)]),
    ]
}

#[test]
fn every_role_flag_combination_matches_or_refuses_contradictions() {
    let (mut matched, mut refused) = (0, 0);
    for shape in shapes() {
        for bits in 0u32..(1 << 8) {
            let mut node = shape.clone();
            node.repeating = bits & 1 != 0;
            node.attribute = bits & 2 != 0;
            node.text = bits & 4 != 0;
            node.nillable = bits & 8 != 0;
            node.xml_optional = bits & 16 != 0;
            node.xml_attribute_required = bits & 32 != 0;
            // JSON-only metadata, including contradictory roles, is not
            // consulted by the XML boundary.
            node.nullable = bits & 64 != 0;
            node.container_nullable = bits & 128 != 0;
            match declaration(&node) {
                Ok(declared) => {
                    matched += 1;
                    assert!(legacy_matches(&node, declared), "{node:?}");
                }
                Err(error) => {
                    refused += 1;
                    if node.xml_optional_is_valid() {
                        assert!(
                            matches!(error, XmlFormatError::InvalidXmlAttributeRequired { .. }),
                            "{node:?}: {error:?}"
                        );
                    } else {
                        assert!(
                            matches!(error, XmlFormatError::InvalidXmlOptional { .. }),
                            "{node:?}: {error:?}"
                        );
                    }
                }
            }
        }
    }
    assert!(matched > 200 && refused > 200, "{matched} {refused}");
}

#[test]
fn independently_authored_occurrence_controls() {
    let mut optional = SchemaNode::scalar("Note", ScalarType::String).nillable();
    assert!(optional.set_xml_optional(true));
    let mut required = SchemaNode::scalar("code", ScalarType::String).attribute();
    assert!(required.set_xml_attribute_required(true));
    // JSON container metadata on a singular scalar is not an XML role.
    let mut json_only = SchemaNode::scalar("Total", ScalarType::Int);
    json_only.container_nullable = true;
    let schema = SchemaNode::group(
        "Order",
        vec![
            optional,
            json_only,
            SchemaNode::scalar("Line", ScalarType::String).repeating(),
            required,
            SchemaNode::scalar("kind", ScalarType::String).attribute(),
        ],
    );
    let text = export(&schema).unwrap();
    for line in [
        r#"<xs:element name="Note" type="xs:string" minOccurs="0" nillable="true"/>"#,
        r#"<xs:element name="Total" type="xs:integer"/>"#,
        r#"<xs:element name="Line" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>"#,
        r#"<xs:attribute name="code" type="xs:string" use="required"/>"#,
        r#"<xs:attribute name="kind" type="xs:string"/>"#,
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }

    let mut optional_attribute = SchemaNode::scalar("code", ScalarType::String).attribute();
    optional_attribute.xml_optional = true;
    let mut required_element = SchemaNode::scalar("Name", ScalarType::String);
    required_element.xml_attribute_required = true;
    let refused = [
        (optional_attribute, "InvalidXmlOptional"),
        (required_element, "InvalidXmlAttributeRequired"),
    ];
    for (child, expected) in refused {
        let error = export(&SchemaNode::group("Order", vec![child])).unwrap_err();
        assert!(format!("{error:?}").starts_with(expected), "{error:?}");
    }
}

fn fixtures(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            fixtures(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "xsd") {
            found.push(path);
        }
    }
}

fn walk(node: &SchemaNode, visit: &mut impl FnMut(&SchemaNode)) {
    visit(node);
    if let SchemaKind::Group {
        children, dynamic, ..
    } = &node.kind
    {
        for child in children {
            walk(child, visit);
        }
        if let Some(dynamic) = dynamic {
            walk(dynamic, visit);
        }
    }
}

#[test]
fn every_workspace_xml_schema_fixture_matches_before_and_after_migration() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut paths = Vec::new();
    fixtures(&crates, &mut paths);
    let (mut schemas, mut nodes, mut qualified) = (0, 0, 0);
    for path in &paths {
        // Every global element of each fixture is a separately imported root.
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let roots = document
            .root_element()
            .children()
            .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "element")))
            .filter_map(|node| node.attribute("name"))
            .filter_map(|name| import_root(path, Some(name)).ok())
            .collect::<Vec<_>>();
        for schema in roots {
            schemas += 1;
            walk(&schema, &mut |node| {
                nodes += 1;
                qualified +=
                    usize::from(node.nillable || node.xml_optional || node.xml_attribute_required);
                let declared = declaration(node).unwrap();
                assert!(
                    legacy_matches(node, declared),
                    "{}: {node:?}",
                    path.display()
                );
            });
        }
    }
    assert!(schemas > 100, "{schemas}");
    assert!(nodes > 500, "{nodes}");
    assert!(qualified > 40, "{qualified}");
}
