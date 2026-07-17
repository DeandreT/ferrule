//! Schema-guided SAP IDoc fixed-record input.

use std::path::Path;

use ir::{Instance, SchemaNode};
use mapping::IdocLayout;

use crate::segments::Segment;
use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES, read_bounded_input};

const CONTROL_RECORD: &[u8] = b"EDI_DC40";
const MAX_RECORDS: usize = 100_000;

pub fn read(
    path: &Path,
    schema: &SchemaNode,
    layout: &IdocLayout,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    let bytes = read_bounded_input(path, EdiFormatError::IdocLimit("input size"))?;
    from_bytes(&bytes, schema, layout, lenient)
}

pub fn from_bytes(
    bytes: &[u8],
    schema: &SchemaNode,
    layout: &IdocLayout,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    if bytes.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(EdiFormatError::IdocLimit("input size"));
    }
    let mut segments = Vec::new();
    let mut record_count = 0usize;
    for (index, record) in records(bytes).enumerate() {
        let record = trim_record(record);
        if record.is_empty() {
            continue;
        }
        record_count += 1;
        if record_count > MAX_RECORDS {
            return Err(EdiFormatError::IdocLimit("record count"));
        }
        let Some(segment_layout) = layout.segments().iter().find(|segment| {
            let name = segment.name().as_bytes();
            record.starts_with(name)
                && record
                    .get(name.len())
                    .is_none_or(|byte| byte.is_ascii_whitespace())
        }) else {
            if record.starts_with(CONTROL_RECORD) || lenient {
                continue;
            }
            return Err(EdiFormatError::UnrecognizedIdocSegment {
                index: index + 1,
                found: record_prefix(record),
            });
        };

        let elements = segment_layout
            .fields()
            .iter()
            .map(|field| {
                let start = field.first_byte().get() as usize - 1;
                let end = field.last_byte().get() as usize;
                let raw = record.get(start..record.len().min(end)).unwrap_or_default();
                let raw = trim_field(raw);
                let text =
                    std::str::from_utf8(raw).map_err(|_| EdiFormatError::InvalidIdocText {
                        record: index + 1,
                        field: field.name().to_string(),
                    })?;
                Ok(vec![vec![text.to_string()]])
            })
            .collect::<Result<Vec<_>, EdiFormatError>>()?;
        segments.push(Segment {
            id: segment_layout.name().to_string(),
            elements,
        });
    }

    crate::segments::read_segments(schema, &segments, ' ', None, lenient)
}

fn records(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split(|byte| matches!(byte, b'\r' | b'\n'))
}

fn trim_record(mut value: &[u8]) -> &[u8] {
    if value.starts_with(&[0xef, 0xbb, 0xbf]) {
        value = &value[3..];
    }
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | 0))
    {
        value = &value[..value.len() - 1];
    }
    value
}

fn trim_field(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | 0))
    {
        value = &value[1..];
    }
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | 0))
    {
        value = &value[..value.len() - 1];
    }
    value
}

fn record_prefix(record: &[u8]) -> String {
    String::from_utf8_lossy(&record[..record.len().min(30)])
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use ir::{ScalarType, Value};
    use mapping::{IdocFieldLayout, IdocSegmentLayout};

    use super::*;

    fn field(name: &str, first: u32, last: u32) -> IdocFieldLayout {
        IdocFieldLayout::new(
            name,
            NonZeroU32::new(first).unwrap(),
            NonZeroU32::new(last).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn reads_control_record_fixed_fields_and_repeating_groups() {
        let header = IdocSegmentLayout::new(
            "HEADER0001",
            vec![field("number", 12, 16), field("kind", 17, 18)],
        )
        .unwrap();
        let item = IdocSegmentLayout::new(
            "ITEM000001",
            vec![field("code", 12, 15), field("count", 16, 18)],
        )
        .unwrap();
        let layout = IdocLayout::new(vec![header, item]).unwrap();
        let mut item_schema = SchemaNode::group(
            "ITEM000001",
            vec![
                SchemaNode::scalar("code", ScalarType::String),
                SchemaNode::scalar("count", ScalarType::Int),
            ],
        );
        item_schema.repeating = true;
        let schema = SchemaNode::group(
            "IDOC",
            vec![
                SchemaNode::group(
                    "HEADER0001",
                    vec![
                        SchemaNode::scalar("number", ScalarType::String),
                        SchemaNode::scalar("kind", ScalarType::String),
                    ],
                ),
                item_schema,
            ],
        );
        let input =
            b"EDI_DC40 ignored\rHEADER0001 ABC12XY\rITEM000001 P100  2\rITEM000001 P200 13\r";

        let value = from_bytes(input, &schema, &layout, false).unwrap();
        assert_eq!(
            value
                .field("HEADER0001")
                .unwrap()
                .field("number")
                .unwrap()
                .as_scalar(),
            Some(&Value::String("ABC12".into()))
        );
        let items = value.field("ITEM000001").unwrap().as_repeated().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[1].field("count").unwrap().as_scalar(),
            Some(&Value::Int(13))
        );
    }

    #[test]
    fn rejects_unknown_records_unless_lenient() {
        let layout = IdocLayout::new(vec![
            IdocSegmentLayout::new("KNOWN00001", vec![field("value", 12, 13)]).unwrap(),
        ])
        .unwrap();
        let schema = SchemaNode::group(
            "IDOC",
            vec![SchemaNode::group(
                "KNOWN00001",
                vec![SchemaNode::scalar("value", ScalarType::String)],
            )],
        );
        assert!(matches!(
            from_bytes(b"UNKNOWN000 xx\r", &schema, &layout, false),
            Err(EdiFormatError::UnrecognizedIdocSegment { .. })
        ));
        assert!(from_bytes(b"UNKNOWN000 xx\rKNOWN00001 ok\r", &schema, &layout, true).is_ok());
    }

    #[test]
    fn bounds_non_empty_record_count() {
        let layout = IdocLayout::new(vec![
            IdocSegmentLayout::new("KNOWN00001", vec![field("value", 12, 13)]).unwrap(),
        ])
        .unwrap();
        let schema = SchemaNode::group("IDOC", Vec::new());
        let input = "X\n".repeat(MAX_RECORDS + 1);

        assert!(matches!(
            from_bytes(input.as_bytes(), &schema, &layout, true),
            Err(EdiFormatError::IdocLimit("record count"))
        ));
    }
}
