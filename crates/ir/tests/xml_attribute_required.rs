use ir::{ScalarType, ScalarTypeSet, SchemaNode};

#[test]
fn legacy_attribute_wire_and_checked_required_setter_remain_exact() {
    let mut schema = SchemaNode::scalar("Code", ScalarType::String).attribute();
    let legacy = serde_json::to_string(&schema).unwrap();
    assert!(!legacy.contains("xml_attribute_required"));
    assert!(schema.set_xml_attribute_required(true));
    assert!(schema.metadata_is_valid());
    let wire = serde_json::to_string(&schema).unwrap();
    assert!(wire.contains("\"xml_attribute_required\":true"));
    assert_eq!(serde_json::from_str::<SchemaNode>(&wire).unwrap(), schema);
    assert!(schema.set_xml_attribute_required(false));
    assert_eq!(serde_json::to_string(&schema).unwrap(), legacy);
    assert_eq!(serde_json::from_str::<SchemaNode>(&legacy).unwrap(), schema);
}

#[test]
fn requiredness_only_belongs_to_named_singular_scalar_attributes() {
    let mut group = SchemaNode::group("Code", vec![]);
    group.attribute = true;
    let mut invalid = vec![
        SchemaNode::scalar("Code", ScalarType::String),
        SchemaNode::scalar("Code", ScalarType::String)
            .attribute()
            .repeating(),
        SchemaNode::scalar("Code", ScalarType::String)
            .attribute()
            .text(),
        SchemaNode::scalar("", ScalarType::String).attribute(),
        SchemaNode::scalar("#text", ScalarType::String).attribute(),
        SchemaNode::scalar("element()", ScalarType::String).attribute(),
        SchemaNode::scalar("attribute()", ScalarType::String).attribute(),
        SchemaNode::scalar("\u{1f}private", ScalarType::String).attribute(),
        SchemaNode::scalar_union(
            "Code",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        )
        .attribute(),
        group,
    ];
    for schema in &mut invalid {
        assert!(!schema.set_xml_attribute_required(true), "{}", schema.name);
        assert!(!schema.xml_attribute_required);
        schema.xml_attribute_required = true;
        assert!(!schema.xml_attribute_required_is_valid());
        assert!(!schema.metadata_is_valid());
        let wire = serde_json::to_string(schema).unwrap();
        assert!(serde_json::from_str::<SchemaNode>(&wire).is_err());
        let nested = SchemaNode::group("Root", vec![schema.clone()]);
        assert!(
            serde_json::from_str::<SchemaNode>(&serde_json::to_string(&nested).unwrap()).is_err()
        );
    }
}

#[test]
fn equality_retains_use_without_repurposing_json_required_or_default() {
    let optional = SchemaNode::scalar("Code", ScalarType::String).attribute();
    let mut required = optional.clone();
    required.set_xml_attribute_required(true);
    assert_ne!(optional, required);
    let root = SchemaNode::group("Root", vec![required.clone()]);
    assert!(root.required_fields().is_empty());
    required.default = Some("fallback".into());
    // Declaration/default provenance is rejected by XSD, independently from
    // role validity and the unchanged lenient instance default behavior.
    assert!(required.xml_attribute_required_is_valid());
    assert!(required.default_is_valid());
}
