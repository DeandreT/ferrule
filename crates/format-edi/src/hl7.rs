//! HL7 v2 pipe-encoding input and output.
//!
//! HL7 declares its delimiters in the first header (`FHS`, `BHS`, or
//! `MSH`). Fields repeat with the repetition character, split into
//! components, and may contain one subcomponent level. Message hierarchy
//! still comes from the ordinary ferrule EDI schema.

use std::path::Path;

use ir::{Instance, SchemaNode};

use crate::segments::{
    Segment, WriteOptions, WriteStyle, read_segments_with_subcomponent_escape, write_segments,
};
use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES, read_bounded_input};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Separators {
    field: char,
    component: char,
    repetition: char,
    escape: char,
    subcomponent: char,
}

const WRITE_OPTIONS: WriteOptions = WriteOptions {
    element: '|',
    component: '^',
    terminator: '\r',
    release: Some('\\'),
    repetition: Some('~'),
    style: WriteStyle::Hl7 { subcomponent: '&' },
    interchange_version: None,
};

/// Tokenizes an HL7 v2 file into the dialect-neutral segment model.
pub fn tokenize(text: &str) -> Result<Vec<Segment>, EdiFormatError> {
    tokenize_with_separators(text).map(|(segments, _)| segments)
}

fn tokenize_with_separators(text: &str) -> Result<(Vec<Segment>, Separators), EdiFormatError> {
    if text.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(EdiFormatError::NotHl7("input exceeds the 64 MiB limit"));
    }
    let lines = text
        .split(['\r', '\n'])
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let first = lines
        .first()
        .ok_or(EdiFormatError::NotHl7("message stream is empty"))?;
    let separators = header_separators(first)?;
    let mut segments = Vec::with_capacity(lines.len());
    for line in lines {
        segments.push(tokenize_line(line, separators)?);
    }
    Ok((segments, separators))
}

fn header_separators(line: &str) -> Result<Separators, EdiFormatError> {
    let mut characters = line.chars();
    let id = characters.by_ref().take(3).collect::<String>();
    if !matches!(id.as_str(), "FHS" | "BHS" | "MSH") {
        return Err(EdiFormatError::NotHl7(
            "stream must start with FHS, BHS, or MSH",
        ));
    }
    let field = characters
        .next()
        .ok_or(EdiFormatError::NotHl7("header has no field separator"))?;
    let encoding = characters
        .take_while(|character| *character != field)
        .collect::<Vec<_>>();
    if encoding.len() < 4 {
        return Err(EdiFormatError::NotHl7(
            "header encoding characters must declare ^~\\& equivalents",
        ));
    }
    let separators = Separators {
        field,
        component: encoding[0],
        repetition: encoding[1],
        escape: encoding[2],
        subcomponent: encoding[3],
    };
    let mut distinct = [
        separators.field,
        separators.component,
        separators.repetition,
        separators.escape,
        separators.subcomponent,
    ];
    distinct.sort_unstable();
    if distinct.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(EdiFormatError::NotHl7("header delimiters must be distinct"));
    }
    Ok(separators)
}

fn tokenize_line(line: &str, separators: Separators) -> Result<Segment, EdiFormatError> {
    let mut characters = line.chars();
    let id = characters.by_ref().take(3).collect::<String>();
    if id.len() != 3
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(EdiFormatError::NotHl7(
            "segment ID must be three alphanumerics",
        ));
    }
    if characters.next() != Some(separators.field) {
        return Err(EdiFormatError::NotHl7(
            "segment does not use the declared field separator",
        ));
    }
    let body = characters.collect::<String>();
    let header = matches!(id.as_str(), "FHS" | "BHS" | "MSH");
    let mut raw_fields = body.split(separators.field);
    let mut elements = Vec::new();
    if header {
        let encoding = raw_fields
            .next()
            .ok_or(EdiFormatError::NotHl7("header has no encoding characters"))?;
        elements.push(vec![vec![separators.field.to_string()]]);
        elements.push(vec![vec![encoding.to_string()]]);
    }
    elements.extend(raw_fields.map(|field| tokenize_field(field, separators)));
    Ok(Segment { id, elements })
}

fn tokenize_field(field: &str, separators: Separators) -> Vec<Vec<String>> {
    field
        .split(separators.repetition)
        .map(|repeat| {
            repeat
                .split(separators.component)
                .map(|component| decode_escapes(component, separators))
                .collect()
        })
        .collect()
}

fn decode_escapes(value: &str, separators: Separators) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut parts = value.split(separators.escape);
    decoded.push_str(parts.next().unwrap_or_default());
    let mut escaped = true;
    for part in parts {
        if escaped {
            match part {
                "F" => decoded.push(separators.field),
                "S" => decoded.push(separators.component),
                "R" => decoded.push(separators.repetition),
                // Preserve the encoded form until schema-guided
                // subcomponent splitting has finished.
                "T" => {
                    decoded.push(separators.escape);
                    decoded.push('T');
                    decoded.push(separators.escape);
                }
                "E" => decoded.push(separators.escape),
                other => {
                    decoded.push(separators.escape);
                    decoded.push_str(other);
                }
            }
        } else {
            decoded.push_str(part);
        }
        escaped = !escaped;
    }
    decoded
}

/// Reads an HL7 v2 message stream using its embedded delimiter declaration.
pub fn read(path: &Path, schema: &SchemaNode, lenient: bool) -> Result<Instance, EdiFormatError> {
    let bytes = read_bounded_input(
        path,
        EdiFormatError::NotHl7("input exceeds the 64 MiB limit"),
    )?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| EdiFormatError::NotHl7("input is not UTF-8"))?;
    let (segments, separators) = tokenize_with_separators(text)?;
    read_segments_with_subcomponent_escape(
        schema,
        &segments,
        separators.component,
        separators.subcomponent,
        separators.escape,
        lenient,
    )
}

/// Writes an HL7 v2 message stream using the standard `|^~\\&` encoding.
/// Header separator fields are derived from that encoding when absent and
/// rejected when a supplied value disagrees.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), EdiFormatError> {
    let output = write_segments(schema, instance, &WRITE_OPTIONS)?;
    std::fs::write(path, output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};

    #[test]
    fn tokenizes_declared_fields_repetitions_and_components() {
        let segments = tokenize(
            "MSH|^~\\&|SEND|RECV|||20250101||ADT^A28^ADT_A28\r\
             PID|||one~two^^^AUTH||Family&Prefix^Given",
        )
        .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].elements[0][0][0], "|");
        assert_eq!(segments[0].elements[1][0][0], "^~\\&");
        assert_eq!(segments[1].elements[2].len(), 2);
        assert_eq!(segments[1].elements[4][0][0], "Family&Prefix");
    }

    #[test]
    fn preserves_field_whitespace() {
        let segments = tokenize("MSH|^~\\&|SEND\rOBX| value ").unwrap();
        assert_eq!(segments[1].elements[0][0][0], " value ");
    }

    #[test]
    fn lenient_partial_schema_reads_multiple_message_families_behind_headers()
    -> Result<(), Box<dyn std::error::Error>> {
        let query = SchemaNode::group(
            "Message_VXQ_V01",
            vec![
                SchemaNode::group("QRD", vec![SchemaNode::scalar("QRD-1", ScalarType::String)]),
                SchemaNode::group("QRF", vec![SchemaNode::scalar("QRF-1", ScalarType::String)]),
            ],
        )
        .repeating();
        let update = SchemaNode::group(
            "Message_VXU_V04",
            vec![
                SchemaNode::group("PID", vec![SchemaNode::scalar("PID-1", ScalarType::String)]),
                SchemaNode::group("RXA", vec![SchemaNode::scalar("RXA-1", ScalarType::String)]),
            ],
        )
        .repeating();
        let schema = SchemaNode::group(
            "HL7",
            vec![SchemaNode::group("GroupBHS", vec![query, update]).repeating()],
        );
        let path = std::env::temp_dir().join(format!(
            "ferrule_hl7_partial_multi_message_{}.hl7",
            std::process::id()
        ));
        std::fs::write(
            &path,
            "FHS|^~\\&\rBHS|^~\\&\r\
             MSH|^~\\&|APP\rQRD|query\rQRF|filter\r\
             MSH|^~\\&|APP\rPID|one\rRXA|dose-one\r\
             MSH|^~\\&|APP\rPID|two\rRXA|dose-two\r\
             BTS|3\rFTS|1",
        )?;

        let result = read(&path, &schema, true);
        std::fs::remove_file(path)?;
        let instance = result?;
        let Some(batch) = instance
            .field("GroupBHS")
            .and_then(Instance::as_repeated)
            .and_then(|batches| batches.first())
        else {
            panic!("partial HL7 schema must materialize one batch");
        };
        assert_eq!(
            batch
                .field("Message_VXQ_V01")
                .and_then(Instance::as_repeated)
                .map(<[Instance]>::len),
            Some(1)
        );
        let Some(updates) = batch
            .field("Message_VXU_V04")
            .and_then(Instance::as_repeated)
        else {
            panic!("partial HL7 schema must materialize update messages");
        };
        assert_eq!(updates.len(), 2);
        assert_eq!(
            updates
                .get(1)
                .and_then(|message| message.field("PID"))
                .and_then(|pid| pid.field("PID-1"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("two".into()))
        );
        Ok(())
    }

    #[test]
    fn reads_nested_subcomponents() {
        let schema = SchemaNode::group(
            "ADT_A28",
            vec![
                SchemaNode::group(
                    "MSH",
                    vec![
                        SchemaNode::scalar("MSH-1", ScalarType::String),
                        SchemaNode::scalar("MSH-2", ScalarType::String),
                        SchemaNode::scalar("MSH-3", ScalarType::String),
                    ],
                ),
                SchemaNode::group(
                    "PID",
                    vec![SchemaNode::group(
                        "PID-1",
                        vec![SchemaNode::group(
                            "XPN-1",
                            vec![
                                SchemaNode::scalar("FN-1", ScalarType::String),
                                SchemaNode::scalar("FN-2", ScalarType::String),
                            ],
                        )],
                    )],
                ),
            ],
        );
        let path = std::env::temp_dir().join(format!("ferrule_hl7_{}.hl7", std::process::id()));
        std::fs::write(&path, "MSH|^~\\&|SEND\rPID|Family&Prefix").unwrap();
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            instance
                .field("PID")
                .and_then(|pid| pid.field("PID-1"))
                .and_then(|name| name.field("XPN-1"))
                .and_then(|family| family.field("FN-2"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("Prefix".into()))
        );
    }

    #[test]
    fn escaped_subcomponent_delimiter_remains_literal_data() {
        let schema = SchemaNode::group(
            "ADT_A28",
            vec![
                SchemaNode::group(
                    "MSH",
                    vec![
                        SchemaNode::scalar("MSH-1", ScalarType::String),
                        SchemaNode::scalar("MSH-2", ScalarType::String),
                    ],
                ),
                SchemaNode::group(
                    "PID",
                    vec![SchemaNode::group(
                        "PID-1",
                        vec![
                            SchemaNode::scalar("Part-1", ScalarType::String),
                            SchemaNode::scalar("Part-2", ScalarType::String),
                        ],
                    )],
                ),
            ],
        );
        let path = std::env::temp_dir().join(format!(
            "ferrule_hl7_escaped_subcomponent_{}.hl7",
            std::process::id()
        ));
        std::fs::write(&path, "MSH|^~\\&\rPID|Family\\T\\Prefix").unwrap();
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(path).unwrap();

        let name = instance.field("PID").and_then(|pid| pid.field("PID-1"));
        assert_eq!(
            name.and_then(|value| value.field("Part-1"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("Family&Prefix".into()))
        );
        assert_eq!(
            name.and_then(|value| value.field("Part-2"))
                .and_then(Instance::as_scalar),
            Some(&Value::Null)
        );
    }

    #[test]
    fn writes_headers_repetitions_subcomponents_and_escape_codes() {
        let mut repetitions = SchemaNode::scalar("PID-3", ScalarType::String);
        repetitions.repeating = true;
        let schema = SchemaNode::group(
            "ADT_A28",
            vec![
                SchemaNode::group(
                    "MSH",
                    vec![
                        SchemaNode::scalar("MSH-1", ScalarType::String),
                        SchemaNode::scalar("MSH-2", ScalarType::String),
                        SchemaNode::scalar("MSH-3", ScalarType::String),
                    ],
                ),
                SchemaNode::group(
                    "PID",
                    vec![
                        SchemaNode::scalar("PID-1", ScalarType::Int),
                        SchemaNode::scalar("PID-2", ScalarType::String),
                        repetitions,
                        SchemaNode::group(
                            "PID-4",
                            vec![
                                SchemaNode::group(
                                    "CX-1",
                                    vec![
                                        SchemaNode::scalar("Part-1", ScalarType::String),
                                        SchemaNode::scalar("Part-2", ScalarType::String),
                                    ],
                                ),
                                SchemaNode::scalar("CX-2", ScalarType::String),
                            ],
                        ),
                    ],
                ),
            ],
        );
        let instance = Instance::Group(vec![
            (
                "MSH".into(),
                Instance::Group(vec![
                    ("MSH-1".into(), Instance::Scalar(Value::String("|".into()))),
                    (
                        "MSH-2".into(),
                        Instance::Scalar(Value::String("^~\\&".into())),
                    ),
                    (
                        "MSH-3".into(),
                        Instance::Scalar(Value::String("SEND|APP^~\\&".into())),
                    ),
                ]),
            ),
            (
                "PID".into(),
                Instance::Group(vec![
                    ("PID-1".into(), Instance::Scalar(Value::Int(1))),
                    ("PID-2".into(), Instance::Scalar(Value::Null)),
                    (
                        "PID-3".into(),
                        Instance::Repeated(vec![
                            Instance::Scalar(Value::String("A".into())),
                            Instance::Scalar(Value::String("B".into())),
                        ]),
                    ),
                    (
                        "PID-4".into(),
                        Instance::Group(vec![
                            (
                                "CX-1".into(),
                                Instance::Group(vec![
                                    (
                                        "Part-1".into(),
                                        Instance::Scalar(Value::String("Family&Prefix".into())),
                                    ),
                                    (
                                        "Part-2".into(),
                                        Instance::Scalar(Value::String("Given".into())),
                                    ),
                                ]),
                            ),
                            (
                                "CX-2".into(),
                                Instance::Scalar(Value::String("AUTH".into())),
                            ),
                        ]),
                    ),
                ]),
            ),
        ]);
        let path = std::env::temp_dir().join(format!(
            "ferrule_hl7_write_{}_{}.hl7",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        assert!(write(&path, &schema, &instance).is_ok());
        let output = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            output,
            "MSH|^~\\&|SEND\\F\\APP\\S\\\\R\\\\E\\\\T\\\rPID|1||A~B|Family\\T\\Prefix&Given^AUTH\r"
        );
        let roundtrip = read(&path, &schema, false);
        let _ = std::fs::remove_file(path);
        let Ok(roundtrip) = roundtrip else {
            panic!("written HL7 must read back");
        };
        assert_eq!(roundtrip, instance);
    }

    #[test]
    fn rejects_conflicting_header_separators() {
        let schema = SchemaNode::group(
            "Message",
            vec![SchemaNode::group(
                "MSH",
                vec![
                    SchemaNode::scalar("MSH-1", ScalarType::String),
                    SchemaNode::scalar("MSH-2", ScalarType::String),
                ],
            )],
        );
        let instance = Instance::Group(vec![(
            "MSH".into(),
            Instance::Group(vec![
                ("MSH-1".into(), Instance::Scalar(Value::String("*".into()))),
                ("MSH-2".into(), Instance::Scalar(Value::Null)),
            ]),
        )]);
        let path =
            std::env::temp_dir().join(format!("ferrule_hl7_separator_{}.hl7", std::process::id()));
        assert!(matches!(
            write(&path, &schema, &instance),
            Err(EdiFormatError::InvalidEnvelopeElement { element, .. }) if element == "MSH-1"
        ));
    }
}
