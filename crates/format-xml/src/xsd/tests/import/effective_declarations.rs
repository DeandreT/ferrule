//! Migration controls for XSD import reading the effective XML declaration.
//!
//! Restriction narrowing, prohibited required attributes, xsi:type member
//! profiles, required attributes with defaults and substitution-group head
//! nil checks now derive from the shared declaration. The previous flag
//! readings are retained as oracles and compared over every role-flag
//! combination and every node of every XSD fixture root.

use std::path::Path;

use ir::{ScalarType, SchemaNode, XML_TEXT_FIELD};

use super::super::export::effective_declarations::{fixtures, walk};
use crate::XmlFormatError;
use crate::declaration::{declaration, optional_element, required_attribute, restriction_widens};

/// The previous restriction occurrence and nil check.
fn legacy_restriction_widens(base: &SchemaNode, restricted: &SchemaNode) -> bool {
    (!base.repeating && restricted.repeating)
        || (!base.xml_optional && restricted.xml_optional)
        || (base.xml_attribute_required && !restricted.xml_attribute_required)
        || (!base.nillable && restricted.nillable)
}

/// The previous attribute-use, optional-element and nil readings.
fn legacy_facts(node: &SchemaNode) -> (bool, bool, bool) {
    (
        node.xml_attribute_required,
        node.xml_optional,
        node.nillable,
    )
}

fn derived_facts(node: &SchemaNode) -> Result<(bool, bool, bool), XmlFormatError> {
    Ok((
        required_attribute(node)?,
        optional_element(node)?,
        declaration(node)?.nil,
    ))
}

fn shapes() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("Value", ScalarType::String),
        SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String),
        SchemaNode::group("Value", vec![SchemaNode::scalar("Leaf", ScalarType::Int)]),
    ]
}

/// One node's XML occurrence roles from four bits; JSON-only metadata is
/// never consulted by the XML declaration.
fn with_roles(shape: &SchemaNode, bits: u32, attribute: bool, text: bool) -> SchemaNode {
    let mut node = shape.clone();
    node.attribute = attribute;
    node.text = text;
    node.repeating = bits & 1 != 0;
    node.xml_optional = bits & 2 != 0;
    node.xml_attribute_required = bits & 4 != 0;
    node.nillable = bits & 8 != 0;
    node.nullable = bits & 1 != 0;
    node.container_nullable = bits & 2 != 0;
    node
}

#[test]
fn every_role_flag_combination_matches_or_refuses_contradictions() {
    let (mut matched, mut refused, mut pairs, mut refused_pairs) = (0, 0, 0, 0);
    for shape in shapes() {
        for roles in 0u32..4 {
            let (attribute, text) = (roles & 1 != 0, roles & 2 != 0);
            for base_bits in 0u32..16 {
                let base = with_roles(&shape, base_bits, attribute, text);
                match derived_facts(&base) {
                    Ok(facts) => {
                        matched += 1;
                        assert_eq!(facts, legacy_facts(&base), "{base:?}");
                    }
                    Err(_) => {
                        refused += 1;
                        assert!(
                            !base.xml_optional_is_valid()
                                || !base.xml_attribute_required_is_valid(),
                            "{base:?}"
                        );
                    }
                }
                for restricted_bits in 0u32..16 {
                    let restricted = with_roles(&shape, restricted_bits, attribute, text);
                    match restriction_widens(&base, &restricted) {
                        Ok(widens) => {
                            pairs += 1;
                            assert_eq!(
                                widens,
                                legacy_restriction_widens(&base, &restricted),
                                "{base:?} {restricted:?}"
                            );
                        }
                        // Either side without a view has no restriction.
                        Err(_) => {
                            refused_pairs += 1;
                            assert!(
                                declaration(&base).is_err() || declaration(&restricted).is_err()
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(matched > 50 && refused > 100, "{matched} {refused}");
    assert!(
        pairs > 200 && refused_pairs > 1_000,
        "{pairs} {refused_pairs}"
    );
}

#[test]
fn every_xml_schema_fixture_node_matches_before_and_after_migration() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut paths = Vec::new();
    fixtures(&crates, &mut paths);
    let (mut nodes, mut qualified, mut variants) = (0, 0, 0);
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
                qualified +=
                    usize::from(node.nillable || node.xml_optional || node.xml_attribute_required);
                assert_eq!(
                    derived_facts(node).unwrap(),
                    legacy_facts(node),
                    "{}: {node:?}",
                    path.display()
                );
                // Each fixture declaration restricted by every role variant
                // of itself.
                for bits in 0u32..16 {
                    let restricted = with_roles(node, bits, node.attribute, node.text);
                    if let Ok(widens) = restriction_widens(node, &restricted) {
                        variants += 1;
                        assert_eq!(
                            widens,
                            legacy_restriction_widens(node, &restricted),
                            "{}: {node:?} {restricted:?}",
                            path.display()
                        );
                    }
                }
            });
        }
    }
    assert!(nodes > 500, "{nodes}");
    assert!(qualified > 40, "{qualified}");
    assert!(variants > 2_000, "{variants}");
}

struct Schema(std::path::PathBuf);

impl Schema {
    fn new(label: &str, text: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_effective_{label}_{}.xsd",
            std::process::id()
        ));
        std::fs::write(&path, text).unwrap();
        Self(path)
    }
}

impl Drop for Schema {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Imports an envelope whose `record` field may carry the `Restricted`
/// xsi:type view of `Record`, and returns that view's alternatives.
fn restricted_alternatives(
    label: &str,
    base: &str,
    restricted: &str,
) -> Result<Vec<String>, XmlFormatError> {
    let schema = Schema::new(
        label,
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:effective" targetNamespace="urn:ferrule:effective"
                elementFormDefault="qualified">
              <xs:complexType name="Record">{base}</xs:complexType>
              <xs:complexType name="Restricted"><xs:complexContent>
                <xs:restriction base="t:Record">{restricted}</xs:restriction>
              </xs:complexContent></xs:complexType>
              <xs:element name="Envelope"><xs:complexType><xs:sequence>
                <xs:element name="record" type="t:Record"/>
              </xs:sequence></xs:complexType></xs:element>
            </xs:schema>"#
        ),
    );
    let envelope = crate::xsd::import_root(&schema.0, Some("{urn:ferrule:effective}Envelope"))?;
    Ok(envelope
        .child("record")
        .map(|record| {
            record
                .alternatives()
                .iter()
                .map(|alternative| alternative.name.clone())
                .collect()
        })
        .unwrap_or_default())
}

fn refused_restriction(result: &Result<Vec<String>, XmlFormatError>, reason: &str) -> bool {
    matches!(
        result,
        Err(XmlFormatError::UnsupportedComplexContentRestriction { reason: actual, .. })
            if *actual == reason
    )
}

#[test]
fn independently_authored_xsd_declaration_controls() {
    let element = |occurs: &str, nillable: bool| {
        format!(
            r#"<xs:sequence><xs:element name="note" type="xs:string" {occurs} nillable="{nillable}"/></xs:sequence>"#
        )
    };
    let particle = "a restricted particle changes an incompatible field shape or widens repetition";
    // An unchanged restriction becomes an xsi:type view.
    assert_eq!(
        restricted_alternatives("same", &element("", false), &element("", false)).unwrap(),
        [
            "{urn:ferrule:effective}Record",
            "{urn:ferrule:effective}Restricted"
        ]
    );
    // Making an optional element required is a restriction, but the flattened
    // views then disagree on that member's optional occurrence.
    assert!(matches!(
        restricted_alternatives(
            "narrow-optional",
            &element(r#"minOccurs="0""#, false),
            &element("", false)
        ),
        Err(XmlFormatError::UnsupportedXmlAlternativeOccurrence { ref field, .. }) if field == "note"
    ));
    assert!(restricted_alternatives("narrow-nil", &element("", true), &element("", false)).is_ok());
    // Making a required element optional or nillable, or repeating a singular
    // one, widens it.
    for (label, widened) in [
        ("widen-optional", element(r#"minOccurs="0""#, false)),
        ("widen-nil", element("", true)),
        ("widen-repeat", element(r#"maxOccurs="unbounded""#, false)),
    ] {
        let result = restricted_alternatives(label, &element("", false), &widened);
        assert!(
            refused_restriction(&result, particle),
            "{label}: {result:?}"
        );
    }

    // A required attribute may be neither made optional nor prohibited; an
    // optional one may be prohibited, and requiring it changes its use.
    let attribute = |usage: &str| {
        format!(r#"<xs:sequence/><xs:attribute name="code" type="xs:string" use="{usage}"/>"#)
    };
    assert!(
        restricted_alternatives(
            "attribute-prohibit-optional",
            &attribute("optional"),
            &attribute("prohibited")
        )
        .is_ok()
    );
    assert!(matches!(
        restricted_alternatives("attribute-narrow", &attribute("optional"), &attribute("required")),
        Err(XmlFormatError::UnsupportedXmlAlternativeAttributeUse { ref field, .. }) if field == "code"
    ));
    let result = restricted_alternatives(
        "attribute-widen",
        &attribute("required"),
        &attribute("optional"),
    );
    assert!(
        refused_restriction(
            &result,
            "a restricted attribute changes an incompatible field shape"
        ),
        "{result:?}"
    );
    let result = restricted_alternatives(
        "attribute-prohibit-required",
        &attribute("required"),
        &attribute("prohibited"),
    );
    assert!(
        refused_restriction(
            &result,
            "a restriction cannot prohibit a required attribute"
        ),
        "{result:?}"
    );

    // A required attribute cannot also carry a default.
    let schema = Schema::new(
        "attribute-default",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Root"><xs:complexType>
            <xs:attribute name="code" type="xs:string" use="required" default="x"/>
          </xs:complexType></xs:element>
        </xs:schema>"#,
    );
    assert!(matches!(
        crate::xsd::import_root(&schema.0, Some("Root")),
        Err(XmlFormatError::UnsupportedXmlAttributeDefault { .. })
    ));

    // A nillable complex substitution head is not executable.
    let head = |nillable: bool| {
        Schema::new(
            &format!("substitution-{nillable}"),
            &format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
                  <xs:complexType name="AnimalType"><xs:sequence>
                    <xs:element name="Name" type="xs:string"/>
                  </xs:sequence></xs:complexType>
                  <xs:element name="Animal" type="AnimalType" nillable="{nillable}"/>
                  <xs:element name="Cat" type="AnimalType" substitutionGroup="Animal"/>
                  <xs:element name="Zoo"><xs:complexType><xs:sequence>
                    <xs:element ref="Animal"/>
                  </xs:sequence></xs:complexType></xs:element>
                </xs:schema>"#
            ),
        )
    };
    assert!(crate::xsd::import_root(&head(false).0, Some("Zoo")).is_ok());
    assert!(matches!(
        crate::xsd::import_root(&head(true).0, Some("Zoo")),
        Err(XmlFormatError::UnsupportedSubstitutionGroup { .. })
    ));
}
