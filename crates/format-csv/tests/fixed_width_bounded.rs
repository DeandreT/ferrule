use format_csv::{
    CsvBoundedError, CsvFormatError, FixedWidthReadError, FixedWidthReadLimit, MAX_CSV_INPUT_BYTES,
    MAX_CSV_INPUT_CELL_BYTES, MAX_CSV_INPUT_COLUMNS, MAX_CSV_INPUT_NODES, MAX_CSV_INPUT_RECORDS,
    from_bytes_fixed_width_bounded, from_str_fixed_width, from_str_fixed_width_bounded,
    to_bytes_fixed_width_bounded, to_bytes_fixed_width_validated_bounded, to_string_fixed_width,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{FixedFieldWidth, FixedWidthLayout};

fn schema(columns: usize) -> SchemaNode {
    SchemaNode::group(
        "Row",
        (0..columns)
            .map(|i| SchemaNode::scalar(format!("c{i}"), ScalarType::String))
            .collect(),
    )
}

fn layout(widths: &[u32], fill: char, lines: bool, absent: bool) -> FixedWidthLayout {
    FixedWidthLayout::new(
        widths
            .iter()
            .map(|w| FixedFieldWidth::new(*w).unwrap())
            .collect(),
        fill,
        lines,
        absent,
    )
    .unwrap()
}

#[test]
fn unicode_scalars_fill_bom_and_empty_policies_match_native() {
    for lines in [false, true] {
        for absent in [false, true] {
            let layout = layout(&[3, 2], '🦀', lines, absent);
            let raw = if lines {
                "\u{feff}é😀🦀中🦀\r\n🦀🦀🦀🦀🦀\n"
            } else {
                "\u{feff}é😀🦀中🦀🦀🦀🦀🦀🦀"
            };
            let expected = from_str_fixed_width(raw, &schema(2), &layout).unwrap();
            let actual =
                from_bytes_fixed_width_bounded(raw.as_bytes(), &schema(2), &layout).unwrap();
            assert_eq!(actual, expected);
            let bytes = to_string_fixed_width(&schema(2), &actual, &layout)
                .unwrap()
                .into_bytes();
            assert_eq!(
                to_bytes_fixed_width_bounded(&schema(2), &actual, &layout, bytes.len()).unwrap(),
                bytes
            );
            assert_eq!(
                to_bytes_fixed_width_validated_bounded(&schema(2), &actual, &layout, bytes.len())
                    .unwrap(),
                bytes
            );
            assert!(matches!(
                to_bytes_fixed_width_bounded(&schema(2), &actual, &layout, bytes.len() - 1),
                Err(CsvBoundedError::OutputTooLarge { .. })
            ));
            assert!(matches!(
                to_bytes_fixed_width_validated_bounded(
                    &schema(2),
                    &actual,
                    &layout,
                    bytes.len() - 1
                ),
                Err(CsvBoundedError::OutputTooLarge { .. })
            ));
            for empty in ["", "\u{feff}"] {
                assert!(
                    from_str_fixed_width_bounded(empty, &schema(2), &layout)
                        .unwrap()
                        .is_empty()
                );
            }
        }
    }
    assert!(matches!(
        from_bytes_fixed_width_bounded(&[0xff], &schema(1), &layout(&[1], ' ', false, true)),
        Err(FixedWidthReadError::Encoding(_))
    ));
}

#[test]
fn input_cells_include_utf8_fill_and_limits_are_inclusive() {
    let width = (MAX_CSV_INPUT_CELL_BYTES / 4) as u32;
    let input = "🦀".repeat(width as usize);
    assert_eq!(
        from_str_fixed_width_bounded(&input, &schema(1), &layout(&[width], '🦀', false, false))
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        from_str_fixed_width_bounded(
            &(input + "🦀"),
            &schema(1),
            &layout(&[width + 1], '🦀', false, false)
        ),
        Err(FixedWidthReadError::Limit {
            kind: FixedWidthReadLimit::CellBytes,
            record: Some(0),
            column: Some(0),
            ..
        })
    ));
    let bytes = vec![b' '; MAX_CSV_INPUT_BYTES];
    let widths = [MAX_CSV_INPUT_CELL_BYTES as u32; 8];
    assert_eq!(
        from_bytes_fixed_width_bounded(&bytes, &schema(8), &layout(&widths, ' ', false, true))
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        from_bytes_fixed_width_bounded(
            &vec![b' '; MAX_CSV_INPUT_BYTES + 1],
            &schema(8),
            &layout(&widths, ' ', false, true)
        ),
        Err(FixedWidthReadError::Limit {
            kind: FixedWidthReadLimit::InputBytes,
            ..
        })
    ));
}

#[test]
fn records_columns_and_nodes_reject_without_partial_rows() {
    let one = layout(&[1], ' ', true, true);
    let raw = "x\n".repeat(MAX_CSV_INPUT_RECORDS);
    assert_eq!(
        from_str_fixed_width_bounded(&raw, &schema(1), &one)
            .unwrap()
            .len(),
        MAX_CSV_INPUT_RECORDS
    );
    assert!(matches!(
        from_str_fixed_width_bounded(&(raw + "x\n"), &schema(1), &one),
        Err(FixedWidthReadError::Limit {
            kind: FixedWidthReadLimit::Records,
            record: Some(MAX_CSV_INPUT_RECORDS),
            ..
        })
    ));
    let columns = MAX_CSV_INPUT_COLUMNS;
    let wide = layout(&vec![1; columns], ' ', true, true);
    let count = (MAX_CSV_INPUT_NODES - 1) / (columns + 1);
    let record = "x".repeat(columns) + "\n";
    let raw = record.repeat(count);
    assert_eq!(
        from_str_fixed_width_bounded(&raw, &schema(columns), &wide)
            .unwrap()
            .len(),
        count
    );
    assert!(
        matches!(from_str_fixed_width_bounded(&(raw + &record), &schema(columns), &wide), Err(FixedWidthReadError::Limit { kind: FixedWidthReadLimit::Nodes, record: Some(row), .. }) if row == count)
    );
    assert!(matches!(
        from_str_fixed_width_bounded(
            "",
            &schema(columns + 1),
            &layout(&vec![1; columns + 1], ' ', true, true)
        ),
        Err(FixedWidthReadError::Limit {
            kind: FixedWidthReadLimit::Columns,
            ..
        })
    ));
}

#[test]
fn huge_output_is_measured_and_native_errors_keep_their_cause() {
    let one = schema(1);
    let huge = layout(&[u32::MAX], '🦀', false, false);
    let row =
        Instance::Group(vec![("c0".into(), Instance::Scalar(Value::String("x".into())))].into());
    assert!(matches!(
        to_bytes_fixed_width_bounded(&one, std::slice::from_ref(&row), &huge, 64 * 1024 * 1024),
        Err(CsvBoundedError::OutputTooLarge { .. })
    ));
    assert_eq!(
        to_bytes_fixed_width_bounded(&one, &[], &huge, 0).unwrap(),
        b""
    );
    let bad = Instance::Scalar(Value::Null);
    assert!(matches!(
        to_bytes_fixed_width_bounded(&one, &[row, bad], &layout(&[1], ' ', false, false), 100),
        Err(CsvBoundedError::Format(CsvFormatError::RowShape {
            row: 1,
            ..
        }))
    ));
    let ints = SchemaNode::group("Row", vec![SchemaNode::scalar("c0", ScalarType::Int)]);
    assert!(matches!(
        from_str_fixed_width_bounded("bad\nx\n", &ints, &layout(&[3], ' ', true, true)),
        Err(FixedWidthReadError::Format(
            CsvFormatError::PartialFixedWidthRecord { record: 1, .. }
        ))
    ));
    for bad_row in 0..3 {
        let rows = (0..3)
            .map(|i| if i == bad_row { "bad\n" } else { "123\n" })
            .collect::<String>();
        assert!(
            matches!(from_str_fixed_width_bounded(&rows, &ints, &layout(&[3], ' ', true, true)), Err(FixedWidthReadError::Format(CsvFormatError::Parse { row, .. })) if row == bad_row)
        );
        assert_eq!(
            from_str_fixed_width_bounded("123\n123\n123\n", &ints, &layout(&[3], ' ', true, true))
                .unwrap()
                .len(),
            3
        );
    }
}

fn text_row(values: &[&str]) -> Instance {
    Instance::Group(
        values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                (
                    format!("c{i}"),
                    Instance::Scalar(Value::String((*value).into())),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    )
}

#[test]
fn complete_validation_preserves_late_field_and_row_causes_after_oversize() {
    let schema = schema(2);
    let layout = layout(&[1024, 1], '🦀', true, false);
    // The first field already exceeds the limit. A later field in that same
    // row must still report its native error, not the resource refusal.
    let same_row = vec![text_row(&["x", "xx"])];
    assert!(matches!(
        to_bytes_fixed_width_validated_bounded(&schema, &same_row, &layout, 1),
        Err(CsvBoundedError::Format(CsvFormatError::FixedWidthFieldOverflow { row: 0, field, width: 1, got: 2 })) if field == "c1"
    ));
    assert!(matches!(
        to_bytes_fixed_width_bounded(&schema, &same_row, &layout, 1),
        Err(CsvBoundedError::OutputTooLarge { .. })
    ));
    let valid = text_row(&["x", "y"]);
    for bad in [text_row(&["x", "xx"]), Instance::Scalar(Value::Null)] {
        let rows = vec![valid.clone(), bad];
        let error = to_bytes_fixed_width_validated_bounded(&schema, &rows, &layout, 1).unwrap_err();
        assert!(matches!(
            error,
            CsvBoundedError::Format(
                CsvFormatError::FixedWidthFieldOverflow { row: 1, .. }
                    | CsvFormatError::RowShape { row: 1, .. }
            )
        ));
        assert!(matches!(
            to_bytes_fixed_width_bounded(&schema, &rows, &layout, 1),
            Err(CsvBoundedError::OutputTooLarge { .. })
        ));
    }
    assert!(matches!(
        to_bytes_fixed_width_validated_bounded(&schema, std::slice::from_ref(&valid), &layout, 1),
        Err(CsvBoundedError::OutputTooLarge { maximum: 1 })
    ));
    // Correcting both errors and using an adequate budget produces the exact
    // native bytes, without carrying any failed-run state forward.
    let expected = to_string_fixed_width(&schema, std::slice::from_ref(&valid), &layout)
        .unwrap()
        .into_bytes();
    assert_eq!(
        to_bytes_fixed_width_validated_bounded(&schema, &[valid], &layout, expected.len()).unwrap(),
        expected
    );
}

#[test]
fn complete_validation_zero_budget_huge_width_and_corrected_retry() {
    let schema = schema(1);
    let huge = layout(&[u32::MAX], '🦀', false, false);
    assert_eq!(
        to_bytes_fixed_width_validated_bounded(&schema, &[], &huge, 0).unwrap(),
        b""
    );
    let rows = vec![text_row(&["😀"])];
    assert!(matches!(
        to_bytes_fixed_width_validated_bounded(&schema, &rows, &huge, 0),
        Err(CsvBoundedError::OutputTooLarge { maximum: 0 })
    ));
    let small = layout(&[2], '🦀', true, false);
    assert!(matches!(
        to_bytes_fixed_width_validated_bounded(&schema, &rows, &small, 8),
        Err(CsvBoundedError::OutputTooLarge { maximum: 8 })
    ));
    assert_eq!(
        to_bytes_fixed_width_validated_bounded(&schema, &rows, &small, 9).unwrap(),
        "😀🦀\n".as_bytes()
    );
    assert!(matches!(
        to_bytes_fixed_width_validated_bounded(&schema, &[text_row(&["123"])], &small, 0),
        Err(CsvBoundedError::Format(
            CsvFormatError::FixedWidthFieldOverflow { row: 0, .. }
        ))
    ));
    assert_eq!(
        to_bytes_fixed_width_validated_bounded(&schema, &rows, &small, 9).unwrap(),
        "😀🦀\n".as_bytes()
    );
}
