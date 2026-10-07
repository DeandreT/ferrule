use std::error::Error as _;

use format_csv::{
    CsvBoundedError, CsvFormatError, CsvWriteOptions, to_bytes_with_options_bounded,
    to_string_with_options,
};
use ir::{Instance, ScalarType, SchemaNode, Value};

fn schema() -> SchemaNode {
    SchemaNode::group(
        "row",
        vec![
            SchemaNode::scalar("text", ScalarType::String),
            SchemaNode::scalar("tail", ScalarType::Int),
        ],
    )
}

fn row(text: String, tail: Value) -> Instance {
    Instance::Group(
        vec![
            ("text".into(), Instance::Scalar(Value::String(text))),
            ("tail".into(), Instance::Scalar(tail)),
        ]
        .into(),
    )
}

fn complete_bytes(
    schema: &SchemaNode,
    rows: &[Instance],
    options: &CsvWriteOptions,
    expected: &str,
) {
    assert_eq!(
        to_string_with_options(schema, rows, options)
            .unwrap()
            .as_bytes(),
        expected.as_bytes(),
    );
    assert_eq!(
        to_bytes_with_options_bounded(schema, rows, options, expected.len()).unwrap(),
        expected.as_bytes(),
    );
    let maximum = expected.len() - 1;
    let error = to_bytes_with_options_bounded(schema, rows, options, maximum).unwrap_err();
    assert!(
        matches!(&error, CsvBoundedError::OutputTooLarge { maximum: actual } if *actual == maximum),
    );
    assert!(error.source().is_none());
}

#[test]
fn late_quotes_and_quoted_fields_without_quotes_keep_complete_native_bytes() {
    let options = CsvWriteOptions {
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    let mut rows = Vec::new();
    let mut expected = String::from("\u{feff}text,tail\n");
    for prefix_bytes in [4095, 4096, 4097, 8191, 8192, 8193, 16_385] {
        let prefix = "x".repeat(prefix_bytes);
        rows.push(row(format!("{prefix}\"end"), Value::Int(7)));
        expected.push('"');
        expected.push_str(&prefix);
        expected.push_str("\"\"end\",7\n");
        rows.push(row(format!("{prefix},end"), Value::Int(8)));
        expected.push('"');
        expected.push_str(&prefix);
        expected.push_str(",end\",8\n");
    }
    rows.push(row("plain".into(), Value::Int(9)));
    expected.push_str("plain,9\n");
    complete_bytes(&schema(), &rows, &options, &expected);
}

#[test]
fn dense_custom_quotes_and_utf8_crossings_keep_field_and_record_state() {
    let schema = SchemaNode::group(
        "row",
        vec![
            SchemaNode::scalar("te'xt", ScalarType::String),
            SchemaNode::scalar("tail", ScalarType::Int),
        ],
    );
    let options = CsvWriteOptions {
        delimiter: Some(';'),
        quote: Some('\''),
        ..CsvWriteOptions::default()
    };
    let mut rows = vec![Instance::Group(
        vec![
            (
                "te'xt".into(),
                Instance::Scalar(Value::String("'".repeat(4097))),
            ),
            ("tail".into(), Instance::Scalar(Value::Int(7))),
        ]
        .into(),
    )];
    let mut expected = String::from("'te''xt';tail\n'");
    expected.push_str(&"'".repeat(8194));
    expected.push_str("';7\n");
    for prefix_bytes in [4094, 4095, 4096, 8191, 8192, 8193, 12_287] {
        let prefix = "x".repeat(prefix_bytes);
        rows.push(Instance::Group(
            vec![
                (
                    "te'xt".into(),
                    Instance::Scalar(Value::String(format!("{prefix}é;\r\n'終"))),
                ),
                (
                    "tail".into(),
                    Instance::Scalar(Value::String(" +008 ".into())),
                ),
            ]
            .into(),
        ));
        expected.push('\'');
        expected.push_str(&prefix);
        expected.push_str("é;\r\n''終';8\n");
    }
    rows.push(Instance::Group(
        vec![
            ("te'xt".into(), Instance::Scalar(Value::Null)),
            ("tail".into(), Instance::Scalar(Value::Int(9))),
        ]
        .into(),
    ));
    expected.push_str(";9\n");
    complete_bytes(&schema, &rows, &options, &expected);
}

#[test]
fn disabled_quoting_keeps_long_quotes_and_unicode_as_unquoted_bytes() {
    let options = CsvWriteOptions {
        quote_disabled: true,
        has_headers: false,
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    let prefix = "x".repeat(16_385);
    let rows = vec![
        row(format!("{prefix}\"é終"), Value::Int(7)),
        row("plain\"text".into(), Value::Int(8)),
    ];
    let expected = format!("\u{feff}{prefix}\"é終,7\nplain\"text,8\n");
    complete_bytes(&schema(), &rows, &options, &expected);
}

#[test]
fn late_types_precede_earlier_long_unquoted_boundaries_and_zero_byte_limits() {
    let options = CsvWriteOptions {
        quote_disabled: true,
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    let rows = vec![
        row(format!("{},boundary", "x".repeat(16_385)), Value::Int(7)),
        row("late".into(), Value::Float(0.5)),
    ];
    let ordinary = to_string_with_options(&schema(), &rows, &options).unwrap_err();
    assert!(matches!(
        &ordinary,
        CsvFormatError::ValueType {
            row: 1,
            field,
            expected: ScalarType::Int,
            got: "float",
        } if field == "tail"
    ));
    let bounded = to_bytes_with_options_bounded(&schema(), &rows, &options, 0).unwrap_err();
    assert!(matches!(
        &bounded,
        CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 1,
            field,
            expected: ScalarType::Int,
            got: "float",
        }) if field == "tail"
    ));
    assert!(
        bounded
            .source()
            .unwrap()
            .downcast_ref::<CsvFormatError>()
            .is_some(),
    );
}
