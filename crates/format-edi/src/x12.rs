//! ANSI X12 tokenizing plus schema-guided read/write (the schema
//! conventions live in [`crate::segments`]).
//!
//! Separators are discovered from the ISA envelope on read: element
//! separator from byte 3, component separator from ISA16, segment
//! terminator from the character after ISA16, and the 5010 repetition
//! separator from ISA11 when ISA12 selects the 5010-or-newer envelope
//! layout. Writing uses the standard
//! `*`/`:`/`~`/`^` with one segment per line. A schema that writes X12
//! must declare all 16 ISA elements, since re-reading depends on them.

use std::path::Path;

use ir::{Instance, SchemaKind, SchemaNode, Value};

use crate::EdiFormatError;
use crate::segments::{Segment, WriteOptions, read_segments, write_segments};

const WRITE_OPTIONS: WriteOptions = WriteOptions {
    element: '*',
    component: ':',
    terminator: '~',
    release: None,
    repetition: Some('^'),
};

/// Splits raw X12 text into segments (elements split into repeats, repeats
/// into components), discovering the separators from the ISA envelope.
pub fn tokenize(text: &str) -> Result<Vec<Segment>, EdiFormatError> {
    tokenize_with_component_separator(text).map(|(segments, _)| segments)
}

fn tokenize_with_component_separator(text: &str) -> Result<(Vec<Segment>, char), EdiFormatError> {
    let text = text.trim_start();
    if !text.starts_with("ISA") {
        return Err(EdiFormatError::NotX12("interchange must start with ISA"));
    }
    let element_separator = text
        .chars()
        .nth(3)
        .ok_or(EdiFormatError::NotX12("truncated ISA segment"))?;

    // ISA is self-describing: its 16th element is the component separator,
    // the character after that is the segment terminator, and (since 5010)
    // its 11th element is the repetition separator. ISA12 determines which
    // meaning ISA11 has; guessing from punctuation misreads malformed 4010
    // envelopes and accepts malformed 5010 envelopes.
    let mut separator_positions = Vec::with_capacity(16);
    for (i, c) in text.char_indices() {
        if c == element_separator {
            separator_positions.push(i);
            if separator_positions.len() == 16 {
                break;
            }
        }
    }
    if separator_positions.len() < 16 {
        return Err(EdiFormatError::NotX12("ISA has fewer than 16 elements"));
    }
    let isa11 =
        &text[separator_positions[10] + element_separator.len_utf8()..separator_positions[11]];
    let isa12 =
        &text[separator_positions[11] + element_separator.len_utf8()..separator_positions[12]];
    let repetition_separator = repetition_separator(isa11, isa12)?;

    let isa16_start = separator_positions[15] + element_separator.len_utf8();
    let mut rest = text[isa16_start..].chars();
    let component_separator = rest
        .next()
        .ok_or(EdiFormatError::NotX12("truncated ISA segment"))?;
    let segment_terminator = rest
        .next()
        .ok_or(EdiFormatError::NotX12("missing segment terminator"))?;

    let mut segments = Vec::new();
    for (index, raw) in text.split(segment_terminator).enumerate() {
        // Only whitespace between a terminator and the next segment is
        // formatting. Spaces before the terminator belong to the final
        // element and must survive tokenization.
        let raw = raw.trim_start();
        if raw.is_empty() {
            continue;
        }
        let mut parts = raw.split(element_separator);
        let id = parts.next().unwrap_or_default().to_string();
        // The ISA segment's own elements ARE the separator characters
        // (ISA11, ISA16), so splitting them would corrupt the envelope.
        let is_isa = index == 0;
        let elements = parts
            .map(|element| {
                if is_isa {
                    return vec![vec![element.to_string()]];
                }
                let repeats: Vec<&str> = match repetition_separator {
                    Some(sep) => element.split(sep).collect(),
                    None => vec![element],
                };
                repeats
                    .into_iter()
                    .map(|repeat| {
                        repeat
                            .split(component_separator)
                            .map(str::to_string)
                            .collect()
                    })
                    .collect()
            })
            .collect();
        segments.push(Segment { id, elements });
    }
    Ok((segments, component_separator))
}

fn repetition_separator(isa11: &str, isa12: &str) -> Result<Option<char>, EdiFormatError> {
    if isa12.len() != 5 || !isa12.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: "ISA12".into(),
            value: isa12.to_string(),
            reason: "expected a five-digit X12 version such as 00401 or 00501",
        });
    }
    if isa12 < "00501" {
        if isa11.is_empty() || !isa11.chars().all(char::is_alphanumeric) {
            return Err(EdiFormatError::InvalidEnvelopeElement {
                element: "ISA11".into(),
                value: isa11.to_string(),
                reason: "pre-5010 envelopes require an alphanumeric standards identifier",
            });
        }
        return Ok(None);
    }

    let mut characters = isa11.chars();
    let separator = characters.next().filter(|found| !found.is_alphanumeric());
    match (separator, characters.next()) {
        (Some(separator), None) => Ok(Some(separator)),
        _ => Err(EdiFormatError::InvalidEnvelopeElement {
            element: "ISA11".into(),
            value: isa11.to_string(),
            reason: "5010 and newer envelopes require one non-alphanumeric repetition separator",
        }),
    }
}

/// Reads an X12 file into an [`Instance`] tree shaped by `schema`. With
/// `lenient`, segments the schema doesn't mention are skipped (bounded by
/// the schema's own expectations) instead of erroring.
pub fn read(path: &Path, schema: &SchemaNode, lenient: bool) -> Result<Instance, EdiFormatError> {
    let text = std::fs::read_to_string(path)?;
    let (segments, component_separator) = tokenize_with_component_separator(&text)?;
    read_segments(schema, &segments, component_separator, lenient)
}

/// Writes an [`Instance`] tree shaped by `schema` as X12.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), EdiFormatError> {
    let options = write_options(schema, instance)?;
    let out = write_segments(schema, instance, &options)?;
    std::fs::write(path, out)?;
    Ok(())
}

fn write_options(schema: &SchemaNode, instance: &Instance) -> Result<WriteOptions, EdiFormatError> {
    let Some(isa_schema) = schema.child("ISA") else {
        return Ok(WRITE_OPTIONS);
    };
    let isa11_schema = isa_element(isa_schema, 10);
    let isa12_schema = isa_element(isa_schema, 11);
    let isa11 = isa11_schema
        .and_then(|element| envelope_value(instance, element))
        .unwrap_or_default();
    let isa12 = isa12_schema
        .and_then(|element| envelope_value(instance, element))
        .unwrap_or_default();

    if isa12.len() != 5 || !isa12.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: isa12_schema.map_or_else(|| "ISA12".into(), |element| element.name.clone()),
            value: isa12.to_string(),
            reason: "expected a five-digit X12 version such as 00401 or 00501",
        });
    }

    let mut options = WRITE_OPTIONS;
    if isa12 < "00501" {
        if isa11.is_empty() || !isa11.chars().all(char::is_alphanumeric) {
            return Err(EdiFormatError::InvalidEnvelopeElement {
                element: isa11_schema
                    .map_or_else(|| "ISA11".into(), |element| element.name.clone()),
                value: isa11.to_string(),
                reason: "pre-5010 envelopes require an alphanumeric standards identifier",
            });
        }
        options.repetition = None;
    } else {
        let expected = options.repetition.unwrap_or('^');
        let mut characters = isa11.chars();
        let valid_separator = characters
            .next()
            .is_some_and(|found| found == expected && !found.is_alphanumeric())
            && characters.next().is_none();
        if !valid_separator {
            return Err(EdiFormatError::EnvelopeSeparatorMismatch {
                element: isa11_schema
                    .map_or_else(|| "ISA11".into(), |element| element.name.clone()),
                expected,
                found: isa11.to_string(),
            });
        }
    }
    Ok(options)
}

fn isa_element(schema: &SchemaNode, index: usize) -> Option<&SchemaNode> {
    match &schema.kind {
        SchemaKind::Group { children } => children.get(index),
        SchemaKind::Scalar { .. } => None,
    }
}

fn envelope_value<'a>(instance: &'a Instance, schema: &'a SchemaNode) -> Option<&'a str> {
    instance
        .field("ISA")
        .and_then(|isa| isa.field(&schema.name))
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::String(text) if !text.is_empty() => Some(text.as_str()),
            _ => None,
        })
        .or(schema.fixed.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    fn segment(name: &str, elements: &[(&str, ScalarType)]) -> SchemaNode {
        SchemaNode::group(
            name,
            elements
                .iter()
                .map(|(n, ty)| SchemaNode::scalar(*n, *ty))
                .collect(),
        )
    }

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ferrule_x12_{name}_{}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn set_scalar(instance: &mut Instance, segment: &str, element: &str, value: &str) {
        let Instance::Group(fields) = instance else {
            panic!("test interchange is a group");
        };
        let (_, segment) = fields.iter_mut().find(|(name, _)| name == segment).unwrap();
        let Instance::Group(fields) = segment else {
            panic!("test segment is a group");
        };
        let (_, element) = fields.iter_mut().find(|(name, _)| name == element).unwrap();
        *element = Instance::Scalar(Value::String(value.into()));
    }

    const PO_850: &str = "\
ISA*00*          *00*          *ZZ*SENDERID       *ZZ*RECEIVERID     *260702*1200*U*00401*000000001*0*P*:~
GS*PO*SENDERID*RECEIVERID*20260702*1200*1*X*004010~
ST*850*0001~
BEG*00*SA*PO12345~
PO1*1*10*EA*7.99~
PID*F***HAMMER~
PO1*2*4*EA*3.49~
SE*6*0001~
GE*1*1~
IEA*1*000000001~
";

    /// ISA must declare all 16 elements if the schema is ever used to
    /// *write* X12 -- separator discovery on re-read depends on them.
    fn isa_segment() -> SchemaNode {
        let elements: Vec<(String, ScalarType)> = (1..=16)
            .map(|i| (format!("{i:02}"), ScalarType::String))
            .collect();
        SchemaNode::group(
            "ISA",
            elements
                .into_iter()
                .map(|(n, ty)| SchemaNode::scalar(n, ty))
                .collect(),
        )
    }

    fn descriptively_named_isa_segment() -> SchemaNode {
        let names = [
            "authorization_qualifier",
            "authorization_information",
            "security_qualifier",
            "security_information",
            "sender_qualifier",
            "sender_id",
            "receiver_qualifier",
            "receiver_id",
            "date",
            "time",
            "standards_or_repetition",
            "version",
            "control_number",
            "acknowledgement_requested",
            "usage_indicator",
            "component_separator",
        ];
        SchemaNode::group(
            "ISA",
            names
                .into_iter()
                .map(|name| SchemaNode::scalar(name, ScalarType::String))
                .collect(),
        )
    }

    fn po_schema() -> SchemaNode {
        SchemaNode::group(
            "X12",
            vec![
                isa_segment(),
                segment("GS", &[]),
                segment("ST", &[("01", ScalarType::String)]),
                segment(
                    "BEG",
                    &[
                        ("01", ScalarType::String),
                        ("02", ScalarType::String),
                        ("03", ScalarType::String),
                    ],
                ),
                SchemaNode::group(
                    "Item",
                    vec![
                        segment(
                            "PO1",
                            &[
                                ("01", ScalarType::Int),
                                ("02", ScalarType::Int),
                                ("03", ScalarType::String),
                                ("04", ScalarType::Float),
                            ],
                        ),
                        segment(
                            "PID",
                            &[
                                ("01", ScalarType::String),
                                ("02", ScalarType::String),
                                ("03", ScalarType::String),
                                ("04", ScalarType::String),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
                segment("SE", &[]),
                segment("GE", &[]),
                segment("IEA", &[]),
            ],
        )
    }

    #[test]
    fn tokenize_discovers_separators_from_isa() {
        // Nonstandard separators: `|` for elements, `>` component, `!` terminator.
        let text = "ISA|00|          |00|          |ZZ|S              |ZZ|R              |260702|1200|U|00401|000000001|0|P|>!ST|850|0001!SV3|AD>D4342!";
        let segments = tokenize(text).unwrap();
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].id, "ISA");
        // ISA16 must survive as the raw component-separator character.
        assert_eq!(segments[0].elements[15], vec![vec![">"]]);
        assert_eq!(
            segments[1].elements,
            vec![vec![vec!["850"]], vec![vec!["0001"]]]
        );
        // Composite element split on the discovered component separator.
        assert_eq!(segments[2].elements, vec![vec![vec!["AD", "D4342"]]]);
    }

    /// A 5010-style ISA11 (`^`) splits element repeats; a 4010-style ISA11
    /// (`U`, the standards identifier) must not.
    #[test]
    fn tokenize_discovers_the_repetition_separator() {
        let with_rep = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *110530*1549*^*00501*000000001*1*P*:~EB*1**1^33^35~";
        let segments = tokenize(with_rep).unwrap();
        assert_eq!(
            segments[1].elements[2],
            vec![vec!["1"], vec!["33"], vec!["35"]]
        );

        let without_rep = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *260702*1200*U*00401*000000001*0*P*:~EB*1**1^33^35~";
        let segments = tokenize(without_rep).unwrap();
        assert_eq!(segments[1].elements[2], vec![vec!["1^33^35"]]);
    }

    #[test]
    fn tokenize_rejects_isa11_values_that_contradict_isa12() {
        let legacy_with_separator = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *260702*1200*^*00401*000000001*0*P*:~";
        let modern_with_identifier = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *260702*1200*U*00501*000000001*0*P*:~";

        for text in [legacy_with_separator, modern_with_identifier] {
            assert!(matches!(
                tokenize(text),
                Err(EdiFormatError::InvalidEnvelopeElement { ref element, .. })
                    if element == "ISA11"
            ));
        }
    }

    #[test]
    fn trailing_element_spaces_survive_formatting_whitespace_and_roundtrip() {
        let text = PO_850.replace("PO12345~", "PO12345   ~");
        let path = write_temp("trailing_spaces", &text);
        let instance = read(&path, &po_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            instance
                .field("BEG")
                .and_then(|segment| segment.field("03"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("PO12345   ".into()))
        );

        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_trailing_spaces_out_{}.edi",
            std::process::id()
        ));
        write(&out_path, &po_schema(), &instance).unwrap();
        let read_back = read(&out_path, &po_schema(), false).unwrap();
        std::fs::remove_file(&out_path).unwrap();
        assert_eq!(read_back, instance);
    }

    /// A schema element child marked `repeating` reads every repeat --
    /// the HIPAA 271 pattern (EB03 service type codes).
    #[test]
    fn repeating_element_reads_all_repeats_and_roundtrips() {
        let text = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *110530*1549*^*00501*000000001*1*P*:~EB*1**1^33^35~";
        let schema = SchemaNode::group(
            "X12",
            vec![
                descriptively_named_isa_segment(),
                SchemaNode::group(
                    "EB",
                    vec![
                        SchemaNode::scalar("01", ScalarType::String),
                        SchemaNode::scalar("02", ScalarType::String),
                        SchemaNode::scalar("03", ScalarType::Int).repeating(),
                    ],
                ),
            ],
        );

        let path = write_temp("repeats", text);
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(&path).unwrap();

        let codes = instance
            .field("EB")
            .and_then(|eb| eb.field("03"))
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(
            codes,
            &[
                Instance::Scalar(Value::Int(1)),
                Instance::Scalar(Value::Int(33)),
                Instance::Scalar(Value::Int(35)),
            ]
        );

        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_repeats_out_{}.edi",
            std::process::id()
        ));
        write(&out_path, &schema, &instance).unwrap();
        let read_back = read(&out_path, &schema, false).unwrap();
        std::fs::remove_file(&out_path).unwrap();
        assert_eq!(read_back, instance);
    }

    #[test]
    fn legacy_4010_envelopes_reject_repeating_elements_on_write() {
        let text = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *260702*1200*U*00401*000000001*0*P*:~EB*1**33~";
        let schema = SchemaNode::group(
            "X12",
            vec![
                descriptively_named_isa_segment(),
                SchemaNode::group(
                    "EB",
                    vec![
                        SchemaNode::scalar("01", ScalarType::String),
                        SchemaNode::scalar("02", ScalarType::String),
                        SchemaNode::scalar("03", ScalarType::Int).repeating(),
                    ],
                ),
            ],
        );
        let path = write_temp("legacy_repeats", text);
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(&path).unwrap();
        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_legacy_repeats_out_{}.edi",
            std::process::id()
        ));

        let error = write(&out_path, &schema, &instance).unwrap_err();
        assert!(matches!(
            error,
            EdiFormatError::UnsupportedSchema(ref message)
                if message.contains("this dialect has no repetition separator")
        ));
        assert!(!out_path.exists());
    }

    #[test]
    fn reads_loops_with_typed_elements_and_empty_optionals() {
        let path = write_temp("read", PO_850);
        let instance = read(&path, &po_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(
            instance
                .field("BEG")
                .and_then(|beg| beg.field("03"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("PO12345".into()))
        );

        let items = instance
            .field("Item")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(items.len(), 2);

        let po1 = items[0].field("PO1").unwrap();
        assert_eq!(
            po1.field("02").and_then(Instance::as_scalar),
            Some(&Value::Int(10))
        );
        assert_eq!(
            po1.field("04").and_then(Instance::as_scalar),
            Some(&Value::Float(7.99))
        );

        let pids = items[0]
            .field("PID")
            .and_then(Instance::as_repeated)
            .unwrap();
        // PID*F***HAMMER: elements 2 and 3 are empty -> Null.
        assert_eq!(
            pids[0].field("02").and_then(Instance::as_scalar),
            Some(&Value::Null)
        );
        assert_eq!(
            pids[0].field("04").and_then(Instance::as_scalar),
            Some(&Value::String("HAMMER".into()))
        );

        // The second item has no PID at all -> empty loop.
        assert_eq!(items[1].field("PID"), Some(&Instance::Repeated(vec![])));
    }

    /// An 837-style claim line: `SV3*AD:D4342:::::desc*150~` -- element 1
    /// is a composite (schema group), element 2 a plain scalar, and a
    /// scalar declaration of a composite element captures its raw text.
    #[test]
    fn reads_composite_elements() {
        let text = "\
ISA*00*          *00*          *ZZ*S              *ZZ*R              *110530*1549*^*00501*000000001*1*P*:~
SV3*AD:D4342:::::One quadrant*150~
SV3*AD:D4341*450~
";
        let schema = SchemaNode::group(
            "X12",
            vec![
                segment("ISA", &[]),
                SchemaNode::group(
                    "SV3",
                    vec![
                        SchemaNode::group(
                            "01",
                            vec![
                                SchemaNode::scalar("qualifier", ScalarType::String),
                                SchemaNode::scalar("code", ScalarType::String),
                                SchemaNode::scalar("c3", ScalarType::String),
                                SchemaNode::scalar("c4", ScalarType::String),
                                SchemaNode::scalar("c5", ScalarType::String),
                                SchemaNode::scalar("c6", ScalarType::String),
                                SchemaNode::scalar("description", ScalarType::String),
                            ],
                        ),
                        SchemaNode::scalar("02", ScalarType::Float),
                    ],
                )
                .repeating(),
            ],
        );

        let path = write_temp("composite", text);
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(&path).unwrap();

        let claims = instance
            .field("SV3")
            .and_then(Instance::as_repeated)
            .unwrap();
        let first = claims[0].field("01").unwrap();
        assert_eq!(
            first.field("code").and_then(Instance::as_scalar),
            Some(&Value::String("D4342".into()))
        );
        assert_eq!(
            first.field("description").and_then(Instance::as_scalar),
            Some(&Value::String("One quadrant".into()))
        );
        assert_eq!(
            claims[0].field("02").and_then(Instance::as_scalar),
            Some(&Value::Float(150.0))
        );
        // Second SV3's composite only has 2 of 7 components -> rest Null.
        assert_eq!(
            claims[1]
                .field("01")
                .unwrap()
                .field("description")
                .and_then(Instance::as_scalar),
            Some(&Value::Null)
        );
    }

    #[test]
    fn scalar_composites_preserve_the_discovered_separator() {
        let text = "ISA|00|          |00|          |ZZ|S              |ZZ|R              |260702|1200|U|00401|000000001|0|P|>!NTE|A>B!";
        let schema = SchemaNode::group(
            "X12",
            vec![
                segment("ISA", &[]),
                segment("NTE", &[("01", ScalarType::String)]),
            ],
        );

        let path = write_temp("custom_component_scalar", text);
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(
            instance
                .field("NTE")
                .and_then(|segment| segment.field("01"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("A>B".into()))
        );
    }

    #[test]
    fn unexpected_segment_is_reported_with_position() {
        let text = PO_850.replace("SE*6*0001~\n", "");
        let path = write_temp("missing_se", &text);
        let err = read(&path, &po_schema(), false).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(
            matches!(err, EdiFormatError::UnexpectedSegment { ref expected, ref found, .. }
                if expected == "SE" && found == "GE")
        );
    }

    #[test]
    fn write_then_read_roundtrips() {
        let path = write_temp("roundtrip_src", PO_850);
        let instance = read(&path, &po_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();

        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_roundtrip_out_{}.edi",
            std::process::id()
        ));
        write(&out_path, &po_schema(), &instance).unwrap();
        let read_back = read(&out_path, &po_schema(), false).unwrap();
        std::fs::remove_file(&out_path).unwrap();

        assert_eq!(read_back, instance);
    }

    #[test]
    fn writing_reserved_delimiters_without_a_release_character_is_rejected() {
        let text = PO_850.replace("*U*00401*", "*^*00501*");
        let path = write_temp("reserved_src", &text);
        let instance = read(&path, &po_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();
        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_reserved_out_{}.edi",
            std::process::id()
        ));

        for delimiter in ['*', ':', '^', '~'] {
            let mut invalid = instance.clone();
            set_scalar(&mut invalid, "BEG", "03", &format!("A{delimiter}B"));
            let error = write(&out_path, &po_schema(), &invalid).unwrap_err();
            assert!(matches!(
                error,
                EdiFormatError::UnescapableDelimiter {
                    ref element,
                    delimiter: found,
                } if element == "03" && found == delimiter
            ));
        }
        assert!(!out_path.exists());
    }

    #[test]
    fn isa_separator_declarations_must_match_writer_options() {
        let path = write_temp("separator_mismatch_src", PO_850);
        let instance = read(&path, &po_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();
        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_separator_mismatch_out_{}.edi",
            std::process::id()
        ));

        let mut component_mismatch = instance.clone();
        set_scalar(&mut component_mismatch, "ISA", "16", ">");
        let error = write(&out_path, &po_schema(), &component_mismatch).unwrap_err();
        assert!(matches!(
            error,
            EdiFormatError::EnvelopeSeparatorMismatch {
                ref element,
                expected: ':',
                ref found,
            } if element == "16" && found == ">"
        ));

        let mut repetition_mismatch = instance;
        set_scalar(&mut repetition_mismatch, "ISA", "12", "00501");
        set_scalar(&mut repetition_mismatch, "ISA", "11", "!");
        let error = write(&out_path, &po_schema(), &repetition_mismatch).unwrap_err();
        assert!(matches!(
            error,
            EdiFormatError::EnvelopeSeparatorMismatch {
                ref element,
                expected: '^',
                ref found,
            } if element == "11" && found == "!"
        ));
        assert!(!out_path.exists());
    }

    #[test]
    fn isa12_version_selects_legacy_or_repetition_mode() {
        let path = write_temp("version_mode_src", PO_850);
        let instance = read(&path, &po_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();
        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_version_mode_out_{}.edi",
            std::process::id()
        ));

        let mut modern_with_legacy_isa11 = instance.clone();
        set_scalar(&mut modern_with_legacy_isa11, "ISA", "12", "00501");
        let error = write(&out_path, &po_schema(), &modern_with_legacy_isa11).unwrap_err();
        assert!(matches!(
            error,
            EdiFormatError::EnvelopeSeparatorMismatch {
                ref element,
                expected: '^',
                ref found,
            } if element == "11" && found == "U"
        ));

        let mut empty_legacy_isa11 = instance;
        set_scalar(&mut empty_legacy_isa11, "ISA", "11", "");
        let error = write(&out_path, &po_schema(), &empty_legacy_isa11).unwrap_err();
        assert!(matches!(
            error,
            EdiFormatError::InvalidEnvelopeElement {
                ref element,
                ref value,
                ..
            } if element == "11" && value.is_empty()
        ));
        assert!(!out_path.exists());
    }

    /// The HIPAA pattern: sibling loops both triggered by `HL`, told apart
    /// only by a `fixed` qualifier on `HL03` (20 = billing provider, 22 =
    /// subscriber), and repeated `NM1` segments told apart by `NM101`.
    #[test]
    fn fixed_qualifiers_disambiguate_shared_triggers() {
        let text = "\
ISA*00*          *00*          *ZZ*S              *ZZ*R              *110530*1549*^*00501*000000001*1*P*:~
HL*1**20*1~
NM1*85*1*MOLAR*SARA~
HL*2*1*22*0~
NM1*IL*1*PATIENT*PAT~
NM1*PR*2*ACME DENTAL~
HL*3*1*22*0~
NM1*IL*1*OTHER*OLIVER~
IEA*1*000000001~
";
        let hl = |qualifier: &str| {
            SchemaNode::group(
                "HL",
                vec![
                    SchemaNode::scalar("01", ScalarType::Int),
                    SchemaNode::scalar("02", ScalarType::String),
                    SchemaNode::scalar("03", ScalarType::String).fixed(qualifier),
                ],
            )
        };
        let nm1 = |qualifier: &str| {
            SchemaNode::group(
                "NM1",
                vec![
                    SchemaNode::scalar("01", ScalarType::String).fixed(qualifier),
                    SchemaNode::scalar("02", ScalarType::String),
                    SchemaNode::scalar("03", ScalarType::String),
                ],
            )
        };
        let schema = SchemaNode::group(
            "X12",
            vec![
                segment("ISA", &[]),
                SchemaNode::group("Provider", vec![hl("20"), nm1("85")]),
                SchemaNode::group(
                    "Subscriber",
                    vec![
                        hl("22"),
                        SchemaNode::group("Patient", vec![nm1("IL")]),
                        SchemaNode::group("Payer", vec![nm1("PR")]).repeating(),
                    ],
                )
                .repeating(),
                segment("IEA", &[]),
            ],
        );

        let path = write_temp("qualifiers", text);
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(&path).unwrap();

        let last_name = |group: &Instance, container: &str| {
            group
                .field(container)
                .and_then(|c| c.field("NM1"))
                .and_then(|n| n.field("03"))
                .and_then(Instance::as_scalar)
                .cloned()
        };

        assert_eq!(
            instance
                .field("Provider")
                .and_then(|p| p.field("NM1"))
                .and_then(|n| n.field("03"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("MOLAR".into()))
        );
        let subscribers = instance
            .field("Subscriber")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(subscribers.len(), 2);
        assert_eq!(
            last_name(&subscribers[0], "Patient"),
            Some(Value::String("PATIENT".into()))
        );
        // The first subscriber has a payer NM1, the second doesn't.
        assert_eq!(
            subscribers[0]
                .field("Payer")
                .and_then(Instance::as_repeated)
                .map(<[Instance]>::len),
            Some(1)
        );
        assert_eq!(
            subscribers[1]
                .field("Payer")
                .and_then(Instance::as_repeated)
                .map(<[Instance]>::len),
            Some(0)
        );
        assert_eq!(
            last_name(&subscribers[1], "Patient"),
            Some(Value::String("OTHER".into()))
        );
    }

    /// A `fixed` mismatch on a required segment is a positioned error that
    /// names the constraint.
    #[test]
    fn fixed_mismatch_is_reported_with_the_constraint() {
        let text = "\
ISA*00*          *00*          *ZZ*S              *ZZ*R              *110530*1549*^*00501*000000001*1*P*:~
HL*1**99*1~
IEA*1*000000001~
";
        let schema = SchemaNode::group(
            "X12",
            vec![
                segment("ISA", &[]),
                SchemaNode::group(
                    "HL",
                    vec![
                        SchemaNode::scalar("01", ScalarType::Int),
                        SchemaNode::scalar("02", ScalarType::String),
                        SchemaNode::scalar("03", ScalarType::String).fixed("20"),
                    ],
                ),
                segment("IEA", &[]),
            ],
        );
        let path = write_temp("fixed_mismatch", text);
        let err = read(&path, &schema, false).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(
            matches!(err, EdiFormatError::UnexpectedSegment { ref expected, ref found, .. }
                if expected == "HL(03=20)" && found == "HL")
        );
    }

    /// Lenient mode: the schema declares only the segments it cares about;
    /// everything else (GS, BHT, PER, N3, N4, trailing envelope) is
    /// skipped -- but only segments matching no current or upcoming
    /// expectation, so declared loops and their next iterations are never
    /// swallowed.
    #[test]
    fn lenient_mode_skips_unmentioned_segments() {
        let text = "\
ISA*00*          *00*          *ZZ*S              *ZZ*R              *110530*1549*^*00501*000000001*1*P*:~
GS*HC*S*R*20110530*1549*1*X*005010~
ST*837*0001~
BHT*0019*00*0123*20110530*1549*CH~
NM1*41*2*CLEARINGHOUSE~
PER*IC*JERRY~
HL*1**20*1~
NM1*85*1*DOE*MEGAN~
N3*123 TOOTH DRIVE~
N4*MIAMI*FL*33411~
HL*2*1*22*0~
NM1*IL*1*SMITH*JANE~
N3*236 N MAIN STREET~
CLM*SMITH878*1250~
LX*1~
SV3*AD:D4342*150~
LX*2~
SV3*AD:D4341*450~
SE*18*0001~
GE*1*1~
IEA*1*000000001~
";
        // Only ISA, the two qualifier-split NM1s, CLM, and the LX/SV3
        // service lines are declared.
        let nm1 = |qualifier: &str| {
            SchemaNode::group(
                "NM1",
                vec![
                    SchemaNode::scalar("01", ScalarType::String).fixed(qualifier),
                    SchemaNode::scalar("02", ScalarType::String),
                    SchemaNode::scalar("03", ScalarType::String),
                ],
            )
        };
        let schema = SchemaNode::group(
            "X12",
            vec![
                segment("ISA", &[]),
                SchemaNode::group("Provider", vec![nm1("85")]),
                SchemaNode::group(
                    "Subscriber",
                    vec![
                        nm1("IL"),
                        segment("CLM", &[("01", ScalarType::String)]),
                        SchemaNode::group(
                            "ServiceLine",
                            vec![
                                segment("LX", &[("01", ScalarType::Int)]),
                                segment(
                                    "SV3",
                                    &[("01", ScalarType::String), ("02", ScalarType::Float)],
                                ),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        );

        let path = write_temp("lenient", text);
        // Strict mode must reject the same schema/file pair.
        assert!(read(&path, &schema, false).is_err());
        let instance = read(&path, &schema, true).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(
            instance
                .field("Provider")
                .and_then(|p| p.field("NM1"))
                .and_then(|n| n.field("03"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("DOE".into()))
        );
        let subscribers = instance
            .field("Subscriber")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(subscribers.len(), 1);
        let lines = subscribers[0]
            .field("ServiceLine")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[1]
                .field("SV3")
                .and_then(|s| s.field("02"))
                .and_then(Instance::as_scalar),
            Some(&Value::Float(450.0))
        );
    }

    /// Writing emits `fixed` values for elements the instance doesn't
    /// provide, so qualifier elements need no explicit bindings.
    #[test]
    fn write_emits_fixed_values_as_defaults() {
        let schema = SchemaNode::group(
            "X12",
            vec![SchemaNode::group(
                "BEG",
                vec![
                    SchemaNode::scalar("01", ScalarType::String).fixed("00"),
                    SchemaNode::scalar("02", ScalarType::String).fixed("SA"),
                    SchemaNode::scalar("03", ScalarType::String),
                ],
            )],
        );
        let instance = Instance::Group(vec![(
            "BEG".into(),
            Instance::Group(vec![(
                "03".into(),
                Instance::Scalar(Value::String("PO1".into())),
            )]),
        )]);

        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_fixed_write_{}.edi",
            std::process::id()
        ));
        write(&out_path, &schema, &instance).unwrap();
        let written = std::fs::read_to_string(&out_path).unwrap();
        std::fs::remove_file(&out_path).unwrap();

        assert_eq!(written, "BEG*00*SA*PO1~\n");
    }
}
