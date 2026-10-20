use format_csv::{
    CsvBoundedError, CsvFormatError, CsvWriteOptions, to_bytes_with_options_bounded,
    to_string_with_options,
};
use ir::{Instance, ScalarType, SchemaNode, Value};

fn options() -> CsvWriteOptions {
    CsvWriteOptions {
        delimiter: Some('|'),
        quote_disabled: true,
        has_headers: false,
        raw_delimited_crlf: true,
        ..Default::default()
    }
}

fn schema() -> SchemaNode {
    SchemaNode::group("row", vec![SchemaNode::scalar("text", ScalarType::String)])
}

fn row(value: Value) -> Instance {
    Instance::Group(vec![("text".into(), Instance::Scalar(value))].into())
}

#[test]
fn raw_text_preserves_delimiters_quotes_unicode_and_record_bytes() {
    let rows = vec![row(Value::String("é|\"猫\"\r\nnext|".into()))];
    let expected = "é|\"猫\"\r\nnext|\r\n";
    assert_eq!(
        to_string_with_options(&schema(), &rows, &options()).unwrap(),
        expected
    );
    assert_eq!(
        to_bytes_with_options_bounded(&schema(), &rows, &options(), expected.len()).unwrap(),
        expected.as_bytes()
    );
    assert!(matches!(
        to_bytes_with_options_bounded(&schema(), &rows, &options(), expected.len() - 1),
        Err(CsvBoundedError::OutputTooLarge { maximum }) if maximum == expected.len() - 1
    ));
    let strict = CsvWriteOptions {
        raw_delimited_crlf: false,
        ..options()
    };
    assert!(matches!(
        to_string_with_options(&schema(), &rows, &strict),
        Err(CsvFormatError::UnquotedFieldBoundary { row: 0, .. })
    ));
}

#[test]
fn empty_records_headers_bom_and_zero_rows_have_literal_bytes() {
    assert_eq!(
        to_string_with_options(&schema(), &[], &options()).unwrap(),
        ""
    );
    for value in [Value::Null, Value::String(String::new())] {
        assert_eq!(
            to_string_with_options(&schema(), &[row(value)], &options()).unwrap(),
            "\r\n"
        );
    }
    assert_eq!(
        to_string_with_options(
            &SchemaNode::group("row", vec![]),
            &[Instance::Group(vec![].into())],
            &options()
        )
        .unwrap(),
        "\r\n"
    );
    let header = SchemaNode::group(
        "row",
        vec![SchemaNode::scalar("a|b\r\n", ScalarType::String)],
    );
    let options = CsvWriteOptions {
        has_headers: true,
        utf8_bom: true,
        ..options()
    };
    assert_eq!(
        to_bytes_with_options_bounded(&header, &[], &options, 10).unwrap(),
        b"\xef\xbb\xbfa|b\r\n\r\n"
    );
    assert!(matches!(
        to_bytes_with_options_bounded(&header, &[], &options, 9),
        Err(CsvBoundedError::OutputTooLarge { .. })
    ));
}

#[test]
fn raw_mode_preserves_complete_validation_before_encoding() {
    let rows = [
        row(Value::String("first|\n".into())),
        row(Value::Float(f64::NAN)),
    ];
    assert!(matches!(
        to_bytes_with_options_bounded(&schema(), &rows, &options(), 0),
        Err(CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 1,
            ..
        }))
    ));
    let quoted = CsvWriteOptions {
        quote_disabled: false,
        ..options()
    };
    assert!(matches!(
        to_string_with_options(&schema(), &[], &quoted),
        Err(CsvFormatError::RawDelimitedRequiresDisabledQuotes)
    ));
    let conflict = CsvWriteOptions {
        quote: Some('"'),
        ..options()
    };
    assert!(matches!(
        to_string_with_options(&schema(), &[], &conflict),
        Err(CsvFormatError::ConflictingQuoteSettings)
    ));
    assert!(
        !CsvWriteOptions::from(&mapping::FormatOptions {
            csv_quote_disabled: true,
            ..Default::default()
        })
        .raw_delimited_crlf
    );
}
