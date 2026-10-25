//! Migration controls for the remaining JSON Schema null reads.
//!
//! The `multipleOf` and `pattern` disjunctions render their scalar null branch
//! from the effective value layer, and the range validator admits null from
//! that layer. The `multipleOf` and `uniqueItems` validators no longer read
//! null flags: null is neither a number, an array nor an object, so their
//! previous null fast paths could not change a result. Each previous reading
//! is retained as an oracle over every role-flag combination and every
//! workspace JSON Schema fixture.

use ir::{
    IntegerRange, JsonMultipleOf, JsonMultipleOfConstraints, JsonPatternConstraints, NumericRange,
    ScalarType, ScalarTypeSet, SchemaKind, SchemaNode,
};

use super::super::{multiples, ranges, unique_items};
use super::effective_layers::{walk, workspace_fixture_schemas};
use crate::null_layers::layers;
use crate::{JsonFormatError, RecursiveSchemas};

/// The range validator's previous null admission.
fn legacy_range_admits_null(node: &SchemaNode) -> bool {
    node.nullable
}

/// The `uniqueItems` validator's previous null fast paths, at the array or
/// object occurrence and at each of its items.
fn legacy_unique_items_shortcut(node: &SchemaNode) -> bool {
    node.container_nullable || node.json_any || node.json_single_container_nullable()
}

fn divisors() -> JsonMultipleOfConstraints {
    let two = JsonMultipleOf::from_decimal_lexical("2").unwrap();
    let three = JsonMultipleOf::from_decimal_lexical("3").unwrap();
    JsonMultipleOfConstraints::new([[two], [three]]).unwrap()
}

fn scalar_shapes() -> Vec<SchemaNode> {
    let union = || {
        SchemaNode::scalar_union(
            "x",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        )
    };
    let range = NumericRange::Integer(IntegerRange::new(Some(2), Some(9)).unwrap());
    vec![
        SchemaNode::scalar("x", ScalarType::Int)
            .with_numeric_range(range)
            .unwrap(),
        SchemaNode::scalar("x", ScalarType::Int)
            .with_json_multiple_of(divisors())
            .unwrap(),
        union().with_json_multiple_of(divisors()).unwrap(),
        union()
            .with_json_patterns(JsonPatternConstraints::new([["^A$"], ["^B$"]]).unwrap())
            .unwrap(),
    ]
}

fn group_shapes() -> Vec<SchemaNode> {
    let object = SchemaNode::group(
        "x",
        vec![SchemaNode::scalar("id", ScalarType::Int).repeating()],
    );
    let mut unique_child = object.clone();
    if let SchemaKind::Group { children, .. } = &mut unique_child.kind {
        children[0].json_unique_items = true;
    }
    vec![
        object,
        unique_child,
        SchemaNode::group(
            "x",
            vec![SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int).repeating()],
        ),
    ]
}

/// Every role-flag combination of `shape`, including XML-only metadata.
fn combinations(shape: &SchemaNode) -> impl Iterator<Item = SchemaNode> + '_ {
    (0u32..(1 << 7)).flat_map(move |bits| {
        [None, Some(false), Some(true)].map(|item| {
            let mut node = shape.clone();
            node.repeating = bits & 1 != 0;
            node.nullable = bits & 2 != 0;
            node.container_nullable = bits & 4 != 0;
            node.json_array_wrapper = bits & 8 != 0;
            node.json_any = bits & 16 != 0;
            node.nillable = bits & 32 != 0;
            node.xml_optional = bits & 64 != 0;
            node.json_object_item_nullable = item;
            node.json_unique_items = node.repeating;
            node
        })
    })
}

/// A null reaches the range check after the scalar decode; returns whether the
/// node has an effective value layer.
fn compare_range(node: &SchemaNode) -> bool {
    let result = ranges::validate_json(node, &serde_json::Value::Null);
    match layers(node) {
        Ok(layers) => {
            assert_eq!(
                layers.value_null,
                legacy_range_admits_null(node),
                "{node:?}"
            );
            if legacy_range_admits_null(node) {
                assert!(result.is_ok(), "{node:?}: {result:?}");
            } else {
                assert!(
                    matches!(result, Err(JsonFormatError::RangeMismatch { .. })),
                    "{node:?}: {result:?}"
                );
            }
            true
        }
        Err(expected) => {
            // Contradictory metadata is refused with the layer error; readers
            // and writers refuse it before any value reaches the range check.
            assert_eq!(
                format!("{result:?}"),
                format!("{:?}", Err::<(), _>(expected))
            );
            false
        }
    }
}

/// Null never reaches a uniqueness or child check, so every node validates it
/// exactly as its recursive-schema preflight does.
fn compare_unique_items_null(node: &SchemaNode) -> bool {
    let preflight = RecursiveSchemas::new(node).map(|_| ());
    let decoded = unique_items::validate_json_tree(node, &serde_json::Value::Null);
    assert_eq!(format!("{decoded:?}"), format!("{preflight:?}"), "{node:?}");
    let raw = unique_items::validate_raw_json_unique_items(node, "null");
    if unique_items::tree_has_unique_items(node) {
        assert_eq!(format!("{raw:?}"), format!("{preflight:?}"), "{node:?}");
    } else {
        assert!(raw.is_ok(), "{node:?}");
    }
    legacy_unique_items_shortcut(node)
}

#[test]
fn every_role_flag_combination_matches_or_refuses_contradictions() {
    let (mut ranged, mut refused, mut shortcuts) = (0, 0, 0);
    for shape in scalar_shapes() {
        for node in combinations(&shape) {
            // The previous null fast path admitted null before any divisor
            // check; null is not a number, so it is admitted without one.
            assert!(multiples::validate_json(&node, &serde_json::Value::Null).is_ok());
            if node.numeric_range.is_some() {
                if compare_range(&node) {
                    ranged += 1;
                } else {
                    refused += 1;
                }
            }
            shortcuts += usize::from(compare_unique_items_null(&node));
        }
    }
    for shape in group_shapes() {
        for node in combinations(&shape) {
            shortcuts += usize::from(compare_unique_items_null(&node));
        }
    }
    assert!(ranged > 40 && refused > 200, "{ranged} {refused}");
    assert!(shortcuts > 500, "{shortcuts}");
}

fn exported(schema: &SchemaNode) -> serde_json::Value {
    serde_json::from_str(&super::super::export(schema).unwrap()).unwrap()
}

fn null_branches(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(map) => {
            usize::from(map.len() == 1 && map.get("type") == Some(&"null".into()))
                + map.values().map(null_branches).sum::<usize>()
        }
        serde_json::Value::Array(items) => items.iter().map(null_branches).sum(),
        _ => 0,
    }
}

#[test]
fn independently_authored_null_branch_and_validator_controls() {
    // A scalar value's null is one branch of its own disjunction; container
    // null wraps the array instead.
    for shape in scalar_shapes().into_iter().skip(1) {
        assert_eq!(null_branches(&exported(&shape)), 0, "{shape:?}");
        let nullable = shape.clone().nullable().unwrap();
        assert_eq!(null_branches(&exported(&nullable)), 1, "{nullable:?}");
        let mut container = shape.clone().repeating();
        container.container_nullable = true;
        let container = exported(&container);
        assert_eq!(null_branches(&container), 1, "{container}");
        assert_eq!(
            container.pointer("/anyOf/1"),
            Some(&serde_json::json!({"type": "null"})),
            "{container}"
        );
        // XML nil is not JSON null.
        assert_eq!(null_branches(&exported(&shape.clone().nillable())), 0);
    }

    let range = || scalar_shapes().remove(0);
    assert!(crate::from_str("null", &range().nullable().unwrap()).is_ok());
    assert!(crate::from_str("null", &range()).is_err());
    assert!(matches!(
        crate::from_str("1", &range().nullable().unwrap()),
        Err(JsonFormatError::RangeMismatch { .. })
    ));
    assert!(matches!(
        ranges::validate_json(&range().nillable(), &serde_json::Value::Null),
        Err(JsonFormatError::RangeMismatch { .. })
    ));

    // Null items still take part in uniqueness, and an admitted null array or
    // object item is accepted.
    let mut unique = SchemaNode::scalar("x", ScalarType::Int)
        .repeating()
        .nullable()
        .unwrap();
    unique.json_unique_items = true;
    assert!(matches!(
        crate::from_str("[null, 1, null]", &unique),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(crate::from_str("[null, 1]", &unique).is_ok());
    let mut nullable_array = unique.clone();
    nullable_array.container_nullable = true;
    assert!(crate::from_str("null", &nullable_array).is_ok());
    let mut rows = group_shapes().remove(1).repeating();
    rows.json_object_item_nullable = Some(true);
    let document = SchemaNode::group("Root", vec![rename(rows, "rows")]);
    assert!(crate::from_str(r#"{"rows":[null,{"id":[1,2]}]}"#, &document).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"rows":[null,{"id":[1,1]}]}"#, &document),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
}

fn rename(mut node: SchemaNode, name: &str) -> SchemaNode {
    node.name = name.into();
    node
}

#[test]
fn every_workspace_json_schema_fixture_matches_before_and_after_migration() {
    let (mut nodes, mut ranged, mut shortcuts) = (0, 0, 0);
    for (path, schema) in workspace_fixture_schemas() {
        walk(&schema, &mut |node| {
            nodes += 1;
            assert!(
                multiples::validate_json(node, &serde_json::Value::Null).is_ok(),
                "{}",
                path.display()
            );
            if node.numeric_range.is_some() {
                assert!(compare_range(node), "{}: {node:?}", path.display());
                ranged += 1;
            }
            if node.is_scalar() {
                let value_null = layers(node).unwrap().value_null;
                assert_eq!(value_null, node.nullable, "{}", path.display());
            }
            shortcuts += usize::from(compare_unique_items_null(node));
        });
    }
    assert!(nodes > 2_000, "{nodes}");
    assert!(ranged > 5, "{ranged}");
    assert!(shortcuts > 10, "{shortcuts}");
}
