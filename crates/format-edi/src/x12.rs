//! ANSI X12 tokenizing and schema-guided reading/writing.
//!
//! Schema conventions (using the ordinary [`SchemaNode`] tree):
//! - A group whose children are all scalars is a **segment** matcher: its
//!   `name` is the segment ID (`ISA`, `BEG`, `PO1`, ...) and its scalar
//!   children map positionally to elements 1..N. A file segment may carry
//!   more elements than the schema declares (extras are ignored) or fewer
//!   (missing/empty elements read as `Null`). An empty group matches the
//!   segment while capturing nothing.
//! - A group whose children are all groups is a **loop/container**: it
//!   matches when its first segment descendant (the trigger) matches the
//!   cursor. `repeating: true` means 0..N occurrences -- which is also the
//!   v1 spelling for optional segments/loops.
//! - Matching is strict and in order: every segment in the file must be
//!   consumed by the schema (envelope segments like `GS`/`GE` included --
//!   an empty group per segment is enough), and a missing non-repeating
//!   node is an error. This doubles as structural validation of the file.
//!
//! Separators are discovered from the ISA envelope on read (element
//! separator from byte 3, segment terminator from the character after
//! ISA16), so nonstandard delimiters just work. Writing uses the standard
//! `*` and `~` with one segment per line.

use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};

use crate::EdiFormatError;

const WRITE_ELEMENT_SEPARATOR: char = '*';
const WRITE_SEGMENT_TERMINATOR: char = '~';

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    pub elements: Vec<String>,
}

/// Splits raw X12 text into segments, discovering the separators from the
/// ISA envelope.
pub fn tokenize(text: &str) -> Result<Vec<Segment>, EdiFormatError> {
    let text = text.trim_start();
    if !text.starts_with("ISA") {
        return Err(EdiFormatError::NotX12("interchange must start with ISA"));
    }
    let element_separator = text
        .chars()
        .nth(3)
        .ok_or(EdiFormatError::NotX12("truncated ISA segment"))?;

    // ISA is self-describing: after its 16th element (the component
    // separator, unused until composite support lands) comes the segment
    // terminator.
    let mut separators_seen = 0;
    let mut isa16_start = None;
    for (i, c) in text.char_indices() {
        if c == element_separator {
            separators_seen += 1;
            if separators_seen == 16 {
                isa16_start = Some(i + element_separator.len_utf8());
                break;
            }
        }
    }
    let isa16_start =
        isa16_start.ok_or(EdiFormatError::NotX12("ISA has fewer than 16 elements"))?;
    let mut rest = text[isa16_start..].chars();
    let _component_separator = rest
        .next()
        .ok_or(EdiFormatError::NotX12("truncated ISA segment"))?;
    let segment_terminator = rest
        .next()
        .ok_or(EdiFormatError::NotX12("missing segment terminator"))?;

    let mut segments = Vec::new();
    for raw in text.split(segment_terminator) {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let mut parts = raw.split(element_separator);
        let id = parts.next().unwrap_or_default().to_string();
        segments.push(Segment {
            id,
            elements: parts.map(str::to_string).collect(),
        });
    }
    Ok(segments)
}

enum NodeShape<'a> {
    Segment(&'a [SchemaNode]),
    Container(&'a [SchemaNode]),
}

fn shape_of(node: &SchemaNode) -> Result<NodeShape<'_>, EdiFormatError> {
    let SchemaKind::Group { children } = &node.kind else {
        return Err(EdiFormatError::UnsupportedSchema(node.name.clone()));
    };
    let scalars = children
        .iter()
        .filter(|c| matches!(c.kind, SchemaKind::Scalar { .. }))
        .count();
    if scalars == children.len() {
        Ok(NodeShape::Segment(children))
    } else if scalars == 0 {
        Ok(NodeShape::Container(children))
    } else {
        Err(EdiFormatError::UnsupportedSchema(node.name.clone()))
    }
}

/// The segment ID that signals the start of `node` (for a container, its
/// first segment descendant).
fn trigger_of(node: &SchemaNode) -> Result<&str, EdiFormatError> {
    match shape_of(node)? {
        NodeShape::Segment(_) => Ok(&node.name),
        NodeShape::Container(children) => {
            let first = children
                .first()
                .ok_or_else(|| EdiFormatError::UnsupportedSchema(node.name.clone()))?;
            trigger_of(first)
        }
    }
}

struct Cursor<'a> {
    segments: &'a [Segment],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<&Segment> {
        self.segments.get(self.pos)
    }
}

/// Reads an X12 file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, EdiFormatError> {
    let text = std::fs::read_to_string(path)?;
    let segments = tokenize(&text)?;
    let mut cursor = Cursor {
        segments: &segments,
        pos: 0,
    };
    let instance = read_node(schema, &mut cursor)?;
    if let Some(segment) = cursor.peek() {
        return Err(EdiFormatError::TrailingSegment {
            index: cursor.pos,
            id: segment.id.clone(),
        });
    }
    Ok(instance)
}

fn read_node(node: &SchemaNode, cursor: &mut Cursor) -> Result<Instance, EdiFormatError> {
    match shape_of(node)? {
        NodeShape::Segment(elements) => read_segment(node, elements, cursor),
        NodeShape::Container(children) => {
            let mut fields = Vec::with_capacity(children.len());
            for child in children {
                let trigger = trigger_of(child)?;
                if child.repeating {
                    let mut items = Vec::new();
                    while cursor.peek().is_some_and(|s| s.id == trigger) {
                        items.push(read_node(child, cursor)?);
                    }
                    fields.push((child.name.clone(), Instance::Repeated(items)));
                } else if cursor.peek().is_some_and(|s| s.id == trigger) {
                    fields.push((child.name.clone(), read_node(child, cursor)?));
                } else {
                    return Err(EdiFormatError::UnexpectedSegment {
                        index: cursor.pos,
                        expected: trigger.to_string(),
                        found: cursor
                            .peek()
                            .map_or_else(|| "end of interchange".to_string(), |s| s.id.clone()),
                    });
                }
            }
            Ok(Instance::Group(fields))
        }
    }
}

fn read_segment(
    node: &SchemaNode,
    element_schemas: &[SchemaNode],
    cursor: &mut Cursor,
) -> Result<Instance, EdiFormatError> {
    let segment = cursor
        .peek()
        .expect("caller checked the trigger before consuming");
    debug_assert_eq!(segment.id, node.name);
    let mut fields = Vec::with_capacity(element_schemas.len());
    for (i, element_schema) in element_schemas.iter().enumerate() {
        let SchemaKind::Scalar { ty } = element_schema.kind else {
            unreachable!("shape_of only classifies all-scalar groups as segments");
        };
        let raw = segment.elements.get(i).map_or("", String::as_str);
        let value = parse_element(&segment.id, i + 1, ty, raw)?;
        fields.push((element_schema.name.clone(), Instance::Scalar(value)));
    }
    cursor.pos += 1;
    Ok(Instance::Group(fields))
}

fn parse_element(
    segment: &str,
    element: usize,
    ty: ScalarType,
    raw: &str,
) -> Result<Value, EdiFormatError> {
    if raw.is_empty() {
        return Ok(Value::Null);
    }
    let bad = || EdiFormatError::ElementParse {
        segment: segment.to_string(),
        element,
        expected: ty,
        value: raw.to_string(),
    };
    Ok(match ty {
        ScalarType::String => Value::String(raw.to_string()),
        ScalarType::Int => Value::Int(raw.parse().map_err(|_| bad())?),
        ScalarType::Float => Value::Float(raw.parse().map_err(|_| bad())?),
        ScalarType::Bool => Value::Bool(raw.parse().map_err(|_| bad())?),
    })
}

/// Writes an [`Instance`] tree shaped by `schema` as X12 with standard
/// separators, one segment per line. Trailing empty elements are trimmed,
/// except for `ISA` whose 16 elements are positional by definition.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), EdiFormatError> {
    let mut out = String::new();
    write_node(schema, instance, &mut out)?;
    std::fs::write(path, out)?;
    Ok(())
}

fn write_node(
    node: &SchemaNode,
    instance: &Instance,
    out: &mut String,
) -> Result<(), EdiFormatError> {
    if let Instance::Repeated(items) = instance {
        for item in items {
            write_node(node, item, out)?;
        }
        return Ok(());
    }
    match shape_of(node)? {
        NodeShape::Segment(element_schemas) => {
            let mut elements: Vec<String> = element_schemas
                .iter()
                .map(|e| format_element(instance.field(&e.name).and_then(Instance::as_scalar)))
                .collect();
            if node.name != "ISA" {
                while elements.last().is_some_and(String::is_empty) {
                    elements.pop();
                }
            }
            out.push_str(&node.name);
            for element in &elements {
                out.push(WRITE_ELEMENT_SEPARATOR);
                out.push_str(element);
            }
            out.push(WRITE_SEGMENT_TERMINATOR);
            out.push('\n');
        }
        NodeShape::Container(children) => {
            for child in children {
                if let Some(field) = instance.field(&child.name) {
                    write_node(child, field, out)?;
                }
            }
        }
    }
    Ok(())
}

fn format_element(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Int(i)) => i.to_string(),
        Some(Value::Float(f)) => f.to_string(),
        Some(Value::String(s)) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let text = "ISA|00|          |00|          |ZZ|S              |ZZ|R              |260702|1200|U|00401|000000001|0|P|>!ST|850|0001!";
        let segments = tokenize(text).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].id, "ISA");
        assert_eq!(segments[1].id, "ST");
        assert_eq!(segments[1].elements, vec!["850", "0001"]);
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
}
