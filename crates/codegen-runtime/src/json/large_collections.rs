use ir::{
    Instance, ItemCountRange, JsonContainsConstraint, JsonContainsConstraints,
    JsonContainsPredicate, ScalarType, SchemaNode,
};

use super::{MAX_JSON_DOCUMENT_BYTES, parse_json, serialize_json};

const ITEMS: usize = 1_000_001;

fn assert_matches_native(
    schema: SchemaNode,
    document: String,
    output_bytes: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(document.len() < MAX_JSON_DOCUMENT_BYTES);
    let descriptor = serde_json::to_string(&schema)?;
    let native = format_json::from_str(&document, &schema)?;
    let generated = parse_json(&descriptor, &document)?;
    assert_eq!(generated, native);
    drop(native);
    let Instance::Repeated(items) = &generated else {
        return Err("large collection remains repeated".into());
    };
    assert_eq!(items.len(), ITEMS);

    let native_output = format_json::to_string(&schema, &generated)?;
    assert_eq!(native_output.len(), output_bytes);
    assert!(native_output.len() < MAX_JSON_DOCUMENT_BYTES);
    assert_eq!(serialize_json(&descriptor, &generated)?, native_output);
    Ok(())
}

#[test]
fn ordinary_group_collection_above_one_million_matches_native_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let schema =
        SchemaNode::group("Rows", vec![SchemaNode::scalar("v", ScalarType::Int)]).repeating();
    let document = format!(
        "[{}]",
        std::iter::repeat_n(r#"{"v":0}"#, ITEMS)
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(document.len(), 8_000_009);
    assert_matches_native(schema, document, 20_000_023)
}

#[test]
fn contains_collection_above_one_million_matches_native_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let predicate = SchemaNode::scalar("any", ScalarType::String)
        .json_any()
        .ok_or("arbitrary JSON predicate is valid")?;
    let constraints = JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonContainsPredicate::schema(predicate),
        ItemCountRange::new(1, None).ok_or("positive contains interval is valid")?,
    )])
    .ok_or("contains constraint is effective")?;
    let schema = SchemaNode::scalar("Values", ScalarType::Int)
        .repeating()
        .with_json_contains(constraints)
        .ok_or("contains belongs to an array")?;
    let document = format!(
        "[{}]",
        std::iter::repeat_n("0", ITEMS)
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(document.len(), 2_000_003);
    assert_matches_native(schema, document, 5_000_008)
}
