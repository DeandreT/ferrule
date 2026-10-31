use super::*;
use crate::MAX_WORKSHEET_ROW;
use calamine::Data;
use rust_xlsxwriter::Workbook;

fn temp_file(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ferrule-xlsx-{name}-{}-{}.xlsx",
        std::process::id(),
        crate::test_support::thread_file_label()
    ))
}

#[test]
fn transposed_reader_uses_driver_cells_and_physical_column_positions() {
    let schema = SchemaNode::group(
        "records",
        vec![
            SchemaNode::scalar("n", ScalarType::Int),
            SchemaNode::scalar("month", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Float),
            SchemaNode::scalar("closed", ScalarType::Bool),
        ],
    );
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("Columns").unwrap();
    worksheet.write_string(1, 1, "Jan").unwrap();
    worksheet.write_number(3, 1, 12.5).unwrap();
    worksheet.write_boolean(5, 1, true).unwrap();
    worksheet.write_number(3, 2, 99.0).unwrap();
    worksheet.write_string(1, 3, "Mar").unwrap();
    worksheet.write_number(3, 3, 8.0).unwrap();
    worksheet.write_boolean(5, 3, false).unwrap();
    // A value in a non-driver row must not create a record of its own.
    worksheet.write_number(3, 5, 17.0).unwrap();
    let bytes = workbook.save_to_buffer().unwrap();

    let actual = from_bytes_transposed(&bytes, &schema, Some("Columns"), &[2, 4, 6]).unwrap();

    assert_eq!(
        actual,
        vec![
            Instance::Group(
                (vec![
                    ("n".into(), Instance::Scalar(Value::Int(2))),
                    (
                        "month".into(),
                        Instance::Scalar(Value::String("Jan".into())),
                    ),
                    ("amount".into(), Instance::Scalar(Value::Float(12.5))),
                    ("closed".into(), Instance::Scalar(Value::Bool(true))),
                ])
                .into()
            ),
            Instance::Group(
                (vec![
                    ("n".into(), Instance::Scalar(Value::Int(4))),
                    (
                        "month".into(),
                        Instance::Scalar(Value::String("Mar".into())),
                    ),
                    ("amount".into(), Instance::Scalar(Value::Float(8.0))),
                    ("closed".into(), Instance::Scalar(Value::Bool(false))),
                ])
                .into()
            ),
        ]
    );
}

#[test]
fn native_transposed_wrapper_reads_generated_workbook() {
    let schema = SchemaNode::group(
        "records",
        vec![
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("value", ScalarType::Int),
        ],
    );
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.write_string(0, 0, "first").unwrap();
    worksheet.write_number(1, 0, 7.0).unwrap();
    let bytes = workbook.save_to_buffer().unwrap();
    let path = temp_file("transposed");
    std::fs::write(&path, bytes).unwrap();

    let actual = read_transposed(&path, &schema, None, &[1, 2]).unwrap();

    std::fs::remove_file(path).ok();
    assert_eq!(
        actual,
        vec![Instance::Group(
            (vec![
                (
                    "label".into(),
                    Instance::Scalar(Value::String("first".into())),
                ),
                ("value".into(), Instance::Scalar(Value::Int(7))),
            ])
            .into()
        )]
    );
}

#[test]
fn transposed_reader_validates_rows_and_synthetic_position() {
    let schema = SchemaNode::group(
        "records",
        vec![
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("n", ScalarType::Int),
            SchemaNode::scalar("value", ScalarType::String),
        ],
    );

    let count = transposed_fields(&schema, &[1]).unwrap_err();
    assert!(matches!(
        count,
        XlsxFormatError::RowCount {
            expected: 2,
            got: 1
        }
    ));
    let duplicate = transposed_fields(&schema, &[1, 1]).unwrap_err();
    assert!(matches!(duplicate, XlsxFormatError::InvalidCoordinate));
    let out_of_range = transposed_fields(&schema, &[1, MAX_WORKSHEET_ROW + 1]).unwrap_err();
    assert!(matches!(out_of_range, XlsxFormatError::InvalidCoordinate));

    let invalid_n = SchemaNode::group("records", vec![SchemaNode::scalar("n", ScalarType::Float)]);
    let error = transposed_fields(&invalid_n, &[]).unwrap_err();
    assert!(matches!(error, XlsxFormatError::UnsupportedSchema));
}

#[test]
fn transposed_reader_reports_the_selected_row_for_parse_errors() {
    let schema = SchemaNode::group(
        "records",
        vec![
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Float),
        ],
    );
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.write_string(1, 0, "Jan").unwrap();
    worksheet.write_string(3, 0, "not-a-number").unwrap();
    let bytes = workbook.save_to_buffer().unwrap();

    let error = from_bytes_transposed(&bytes, &schema, None, &[2, 4]).unwrap_err();

    assert!(matches!(
        error,
        XlsxFormatError::Parse { row: 4, field, .. } if field == "amount"
    ));
}

fn record(fields: &[(&str, Value)]) -> Instance {
    Instance::Group(
        (fields
            .iter()
            .map(|(name, value)| ((*name).to_string(), Instance::Scalar(value.clone())))
            .collect::<Vec<_>>())
        .into(),
    )
}

fn ledger(position: bool) -> SchemaNode {
    let mut fields = vec![
        SchemaNode::scalar("month", ScalarType::String),
        SchemaNode::scalar("amount", ScalarType::Float),
    ];
    if position {
        fields.push(SchemaNode::scalar("n", ScalarType::Int));
    }
    fields.push(SchemaNode::scalar("closed", ScalarType::Bool));
    SchemaNode::group("records", fields)
}

fn month(name: &str, amount: Value, closed: Value) -> Instance {
    record(&[
        ("month", Value::String(name.into())),
        ("amount", amount),
        ("closed", closed),
    ])
}

fn positioned(column: i64, name: &str, amount: Value, closed: Value) -> Instance {
    record(&[
        ("month", Value::String(name.into())),
        ("amount", amount),
        ("n", Value::Int(column)),
        ("closed", closed),
    ])
}

#[test]
fn writer_round_trips_records_from_column_a_without_a_header() {
    let records = vec![
        month("Jan", Value::Float(12.5), Value::Bool(true)),
        month("Feb", Value::Float(0.0), Value::Bool(false)),
        month("Mar", Value::Float(8.0), Value::Bool(true)),
    ];
    let bytes = to_bytes_transposed(&ledger(false), &records, Some("Columns"), &[2, 4, 6]).unwrap();

    let range = worksheet_range(&bytes, Some("Columns")).unwrap();
    assert_eq!(range.start(), Some((1, 0)));
    assert_eq!(range.get_value((1, 0)), Some(&Data::String("Jan".into())));
    assert_eq!(range.get_value((1, 2)), Some(&Data::String("Mar".into())));
    assert_eq!(range.get_value((5, 1)), Some(&Data::Bool(false)));
    assert_eq!(
        from_bytes_transposed(&bytes, &ledger(false), Some("Columns"), &[2, 4, 6]).unwrap(),
        records
    );
}

#[test]
fn writer_places_records_at_n_and_keeps_gaps_and_missing_values() {
    // Records arrive out of column order; a reader returns column order.
    let records = vec![
        positioned(9, "Sep", Value::Float(4.0), Value::Bool(true)),
        positioned(2, "Feb", Value::Null, Value::Bool(false)),
        positioned(5, "May", Value::Float(1.5), Value::Null),
    ];
    let schema = ledger(true);
    let bytes = to_bytes_transposed(&schema, &records, None, &[1, 3, 4]).unwrap();

    let range = worksheet_range(&bytes, None).unwrap();
    for column in [0, 2, 3, 5, 6, 7] {
        assert!(matches!(
            range.get_value((0, column)),
            None | Some(Data::Empty)
        ));
    }
    let path = temp_file("transposed-write");
    write_transposed(&path, &schema, &records, None, &[1, 3, 4]).unwrap();
    let actual = read_transposed(&path, &schema, None, &[1, 3, 4]).unwrap();
    std::fs::remove_file(path).ok();
    assert_eq!(
        actual,
        vec![records[1].clone(), records[2].clone(), records[0].clone()]
    );
}

#[test]
fn non_driver_empty_strings_read_back_as_missing_values() {
    let schema = SchemaNode::group(
        "records",
        vec![
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("note", ScalarType::String),
        ],
    );
    let records = vec![record(&[
        ("label", Value::String("first".into())),
        ("note", Value::String(String::new())),
    ])];
    let bytes = to_bytes_transposed(&schema, &records, None, &[1, 2]).unwrap();

    assert_eq!(
        from_bytes_transposed(&bytes, &schema, None, &[1, 2]).unwrap(),
        vec![record(&[
            ("label", Value::String("first".into())),
            ("note", Value::Null),
        ])]
    );
}

#[test]
fn writer_refuses_invalid_records_before_writing_a_file() {
    let schema = ledger(true);
    let rows = [1, 3, 4];
    let cases: Vec<(&str, Vec<Instance>)> = vec![
        (
            "zero column",
            vec![positioned(0, "Jan", Value::Null, Value::Null)],
        ),
        (
            "column past the limit",
            vec![positioned(16_385, "Jan", Value::Null, Value::Null)],
        ),
        (
            "duplicate column",
            vec![
                positioned(3, "Jan", Value::Null, Value::Null),
                positioned(3, "Feb", Value::Null, Value::Null),
            ],
        ),
        (
            "missing column",
            vec![record(&[
                ("month", Value::String("Jan".into())),
                ("amount", Value::Null),
                ("n", Value::Null),
                ("closed", Value::Null),
            ])],
        ),
        (
            "missing driver",
            vec![record(&[
                ("month", Value::Null),
                ("amount", Value::Float(1.0)),
                ("n", Value::Int(1)),
                ("closed", Value::Null),
            ])],
        ),
        (
            "empty driver",
            vec![positioned(1, "", Value::Float(1.0), Value::Null)],
        ),
    ];
    let path = temp_file("transposed-refusal");
    std::fs::remove_file(&path).ok();
    for (label, records) in cases {
        let error = write_transposed(&path, &schema, &records, None, &rows).unwrap_err();
        match label {
            "zero column" | "column past the limit" => assert!(
                matches!(
                    error,
                    XlsxFormatError::TransposedColumn {
                        record: 0,
                        reason: "must be a column number from 1 to 16384"
                    }
                ),
                "{label}: {error}"
            ),
            "duplicate column" => assert!(
                matches!(
                    error,
                    XlsxFormatError::DuplicateTransposedColumn {
                        record: 1,
                        first: 0,
                        column: 3
                    }
                ),
                "{label}: {error}"
            ),
            "missing column" => assert!(
                matches!(
                    error,
                    XlsxFormatError::TransposedColumn {
                        record: 0,
                        reason: "is missing"
                    }
                ),
                "{label}: {error}"
            ),
            _ => assert!(
                matches!(
                    &error,
                    XlsxFormatError::EmptyTransposedDriver { record: 0, field } if field == "month"
                ),
                "{label}: {error}"
            ),
        }
        assert!(!path.exists(), "{label} wrote a file");
    }

    let gap = vec![
        month("Jan", Value::Float(1.0), Value::Bool(true)),
        month("Feb", Value::Null, Value::Bool(true)),
    ];
    let error = write_transposed(&path, &ledger(false), &gap, None, &rows).unwrap_err();
    assert!(
        matches!(
            &error,
            XlsxFormatError::TransposedGap { record: 1, field } if field == "amount"
        ),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "record 1: field `amount` has no value; without an `n` field every record must fill every row, or the rest of that worksheet row would move left"
    );
    let shape = write_transposed(
        &path,
        &ledger(false),
        &[Instance::Scalar(Value::Int(1))],
        None,
        &rows,
    )
    .unwrap_err();
    assert!(matches!(shape, XlsxFormatError::RowShape { row: 0, .. }));
    let selectors = write_transposed(&path, &ledger(false), &gap, None, &[1, 2]).unwrap_err();
    assert!(matches!(
        selectors,
        XlsxFormatError::RowCount {
            expected: 3,
            got: 2
        }
    ));
    assert!(!path.exists());
}

#[test]
fn writer_without_n_refuses_more_records_than_worksheet_columns() {
    let schema = SchemaNode::group(
        "records",
        vec![SchemaNode::scalar("label", ScalarType::String)],
    );
    let records = vec![record(&[("label", Value::String("x".into()))]); 16_385];

    let error = to_bytes_transposed(&schema, &records, None, &[1]).unwrap_err();

    assert!(matches!(
        error,
        XlsxFormatError::TransposedRecordCount { count: 16_385 }
    ));
    assert!(to_bytes_transposed(&schema, &records[..16_384], None, &[1]).is_ok());
}
