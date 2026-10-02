use ir::{Instance, ScalarType, SchemaNode, Value};
fn schema(optional: bool) -> SchemaNode {
    let mut value = SchemaNode::scalar("Value", ScalarType::String);
    value.set_xml_optional(optional);
    let mut group = SchemaNode::group("Maybe", vec![]);
    group.set_xml_optional(optional);
    SchemaNode::group("Root", vec![value, group])
}
#[test]
fn xml_occurrence_metadata_does_not_change_json_presence_or_exact_fixed_points() {
    let old = schema(false);
    let new = schema(true);
    for doc in [
        "{}",
        r#"{"Value":"","Maybe":{}}"#,
        r#"{"Value":"x"}"#,
        r#"{"Maybe":{}}"#,
    ] {
        let value = format_json::from_str(doc, &old).unwrap();
        assert_eq!(format_json::from_str(doc, &new).unwrap(), value);
        assert_eq!(
            format_json::to_string(&old, &value).unwrap(),
            format_json::to_string(&new, &value).unwrap()
        );
        let encoded = format_json::to_string(&new, &value).unwrap();
        assert_eq!(format_json::from_str(&encoded, &new).unwrap(), value);
    }
    let absent = format_json::from_str("{}", &new).unwrap();
    assert_eq!(absent.field("Value"), Some(&Instance::Scalar(Value::Null)));
    assert_eq!(
        absent.field("Maybe"),
        Some(&Instance::Group((vec![]).into()))
    );
}
