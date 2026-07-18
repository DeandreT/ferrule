//! GS1 TRADACOMS input.
//!
//! Segments use `TAG=...` with `+` elements, `:` components, `'` segment
//! terminators, and `?` release escaping. Hierarchy comes from the imported
//! configuration schema just as it does for EDIFACT and X12.

use std::path::Path;

use ir::{Instance, SchemaNode};

use crate::segments::{Segment, read_segments};
use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES, read_bounded_input};

/// Tokenizes one TRADACOMS interchange.
pub fn tokenize(text: &str) -> Result<Vec<Segment>, EdiFormatError> {
    if text.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(EdiFormatError::NotTradacoms(
            "input exceeds the 64 MiB limit",
        ));
    }

    let mut segments = Vec::new();
    let mut id = String::new();
    let mut in_body = false;
    let mut elements = Vec::new();
    let mut components = Vec::new();
    let mut component = String::new();
    let mut characters = text.trim_start_matches('\u{feff}').chars();
    while let Some(character) = characters.next() {
        if character == '?' {
            let escaped = characters
                .next()
                .ok_or(EdiFormatError::NotTradacoms("dangling release character"))?;
            if in_body {
                component.push(escaped);
            } else {
                id.push(escaped);
            }
            continue;
        }

        if !in_body {
            match character {
                '=' => {
                    id = id.trim().to_string();
                    validate_segment_id(&id)?;
                    in_body = true;
                }
                '\'' if id.trim().is_empty() => id.clear(),
                '\'' => {
                    return Err(EdiFormatError::NotTradacoms("segment has no `=` separator"));
                }
                value if id.is_empty() && value.is_whitespace() => {}
                value => id.push(value),
            }
            continue;
        }

        match character {
            ':' => components.push(std::mem::take(&mut component)),
            '+' => finish_element(&mut elements, &mut components, &mut component),
            '\'' => {
                finish_element(&mut elements, &mut components, &mut component);
                segments.push(Segment {
                    id: std::mem::take(&mut id),
                    elements: std::mem::take(&mut elements),
                });
                in_body = false;
            }
            value => component.push(value),
        }
    }
    if in_body {
        finish_element(&mut elements, &mut components, &mut component);
        segments.push(Segment { id, elements });
    } else if !id.trim().is_empty() {
        return Err(EdiFormatError::NotTradacoms("segment has no `=` separator"));
    }
    if segments.first().is_none_or(|segment| segment.id != "STX") {
        return Err(EdiFormatError::NotTradacoms(
            "interchange must start with STX",
        ));
    }
    Ok(segments)
}

fn finish_element(
    elements: &mut Vec<Vec<Vec<String>>>,
    components: &mut Vec<String>,
    component: &mut String,
) {
    components.push(std::mem::take(component));
    elements.push(vec![std::mem::take(components)]);
}

fn validate_segment_id(id: &str) -> Result<(), EdiFormatError> {
    if id.len() == 3
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        Ok(())
    } else {
        Err(EdiFormatError::NotTradacoms(
            "segment ID must be three alphanumerics",
        ))
    }
}

/// Reads a TRADACOMS interchange.
pub fn read(path: &Path, schema: &SchemaNode, lenient: bool) -> Result<Instance, EdiFormatError> {
    let bytes = read_bounded_input(
        path,
        EdiFormatError::NotTradacoms("input exceeds the 64 MiB limit"),
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| EdiFormatError::NotTradacoms("input is not UTF-8"))?;
    let segments = tokenize(text)?;
    read_segments(schema, &segments, ':', None, lenient)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};

    #[test]
    fn tokenizes_elements_components_and_release_escapes() {
        let segments = tokenize("STX=ANA:1+SENDER?'S?+NAME+PART?:TWO'MHD=1+INVFIL:9'").unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].elements[0][0], ["ANA", "1"]);
        assert_eq!(segments[0].elements[1][0][0], "SENDER'S+NAME");
        assert_eq!(segments[0].elements[2][0][0], "PART:TWO");
    }

    #[test]
    fn reads_a_schema_guided_interchange() {
        let schema = SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchange",
                    vec![
                        SchemaNode::group(
                            "STX",
                            vec![SchemaNode::group(
                                "Syntax",
                                vec![
                                    SchemaNode::scalar("Code", ScalarType::String),
                                    SchemaNode::scalar("Version", ScalarType::Int),
                                ],
                            )],
                        ),
                        SchemaNode::group(
                            "END",
                            vec![SchemaNode::scalar("Count", ScalarType::Int)],
                        ),
                    ],
                )
                .repeating(),
            ],
        );
        let path =
            std::env::temp_dir().join(format!("ferrule_tradacoms_{}.edi", std::process::id()));
        std::fs::write(&path, "STX=ANA:1'END=1'").unwrap();
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            instance
                .field("Interchange")
                .and_then(Instance::as_repeated)
                .and_then(|items| items[0].field("END"))
                .and_then(|end| end.field("Count"))
                .and_then(Instance::as_scalar),
            Some(&Value::Int(1))
        );
    }

    #[test]
    fn reads_nested_message_loops_selected_by_a_fixed_header() {
        let message_header = SchemaNode::group(
            "MHD",
            vec![
                SchemaNode::scalar("Reference", ScalarType::Int),
                SchemaNode::group(
                    "Type",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String).fixed("ORDER"),
                        SchemaNode::scalar("Version", ScalarType::Int).fixed("1"),
                    ],
                ),
            ],
        );
        let message = SchemaNode::group(
            "Message_ORDER",
            vec![SchemaNode::group(
                "ORDER",
                vec![
                    message_header,
                    SchemaNode::group("ODT", vec![SchemaNode::scalar("Date", ScalarType::Int)]),
                ],
            )],
        )
        .repeating();
        let schema = SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchange",
                    vec![
                        SchemaNode::group(
                            "STX",
                            vec![SchemaNode::scalar("Syntax", ScalarType::String)],
                        ),
                        SchemaNode::group("Batch", vec![message]).repeating(),
                        SchemaNode::group(
                            "END",
                            vec![SchemaNode::scalar("Count", ScalarType::Int)],
                        ),
                    ],
                )
                .repeating(),
            ],
        );
        let path = std::env::temp_dir().join(format!(
            "ferrule_tradacoms_nested_{}.edi",
            std::process::id()
        ));
        std::fs::write(&path, "STX=ANA'MHD=1+ORDER:1'ODT=250101'END=1'").unwrap();
        let instance = read(&path, &schema, true).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(
            instance
                .field("Interchange")
                .and_then(Instance::as_repeated)
                .and_then(|items| items.first())
                .and_then(|interchange| interchange.field("Batch"))
                .and_then(Instance::as_repeated)
                .and_then(|items| items.first())
                .and_then(|batch| batch.field("Message_ORDER"))
                .and_then(Instance::as_repeated)
                .and_then(|items| items.first())
                .and_then(|message| message.field("ORDER"))
                .and_then(|order| order.field("ODT"))
                .and_then(|date| date.field("Date"))
                .and_then(Instance::as_scalar),
            Some(&Value::Int(250101))
        );
    }
}
