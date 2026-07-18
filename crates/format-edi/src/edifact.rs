//! UN/EDIFACT tokenizing plus schema-guided read/write (the schema
//! conventions live in [`crate::segments`]).
//!
//! Default separators are `+` (element), `:` (component), `'` (segment
//! terminator), and `?` (release/escape character, which makes any
//! following character literal). An optional leading UNA service string
//! advises different separators and is honored on read. Both complete UNB
//! interchanges and standalone UNH messages are accepted. Writing uses the
//! defaults with one segment per line, escaping separators in values with
//! the release character.

use std::path::Path;

use ir::{Instance, SchemaNode};

use crate::autocomplete as envelope;
use crate::segments::{Segment, WriteOptions, read_segments, serialize_segments, write_segments};
use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES, read_bounded_input};

// No repetition separator: EDIFACT syntax v4 defines `*`, but most traffic
// is v3 where a bare `*` is ordinary data -- splitting it by default would
// corrupt those files.
const WRITE_OPTIONS: WriteOptions = WriteOptions {
    element: '+',
    component: ':',
    terminator: '\'',
    release: Some('?'),
    repetition: None,
    interchange_version: None,
};

#[derive(Debug, Clone, Copy)]
struct Separators {
    component: char,
    element: char,
    release: char,
    terminator: char,
}

/// Stable run data used when an EDIFACT target requests envelope completion.
#[derive(Debug, Clone, Copy)]
pub struct Autocomplete<'a> {
    pub current_datetime: &'a str,
    pub syntax_level: Option<&'a str>,
    pub syntax_version: Option<&'a str>,
    pub controlling_agency: Option<&'a str>,
    pub message_type: Option<&'a str>,
}

const DEFAULT_SEPARATORS: Separators = Separators {
    component: ':',
    element: '+',
    release: '?',
    terminator: '\'',
};

/// Splits raw EDIFACT text into segments (elements split into components),
/// honoring a leading UNA service string advice if present.
///
/// Segments, elements, and components are split in a single pass so the
/// release character is interpreted exactly once -- a two-pass split would
/// unescape `?+` into a literal `+` and then wrongly re-split on it.
pub fn tokenize(text: &str) -> Result<Vec<Segment>, EdiFormatError> {
    tokenize_with_separators(text).map(|(segments, _)| segments)
}

fn tokenize_with_separators(text: &str) -> Result<(Vec<Segment>, Separators), EdiFormatError> {
    if text.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(EdiFormatError::NotEdifact("input exceeds the 64 MiB limit"));
    }
    let text = text.trim_start();
    let (separators, body) = if let Some(rest) = text.strip_prefix("UNA") {
        let advice: Vec<char> = rest.chars().take(6).collect();
        if advice.len() < 6 {
            return Err(EdiFormatError::NotEdifact("truncated UNA service string"));
        }
        let separators = Separators {
            component: advice[0],
            element: advice[1],
            // advice[2] is the decimal notation, advice[4] is reserved.
            release: advice[3],
            terminator: advice[5],
        };
        let consumed = 3 + advice.iter().map(|c| c.len_utf8()).sum::<usize>();
        (separators, text[consumed..].trim_start())
    } else {
        (DEFAULT_SEPARATORS, text)
    };
    let starts_with = |id: &str| {
        body.strip_prefix(id).is_some_and(|rest| {
            rest.starts_with(separators.element) || rest.starts_with(separators.terminator)
        })
    };
    if !starts_with("UNB") && !starts_with("UNH") {
        return Err(EdiFormatError::NotEdifact(
            "interchange or standalone message must start with UNA, UNB, or UNH",
        ));
    }

    // Elements always hold exactly one repeat in EDIFACT (see the
    // WRITE_OPTIONS note about repetition).
    let mut segments = Vec::new();
    let mut current: Vec<Vec<Vec<String>>> = vec![vec![vec![String::new()]]];
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == separators.release {
            let Some(escaped) = chars.next() else {
                return Err(EdiFormatError::NotEdifact(
                    "dangling release character at end of interchange",
                ));
            };
            push_char(&mut current, escaped)?;
        } else if c == separators.terminator {
            finish_segment(
                &mut segments,
                std::mem::replace(&mut current, vec![vec![vec![String::new()]]]),
            );
        } else if c == separators.element {
            current.push(vec![vec![String::new()]]);
        } else if c == separators.component {
            let components = current
                .last_mut()
                .ok_or(EdiFormatError::NotEdifact("invalid tokenizer state"))?
                .last_mut()
                .ok_or(EdiFormatError::NotEdifact("invalid tokenizer state"))?;
            components.push(String::new());
        } else if c.is_whitespace() && at_segment_start(&current) {
            // Skip formatting whitespace between segments (e.g. newlines).
        } else {
            push_char(&mut current, c)?;
        }
    }
    // Tolerate a missing terminator on the final segment.
    finish_segment(&mut segments, current);
    Ok((segments, separators))
}

fn at_segment_start(current: &[Vec<Vec<String>>]) -> bool {
    current.len() == 1
        && current[0].len() == 1
        && current[0][0].len() == 1
        && current[0][0][0].is_empty()
}

fn finish_segment(segments: &mut Vec<Segment>, mut parts: Vec<Vec<Vec<String>>>) {
    if at_segment_start(&parts) {
        return;
    }
    let id = parts.remove(0).concat().join("");
    segments.push(Segment {
        id,
        elements: parts,
    });
}

fn push_char(elements: &mut [Vec<Vec<String>>], c: char) -> Result<(), EdiFormatError> {
    elements
        .last_mut()
        .ok_or(EdiFormatError::NotEdifact("invalid tokenizer state"))?
        .last_mut()
        .ok_or(EdiFormatError::NotEdifact("invalid tokenizer state"))?
        .last_mut()
        .ok_or(EdiFormatError::NotEdifact("invalid tokenizer state"))?
        .push(c);
    Ok(())
}

/// Reads an EDIFACT file into an [`Instance`] tree shaped by `schema`.
/// With `lenient`, segments the schema doesn't mention are skipped
/// (bounded by the schema's own expectations) instead of erroring.
pub fn read(path: &Path, schema: &SchemaNode, lenient: bool) -> Result<Instance, EdiFormatError> {
    let bytes = read_bounded_input(
        path,
        EdiFormatError::NotEdifact("input exceeds the 64 MiB limit"),
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| EdiFormatError::NotEdifact("input is not UTF-8"))?;
    let (segments, separators) = tokenize_with_separators(text)?;
    read_segments(schema, &segments, separators.component, None, lenient)
}

/// Writes an [`Instance`] tree shaped by `schema` as EDIFACT.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), EdiFormatError> {
    let out = write_segments(schema, instance, &WRITE_OPTIONS)?;
    std::fs::write(path, out)?;
    Ok(())
}

/// Writes EDIFACT and derives missing envelope dates, identifiers, counts,
/// and trailers from one stable mapping-run timestamp.
pub fn write_with_autocomplete(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    autocomplete: Autocomplete<'_>,
) -> Result<(), EdiFormatError> {
    let out = write_segments(schema, instance, &WRITE_OPTIONS)?;
    let (segments, _) = tokenize_with_separators(&out)?;
    let completed = envelope::edifact(
        segments,
        autocomplete.current_datetime,
        autocomplete.syntax_level,
        autocomplete.syntax_version,
        autocomplete.controlling_agency,
        autocomplete.message_type,
    )?;
    let out = serialize_segments(&completed, &WRITE_OPTIONS)?;
    std::fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};

    fn segment(name: &str, elements: Vec<SchemaNode>) -> SchemaNode {
        SchemaNode::group(name, elements)
    }

    fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
        SchemaNode::scalar(name, ty)
    }

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("ferrule_edifact_{name}_{}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn autocomplete_writes_edifact_headers_and_trailers() {
        let composite = |name: &str, fields: &[&str]| {
            SchemaNode::group(
                name,
                fields
                    .iter()
                    .map(|field| scalar(field, ScalarType::String))
                    .collect(),
            )
        };
        let schema = SchemaNode::group(
            "Envelope",
            vec![
                segment(
                    "UNB",
                    vec![
                        composite("S001", &["F0001", "F0002"]),
                        composite("S002", &["F0004"]),
                        composite("S003", &["F0010"]),
                        composite("S004", &["F0017", "F0019"]),
                        scalar("F0020", ScalarType::String),
                    ],
                ),
                segment(
                    "UNH",
                    vec![
                        scalar("F0062", ScalarType::String),
                        composite("S009", &["F0065", "F0052", "F0054", "F0051"]),
                    ],
                ),
                segment("BGM", vec![scalar("F1001", ScalarType::String)]),
            ],
        );
        let instance = Instance::Group(vec![
            (
                "UNB".into(),
                Instance::Group(vec![
                    (
                        "S002".into(),
                        Instance::Group(vec![(
                            "F0004".into(),
                            Instance::Scalar(Value::String("MFGB".into())),
                        )]),
                    ),
                    (
                        "S003".into(),
                        Instance::Group(vec![(
                            "F0010".into(),
                            Instance::Scalar(Value::String("ID".into())),
                        )]),
                    ),
                ]),
            ),
            (
                "UNH".into(),
                Instance::Group(vec![(
                    "S009".into(),
                    Instance::Group(vec![
                        ("F0052".into(), Instance::Scalar(Value::String("D".into()))),
                        (
                            "F0054".into(),
                            Instance::Scalar(Value::String("24A".into())),
                        ),
                    ]),
                )]),
            ),
            (
                "BGM".into(),
                Instance::Group(vec![(
                    "F1001".into(),
                    Instance::Scalar(Value::String("order".into())),
                )]),
            ),
        ]);
        let path = write_temp("autocomplete", "previous");

        write_with_autocomplete(
            &path,
            &schema,
            &instance,
            Autocomplete {
                current_datetime: "2026-07-18T12:34:56-07:00",
                syntax_level: Some("A"),
                syntax_version: Some("4"),
                controlling_agency: Some("UNO"),
                message_type: Some("ORDERS"),
            },
        )
        .unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            written,
            "UNB+UNOA:4+MFGB+ID+20260718:1234+1'\n\
             UNH+1+ORDERS:D:24A:UN'\n\
             BGM+order'\n\
             UNT+3+1'\n\
             UNZ+1+1'\n"
        );
    }

    #[test]
    fn tokenize_honors_release_character() {
        let text = "UNB+UNOB:1+SENDER+RECEIVER+970101:1230+1'FTX+PUR+3++Extra ?+ cheese?::yes'";
        let segments = tokenize(text).unwrap();
        assert_eq!(segments[1].id, "FTX");
        // "?+" is a literal plus, "?:" a literal colon.
        assert_eq!(
            segments[1].elements[3],
            vec![vec!["Extra + cheese:", "yes"]]
        );
    }

    #[test]
    fn tokenize_honors_una_advice() {
        // UNA declaring `;` components, `|` elements, `!` release, `$` terminator.
        let text = "UNA;|.! $UNB|UNOB;1|S|R|970101;1230|1$QTY|21;5;H87$";
        let segments = tokenize(text).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].id, "QTY");
        assert_eq!(segments[1].elements, vec![vec![vec!["21", "5", "H87"]]]);
    }

    #[test]
    fn scalar_composites_preserve_the_una_component_separator() {
        let text = "UNA;|.! $UNB|UNOB;1|S|R|970101;1230|1$RFF|AA;123$";
        let schema = SchemaNode::group(
            "EDIFACT",
            vec![
                segment("UNB", vec![]),
                segment("RFF", vec![scalar("01", ScalarType::String)]),
            ],
        );

        let path = write_temp("custom_component_scalar", text);
        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(
            instance
                .field("RFF")
                .and_then(|segment| segment.field("01"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("AA;123".into()))
        );
    }

    #[test]
    fn missing_unb_is_reported() {
        let err = tokenize("ISA*00~").unwrap_err();
        assert!(matches!(err, EdiFormatError::NotEdifact(_)));
    }

    #[test]
    fn standalone_message_after_una_is_accepted() {
        let segments = tokenize("UNA:+.?*'\r\nUNH+1+TEST:1'\r\nUNT+2+1'").unwrap();

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].id, "UNH");
        assert_eq!(segments[1].id, "UNT");
    }

    #[test]
    fn optional_interchange_prefix_can_be_absent() {
        let schema = SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchange",
                    vec![
                        segment("UNB", Vec::new()).repeating(),
                        SchemaNode::group(
                            "Messages",
                            vec![
                                segment("UNH", vec![scalar("reference", ScalarType::String)]),
                                segment("UNT", Vec::new()),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        );
        let path = write_temp(
            "standalone_message",
            "UNA:+.?*'\r\nUNH+message-1'\r\nUNT+2+message-1'",
        );

        let instance = read(&path, &schema, false).unwrap();
        std::fs::remove_file(path).unwrap();

        let interchange = &instance
            .field("Interchange")
            .and_then(Instance::as_repeated)
            .unwrap()[0];
        assert!(
            interchange
                .field("UNB")
                .and_then(Instance::as_repeated)
                .unwrap()
                .is_empty()
        );
        let message = &interchange
            .field("Messages")
            .and_then(Instance::as_repeated)
            .unwrap()[0];
        assert_eq!(
            message
                .field("UNH")
                .and_then(|unh| unh.field("reference"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("message-1".into()))
        );
    }

    #[test]
    fn dangling_release_character_is_rejected() {
        let error = tokenize("UNB+UNOB:1+S+R+970101:1230+1?").unwrap_err();
        assert!(matches!(
            error,
            EdiFormatError::NotEdifact("dangling release character at end of interchange")
        ));
    }

    #[test]
    fn non_finite_float_elements_are_rejected_on_read_and_write() {
        let schema = SchemaNode::group(
            "EDIFACT",
            vec![
                segment("UNB", vec![]),
                segment("MEA", vec![scalar("01", ScalarType::Float)]),
            ],
        );
        let path = write_temp("non_finite", "UNB+UNOB:1+S+R+970101:1230+1'MEA+NaN'");
        assert!(matches!(
            read(&path, &schema, false),
            Err(EdiFormatError::ElementParse {
                ref segment,
                element: 1,
                expected: ScalarType::Float,
                ..
            }) if segment == "MEA"
        ));
        std::fs::remove_file(&path).unwrap();

        let instance = Instance::Group(vec![
            ("UNB".into(), Instance::Group(Vec::new())),
            (
                "MEA".into(),
                Instance::Group(vec![(
                    "01".into(),
                    Instance::Scalar(Value::Float(f64::INFINITY)),
                )]),
            ),
        ]);
        assert!(matches!(
            write(&path, &schema, &instance),
            Err(EdiFormatError::NonFiniteFloat { ref element }) if element == "01"
        ));

        let incompatible = Instance::Group(vec![
            ("UNB".into(), Instance::Group(Vec::new())),
            (
                "MEA".into(),
                Instance::Group(vec![(
                    "01".into(),
                    Instance::Scalar(Value::String("not a number".into())),
                )]),
            ),
        ]);
        assert!(matches!(
            write(&path, &schema, &incompatible),
            Err(EdiFormatError::ValueType {
                ref element,
                expected: ScalarType::Float,
                got: "string",
            }) if element == "01"
        ));
        assert!(!path.exists());
    }

    #[test]
    fn typed_fixed_values_preserve_their_lexical_form() {
        let schema = SchemaNode::group(
            "EDIFACT",
            vec![
                segment("UNB", vec![]),
                segment(
                    "QTY",
                    vec![SchemaNode::scalar("01", ScalarType::Int).fixed("01")],
                ),
            ],
        );
        let instance = Instance::Group(vec![
            ("UNB".into(), Instance::Group(Vec::new())),
            ("QTY".into(), Instance::Group(Vec::new())),
        ]);
        let path = std::env::temp_dir().join(format!(
            "ferrule_edifact_fixed_lexical_{}",
            std::process::id()
        ));
        let roundtrip_path = std::env::temp_dir().join(format!(
            "ferrule_edifact_fixed_lexical_roundtrip_{}",
            std::process::id()
        ));

        write(&path, &schema, &instance).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("QTY+01'"), "{text}");
        let read_back = read(&path, &schema, false).unwrap();
        assert_eq!(
            read_back
                .field("QTY")
                .and_then(|segment| segment.field("01"))
                .and_then(Instance::as_scalar),
            Some(&Value::Int(1))
        );

        write(&roundtrip_path, &schema, &read_back).unwrap();
        let roundtrip = std::fs::read_to_string(&roundtrip_path).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(roundtrip_path).unwrap();
        assert!(roundtrip.contains("QTY+01'"), "{roundtrip}");
    }

    #[test]
    fn conflicting_typed_fixed_values_are_rejected_before_write() {
        let schema = SchemaNode::group(
            "EDIFACT",
            vec![
                segment("UNB", vec![]),
                segment(
                    "QTY",
                    vec![SchemaNode::scalar("01", ScalarType::Int).fixed("01")],
                ),
            ],
        );
        let instance = Instance::Group(vec![
            ("UNB".into(), Instance::Group(Vec::new())),
            (
                "QTY".into(),
                Instance::Group(vec![("01".into(), Instance::Scalar(Value::Int(2)))]),
            ),
        ]);
        let path = write_temp("fixed_mismatch_preserves_destination", "sentinel");

        assert!(matches!(
            write(&path, &schema, &instance),
            Err(EdiFormatError::FixedValueMismatch {
                ref element,
                ref expected,
                ref found,
            }) if element == "01" && expected == "01" && found == "2"
        ));
        let contents = std::fs::read_to_string(&path);
        assert!(matches!(contents, Ok(ref text) if text == "sentinel"));
        let _ = std::fs::remove_file(path);
    }

    fn validation_schema() -> SchemaNode {
        SchemaNode::group(
            "EDIFACT",
            vec![
                segment("UNB", vec![scalar("01", ScalarType::String)]),
                SchemaNode::group(
                    "Line",
                    vec![segment("LIN", vec![scalar("01", ScalarType::String)])],
                )
                .repeating(),
            ],
        )
    }

    #[test]
    fn writer_rejects_instance_kind_and_cardinality_mismatches() {
        let schema = validation_schema();
        assert!(matches!(
            write_segments(
                &schema,
                &Instance::MappedSequence(Vec::new()),
                &WRITE_OPTIONS,
            ),
            Err(EdiFormatError::InstanceShape {
                ref name,
                expected: "a group",
                got: "a mapped sequence",
            }) if name == "EDIFACT"
        ));

        let wrong_scalar = Instance::Group(vec![(
            "UNB".into(),
            Instance::Group(vec![("01".into(), Instance::Group(Vec::new()))]),
        )]);
        assert!(matches!(
            write_segments(&schema, &wrong_scalar, &WRITE_OPTIONS),
            Err(EdiFormatError::InstanceShape {
                ref name,
                expected: "a scalar",
                got: "a group",
            }) if name == "01"
        ));

        let mapped_scalar = Instance::Group(vec![(
            "UNB".into(),
            Instance::Group(vec![("01".into(), Instance::MappedSequence(Vec::new()))]),
        )]);
        assert!(matches!(
            write_segments(&schema, &mapped_scalar, &WRITE_OPTIONS),
            Err(EdiFormatError::InstanceShape {
                ref name,
                expected: "a scalar",
                got: "a mapped sequence",
            }) if name == "01"
        ));

        let repeated_non_repeating = Instance::Group(vec![(
            "UNB".into(),
            Instance::Repeated(vec![Instance::Group(Vec::new())]),
        )]);
        assert!(matches!(
            write_segments(&schema, &repeated_non_repeating, &WRITE_OPTIONS),
            Err(EdiFormatError::InstanceShape {
                ref name,
                expected: "one value",
                got: "repeating values",
            }) if name == "UNB"
        ));

        let non_repeated_repeating =
            Instance::Group(vec![("Line".into(), Instance::Group(Vec::new()))]);
        assert!(matches!(
            write_segments(&schema, &non_repeated_repeating, &WRITE_OPTIONS),
            Err(EdiFormatError::InstanceShape {
                ref name,
                expected: "repeating values",
                got: "a group",
            }) if name == "Line"
        ));
    }

    #[test]
    fn writer_rejects_unexpected_and_duplicate_fields() {
        let schema = validation_schema();
        let unexpected = Instance::Group(vec![("UNZ".into(), Instance::Group(Vec::new()))]);
        assert!(matches!(
            write_segments(&schema, &unexpected, &WRITE_OPTIONS),
            Err(EdiFormatError::UnexpectedField {
                ref group,
                ref field,
            }) if group == "EDIFACT" && field == "UNZ"
        ));

        let duplicate = Instance::Group(vec![
            ("UNB".into(), Instance::Group(Vec::new())),
            ("UNB".into(), Instance::Group(Vec::new())),
        ]);
        assert!(matches!(
            write_segments(&schema, &duplicate, &WRITE_OPTIONS),
            Err(EdiFormatError::DuplicateField {
                ref group,
                ref field,
            }) if group == "EDIFACT" && field == "UNB"
        ));
    }

    #[test]
    fn validation_failure_does_not_truncate_an_existing_destination() {
        let path = write_temp("shape_preserves_destination", "sentinel");
        let malformed = Instance::Group(vec![(
            "UNB".into(),
            Instance::Repeated(vec![Instance::Group(Vec::new())]),
        )]);

        assert!(matches!(
            write(&path, &validation_schema(), &malformed),
            Err(EdiFormatError::InstanceShape { ref name, .. }) if name == "UNB"
        ));
        let contents = std::fs::read_to_string(&path);
        assert!(matches!(contents, Ok(ref text) if text == "sentinel"));
        let _ = std::fs::remove_file(path);
    }

    fn orders_schema() -> SchemaNode {
        SchemaNode::group(
            "EDIFACT",
            vec![
                segment("UNB", vec![]),
                segment("UNH", vec![]),
                segment(
                    "BGM",
                    vec![
                        scalar("01", ScalarType::String),
                        scalar("02", ScalarType::String),
                    ],
                ),
                SchemaNode::group(
                    "Line",
                    vec![
                        segment(
                            "LIN",
                            vec![
                                scalar("01", ScalarType::Int),
                                scalar("02", ScalarType::String),
                                SchemaNode::group(
                                    "03",
                                    vec![
                                        scalar("item", ScalarType::String),
                                        scalar("qualifier", ScalarType::String),
                                    ],
                                ),
                            ],
                        ),
                        segment(
                            "QTY",
                            vec![SchemaNode::group(
                                "01",
                                vec![
                                    scalar("qualifier", ScalarType::String),
                                    scalar("quantity", ScalarType::Int),
                                    scalar("unit", ScalarType::String),
                                ],
                            )],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
                segment("UNT", vec![]),
                segment("UNZ", vec![]),
            ],
        )
    }

    const ORDERS: &str = "\
UNB+UNOB:1+SENDER+RECEIVER+970101:1230+1'
UNH+0001+ORDERS:D:24A:UN'
BGM+221+PO9876'
LIN+1++42:PD'
QTY+21:10:H87'
LIN+2++105:PD'
QTY+21:2:H87'
UNT+7+0001'
UNZ+1+1'
";

    #[test]
    fn reads_loops_and_composites() {
        let path = write_temp("orders", ORDERS);
        let instance = read(&path, &orders_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(
            instance
                .field("BGM")
                .and_then(|b| b.field("02"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("PO9876".into()))
        );
        let lines = instance
            .field("Line")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0]
                .field("LIN")
                .and_then(|l| l.field("03"))
                .and_then(|c| c.field("item"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("42".into()))
        );
        let qty = lines[1]
            .field("QTY")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(
            qty[0]
                .field("01")
                .and_then(|c| c.field("quantity"))
                .and_then(Instance::as_scalar),
            Some(&Value::Int(2))
        );
    }

    #[test]
    fn write_then_read_roundtrips_with_escaping() {
        let path = write_temp("roundtrip_src", ORDERS);
        let mut instance = read(&path, &orders_schema(), false).unwrap();
        std::fs::remove_file(&path).unwrap();

        // Inject a value containing every separator to prove escaping.
        if let Instance::Group(fields) = &mut instance
            && let Some((_, bgm)) = fields.iter_mut().find(|(n, _)| n == "BGM")
            && let Instance::Group(bgm_fields) = bgm
            && let Some((_, v)) = bgm_fields.iter_mut().find(|(n, _)| n == "02")
        {
            *v = Instance::Scalar(Value::String("A+B:C'D?E".into()));
        }

        let out_path = std::env::temp_dir().join(format!(
            "ferrule_edifact_roundtrip_out_{}.edi",
            std::process::id()
        ));
        write(&out_path, &orders_schema(), &instance).unwrap();
        let read_back = read(&out_path, &orders_schema(), false).unwrap();
        std::fs::remove_file(&out_path).unwrap();

        assert_eq!(read_back, instance);
    }
}
