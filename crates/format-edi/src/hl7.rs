//! HL7 v2 pipe-encoding input.
//!
//! HL7 declares its delimiters in the first header (`FHS`, `BHS`, or
//! `MSH`). Fields repeat with the repetition character, split into
//! components, and may contain one subcomponent level. Message hierarchy
//! still comes from the ordinary ferrule EDI schema.

use std::path::Path;

use ir::{Instance, SchemaNode};

use crate::segments::{Segment, read_segments_with_subcomponent_escape};
use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES, read_bounded_input};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Separators {
    field: char,
    component: char,
    repetition: char,
    escape: char,
    subcomponent: char,
}

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
}
