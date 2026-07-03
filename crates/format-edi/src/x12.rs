//! ANSI X12 tokenizing plus schema-guided read/write (the schema
//! conventions live in [`crate::segments`]).
//!
//! Separators are discovered from the ISA envelope on read: element
//! separator from byte 3, component separator from ISA16, segment
//! terminator from the character after ISA16, and the 5010 repetition
//! separator from ISA11 (only when it's a single non-alphanumeric
//! character -- in 4010 that position held the standards identifier) -- so
//! nonstandard delimiters just work. Writing uses the standard
//! `*`/`:`/`~`/`^` with one segment per line. A schema that writes X12
//! must declare all 16 ISA elements, since re-reading depends on them.

use std::path::Path;

use ir::{Instance, SchemaNode};

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
    // its 11th element is the repetition separator. Before 5010, ISA11 was
    // the standards identifier (`U`), so it only counts as a separator
    // when it's a single non-alphanumeric character -- the standard
    // version-agnostic heuristic.
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
    let repetition_separator = match isa11.chars().collect::<Vec<_>>().as_slice() {
        [c] if !c.is_alphanumeric() => Some(*c),
        _ => None,
    };

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
        let raw = raw.trim();
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
    Ok(segments)
}

/// Reads an X12 file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, EdiFormatError> {
    let text = std::fs::read_to_string(path)?;
    let segments = tokenize(&text)?;
    read_segments(schema, &segments, ':')
}

/// Writes an [`Instance`] tree shaped by `schema` as X12.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), EdiFormatError> {
    let out = write_segments(schema, instance, &WRITE_OPTIONS)?;
    std::fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};

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

    /// A schema element child marked `repeating` reads every repeat --
    /// the HIPAA 271 pattern (EB03 service type codes).
    #[test]
    fn repeating_element_reads_all_repeats_and_roundtrips() {
        let text = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *110530*1549*^*00501*000000001*1*P*:~EB*1**1^33^35~";
        let schema = SchemaNode::group(
            "X12",
            vec![
                isa_segment(),
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
        let instance = read(&path, &schema).unwrap();
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
        let read_back = read(&out_path, &schema).unwrap();
        std::fs::remove_file(&out_path).unwrap();
        assert_eq!(read_back, instance);
    }

    #[test]
    fn reads_loops_with_typed_elements_and_empty_optionals() {
        let path = write_temp("read", PO_850);
        let instance = read(&path, &po_schema()).unwrap();
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
        let instance = read(&path, &schema).unwrap();
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
    fn unexpected_segment_is_reported_with_position() {
        let text = PO_850.replace("SE*6*0001~\n", "");
        let path = write_temp("missing_se", &text);
        let err = read(&path, &po_schema()).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(
            matches!(err, EdiFormatError::UnexpectedSegment { ref expected, ref found, .. }
                if expected == "SE" && found == "GE")
        );
    }

    #[test]
    fn write_then_read_roundtrips() {
        let path = write_temp("roundtrip_src", PO_850);
        let instance = read(&path, &po_schema()).unwrap();
        std::fs::remove_file(&path).unwrap();

        let out_path = std::env::temp_dir().join(format!(
            "ferrule_x12_roundtrip_out_{}.edi",
            std::process::id()
        ));
        write(&out_path, &po_schema(), &instance).unwrap();
        let read_back = read(&out_path, &po_schema()).unwrap();
        std::fs::remove_file(&out_path).unwrap();

        assert_eq!(read_back, instance);
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
        let instance = read(&path, &schema).unwrap();
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
        let err = read(&path, &schema).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(
            matches!(err, EdiFormatError::UnexpectedSegment { ref expected, ref found, .. }
                if expected == "HL(03=20)" && found == "HL")
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
