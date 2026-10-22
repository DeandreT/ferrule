//! Migration controls for the JSON instance boundary's null reads.
//!
//! The reader and both writers now accept and emit `null` from the effective
//! JSON layers. Their previous flag reads are retained as an oracle and
//! compared over every role-flag combination and every workspace JSON Schema
//! and saved-project fixture node; the readers' preflight now also refuses
//! the contradictions the view has no interpretation for.

use ir::{
    EffectiveViewError, GroupAlternative, Instance, ScalarType, ScalarTypeSet, SchemaKind,
    SchemaNode, Value,
};

use super::{InstanceNull, instance_null};
use crate::json_schema::tests::effective_layers::{walk, workspace_fixture_schemas};
use crate::{JsonFormatError, RecursiveSchemas};

/// The previous hand-read null admission of the reader and both writers.
fn legacy(node: &SchemaNode) -> InstanceNull {
    InstanceNull {
        whole: node.container_nullable,
        single: node.json_single_container_nullable(),
        value: node.nullable,
    }
}

/// The previous reader and writer preflight, before the effective view.
fn legacy_preflight_accepts(node: &SchemaNode) -> bool {
    node.json_object_item_nullable_is_valid()
        && node.json_array_wrapper_is_valid()
        && node.xml_attribute_required_is_valid()
}

/// Compares only what the boundary consults: nested-array frames and
/// arbitrary JSON return before the single-occurrence null check.
fn assert_matches_legacy(node: &SchemaNode, context: &str) {
    let derived = instance_null(node).unwrap();
    let legacy = legacy(node);
    assert_eq!(derived.whole, legacy.whole, "{context}: {node:?}");
    assert_eq!(derived.value, legacy.value, "{context}: {node:?}");
    if !node.json_array_wrapper && !node.json_any {
        assert_eq!(derived.single, legacy.single, "{context}: {node:?}");
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

/// Every role-flag combination of `shape`, including XML-only metadata.
fn combinations(shape: &SchemaNode) -> impl Iterator<Item = SchemaNode> + '_ {
    (0u32..(1 << 8)).flat_map(move |bits| {
        [None, Some(false), Some(true)].map(|item| {
            let mut node = shape.clone();
            node.repeating = bits & 1 != 0;
            node.nullable = bits & 2 != 0;
            node.container_nullable = bits & 4 != 0;
            node.json_array_wrapper = bits & 8 != 0;
            node.json_any = bits & 16 != 0;
            node.attribute = bits & 32 != 0;
            node.xml_optional = bits & 64 != 0;
            node.nillable = bits & 128 != 0;
            node.json_object_item_nullable = item;
            node
        })
    })
}

fn root(child: SchemaNode) -> SchemaNode {
    SchemaNode::group("Root", vec![child])
}

#[test]
fn every_role_flag_combination_matches_or_refuses_contradictions() {
    let (mut matched, mut refused) = (0, 0);
    for shape in shapes() {
        for node in combinations(&shape) {
            match RecursiveSchemas::new(&node) {
                Ok(_) => {
                    matched += 1;
                    assert_matches_legacy(&node, "combination");
                }
                Err(error) if legacy_preflight_accepts(&node) => {
                    // Newly refused: scalar null on a group or container null
                    // on a singular scalar, which engine validation rejects.
                    refused += 1;
                    let reason = node.effective_json_nullability().unwrap_err();
                    assert!(
                        matches!(
                            reason,
                            EffectiveViewError::ScalarNullOutsideScalar
                                | EffectiveViewError::ContainerNullOutsideContainer
                        ),
                        "{node:?}"
                    );
                    assert!(
                        !node.nullable_is_valid() || !node.container_nullable_is_valid(),
                        "{node:?}"
                    );
                    assert!(
                        matches!(
                            &error,
                            JsonFormatError::InvalidNullabilityMetadata { name, reason: actual }
                                if name == "x" && *actual == reason
                        ),
                        "{error:?}"
                    );
                    // Every reader and writer entry point refuses it the same way.
                    let document = root(node.clone());
                    for result in [
                        crate::from_str("{}", &document).map(|_| ()),
                        crate::from_json5_str("{}", &document).map(|_| ()),
                        crate::to_string(&document, &Instance::Group(Vec::new().into()))
                            .map(|_| ()),
                        crate::to_value(&document, &Instance::Group(Vec::new().into())).map(|_| ()),
                        crate::PreparedSchema::new(document.clone())
                            .from_str("{}")
                            .map(|_| ()),
                        crate::PreparedSchema::new(document.clone())
                            .to_value(&Instance::Group(Vec::new().into()))
                            .map(|_| ()),
                    ] {
                        assert_eq!(
                            format!("{result:?}"),
                            format!("{:?}", Err::<(), _>(&error)),
                            "{node:?}"
                        );
                    }
                }
                // Already refused before the migration, with the same error.
                Err(_) => {}
            }
        }
    }
    assert!(matched > 500 && refused > 300, "{matched} {refused}");
}

#[test]
fn every_workspace_json_fixture_node_matches_before_and_after_migration() {
    let (mut nodes, mut nulls) = (0, 0);
    for (path, schema) in workspace_fixture_schemas() {
        assert!(RecursiveSchemas::new(&schema).is_ok(), "{}", path.display());
        walk(&schema, &mut |node| {
            nodes += 1;
            let legacy = legacy(node);
            nulls += usize::from(legacy.whole || legacy.single || legacy.value);
            assert_matches_legacy(node, &path.display().to_string());
        });
    }
    assert!(nodes > 2_000, "{nodes}");
    assert!(nulls > 50, "{nulls}");
}

fn field(document: &Instance) -> Option<&Instance> {
    let Instance::Group(fields) = document else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == "x").then_some(value))
}

fn read(child: &SchemaNode, text: &str) -> Result<Instance, String> {
    crate::from_str(text, &root(child.clone()))
        .map_err(|error| format!("{error:?}"))
        .and_then(|document| field(&document).cloned().ok_or_else(String::new))
}

/// Both writers, which must agree.
fn write(child: &SchemaNode, value: Instance) -> Result<serde_json::Value, String> {
    let document = root(child.clone());
    let instance = Instance::Group(vec![("x".to_owned(), value)].into());
    let owned = crate::to_value(&document, &instance).map_err(|error| format!("{error:?}"));
    let borrowed = crate::to_string(&document, &instance)
        .map(|text| serde_json::from_str::<serde_json::Value>(&text).unwrap())
        .map_err(|error| format!("{error:?}"));
    assert_eq!(owned, borrowed, "{child:?}");
    owned.map(|document| {
        document
            .get("x")
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    })
}

fn json_null() -> Instance {
    Instance::Scalar(Value::json_null())
}

fn object_array(container: bool, item: Option<bool>) -> SchemaNode {
    let mut node = SchemaNode::group("x", vec![]).repeating();
    node.container_nullable = container;
    node.json_object_item_nullable = item;
    node
}

#[test]
fn independently_authored_instance_null_controls() {
    let int = || SchemaNode::scalar("x", ScalarType::Int);
    let object = || SchemaNode::group("x", vec![]);

    // Scalar value null, object null and XML nil are separate facts.
    assert_eq!(
        read(&int().nullable().unwrap(), r#"{"x":null}"#),
        Ok(json_null())
    );
    assert!(read(&int(), r#"{"x":null}"#).is_err());
    assert!(read(&int().nillable(), r#"{"x":null}"#).is_err());
    assert_eq!(
        read(&object().nullable_container().unwrap(), r#"{"x":null}"#),
        Ok(json_null())
    );
    assert!(read(&object(), r#"{"x":null}"#).is_err());

    // Array container null and object-item null are independent.
    let container_only = object_array(true, Some(false));
    assert_eq!(read(&container_only, r#"{"x":null}"#), Ok(json_null()));
    assert!(read(&container_only, r#"{"x":[null]}"#).is_err());
    let items_only = object_array(false, Some(true));
    assert!(read(&items_only, r#"{"x":null}"#).is_err());
    assert_eq!(
        read(&items_only, r#"{"x":[null]}"#),
        Ok(Instance::Repeated(vec![json_null()]))
    );
    let scalar_items = int().repeating().nullable().unwrap();
    assert!(read(&scalar_items, r#"{"x":null}"#).is_err());
    assert_eq!(
        read(&scalar_items, r#"{"x":[null]}"#),
        Ok(Instance::Repeated(vec![json_null()]))
    );
    // Known gap #924: a nullable array of non-nullable scalars still accepts
    // a null item through the container layer.
    let scalar_container = int().repeating().nullable_container().unwrap();
    assert_eq!(
        read(&scalar_container, r#"{"x":[null]}"#),
        Ok(Instance::Repeated(vec![json_null()]))
    );

    // A missing nullable object or array is absent; a missing plain array is
    // empty.
    assert_eq!(
        read(&container_only, "{}"),
        Ok(Instance::Scalar(Value::Null))
    );
    assert_eq!(
        read(&object_array(false, None), "{}"),
        Ok(Instance::Repeated(vec![]))
    );
    assert_eq!(
        read(&object().nullable_container().unwrap(), "{}"),
        Ok(Instance::Scalar(Value::Null))
    );

    // Writers emit explicit null only where a layer admits it, and treat a
    // Null nullable object or array as absence.
    assert_eq!(
        write(&object().nullable_container().unwrap(), json_null()),
        Ok(serde_json::Value::Null)
    );
    assert!(write(&object(), json_null()).is_err());
    assert_eq!(
        write(&container_only, json_null()),
        Ok(serde_json::Value::Null)
    );
    assert!(write(&container_only, Instance::Repeated(vec![json_null()])).is_err());
    assert_eq!(
        write(&items_only, Instance::Repeated(vec![json_null()])),
        Ok(serde_json::json!([null]))
    );
    assert_eq!(
        write(&int().nullable().unwrap(), json_null()),
        Ok(serde_json::Value::Null)
    );
    assert!(write(&int(), json_null()).is_err());
    assert!(write(&int().nillable(), json_null()).is_err());
    let document = root(container_only.clone());
    let absent = Instance::Group(vec![("x".to_owned(), Instance::Scalar(Value::Null))].into());
    assert_eq!(
        crate::to_value(&document, &absent).unwrap(),
        serde_json::json!({})
    );

    // JSON Lines cannot encode a nullable array container.
    assert!(matches!(
        crate::from_lines("{}\n", &container_only),
        Err(JsonFormatError::NullableJsonLinesContainer { .. })
    ));
    assert!(crate::from_lines("{}\n", &object_array(false, Some(true))).is_ok());

    // A required alternative member's null counts only when its scalar value
    // layer admits null.
    let alternatives = |member: SchemaNode| {
        let mut node = SchemaNode::group("Root", vec![member]);
        let SchemaKind::Group { alternatives, .. } = &mut node.kind else {
            unreachable!()
        };
        alternatives.push(GroupAlternative {
            name: "only".into(),
            members: vec!["x".into()],
            required: vec!["x".into()],
            constraints: Vec::new(),
        });
        node
    };
    assert!(crate::from_str(r#"{"x":null}"#, &alternatives(int().nullable().unwrap())).is_ok());
    assert!(matches!(
        crate::from_str(
            r#"{"x":null}"#,
            &alternatives(object().nullable_container().unwrap())
        ),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));

    // Contradictions are refused by name and reason.
    let mut group_scalar_null = object();
    group_scalar_null.nullable = true;
    let mut scalar_container_null = int();
    scalar_container_null.container_nullable = true;
    for (node, reason) in [
        (
            group_scalar_null,
            EffectiveViewError::ScalarNullOutsideScalar,
        ),
        (
            scalar_container_null,
            EffectiveViewError::ContainerNullOutsideContainer,
        ),
    ] {
        let error = crate::from_str("{}", &root(node)).unwrap_err();
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
