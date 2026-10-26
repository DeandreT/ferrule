//! Migration controls for admitting `xsi:nil` from the effective XML view.
//!
//! The reader, the bounded structured reader and the writer now admit
//! `xsi:nil="true"` from the shared XML declaration. The previous flag read is
//! retained as an oracle and compared over every role-flag combination and
//! every node of every XML Schema fixture root.

use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value, XML_TEXT_FIELD};

use super::{XmlFormatError, from_str, structured::from_str_structured, to_string};
use crate::declaration::declaration;
use crate::xsd::tests::export::effective_declarations::{fixtures, walk};

const NIL: &str = r#"xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true""#;

/// The previous hand-read nil admission.
fn legacy_nil(node: &SchemaNode) -> bool {
    node.nillable
}

fn shapes() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("Value", ScalarType::String),
        SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String),
        SchemaNode::group("Value", vec![]),
        SchemaNode::group("Value", vec![SchemaNode::scalar("Leaf", ScalarType::Int)]),
    ]
}

/// Reads one nil occurrence of `child` inside a `Root` element.
fn read_nil(child: &SchemaNode) -> Result<Instance, XmlFormatError> {
    let root = SchemaNode::group("Root", vec![child.clone()]);
    from_str(&format!("<Root><{} {NIL}/></Root>", child.name), &root)
}

fn write_nil(child: &SchemaNode) -> Result<String, XmlFormatError> {
    let root = SchemaNode::group("Root", vec![child.clone()]);
    let instance =
        Instance::Group(vec![(child.name.clone(), Instance::Scalar(Value::xml_nil()))].into());
    to_string(&root, &instance)
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
            // JSON-only metadata is not consulted by the XML boundary.
            node.nullable = bits & 64 != 0;
            node.container_nullable = bits & 128 != 0;
            match (
                crate::declaration::validate_schema(&node),
                declaration(&node),
            ) {
                (Ok(()), Ok(declared)) => {
                    matched += 1;
                    assert_eq!(declared.nil, legacy_nil(&node), "{node:?}");
                }
                // The reader and writer preflight refuses every contradiction
                // with the declaration's own error before nil is consulted.
                (Err(preflight), Err(expected)) => {
                    refused += 1;
                    assert_eq!(
                        format!("{preflight:?}"),
                        format!("{expected:?}"),
                        "{node:?}"
                    );
                    let read = read_nil(&node).unwrap_err();
                    assert_eq!(format!("{read:?}"), format!("{expected:?}"), "{node:?}");
                    let written = write_nil(&node).unwrap_err();
                    assert_eq!(format!("{written:?}"), format!("{expected:?}"), "{node:?}");
                }
                // Other XML metadata the preflight refuses has a declaration.
                (Err(_), Ok(declared)) => {
                    matched += 1;
                    assert_eq!(declared.nil, legacy_nil(&node), "{node:?}");
                }
                (Ok(()), Err(error)) => panic!("{node:?}: {error:?}"),
            }
        }
    }
    assert!(matched > 250 && refused > 500, "{matched} {refused}");
}

#[test]
fn every_xml_schema_fixture_node_matches_before_and_after_migration() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut paths = Vec::new();
    fixtures(&crates, &mut paths);
    let (mut nodes, mut nillable) = (0, 0);
    for path in &paths {
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
            .filter_map(|name| crate::xsd::import_root(path, Some(name)).ok());
        for schema in roots {
            walk(&schema, &mut |node| {
                nodes += 1;
                nillable += usize::from(node.nillable);
                assert_eq!(
                    declaration(node).unwrap().nil,
                    legacy_nil(node),
                    "{}: {node:?}",
                    path.display()
                );
            });
        }
    }
    assert!(nodes > 500, "{nodes}");
    assert!(nillable > 1, "{nillable}");
}

#[test]
fn independently_authored_xml_nil_controls() {
    let value = || SchemaNode::scalar("Value", ScalarType::String);
    let nil_field = |document: Instance| {
        let Instance::Group(fields) = document else {
            panic!("{document:?}")
        };
        fields[0].1.clone()
    };

    // A nillable element reads and writes xsi:nil through both readers.
    let nillable = value().nillable();
    assert_eq!(
        nil_field(read_nil(&nillable).unwrap()),
        Instance::Scalar(Value::xml_nil())
    );
    let structured = from_str_structured(
        &format!("<Root><Value {NIL}/></Root>"),
        &SchemaNode::group("Root", vec![nillable.clone()]),
    )
    .unwrap();
    assert_eq!(nil_field(structured), Instance::Scalar(Value::xml_nil()));
    assert!(write_nil(&nillable).unwrap().contains(r#"xsi:nil="true""#));

    // Neither absence nor JSON null admits xsi:nil.
    let mut optional = value();
    assert!(optional.set_xml_optional(true));
    for node in [value(), optional, value().nullable().unwrap()] {
        assert!(
            matches!(
                read_nil(&node),
                Err(XmlFormatError::UnexpectedXmlNil { .. })
            ),
            "{node:?}"
        );
        assert!(
            matches!(
                write_nil(&node),
                Err(XmlFormatError::UnexpectedXmlNil { .. })
            ),
            "{node:?}"
        );
    }

    // A nil element still may not carry content.
    let root = SchemaNode::group("Root", vec![nillable]);
    assert!(matches!(
        from_str(&format!("<Root><Value {NIL}>x</Value></Root>"), &root),
        Err(XmlFormatError::XmlNilWithContent { .. })
    ));
}
