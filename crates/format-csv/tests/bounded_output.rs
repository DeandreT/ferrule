use std::error::Error as _;

use format_csv::{
    CsvBoundedError, CsvFormatError, CsvWriteOptions, to_bytes_with_options_bounded,
    to_string_with_options, to_string_with_options_bounded,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{CsvTextRepairCause, CsvTextRepairDependency};

fn schema(fields: &[(&str, ScalarType)]) -> SchemaNode {
    SchemaNode::group(
        "row",
        fields
            .iter()
            .map(|(name, ty)| SchemaNode::scalar(*name, *ty))
            .collect(),
    )
}

fn row(fields: Vec<(&str, Value)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), Instance::Scalar(value)))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn limit(error: CsvBoundedError, maximum: usize) {
    assert!(
        matches!(error, CsvBoundedError::OutputTooLarge { maximum: actual } if actual == maximum)
    );
}

#[test]
fn bounded_native_bytes_match_legacy_dialects_unicode_and_record_boundaries() {
    let schema = schema(&[("title", ScalarType::String), ("number", ScalarType::Int)]);
    let rows = vec![
        row(vec![
            ("title", Value::String("é,;'quoted'\r\nnext".into())),
            ("number", Value::String("  +0037  ".into())),
        ]),
        row(vec![("title", Value::Null), ("number", Value::Int(-19))]),
    ];
    for options in [
        CsvWriteOptions::default(),
        CsvWriteOptions {
            delimiter: Some(';'),
            quote: Some('\''),
            ..CsvWriteOptions::default()
        },
        CsvWriteOptions {
            has_headers: false,
            utf8_bom: true,
            ..CsvWriteOptions::default()
        },
    ] {
        let original = to_string_with_options(&schema, &rows, &options).unwrap();
        let maximum = original.len();
        assert_eq!(
            to_bytes_with_options_bounded(&schema, &rows, &options, maximum).unwrap(),
            original.as_bytes()
        );
        assert_eq!(
            to_string_with_options_bounded(&schema, &rows, &options, maximum).unwrap(),
            original
        );
        limit(
            to_bytes_with_options_bounded(&schema, &rows, &options, maximum - 1).unwrap_err(),
            maximum - 1,
        );
    }
}

#[test]
fn zero_fields_empty_headers_and_single_empty_data_keep_native_distinctions() {
    let empty_schema = schema(&[]);
    let empty_row = row(Vec::new());
    for has_headers in [false, true] {
        for quote_disabled in [false, true] {
            let options = CsvWriteOptions {
                has_headers,
                quote_disabled,
                ..CsvWriteOptions::default()
            };
            for rows in [Vec::new(), vec![empty_row.clone()]] {
                let original = to_string_with_options(&empty_schema, &rows, &options).unwrap();
                assert_eq!(
                    to_bytes_with_options_bounded(&empty_schema, &rows, &options, original.len())
                        .unwrap(),
                    original.as_bytes()
                );
            }
        }
    }
    let empty_header = schema(&[("", ScalarType::String)]);
    let options = CsvWriteOptions {
        quote_disabled: true,
        ..CsvWriteOptions::default()
    };
    let original = to_string_with_options(&empty_header, &[], &options).unwrap();
    assert_eq!(original, "\"\"\n");
    assert_eq!(
        to_bytes_with_options_bounded(&empty_header, &[], &options, original.len()).unwrap(),
        original.as_bytes()
    );
    let rows = vec![row(vec![("", Value::String(String::new()))])];
    assert!(matches!(
        to_bytes_with_options_bounded(&empty_header, &rows, &options, 0),
        Err(CsvBoundedError::Format(
            CsvFormatError::UnquotedSingleEmptyRow { row: 0 }
        ))
    ));
}

#[test]
fn native_target_coercion_extreme_float_lexicals_and_presence_are_preserved() {
    let schema = schema(&[("text", ScalarType::String), ("number", ScalarType::Float)]);
    let options = CsvWriteOptions {
        has_headers: false,
        ..CsvWriteOptions::default()
    };
    for value in [
        -0.0,
        1e-7,
        1e20,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
    ] {
        let rows = vec![row(vec![
            ("text", Value::Float(value)),
            ("number", Value::Float(value)),
        ])];
        let original = to_string_with_options(&schema, &rows, &options).unwrap();
        assert_eq!(
            to_bytes_with_options_bounded(&schema, &rows, &options, original.len()).unwrap(),
            original.as_bytes()
        );
        assert!(!original.contains(['e', 'E']));
        if value == 0.0 && value.is_sign_negative() {
            assert_eq!(original, "-0,-0\n");
        }
    }
    let rows = vec![row(vec![
        ("text", Value::json_null()),
        ("number", Value::Null),
    ])];
    assert_eq!(
        to_bytes_with_options_bounded(&schema, &rows, &options, 2).unwrap(),
        b",\n"
    );
    let rows = vec![row(vec![
        ("text", Value::xml_nil()),
        ("number", Value::Null),
    ])];
    assert!(matches!(
        to_bytes_with_options_bounded(&schema, &rows, &options, 0),
        Err(CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 0,
            got: "xml nil",
            ..
        }))
    ));
    let rows = vec![row(vec![
        ("text", Value::String("valid".into())),
        ("number", Value::Int(9_007_199_254_740_993)),
    ])];
    assert!(matches!(
        to_bytes_with_options_bounded(&schema, &rows, &options, 0),
        Err(CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 0,
            got: "int outside the exact f64 range",
            ..
        }))
    ));
}

#[test]
fn late_row_validation_wins_over_budget_and_disabled_quote_output() {
    let schema = schema(&[("text", ScalarType::String)]);
    let rows = vec![
        row(vec![("text", Value::String("large,".repeat(2048)))]),
        row(Vec::new()),
    ];
    let options = CsvWriteOptions {
        quote_disabled: true,
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    let original = to_string_with_options(&schema, &rows, &options).unwrap_err();
    let bounded = to_bytes_with_options_bounded(&schema, &rows, &options, 1).unwrap_err();
    assert_eq!(bounded.to_string(), original.to_string());
    assert!(
        matches!(&bounded, CsvBoundedError::Format(CsvFormatError::MissingField { row: 1, field }) if field == "text")
    );
    assert!(
        bounded
            .source()
            .unwrap()
            .downcast_ref::<CsvFormatError>()
            .is_some()
    );
    let rows = vec![
        row(vec![("text", Value::String("valid".into()))]),
        row(vec![("text", Value::String("late\nrecord".into()))]),
    ];
    assert!(matches!(
        to_bytes_with_options_bounded(&schema, &rows, &options, 1),
        Err(CsvBoundedError::Format(
            CsvFormatError::UnquotedFieldBoundary { row: 1, .. }
        ))
    ));
}

#[test]
fn schema_dialect_and_row_payloads_remain_native_before_output_limits() {
    let row_schema = schema(&[("text", ScalarType::String)]);
    let cases = [
        (
            SchemaNode::scalar("text", ScalarType::String),
            CsvWriteOptions {
                delimiter: Some('\n'),
                repair_dependency: Some(CsvTextRepairDependency::new(CsvTextRepairCause::Encoding)),
                ..CsvWriteOptions::default()
            },
            vec![],
        ),
        (
            SchemaNode::scalar("text", ScalarType::String),
            CsvWriteOptions::default(),
            vec![],
        ),
        (
            row_schema.clone(),
            CsvWriteOptions {
                delimiter: Some('\n'),
                ..CsvWriteOptions::default()
            },
            vec![],
        ),
        (
            row_schema.clone(),
            CsvWriteOptions {
                quote_disabled: true,
                quote: Some('\''),
                ..CsvWriteOptions::default()
            },
            vec![],
        ),
        (
            row_schema.clone(),
            CsvWriteOptions::default(),
            vec![Instance::Scalar(Value::Int(1))],
        ),
        (
            row_schema.clone(),
            CsvWriteOptions::default(),
            vec![row(vec![("other", Value::Int(1))])],
        ),
        (
            row_schema.clone(),
            CsvWriteOptions::default(),
            vec![row(vec![("text", Value::Int(1)), ("text", Value::Int(2))])],
        ),
    ];
    for (schema, options, rows) in cases {
        let original = to_string_with_options(&schema, &rows, &options).unwrap_err();
        let bounded = to_bytes_with_options_bounded(&schema, &rows, &options, 0).unwrap_err();
        assert_eq!(bounded.to_string(), original.to_string());
        assert!(matches!(bounded, CsvBoundedError::Format(_)));
    }
}

#[test]
fn bom_header_quotes_and_utf8_each_use_real_output_bytes_at_the_limit() {
    let schema = schema(&[("a,b", ScalarType::String)]);
    let rows = vec![row(vec![("a,b", Value::String("é,\"\n".into()))])];
    let options = CsvWriteOptions {
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    let original = to_string_with_options(&schema, &rows, &options).unwrap();
    assert!(original.starts_with('\u{feff}'));
    for maximum in 0..=original.len() {
        let actual = to_bytes_with_options_bounded(&schema, &rows, &options, maximum);
        if maximum == original.len() {
            assert_eq!(actual.unwrap(), original.as_bytes());
        } else {
            limit(actual.unwrap_err(), maximum);
        }
    }
}

#[test]
fn actual_64_mib_boundary_and_plus_one_include_bom_header_and_quoted_record() {
    const MAXIMUM: usize = 64 * 1024 * 1024;
    let schema = schema(&[("a,b", ScalarType::String)]);
    let options = CsvWriteOptions {
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    // Three BOM bytes, six header bytes, and two quotes plus LF around the data.
    let value_length = MAXIMUM - 12;
    let mut value = String::with_capacity(value_length);
    value.push(',');
    value.extend(std::iter::repeat_n('a', value_length - 1));
    let rows = vec![row(vec![("a,b", Value::String(value))])];
    let bytes = to_bytes_with_options_bounded(&schema, &rows, &options, MAXIMUM).unwrap();
    assert_eq!(bytes.len(), MAXIMUM);
    assert!(bytes.starts_with(b"\xef\xbb\xbf\"a,b\"\n\","));
    assert!(bytes.ends_with(b"\"\n"));
    assert!(bytes[11..MAXIMUM - 2].iter().all(|byte| *byte == b'a'));
    drop(bytes);
    drop(rows);
    let mut value = String::with_capacity(value_length + 1);
    value.push(',');
    value.extend(std::iter::repeat_n('a', value_length));
    let rows = vec![row(vec![("a,b", Value::String(value))])];
    limit(
        to_bytes_with_options_bounded(&schema, &rows, &options, MAXIMUM).unwrap_err(),
        MAXIMUM,
    );
}

#[test]
fn late_delimiters_quotes_and_following_records_match_native_across_chunks() {
    let schema = schema(&[
        ("first", ScalarType::String),
        ("second", ScalarType::String),
        ("empty", ScalarType::String),
    ]);
    for (delimiter, quote) in [(',', '"'), (';', '\'')] {
        let options = CsvWriteOptions {
            delimiter: Some(delimiter),
            quote: Some(quote),
            has_headers: false,
            ..CsvWriteOptions::default()
        };
        for length in [8193, 8194, 8195, 12289] {
            // Both special bytes are beyond the first encoder output buffer.
            let late_delimiter = format!("{}{delimiter}", "a".repeat(length));
            let late_quote = format!("{}{quote}", "b".repeat(length));
            let rows = vec![
                row(vec![
                    ("first", Value::String(late_delimiter)),
                    ("second", Value::String(late_quote)),
                    ("empty", Value::Null),
                ]),
                row(vec![
                    ("first", Value::String("plain next row".into())),
                    ("second", Value::String("é\r\nnext".into())),
                    ("empty", Value::String(String::new())),
                ]),
            ];
            let original = to_string_with_options(&schema, &rows, &options).unwrap();
            assert!(original.starts_with(quote));
            assert!(original.contains("\nplain next row"));
            let bytes =
                to_bytes_with_options_bounded(&schema, &rows, &options, original.len()).unwrap();
            assert_eq!(bytes, original.as_bytes());
            limit(
                to_bytes_with_options_bounded(&schema, &rows, &options, original.len() - 1)
                    .unwrap_err(),
                original.len() - 1,
            );
        }
    }
}

#[test]
fn dense_quotes_split_utf8_and_long_headers_match_native_without_field_state_leaks() {
    let header = format!("{},header", "h".repeat(12289));
    let header_schema = schema(&[(&header, ScalarType::String), ("after", ScalarType::String)]);
    let options = CsvWriteOptions {
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    for length in [4095, 4096, 4097, 8193] {
        let dense_quotes = "\"".repeat(length);
        // The opening comma selects quoting, and the first buffer divides the
        // UTF-8 encoding of é. Further chunks divide Unicode scalar encodings.
        let unicode = format!(",{}{}\"tail", "a".repeat(8191), "é中🙂".repeat(2049));
        let rows = vec![
            row(vec![
                (&header, Value::String(dense_quotes)),
                ("after", Value::String(unicode)),
            ]),
            row(vec![
                (&header, Value::Null),
                ("after", Value::String("last".into())),
            ]),
        ];
        let original = to_string_with_options(&header_schema, &rows, &options).unwrap();
        assert!(original.ends_with("\n,last\n"));
        let bytes =
            to_bytes_with_options_bounded(&header_schema, &rows, &options, original.len()).unwrap();
        assert_eq!(bytes, original.as_bytes());
        assert_eq!(
            to_string_with_options_bounded(&header_schema, &rows, &options, original.len())
                .unwrap(),
            original
        );
    }
    let raw_schema = schema(&[("raw", ScalarType::String), ("after", ScalarType::String)]);
    let options = CsvWriteOptions {
        quote_disabled: true,
        has_headers: false,
        ..CsvWriteOptions::default()
    };
    let rows = vec![row(vec![
        ("raw", Value::String("\"".repeat(12289))),
        ("after", Value::String("é🙂".repeat(2049))),
    ])];
    let original = to_string_with_options(&raw_schema, &rows, &options).unwrap();
    assert_eq!(
        to_bytes_with_options_bounded(&raw_schema, &rows, &options, original.len()).unwrap(),
        original.as_bytes()
    );
}
