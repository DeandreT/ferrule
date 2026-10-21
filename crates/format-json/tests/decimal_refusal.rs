use ir::{Decimal96, Instance, ScalarType, ScalarTypeSet, SchemaNode, Value};

#[test]
fn decimal_carrier_is_not_implicitly_admitted_as_json_data() {
    let mut schemas = [
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ]
    .map(|ty| SchemaNode::scalar("Value", ty))
    .to_vec();
    schemas.push(SchemaNode::scalar_union(
        "Value",
        ScalarTypeSet::new([ScalarType::String, ScalarType::Float]).unwrap(),
    ));
    let mut any = SchemaNode::scalar("Value", ScalarType::String);
    any.json_any = true;
    schemas.push(any);
    for text in [
        "0",
        "0.1",
        "9007199254740993",
        "79228162514264337593543950335",
    ] {
        let instance = Instance::Scalar(Value::decimal(Decimal96::parse_exact(text).unwrap()));
        for schema in &schemas {
            assert!(matches!(
                format_json::to_value(schema, &instance),
                Err(format_json::JsonFormatError::Shape { got: "decimal", .. })
            ));
            assert!(matches!(
                format_json::to_string(schema, &instance),
                Err(format_json::JsonFormatError::Shape { got: "decimal", .. })
            ));
        }
    }
}
