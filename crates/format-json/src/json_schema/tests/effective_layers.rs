//! Migration controls for rendering JSON null layers from the effective view.
//!
//! The pre-migration flag reading is retained as an oracle and compared with
//! the view-derived layers over every role-flag combination and every JSON
//! Schema fixture in the workspace.

use std::path::{Path, PathBuf};

use ir::{EffectiveViewError, ScalarType, ScalarTypeSet, SchemaKind, SchemaNode};

use super::super::render::{JsonLayers, layers};
use crate::JsonFormatError;

/// The hand-derived layer flags replaced by the effective JSON view.
fn legacy_layers(node: &SchemaNode) -> JsonLayers {
    JsonLayers {
        container_null: node.container_nullable,
        array: node.repeating,
        object_item_null: node.repeating
            && matches!(node.kind, SchemaKind::Group { .. })
            && !node.json_array_wrapper
            && !node.json_any
            && node.json_single_container_nullable(),
        value_null: node.is_scalar() && node.nullable,
    }
}

fn shapes() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("x", ScalarType::Int),
        SchemaNode::scalar("x", ScalarType::String),
        SchemaNode::scalar_union(
            "x",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        ),
        SchemaNode::group("x", vec![]),
        SchemaNode::group("x", vec![SchemaNode::scalar("id", ScalarType::Int)]),
        SchemaNode::group(
            "x",
            vec![
                SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int)
                    .repeating()
                    .nullable()
                    .unwrap(),
            ],
        ),
    ]
}

#[test]
fn every_role_flag_combination_matches_or_refuses_contradictions() {
    let (mut matched, mut refused) = (0, 0);
    for shape in shapes() {
        for bits in 0u32..(1 << 7) {
            for item in [None, Some(false), Some(true)] {
                let mut node = shape.clone();
                node.repeating = bits & 1 != 0;
                node.nullable = bits & 2 != 0;
                node.container_nullable = bits & 4 != 0;
                node.json_array_wrapper = bits & 8 != 0;
                node.json_any = bits & 16 != 0;
                // XML-only metadata, including contradictory roles, is not
                // consulted by the JSON boundary.
                node.attribute = bits & 32 != 0;
                node.xml_optional = bits & 64 != 0;
                node.json_object_item_nullable = item;
                match node.effective_json_nullability() {
                    Ok(_) => {
                        matched += 1;
                        assert_eq!(layers(&node).unwrap(), legacy_layers(&node), "{node:?}");
                    }
                    Err(_) => {
                        refused += 1;
                        assert!(layers(&node).is_err(), "{node:?}");
                        // Wrapper and object-item metadata were already refused
                        // before rendering; the remaining contradictions are now
                        // refused instead of rendered from one of their flags.
                        let expected = if !node.json_object_item_nullable_is_valid() {
                            "InvalidObjectItemNullabilityMetadata"
                        } else if !node.json_array_wrapper_is_valid() {
                            "InvalidArrayWrapperMetadata"
                        } else {
                            "InvalidNullabilityMetadata"
                        };
                        let rendered = format!("{:?}", super::super::export(&node));
                        assert!(
                            rendered.starts_with(&format!("Err({expected}")),
                            "{node:?}: {rendered}"
                        );
                    }
                }
            }
        }
    }
    assert!(matched > 200 && refused > 200, "{matched} {refused}");
}

fn export_text(schema: &SchemaNode) -> serde_json::Value {
    serde_json::from_str(&super::super::export(schema).unwrap()).unwrap()
}

#[test]
fn independently_authored_null_layer_controls() {
    let int = || SchemaNode::scalar("x", ScalarType::Int);
    let object = || SchemaNode::group("x", vec![]);
    let mut both = int().repeating().nullable().unwrap();
    both.container_nullable = true;
    let mut object_items = object().repeating();
    object_items.container_nullable = true;
    object_items.json_object_item_nullable = Some(false);
    let mut item_only = object().repeating();
    item_only.json_object_item_nullable = Some(true);
    let wrapper = SchemaNode::group(
        "x",
        vec![
            SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int)
                .repeating()
                .nullable()
                .unwrap(),
        ],
    )
    .repeating()
    .with_json_array_wrapper()
    .unwrap()
    .nullable_container()
    .unwrap();
    let mut xml_contradiction = int().attribute();
    xml_contradiction.xml_optional = true;
    let closed =
        serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false});
    let json_null = serde_json::json!({"type": "null"});
    let cases = [
        (int(), serde_json::json!({"title": "x", "type": "integer"})),
        (
            int().nullable().unwrap(),
            serde_json::json!({"title": "x", "type": ["integer", "null"]}),
        ),
        (
            object().nullable_container().unwrap(),
            serde_json::json!({"title": "x", "anyOf": [closed, json_null]}),
        ),
        (
            both,
            serde_json::json!({"title": "x", "anyOf": [
                {"type": "array", "items": {"type": ["integer", "null"]}}, json_null
            ]}),
        ),
        (
            object_items,
            serde_json::json!({"title": "x", "anyOf": [
                {"type": "array", "items": closed}, json_null
            ]}),
        ),
        (
            item_only,
            serde_json::json!({"title": "x", "type": "array", "items": {"anyOf": [closed, json_null]}}),
        ),
        (
            wrapper,
            serde_json::json!({"title": "x", "anyOf": [
                {"type": "array", "items": {"type": "array", "items": {"type": ["integer", "null"]}}},
                json_null
            ]}),
        ),
        (
            xml_contradiction,
            serde_json::json!({"title": "x", "type": "integer"}),
        ),
    ];
    for (schema, expected) in cases {
        assert_eq!(export_text(&schema), expected, "{schema:?}");
    }

    let mut group_scalar_null = object();
    group_scalar_null.nullable = true;
    let mut scalar_container = int();
    scalar_container.container_nullable = true;
    for (schema, reason) in [
        (
            group_scalar_null,
            EffectiveViewError::ScalarNullOutsideScalar,
        ),
        (
            scalar_container,
            EffectiveViewError::ContainerNullOutsideContainer,
        ),
    ] {
        let error = super::super::export(&schema).unwrap_err();
        assert!(
            matches!(
                error,
                JsonFormatError::InvalidNullabilityMetadata { ref name, reason: actual }
                    if name == "x" && actual == reason
            ),
            "{error:?}"
        );
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
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path.components().any(|part| part.as_os_str() == "fixtures")
        {
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
fn every_workspace_json_schema_fixture_matches_before_and_after_migration() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut paths = Vec::new();
    fixtures(&crates, &mut paths);
    let (mut schemas, mut nodes, mut nulls) = (0, 0, 0);
    for path in &paths {
        let mut found = Vec::new();
        if let Ok(schema) = super::import(path) {
            found.push(schema);
        }
        // Saved project fixtures embed their boundary schemas directly.
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(serde_json::Value::Object(project)) = serde_json::from_str(&text)
        {
            let mut embedded = vec![project.get("source"), project.get("target")];
            for key in ["extra_sources", "extra_targets"] {
                if let Some(serde_json::Value::Array(entries)) = project.get(key) {
                    embedded.extend(entries.iter().map(|entry| entry.get("schema")));
                }
            }
            found.extend(
                embedded
                    .into_iter()
                    .flatten()
                    .filter_map(|value| serde_json::from_value::<SchemaNode>(value.clone()).ok()),
            );
        }
        for schema in found {
            schemas += 1;
            walk(&schema, &mut |node| {
                nodes += 1;
                let legacy = legacy_layers(node);
                nulls += usize::from(
                    legacy.container_null || legacy.object_item_null || legacy.value_null,
                );
                assert_eq!(
                    layers(node).unwrap(),
                    legacy,
                    "{}: {node:?}",
                    path.display()
                );
            });
        }
    }
    assert!(schemas > 100, "{schemas}");
    assert!(nodes > 2_000, "{nodes}");
    assert!(nulls > 50, "{nulls}");
}
