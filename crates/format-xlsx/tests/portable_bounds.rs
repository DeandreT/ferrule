use format_xlsx::{from_flat_package, to_flat_package};
use ir::{Instance, ScalarType, SchemaNode, Value};

#[test]
fn canonical_table_limits_and_refusals() {
    let schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Active", ScalarType::Bool),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    );
    let row = Instance::Group(
        vec![
            ("Active".into(), Instance::Scalar(Value::Bool(true))),
            (
                "Count".into(),
                Instance::Scalar(Value::Int(-999999999999999)),
            ),
        ]
        .into(),
    );
    for count in [0, 1, 256] {
        let rows = vec![row.clone(); count];
        let bytes = to_flat_package(&schema, &rows, "Sheet1").unwrap();
        assert_eq!(from_flat_package(&bytes, &schema, "Sheet1").unwrap(), rows);
        assert!(from_flat_package(&bytes, &schema, "Other").is_err());
        let mut bad = bytes;
        bad.push(0);
        assert!(from_flat_package(&bad, &schema, "Sheet1").is_err());
    }
    assert!(to_flat_package(&schema, &vec![row; 257], "Sheet1").is_err());
    let strings = SchemaNode::group("Row", vec![SchemaNode::scalar("Text", ScalarType::String)]);
    let text = Instance::Group(
        vec![(
            "Text".into(),
            Instance::Scalar(Value::String("&".repeat(1024))),
        )]
        .into(),
    );
    assert!(to_flat_package(&strings, &vec![text; 64], "Sheet1").is_err());
}
