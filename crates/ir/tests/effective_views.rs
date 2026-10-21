use ir::{
    EffectiveCardinality, EffectivePresence, EffectiveViewError, ItemCountRange, JsonNullability,
    RepeatedItems, ScalarType, ScalarTypeSet, SchemaNode,
};

fn single(presence: EffectivePresence) -> EffectiveCardinality {
    EffectiveCardinality::Single { presence }
}

fn values(item_count: Option<ItemCountRange>) -> EffectiveCardinality {
    EffectiveCardinality::Repeated {
        item_count,
        items: RepeatedItems::Values,
    }
}

fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn object_array(container: bool, item: Option<bool>) -> SchemaNode {
    let mut schema = SchemaNode::group("Items", vec![text("id")]).repeating();
    schema.container_nullable = container;
    schema.json_object_item_nullable = item;
    schema
}

#[test]
fn singular_occurrences_keep_presence_and_null_layers_distinct() {
    use EffectivePresence::{Optional, Required};
    let mut optional_element = text("Note");
    assert!(optional_element.set_xml_optional(true));
    let mut required_attribute = text("code").attribute();
    assert!(required_attribute.set_xml_attribute_required(true));
    let mut optional_nillable = text("Note").nillable();
    assert!(optional_nillable.set_xml_optional(true));
    let union = SchemaNode::scalar_union(
        "Value",
        ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
    );
    let cases = [
        // (schema, cardinality, xml nil, JSON null, required non-null singleton)
        (text("Name"), single(Required), false, false, true),
        (optional_element, single(Optional), false, false, false),
        (
            text("code").attribute(),
            single(Optional),
            false,
            false,
            false,
        ),
        (required_attribute, single(Required), false, false, true),
        (text("#text").text(), single(Required), false, false, true),
        (
            text("Name").nillable(),
            single(Required),
            true,
            false,
            false,
        ),
        (optional_nillable, single(Optional), true, false, false),
        (
            text("Name").nullable().unwrap(),
            single(Required),
            false,
            true,
            false,
        ),
        (
            union.clone().nullable().unwrap(),
            single(Required),
            false,
            true,
            false,
        ),
        (union, single(Required), false, false, true),
        (
            SchemaNode::group("Order", vec![text("id")]),
            single(Required),
            false,
            false,
            true,
        ),
        (
            SchemaNode::group("Order", vec![text("id")])
                .nullable_container()
                .unwrap(),
            single(Required),
            false,
            true,
            false,
        ),
        (
            text("Payload").json_any().unwrap(),
            single(Required),
            false,
            false,
            true,
        ),
    ];
    for (schema, cardinality, xml_nil, json_null, singleton) in cases {
        let view = schema.effective_view().unwrap();
        assert_eq!(view.cardinality(), cardinality, "{schema:?}");
        assert_eq!(view.nullability().xml_nil(), xml_nil, "{schema:?}");
        assert_eq!(
            view.nullability().json(),
            JsonNullability::Occurrence { null: json_null },
            "{schema:?}"
        );
        assert_eq!(
            view.nullability().admits_null(),
            xml_nil || json_null,
            "{schema:?}"
        );
        assert_eq!(
            view.is_required_non_null_singleton(),
            singleton,
            "{schema:?}"
        );
    }
}

#[test]
fn repeated_containers_and_items_keep_independent_nullability() {
    let bounded = ItemCountRange::new(1, Some(3)).unwrap();
    let mut bounded_scalars = text("tags").repeating();
    bounded_scalars.item_count_range = Some(bounded);
    let mut nullable_scalar_items = text("tags").repeating().nullable().unwrap();
    nullable_scalar_items.container_nullable = true;
    let cases = [
        // (schema, cardinality, container null, item null)
        (text("tags").repeating(), values(None), false, false),
        (bounded_scalars, values(Some(bounded)), false, false),
        (
            text("tags").repeating().nullable().unwrap(),
            values(None),
            false,
            true,
        ),
        (
            text("tags").repeating().nullable_container().unwrap(),
            values(None),
            true,
            false,
        ),
        (nullable_scalar_items, values(None), true, true),
        // #571: legacy descriptors reuse the container flag for object items.
        (object_array(false, None), values(None), false, false),
        (object_array(true, None), values(None), true, true),
        (object_array(true, Some(false)), values(None), true, false),
        (object_array(false, Some(true)), values(None), false, true),
        (object_array(true, Some(true)), values(None), true, true),
        (object_array(false, Some(false)), values(None), false, false),
    ];
    for (schema, cardinality, container_null, item_null) in cases {
        let view = schema.effective_view().unwrap();
        assert_eq!(view.cardinality(), cardinality, "{schema:?}");
        assert!(!view.nullability().xml_nil(), "{schema:?}");
        assert_eq!(
            view.nullability().json(),
            JsonNullability::Array {
                container_null,
                item_null
            },
            "{schema:?}"
        );
        assert_eq!(
            view.nullability().admits_null(),
            container_null || item_null
        );
        assert!(!view.is_required_non_null_singleton(), "{schema:?}");
    }
    let nillable_items = SchemaNode::group("Line", vec![]).repeating().nillable();
    let view = nillable_items.effective_view().unwrap();
    assert!(view.nullability().xml_nil());
    assert_eq!(
        view.nullability().json(),
        JsonNullability::Array {
            container_null: false,
            item_null: false
        }
    );
}

#[test]
fn nested_array_wrappers_leave_item_nullability_to_their_item_child() {
    let inner = text("item").repeating().nullable().unwrap();
    let wrapper = SchemaNode::group("matrix", vec![inner.clone()])
        .repeating()
        .with_json_array_wrapper()
        .unwrap();
    let nullable_wrapper = wrapper.clone().nullable_container().unwrap();
    for (schema, container_null) in [(wrapper, false), (nullable_wrapper, true)] {
        let view = schema.effective_view().unwrap();
        assert_eq!(
            view.cardinality(),
            EffectiveCardinality::Repeated {
                item_count: None,
                items: RepeatedItems::NestedArrays
            }
        );
        assert_eq!(
            view.nullability().json(),
            JsonNullability::NestedArrays { container_null }
        );
    }
    assert_eq!(
        inner.effective_view().unwrap().nullability().json(),
        JsonNullability::Array {
            container_null: false,
            item_null: true
        }
    );
}

#[test]
fn contradictory_role_metadata_has_no_effective_interpretation() {
    let mut group_scalar_null = SchemaNode::group("Order", vec![]);
    group_scalar_null.nullable = true;
    let mut singular_scalar_container = text("Name");
    singular_scalar_container.container_nullable = true;
    let mut singular_object_item = SchemaNode::group("Order", vec![]);
    singular_object_item.json_object_item_nullable = Some(true);
    let mut scalar_object_item = text("tags").repeating();
    scalar_object_item.json_object_item_nullable = Some(false);
    let mut singular_wrapper = SchemaNode::group("matrix", vec![text("item").repeating()]);
    singular_wrapper.json_array_wrapper = true;
    let mut optional_repeating = text("Line").repeating();
    optional_repeating.xml_optional = true;
    let mut optional_attribute = text("code").attribute();
    optional_attribute.xml_optional = true;
    let mut required_element = text("Name");
    required_element.xml_attribute_required = true;
    let cases = [
        (
            group_scalar_null,
            EffectiveViewError::ScalarNullOutsideScalar,
        ),
        (
            singular_scalar_container,
            EffectiveViewError::ContainerNullOutsideContainer,
        ),
        (
            singular_object_item,
            EffectiveViewError::ObjectItemNullOutsideObjectArray,
        ),
        (
            scalar_object_item,
            EffectiveViewError::ObjectItemNullOutsideObjectArray,
        ),
        (
            singular_wrapper,
            EffectiveViewError::InvalidJsonArrayWrapper,
        ),
        (
            optional_repeating,
            EffectiveViewError::XmlOptionalOutsideElement,
        ),
        (
            optional_attribute,
            EffectiveViewError::XmlOptionalOutsideElement,
        ),
        (
            required_element,
            EffectiveViewError::XmlAttributeRequiredOutsideAttribute,
        ),
    ];
    for (schema, error) in cases {
        assert_eq!(schema.effective_view(), Err(error), "{schema:?}");
        assert!(!schema.metadata_is_valid(), "{schema:?}");
    }
}

#[test]
fn views_are_derived_without_changing_saved_schema_wire() {
    let mut schema = object_array(true, Some(false));
    schema.item_count_range = ItemCountRange::new(0, Some(2));
    let wire = serde_json::to_string(&schema).unwrap();
    let view = schema.effective_view().unwrap();
    assert_eq!(serde_json::to_string(&schema).unwrap(), wire);
    let restored: SchemaNode = serde_json::from_str(&wire).unwrap();
    assert_eq!(restored, schema);
    assert_eq!(restored.effective_view().unwrap(), view);
}
