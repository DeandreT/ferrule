use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::FixedWidthLayout;

use crate::{CsvFormatError, format_row, parse_present_value, row_fields};

/// Reads fixed-width UTF-8 text into one flat group per record.
pub fn read_fixed_width(
    path: &Path,
    schema: &SchemaNode,
    layout: &FixedWidthLayout,
) -> Result<Vec<Instance>, CsvFormatError> {
    let text = std::fs::read_to_string(path)?;
    from_str_fixed_width(&text, schema, layout)
}

/// Reads fixed-width UTF-8 text from memory.
///
/// Field widths count Unicode scalar values. A leading UTF-8 BOM is ignored.
/// Delimited layouts accept LF and CRLF, while undelimited layouts consume
/// contiguous records of exactly the layout's total width.
pub fn from_str_fixed_width(
    text: &str,
    schema: &SchemaNode,
    layout: &FixedWidthLayout,
) -> Result<Vec<Instance>, CsvFormatError> {
    let fields = checked_fields(schema, layout)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.is_empty() {
        return Ok(Vec::new());
    }

    let records = if layout.record_delimiters() {
        delimited_records(text, layout.record_width())?
    } else {
        contiguous_records(text, layout.record_width())?
    };

    records
        .into_iter()
        .enumerate()
        .map(|(record, characters)| parse_record(record, &characters, &fields, layout))
        .collect()
}

/// Writes fixed-width UTF-8 records after validating the complete output.
///
/// Materializing the text before opening `path` keeps an existing file intact
/// when a row has an invalid shape/type or a value exceeds its field width.
pub fn write_fixed_width(
    path: &Path,
    schema: &SchemaNode,
    rows: &[Instance],
    layout: &FixedWidthLayout,
) -> Result<(), CsvFormatError> {
    let text = to_string_fixed_width(schema, rows, layout)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Formats flat rows as fixed-width UTF-8 text in memory.
pub fn to_string_fixed_width(
    schema: &SchemaNode,
    rows: &[Instance],
    layout: &FixedWidthLayout,
) -> Result<String, CsvFormatError> {
    let fields = checked_fields(schema, layout)?;
    let mut output = String::new();

    for (row, instance) in rows.iter().enumerate() {
        let values = format_row(row, instance, &fields)?;
        for (((field, _), value), width) in fields.iter().zip(values).zip(layout.field_widths()) {
            let width = width.get() as usize;
            let got = value.chars().count();
            if got > width {
                return Err(CsvFormatError::FixedWidthFieldOverflow {
                    row,
                    field: (*field).to_string(),
                    width,
                    got,
                });
            }
            output.push_str(&value);
            output.extend(std::iter::repeat_n(layout.fill_char(), width - got));
        }
        if layout.record_delimiters() {
            output.push('\n');
        }
    }

    Ok(output)
}

fn checked_fields<'a>(
    schema: &'a SchemaNode,
    layout: &FixedWidthLayout,
) -> Result<Vec<(&'a str, ScalarType)>, CsvFormatError> {
    let fields = row_fields(schema)?;
    if fields.len() != layout.field_widths().len() {
        return Err(CsvFormatError::FixedWidthFieldCount {
            expected: fields.len(),
            got: layout.field_widths().len(),
        });
    }
    Ok(fields)
}

fn delimited_records(text: &str, expected: usize) -> Result<Vec<Vec<char>>, CsvFormatError> {
    text.split_inclusive('\n')
        .enumerate()
        .map(|(record, raw)| {
            let raw = if let Some(without_lf) = raw.strip_suffix('\n') {
                without_lf.strip_suffix('\r').unwrap_or(without_lf)
            } else {
                raw
            };
            checked_record(record, raw.chars().collect(), expected)
        })
        .collect()
}

fn contiguous_records(text: &str, expected: usize) -> Result<Vec<Vec<char>>, CsvFormatError> {
    let characters: Vec<_> = text.chars().collect();
    characters
        .chunks(expected)
        .enumerate()
        .map(|(record, chunk)| checked_record(record, chunk.to_vec(), expected))
        .collect()
}

fn checked_record(
    record: usize,
    characters: Vec<char>,
    expected: usize,
) -> Result<Vec<char>, CsvFormatError> {
    let got = characters.len();
    if got < expected {
        return Err(CsvFormatError::PartialFixedWidthRecord {
            record,
            expected,
            got,
        });
    }
    if got > expected {
        return Err(CsvFormatError::FixedWidthRecordOverflow {
            record,
            expected,
            got,
        });
    }
    Ok(characters)
}

fn parse_record(
    record: usize,
    characters: &[char],
    fields: &[(&str, ScalarType)],
    layout: &FixedWidthLayout,
) -> Result<Instance, CsvFormatError> {
    let mut offset = 0;
    let mut values = Vec::with_capacity(fields.len());
    for ((name, ty), width) in fields.iter().zip(layout.field_widths()) {
        let width = width.get() as usize;
        let raw: String = characters[offset..offset + width].iter().collect();
        let value = raw.trim_end_matches(layout.fill_char());
        let value = if value.is_empty() && layout.treat_empty_as_absent() {
            Value::Null
        } else {
            parse_present_value(name, *ty, value, record)?
        };
        values.push((name.to_string(), Instance::Scalar(value)));
        offset += width;
    }
    Ok(Instance::Group(values))
}

#[cfg(test)]
mod tests {
    use mapping::FixedFieldWidth;

    use super::*;

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
                SchemaNode::scalar("active", ScalarType::Bool),
            ],
        )
    }

    fn layout(
        fill: char,
        record_delimiters: bool,
        treat_empty_as_absent: bool,
    ) -> FixedWidthLayout {
        FixedWidthLayout::new(
            [6, 3, 5]
                .into_iter()
                .map(|width| FixedFieldWidth::new(width).unwrap())
                .collect(),
            fill,
            record_delimiters,
            treat_empty_as_absent,
        )
        .unwrap()
    }

    fn row(name: Value, age: Value, active: Value) -> Instance {
        Instance::Group(vec![
            ("name".into(), Instance::Scalar(name)),
            ("age".into(), Instance::Scalar(age)),
            ("active".into(), Instance::Scalar(active)),
        ])
    }

    #[test]
    fn unicode_width_bom_and_crlf_are_supported() {
        let text = "\u{feff}José__29_true_\r\n李雷____7__false\r\n";
        let rows = from_str_fixed_width(text, &schema(), &layout('_', true, true)).unwrap();

        assert_eq!(
            rows,
            vec![
                row(
                    Value::String("José".into()),
                    Value::Int(29),
                    Value::Bool(true)
                ),
                row(
                    Value::String("李雷".into()),
                    Value::Int(7),
                    Value::Bool(false)
                )
            ]
        );
    }

    #[test]
    fn contiguous_records_roundtrip_without_byte_slicing() {
        let rows = vec![
            row(
                Value::String("李雷".into()),
                Value::Int(7),
                Value::Bool(true),
            ),
            row(
                Value::String("Ana".into()),
                Value::Int(42),
                Value::Bool(false),
            ),
        ];
        let layout = layout(' ', false, true);

        let text = to_string_fixed_width(&schema(), &rows, &layout).unwrap();
        let read_back = from_str_fixed_width(&text, &schema(), &layout).unwrap();

        assert_eq!(text.chars().count(), layout.record_width() * 2);
        assert_eq!(read_back, rows);
    }

    #[test]
    fn filesystem_io_roundtrips_with_lf_and_padding() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_csv_fixed_width_roundtrip_{}.txt",
            std::process::id()
        ));
        let rows = vec![row(
            Value::String("Jane".into()),
            Value::Int(29),
            Value::Bool(true),
        )];
        let layout = layout('_', true, true);

        write_fixed_width(&path, &schema(), &rows, &layout).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let read_back = read_fixed_width(&path, &schema(), &layout).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(text, "Jane__29_true_\n");
        assert_eq!(read_back, rows);
    }

    #[test]
    fn fill_only_fields_follow_the_empty_policy() {
        let absent =
            from_str_fixed_width("Ana______true_\n", &schema(), &layout('_', true, true)).unwrap();
        assert_eq!(
            absent,
            vec![row(
                Value::String("Ana".into()),
                Value::Null,
                Value::Bool(true)
            )]
        );

        let one_string_schema =
            SchemaNode::group("row", vec![SchemaNode::scalar("value", ScalarType::String)]);
        let one_field =
            FixedWidthLayout::new(vec![FixedFieldWidth::new(3).unwrap()], '_', true, false)
                .unwrap();
        let present = from_str_fixed_width("___\n", &one_string_schema, &one_field).unwrap();
        assert_eq!(present, vec![row_one(Value::String(String::new()))]);

        let absent_layout =
            FixedWidthLayout::new(vec![FixedFieldWidth::new(3).unwrap()], '_', true, true).unwrap();
        let absent = from_str_fixed_width("___\n", &one_string_schema, &absent_layout).unwrap();
        assert_eq!(absent, vec![row_one(Value::Null)]);
    }

    #[test]
    fn leading_fill_characters_are_preserved() {
        let one_string_schema =
            SchemaNode::group("row", vec![SchemaNode::scalar("value", ScalarType::String)]);
        let layout =
            FixedWidthLayout::new(vec![FixedFieldWidth::new(5).unwrap()], '@', true, true).unwrap();

        let rows = from_str_fixed_width("@@A@@\n", &one_string_schema, &layout).unwrap();

        assert_eq!(rows, vec![row_one(Value::String("@@A".into()))]);
    }

    fn row_one(value: Value) -> Instance {
        Instance::Group(vec![("value".into(), Instance::Scalar(value))])
    }

    #[test]
    fn record_boundaries_and_layout_width_count_are_validated() {
        let layout = layout(' ', true, true);
        assert!(matches!(
            from_str_fixed_width("Ana   42 true \nshort\n", &schema(), &layout),
            Err(CsvFormatError::PartialFixedWidthRecord {
                record: 1,
                expected: 14,
                got: 5
            })
        ));
        assert!(matches!(
            from_str_fixed_width("Ana   42 true extra\n", &schema(), &layout),
            Err(CsvFormatError::FixedWidthRecordOverflow { record: 0, .. })
        ));

        let wrong_layout =
            FixedWidthLayout::new(vec![FixedFieldWidth::new(14).unwrap()], ' ', true, true)
                .unwrap();
        assert!(matches!(
            from_str_fixed_width("anything      \n", &schema(), &wrong_layout),
            Err(CsvFormatError::FixedWidthFieldCount {
                expected: 3,
                got: 1
            })
        ));
    }

    #[test]
    fn nested_or_repeating_schemas_are_rejected() {
        let nested = SchemaNode::group("row", vec![SchemaNode::group("child", Vec::new())]);
        let repeating = SchemaNode::group(
            "row",
            vec![SchemaNode::scalar("value", ScalarType::String).repeating()],
        );
        let one_field =
            FixedWidthLayout::new(vec![FixedFieldWidth::new(3).unwrap()], ' ', true, true).unwrap();

        assert!(matches!(
            from_str_fixed_width("abc\n", &nested, &one_field),
            Err(CsvFormatError::UnsupportedSchema)
        ));
        assert!(matches!(
            from_str_fixed_width("abc\n", &repeating, &one_field),
            Err(CsvFormatError::UnsupportedSchema)
        ));
    }

    #[test]
    fn typed_parse_errors_report_the_record_and_field() {
        assert!(matches!(
            from_str_fixed_width(
                "Ana___badtrue_\n",
                &schema(),
                &layout('_', true, true)
            ),
            Err(CsvFormatError::Parse {
                row: 0,
                field,
                expected: ScalarType::Int,
                ..
            }) if field == "age"
        ));
    }

    #[test]
    fn overflow_does_not_replace_an_existing_file() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_csv_fixed_width_atomic_{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "existing output\n").unwrap();
        let too_wide = row(
            Value::String("longer than six".into()),
            Value::Int(3),
            Value::Bool(true),
        );

        let error =
            write_fixed_width(&path, &schema(), &[too_wide], &layout(' ', true, true)).unwrap_err();
        let unchanged = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert!(matches!(
            error,
            CsvFormatError::FixedWidthFieldOverflow {
                row: 0,
                field,
                width: 6,
                ..
            } if field == "name"
        ));
        assert_eq!(unchanged, "existing output\n");
    }

    #[test]
    fn empty_input_is_an_empty_record_set_and_blank_lines_are_not() {
        assert_eq!(
            from_str_fixed_width("", &schema(), &layout(' ', true, true)).unwrap(),
            Vec::<Instance>::new()
        );
        assert!(matches!(
            from_str_fixed_width("\n", &schema(), &layout(' ', true, true)),
            Err(CsvFormatError::PartialFixedWidthRecord { got: 0, .. })
        ));
    }
}
