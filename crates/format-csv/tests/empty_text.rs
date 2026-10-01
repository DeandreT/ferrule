use format_csv::{CsvReadOptions, from_str_with_options};
use ir::{Instance, ScalarType, SchemaNode, Value};

fn value<'a>(rows: &'a [Instance], row: usize, field: &str) -> &'a Value {
    rows[row]
        .field(field)
        .and_then(Instance::as_scalar)
        .unwrap()
}

#[test]
fn present_empty_text_is_distinct_from_missing_columns_and_typed_empty_cells() {
    let schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Int", ScalarType::Int),
            SchemaNode::scalar("Float", ScalarType::Float),
            SchemaNode::scalar("Bool", ScalarType::Bool),
            SchemaNode::scalar("Tail", ScalarType::String),
        ],
    );
    let input = ",,,,\n\"\",,,,\nmissing\n";
    let options = CsvReadOptions {
        has_headers: false,
        preserve_empty_strings: true,
        ..Default::default()
    };
    let rows = from_str_with_options(input, &schema, &options).unwrap();
    assert_eq!(rows.len(), 3);
    for row in 0..2 {
        assert_eq!(value(&rows, row, "Text"), &Value::String(String::new()));
        assert_eq!(value(&rows, row, "Tail"), &Value::String(String::new()));
        for field in ["Int", "Float", "Bool"] {
            assert_eq!(value(&rows, row, field), &Value::Null);
        }
    }
    assert_eq!(value(&rows, 2, "Tail"), &Value::Null);
    let legacy = format_csv::from_str(input, &schema, None, false).unwrap();
    assert_eq!(value(&legacy, 0, "Text"), &Value::Null);
    assert_eq!(value(&legacy, 1, "Text"), &Value::Null);
}

#[test]
fn empty_text_policy_respects_custom_and_disabled_quotes() {
    let schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("A", ScalarType::String),
            SchemaNode::scalar("B", ScalarType::String),
        ],
    );
    let mut options = CsvReadOptions {
        delimiter: Some(';'),
        quote: Some('\''),
        has_headers: false,
        preserve_empty_strings: true,
        ..Default::default()
    };
    let rows = from_str_with_options("'';'a;b'\n", &schema, &options).unwrap();
    assert_eq!(value(&rows, 0, "A"), &Value::String(String::new()));
    assert_eq!(value(&rows, 0, "B"), &Value::String("a;b".into()));
    options.quote = None;
    options.quote_disabled = true;
    let rows = from_str_with_options(";\"\"\n", &schema, &options).unwrap();
    assert_eq!(value(&rows, 0, "A"), &Value::String(String::new()));
    assert_eq!(value(&rows, 0, "B"), &Value::String("\"\"".into()));
    options.quote = Some('\'');
    assert!(from_str_with_options("", &schema, &options).is_err());
}
