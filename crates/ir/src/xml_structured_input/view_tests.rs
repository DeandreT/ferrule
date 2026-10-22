//! Migration controls for the structured XML input profile's role proof.
//!
//! The root presence and `xsi:nil` checks now read the effective XML view.
//! The previous flag clauses are retained as an oracle and compared over
//! every role-flag combination and every saved schema node in the workspace.

use std::path::Path;

use super::{supported, xml_roles_supported};
use crate::{
    ScalarType, ScalarTypeSet, SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_TEXT_FIELD,
    XmlNamespace, primary_root_ncname_is_valid, xml_structured_document_input_is_supported,
};

/// The previous hand-read role clauses of the profile.
fn legacy_roles_supported(schema: &SchemaNode, root: bool) -> bool {
    !((root && (schema.attribute || schema.text || schema.xml_optional))
        || (schema.attribute && (schema.text || schema.repeating || schema.nillable))
        || (schema.text && (schema.repeating || schema.nillable || schema.xml_namespace.is_some()))
        || (schema.xml_optional && (schema.attribute || schema.text || schema.repeating))
        || (schema.xml_attribute_required && !schema.attribute)
        || (matches!(schema.kind, SchemaKind::Group { .. }) && schema.nillable))
}

/// The profile refuses this node whatever its XML roles: an invalid name,
/// a scalar union or an attribute group.
fn refused_independently(schema: &SchemaNode) -> bool {
    let text = schema.text && schema.name == XML_TEXT_FIELD;
    (!text && !primary_root_ncname_is_valid(&schema.name))
        || (schema.text && !text)
        || matches!(schema.kind, SchemaKind::ScalarUnion { .. })
        || (schema.attribute && matches!(schema.kind, SchemaKind::Group { .. }))
}

/// Matches the previous clauses wherever the XML view exists; contradictory
/// roles are refused, which the profile already did for every one of them.
fn compare(node: &SchemaNode, root: bool) -> bool {
    let derived = xml_roles_supported(node, root);
    if node.effective_xml_view().is_ok() {
        assert_eq!(
            derived,
            legacy_roles_supported(node, root),
            "{root} {node:?}"
        );
        true
    } else {
        assert!(!derived, "{node:?}");
        assert!(
            !legacy_roles_supported(node, root) || refused_independently(node),
            "{root} {node:?}"
        );
        assert!(!supported(node, root, 1, &mut 0), "{node:?}");
        false
    }
}

fn shapes() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("Value", ScalarType::String),
        SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String),
        SchemaNode::scalar(XML_ELEMENTS_FIELD, ScalarType::String),
        SchemaNode::scalar("", ScalarType::String),
        SchemaNode::scalar("\u{1f}Value", ScalarType::String),
        SchemaNode::scalar_union(
            "Value",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        ),
        SchemaNode::group("Value", vec![]),
        SchemaNode::group("Value", vec![SchemaNode::scalar("Leaf", ScalarType::Int)]),
    ]
}

#[test]
fn every_role_flag_combination_matches_or_refuses_contradictions() {
    let (mut matched, mut refused) = (0, 0);
    for shape in shapes() {
        for bits in 0u32..(1 << 9) {
            let mut node = shape.clone();
            node.repeating = bits & 1 != 0;
            node.attribute = bits & 2 != 0;
            node.text = bits & 4 != 0;
            node.nillable = bits & 8 != 0;
            node.xml_optional = bits & 16 != 0;
            node.xml_attribute_required = bits & 32 != 0;
            if bits & 64 != 0 {
                node.xml_namespace = XmlNamespace::qualified("urn:x");
            }
            // JSON-only metadata is not consulted by the role proof.
            node.nullable = bits & 128 != 0;
            node.container_nullable = bits & 256 != 0;
            for root in [false, true] {
                if compare(&node, root) {
                    matched += 1;
                } else {
                    refused += 1;
                }
            }
        }
    }
    assert!(matched > 2_000 && refused > 2_000, "{matched} {refused}");
}

fn walk(node: &SchemaNode, root: bool, visit: &mut impl FnMut(&SchemaNode, bool)) {
    visit(node, root);
    if let SchemaKind::Group {
        children, dynamic, ..
    } = &node.kind
    {
        for child in children {
            walk(child, false, visit);
        }
        if let Some(dynamic) = dynamic {
            walk(dynamic, false, visit);
        }
    }
}

/// Saved schemas embedded anywhere in one JSON value.
fn embedded(value: &serde_json::Value, found: &mut Vec<SchemaNode>) {
    match value {
        serde_json::Value::Object(object) => {
            if object.contains_key("name")
                && object.contains_key("kind")
                && let Ok(schema) = serde_json::from_value::<SchemaNode>(value.clone())
            {
                found.push(schema);
                return;
            }
            for nested in object.values() {
                embedded(nested, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                embedded(item, found);
            }
        }
        _ => {}
    }
}

fn json_files(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        let skipped = path.file_name().is_some_and(|name| {
            name.to_string_lossy().starts_with('.') || name == "target" || name == "node_modules"
        });
        if skipped {
            continue;
        }
        if path.is_dir() {
            json_files(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            found.push(path);
        }
    }
}

#[test]
fn every_saved_workspace_schema_node_matches_before_and_after_migration() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for directory in ["crates", "conformance", "examples", "docs"] {
        json_files(&workspace.join(directory), &mut paths);
    }
    let (mut schemas, mut nodes, mut roles) = (0, 0, 0);
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let mut found = Vec::new();
        embedded(&value, &mut found);
        for schema in found {
            schemas += 1;
            walk(&schema, true, &mut |node, root| {
                nodes += 1;
                roles += usize::from(node.nillable || node.xml_optional || node.attribute);
                compare(node, root);
            });
        }
    }
    assert!(schemas > 600, "{schemas}");
    assert!(nodes > 3_500, "{nodes}");
    assert!(roles > 100, "{roles}");
}

#[test]
fn independently_authored_structured_input_role_controls() {
    let leaf = || SchemaNode::scalar("Leaf", ScalarType::String);
    let document = |child: SchemaNode| SchemaNode::group("Root", vec![child]);
    assert!(xml_structured_document_input_is_supported(
        &document(leaf())
    ));

    // Scalar elements may be nil or absent; nil attributes, nil groups and an
    // absent document root are outside the profile.
    assert!(xml_structured_document_input_is_supported(&document(
        leaf().nillable()
    )));
    let mut optional = leaf();
    assert!(optional.set_xml_optional(true));
    assert!(xml_structured_document_input_is_supported(&document(
        optional
    )));
    let mut attribute = leaf().attribute();
    assert!(xml_structured_document_input_is_supported(&document(
        attribute.clone()
    )));
    attribute.nillable = true;
    assert!(!xml_structured_document_input_is_supported(&document(
        attribute
    )));
    let nil_group = SchemaNode::group("Leaf", vec![leaf()]).nillable();
    assert!(!xml_structured_document_input_is_supported(&document(
        nil_group
    )));
    let mut optional_root = document(leaf());
    assert!(optional_root.set_xml_optional(true));
    assert!(!xml_structured_document_input_is_supported(&optional_root));

    // JSON null is foreign metadata here, never XML nil or absence.
    assert!(!xml_structured_document_input_is_supported(&document(
        leaf().nullable().unwrap()
    )));

    // A contradictory role is refused rather than read as one of its flags.
    let mut required_element = leaf();
    required_element.xml_attribute_required = true;
    assert!(!xml_structured_document_input_is_supported(&document(
        required_element
    )));
}
