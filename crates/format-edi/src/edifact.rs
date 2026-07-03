//! UN/EDIFACT tokenizing plus schema-guided read/write (the schema
//! conventions live in [`crate::segments`]).
//!
//! Default separators are `+` (element), `:` (component), `'` (segment
//! terminator), and `?` (release/escape character, which makes any
//! following character literal). An optional leading UNA service string
//! advises different separators and is honored on read. Writing uses the
//! defaults with one segment per line, escaping separators in values with
//! the release character.

use std::path::Path;

use ir::{Instance, SchemaNode};

use crate::EdiFormatError;
use crate::segments::{Segment, WriteOptions, read_segments, write_segments};

// No repetition separator: EDIFACT syntax v4 defines `*`, but most traffic
// is v3 where a bare `*` is ordinary data -- splitting it by default would
// corrupt those files.
const WRITE_OPTIONS: WriteOptions = WriteOptions {
    element: '+',
    component: ':',
    terminator: '\'',
    release: Some('?'),
    repetition: None,
};

#[derive(Debug, Clone, Copy)]
struct Separators {
    component: char,
    element: char,
    release: char,
    terminator: char,
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
    if !body.starts_with("UNB") {
        return Err(EdiFormatError::NotEdifact(
            "interchange must start with UNA or UNB",
        ));
    }

    // Elements always hold exactly one repeat in EDIFACT (see the
    // WRITE_OPTIONS note about repetition).
    let mut segments = Vec::new();
    let mut current: Vec<Vec<Vec<String>>> = vec![vec![vec![String::new()]]];
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == separators.release {
            if let Some(escaped) = chars.next() {
                push_char(&mut current, escaped);
            }
        } else if c == separators.terminator {
            finish_segment(
                &mut segments,
                std::mem::replace(&mut current, vec![vec![vec![String::new()]]]),
            );
        } else if c == separators.element {
            current.push(vec![vec![String::new()]]);
        } else if c == separators.component {
            current
                .last_mut()
                .expect("current is never empty")
                .last_mut()
                .expect("repeats are never empty")
                .push(String::new());
        } else if c.is_whitespace() && at_segment_start(&current) {
            // Skip formatting whitespace between segments (e.g. newlines).
        } else {
            push_char(&mut current, c);
        }
    }
    // Tolerate a missing terminator on the final segment.
    finish_segment(&mut segments, current);
    Ok(segments)
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

fn push_char(elements: &mut [Vec<Vec<String>>], c: char) {
    elements
        .last_mut()
        .expect("elements is never empty")
        .last_mut()
        .expect("repeats are never empty")
        .last_mut()
        .expect("components is never empty")
        .push(c);
}

/// Reads an EDIFACT file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, EdiFormatError> {
    let text = std::fs::read_to_string(path)?;
    let segments = tokenize(&text)?;
    read_segments(schema, &segments, ':')
}

/// Writes an [`Instance`] tree shaped by `schema` as EDIFACT.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), EdiFormatError> {
    let out = write_segments(schema, instance, &WRITE_OPTIONS)?;
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
    fn missing_unb_is_reported() {
        let err = tokenize("ISA*00~").unwrap_err();
        assert!(matches!(err, EdiFormatError::NotEdifact(_)));
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
        let instance = read(&path, &orders_schema()).unwrap();
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
        let mut instance = read(&path, &orders_schema()).unwrap();
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
        let read_back = read(&out_path, &orders_schema()).unwrap();
        std::fs::remove_file(&out_path).unwrap();

        assert_eq!(read_back, instance);
    }
}
