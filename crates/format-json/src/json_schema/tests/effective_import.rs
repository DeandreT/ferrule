//! Migration controls for JSON Schema import reading the effective null
//! layers.
//!
//! `allOf` intersection, nullable array and object-item composition, scalar
//! array containment, `oneOf` null overlap, scalar type siblings, exact
//! nullable scalar alternatives, constrained nullability merges,
//! discriminator constraints, presence conditionals and allowed `null` values
//! now read value, container and single-occurrence null from the shared
//! layers. The previous flag reads are retained as oracles and compared over
//! every role-flag combination and every workspace JSON Schema and
//! saved-project fixture node.

use ir::{ScalarType, ScalarTypeSet, SchemaKind, SchemaNode};

use super::super::all_of::intersect_null_admission;
use super::effective_layers::{walk, workspace_fixture_schemas};
use crate::JsonFormatError;
use crate::null_layers::{container_null, layers, single_null, value_null};

/// The previous `allOf` null merge.
fn legacy_intersect_null_admission(target: &mut SchemaNode, branch: &SchemaNode) {
    let explicit_object_items =
        target.json_object_item_nullable.is_some() || branch.json_object_item_nullable.is_some();
    let object_items_nullable =
        target.json_single_container_nullable() && branch.json_single_container_nullable();
    target.nullable &= branch.nullable;
    target.container_nullable &= branch.container_nullable;
    if explicit_object_items {
        target.json_object_item_nullable =
            (target.container_nullable || object_items_nullable).then_some(object_items_nullable);
    }
}

/// Whether the importer consults the single-occurrence fact for `node`: object
/// shapes that are neither nested-array frames nor arbitrary JSON.
fn single_consulted(node: &SchemaNode) -> bool {
    matches!(node.kind, SchemaKind::Group { .. }) && !node.json_array_wrapper && !node.json_any
}

/// Compares every fact on one node with the flag it replaced; returns whether
/// the node has an effective JSON view.
fn compare_facts(node: &SchemaNode, context: &str) -> bool {
    match layers(node) {
        Ok(_) => {
            assert_eq!(
                value_null(node).unwrap(),
                node.nullable,
                "{context}: {node:?}"
            );
            assert_eq!(
                container_null(node).unwrap(),
                node.container_nullable,
                "{context}: {node:?}"
            );
            if single_consulted(node) {
                assert_eq!(
                    single_null(node).unwrap(),
                    node.json_single_container_nullable(),
                    "{context}: {node:?}"
                );
            }
            true
        }
        Err(error) => {
            // Engine validation rejects every role the view refuses.
            assert!(
                !node.nullable_is_valid()
                    || !node.container_nullable_is_valid()
                    || !node.json_object_item_nullable_is_valid()
                    || !node.json_array_wrapper_is_valid(),
                "{context}: {node:?}"
            );
            for fact in [value_null, container_null, single_null] {
                assert_eq!(
                    format!("{:?}", fact(node)),
                    format!("{:?}", Err::<bool, _>(&error))
                );
            }
            false
        }
    }
}

fn shapes() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("x", ScalarType::Int),
        SchemaNode::scalar_union(
            "x",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        ),
        SchemaNode::group("x", vec![]),
        SchemaNode::group("x", vec![SchemaNode::scalar("id", ScalarType::Int)]),
        SchemaNode::group(
            "x",
            vec![SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int).repeating()],
        ),
    ]
}

/// Every role-flag combination of `shape`, including XML-only metadata.
fn combinations(shape: &SchemaNode) -> Vec<SchemaNode> {
    let mut nodes = Vec::new();
    for bits in 0u32..(1 << 7) {
        for item in [None, Some(false), Some(true)] {
            let mut node = shape.clone();
            node.repeating = bits & 1 != 0;
            node.nullable = bits & 2 != 0;
            node.container_nullable = bits & 4 != 0;
            node.json_array_wrapper = bits & 8 != 0;
            node.json_any = bits & 16 != 0;
            node.nillable = bits & 32 != 0;
            node.xml_optional = bits & 64 != 0;
            node.json_object_item_nullable = item;
            nodes.push(node);
        }
    }
    nodes
}

#[test]
fn every_role_flag_combination_matches_or_refuses_contradictions() {
    let (mut matched, mut refused) = (0, 0);
    for shape in shapes() {
        for node in combinations(&shape) {
            if compare_facts(&node, "combination") {
                matched += 1;
            } else {
                refused += 1;
            }
        }
    }
    assert!(matched > 200 && refused > 1_000, "{matched} {refused}");
}

#[test]
fn every_all_of_branch_pair_merges_null_as_before() {
    let mut pairs = 0;
    for shape in shapes() {
        let nodes = combinations(&shape)
            .into_iter()
            // Engine validation also rejects arbitrary JSON outside one
            // singular string scalar, which never reaches an `allOf` merge.
            .filter(|node| layers(node).is_ok() && node.json_any_is_valid())
            .collect::<Vec<_>>();
        for target in &nodes {
            // `allOf` merges null admission only between branches of equal
            // array and nested-array shape.
            for branch in nodes.iter().filter(|branch| {
                branch.repeating == target.repeating
                    && branch.json_array_wrapper == target.json_array_wrapper
            }) {
                let mut legacy = target.clone();
                legacy_intersect_null_admission(&mut legacy, branch);
                let mut migrated = target.clone();
                intersect_null_admission(&mut migrated, branch).unwrap();
                assert_eq!(migrated, legacy, "{target:?} {branch:?}");
                pairs += 1;
            }
        }
    }
    assert!(pairs > 1_000, "{pairs}");
}

#[test]
fn every_workspace_json_fixture_node_matches_before_and_after_migration() {
    let (mut nodes, mut nulls) = (0, 0);
    for (path, schema) in workspace_fixture_schemas() {
        walk(&schema, &mut |node| {
            nodes += 1;
            nulls += usize::from(
                node.nullable
                    || node.container_nullable
                    || node.json_object_item_nullable.is_some(),
            );
            assert!(
                compare_facts(node, &path.display().to_string()),
                "{}",
                path.display()
            );
        });
    }
    assert!(nodes > 2_000, "{nodes}");
    assert!(nulls > 50, "{nulls}");
}

fn import(schema: serde_json::Value) -> Result<SchemaNode, JsonFormatError> {
    super::super::import_str(&schema.to_string())
}

fn property<'a>(schema: &'a SchemaNode, name: &str) -> &'a SchemaNode {
    schema.child(name).unwrap()
}

#[test]
fn independently_authored_import_null_controls() {
    let object = |properties: serde_json::Value| serde_json::json!({"type": "object", "properties": properties});
    // allOf admits null at a layer only where every branch does.
    let both = import(object(serde_json::json!({"x": {"allOf": [
        {"type": ["string", "null"]}, {"type": ["string", "null"]}
    ]}})))
    .unwrap();
    assert!(value_null(property(&both, "x")).unwrap());
    let one = import(object(serde_json::json!({"x": {"allOf": [
        {"type": ["string", "null"]}, {"type": "string"}
    ]}})))
    .unwrap();
    assert!(!value_null(property(&one, "x")).unwrap());
    let arrays = import(object(serde_json::json!({"x": {"allOf": [
        {"type": ["array", "null"], "items": {"type": ["object", "null"]}},
        {"type": "array", "items": {"type": ["object", "null"]}}
    ]}})))
    .unwrap();
    let x = property(&arrays, "x");
    assert!(!container_null(x).unwrap());
    assert!(single_null(x).unwrap());

    // A nullable array of objects keeps its items' own null admission.
    let items = import(object(serde_json::json!({"x": {
        "type": ["array", "null"], "items": {"type": "object"}
    }})))
    .unwrap();
    let x = property(&items, "x");
    assert!(container_null(x).unwrap());
    assert!(!single_null(x).unwrap());

    // An allowed `null` value requires a nullable scalar.
    assert!(
        import(object(
            serde_json::json!({"x": {"type": ["string", "null"], "enum": ["a", null]}})
        ))
        .is_ok()
    );
    assert!(
        import(object(
            serde_json::json!({"x": {"type": ["string", "null"], "enum": ["a", null]}})
        ))
        .map(|schema| value_null(property(&schema, "x")).unwrap())
        .unwrap()
    );
}
