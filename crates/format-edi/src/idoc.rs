//! Schema-guided SAP IDoc fixed-record input and output.

use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode};
use mapping::{IdocLayout, IdocSegmentLayout};

use crate::segments::{Segment, scalar_or_fixed, validate_instance_shape};
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

pub fn write(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    layout: &IdocLayout,
) -> Result<(), EdiFormatError> {
    std::fs::write(path, to_bytes(schema, instance, layout)?)?;
    Ok(())
}

pub fn to_bytes(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &IdocLayout,
) -> Result<Vec<u8>, EdiFormatError> {
    validate_instance_shape(schema, instance)?;
    let mut records = Vec::new();
    let mut output_size = 0usize;
    render_node(
        schema,
        instance,
        layout,
        &mut records,
        &mut output_size,
        true,
    )?;
    let mut output = Vec::with_capacity(output_size);
    for record in records {
        output.extend_from_slice(&record);
        output.extend_from_slice(b"\r\n");
    }
    Ok(output)
}

fn render_node(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &IdocLayout,
    records: &mut Vec<Vec<u8>>,
    output_size: &mut usize,
    is_root: bool,
) -> Result<(), EdiFormatError> {
    if let Instance::Repeated(items) = instance {
        for item in items {
            render_node(schema, item, layout, records, output_size, is_root)?;
        }
        return Ok(());
    }
    if let Some(segment) = (!is_root).then(|| layout.segment(&schema.name)).flatten() {
        if records.len() >= MAX_RECORDS {
            return Err(EdiFormatError::IdocLimit("record count"));
        }
        let record = render_record(schema, instance, segment)?;
        let Some(next_size) = output_size
            .checked_add(record.len())
            .and_then(|size| size.checked_add(2))
        else {
            return Err(EdiFormatError::IdocLimit("output size"));
        };
        if next_size > MAX_RUNTIME_INPUT_BYTES {
            return Err(EdiFormatError::IdocLimit("output size"));
        }
        *output_size = next_size;
        records.push(record);
        return Ok(());
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(EdiFormatError::UnsupportedSchema(schema.name.clone()));
    };
    for child in children {
        if let Some(value) = instance.field(&child.name) {
            render_node(child, value, layout, records, output_size, false)?;
        }
    }
    Ok(())
}

fn render_record(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &IdocSegmentLayout,
) -> Result<Vec<u8>, EdiFormatError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(EdiFormatError::UnsupportedSchema(schema.name.clone()));
    };
    let record_len = layout
        .fields()
        .iter()
        .map(|field| field.last_byte().get() as usize)
        .max()
        .unwrap_or_default()
        .max(layout.name().len());
    let mut record = vec![b' '; record_len];
    let mut occupied = vec![false; record_len];
    reserve_record_range(&mut occupied, 0, layout.name().len(), layout, "<segment>")?;
    record[..layout.name().len()].copy_from_slice(layout.name().as_bytes());
    for field in layout.fields() {
        let field_schema = children
            .iter()
            .find(|child| child.name == field.name())
            .ok_or_else(|| {
                EdiFormatError::UnsupportedSchema(format!(
                    "IDoc layout field `{}` is absent from segment `{}`",
                    field.name(),
                    layout.name()
                ))
            })?;
        let value = scalar_or_fixed(
            field_schema,
            instance.field(field.name()).and_then(Instance::as_scalar),
        )?;
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(EdiFormatError::InvalidIdocOutputText {
                segment: layout.name().to_string(),
                field: field.name().to_string(),
            });
        }
        let start = field.first_byte().get() as usize - 1;
        let width = (field.last_byte().get() - field.first_byte().get() + 1) as usize;
        reserve_record_range(&mut occupied, start, width, layout, field.name())?;
        if value.len() > width {
            return Err(EdiFormatError::IdocFieldTooWide {
                segment: layout.name().to_string(),
                field: field.name().to_string(),
                width,
                actual: value.len(),
            });
        }
        let value_start = if matches!(
            field_schema.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int | ScalarType::Float
            }
        ) {
            start + width - value.len()
        } else {
            start
        };
        record[value_start..value_start + value.len()].copy_from_slice(value.as_bytes());
    }
    Ok(record)
}

fn reserve_record_range(
    occupied: &mut [bool],
    start: usize,
    width: usize,
    segment: &IdocSegmentLayout,
    field: &str,
) -> Result<(), EdiFormatError> {
    let Some(range) = occupied.get_mut(start..start + width) else {
        return Err(EdiFormatError::UnsupportedSchema(format!(
            "IDoc segment `{}` field `{field}` lies outside its record",
            segment.name()
        )));
    };
    if range.iter().any(|occupied| *occupied) {
        return Err(EdiFormatError::IdocFieldOverlap {
            segment: segment.name().to_string(),
            field: field.to_string(),
        });
    }
    range.fill(true);
    Ok(())
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

    #[test]
    fn writes_nested_repeating_records_and_roundtrips() {
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
                SchemaNode::group("Items", vec![item_schema]),
            ],
        );
        let item = |code: &str, count: i64| {
            Instance::Group(vec![
                ("code".into(), Instance::Scalar(Value::String(code.into()))),
                ("count".into(), Instance::Scalar(Value::Int(count))),
            ])
        };
        let instance = Instance::Group(vec![
            (
                "HEADER0001".into(),
                Instance::Group(vec![
                    (
                        "number".into(),
                        Instance::Scalar(Value::String("ABC12".into())),
                    ),
                    ("kind".into(), Instance::Scalar(Value::String("XY".into()))),
                ]),
            ),
            (
                "Items".into(),
                Instance::Group(vec![(
                    "ITEM000001".into(),
                    Instance::Repeated(vec![item("P100", 2), item("P200", 13)]),
                )]),
            ),
        ]);

        let bytes = to_bytes(&schema, &instance, &layout).unwrap();
        assert_eq!(
            bytes,
            b"HEADER0001 ABC12XY\r\nITEM000001 P100  2\r\nITEM000001 P200 13\r\n"
        );
        assert_eq!(
            from_bytes(&bytes, &schema, &layout, false).unwrap(),
            instance
        );
    }

    #[test]
    fn rejects_oversized_control_and_conflicting_fields() {
        let schema = SchemaNode::group(
            "IDOC",
            vec![SchemaNode::group(
                "SEGMENT",
                vec![SchemaNode::scalar("value", ScalarType::String)],
            )],
        );
        let instance = |value: &str| {
            Instance::Group(vec![(
                "SEGMENT".into(),
                Instance::Group(vec![(
                    "value".into(),
                    Instance::Scalar(Value::String(value.into())),
                )]),
            )])
        };

        let narrow = IdocLayout::new(vec![
            IdocSegmentLayout::new("SEGMENT", vec![field("value", 9, 10)]).unwrap(),
        ])
        .unwrap();
        assert!(matches!(
            to_bytes(&schema, &instance("wide"), &narrow),
            Err(EdiFormatError::IdocFieldTooWide {
                width: 2,
                actual: 4,
                ..
            })
        ));
        assert!(matches!(
            to_bytes(&schema, &instance("\n"), &narrow),
            Err(EdiFormatError::InvalidIdocOutputText { .. })
        ));

        let overlap = IdocLayout::new(vec![
            IdocSegmentLayout::new("SEGMENT", vec![field("value", 1, 7)]).unwrap(),
        ])
        .unwrap();
        assert!(matches!(
            to_bytes(&schema, &instance("DIFFER!"), &overlap),
            Err(EdiFormatError::IdocFieldOverlap { .. })
        ));
    }
}
