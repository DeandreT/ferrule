use codegen_runtime::{
    CsvBoundaryError, CsvFormatError, CsvWriteOptions, Instance, RuntimeError, ScalarType,
    SchemaNode, SortDirection, Value, adapt_user_function_value, field, group, item_count, scalar,
    serialize_csv, sort_candidates, value_map,
};
use ir::Decimal96;

fn decimal(text: &str) -> Value {
    Value::decimal(Decimal96::parse_exact(text).unwrap())
}

#[test]
fn decimal_sorting_is_exact_and_stable_in_the_actual_sorter() {
    let values = vec![
        (0, [decimal("9007199254740993")]),
        (1, [decimal("9007199254740992")]),
        (2, [Value::Int(1)]),
        (3, [decimal("9007199254740992")]),
    ];
    assert_eq!(
        sort_candidates(values.clone(), [SortDirection::Ascending]),
        vec![2, 1, 3, 0]
    );
    assert_eq!(
        sort_candidates(values, [SortDirection::Descending]),
        vec![0, 1, 3, 2]
    );
}

#[test]
fn typed_boundaries_do_not_coerce_decimals_or_hide_their_type() {
    for ty in [
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ] {
        let value = decimal("9007199254740993.1");
        assert_eq!(
            adapt_user_function_value(value.clone(), ty, 1, None),
            Err(RuntimeError::UserFunctionType {
                function: 1,
                parameter: None,
                expected: ty,
                found: "decimal",
            })
        );
        let schema = SchemaNode::group("Rows", vec![SchemaNode::scalar("Amount", ty)]);
        let rows = Instance::Repeated(vec![group([field("Amount", scalar(value.clone()))])]);
        assert!(matches!(
            serialize_csv(&schema, &rows, &CsvWriteOptions::default()),
            Err(CsvBoundaryError::Format(CsvFormatError::ValueType {
                got: "decimal",
                ..
            }))
        ));
        let table = [
            (value.clone(), Value::String("exact".into())),
            (
                Value::Float(9007199254740994.0),
                Value::String("float".into()),
            ),
        ];
        assert_eq!(
            value_map(value, Some(ty), &table, None),
            Value::String("exact".into())
        );
    }
    assert_eq!(
        item_count(7, decimal("1")),
        Err(RuntimeError::NotAnItemCount {
            node: 7,
            found: "decimal"
        })
    );
}
