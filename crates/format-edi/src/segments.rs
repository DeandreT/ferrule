//! The dialect-agnostic core shared by X12 and EDIFACT: a tokenized
//! [`Segment`] carries elements that are lists of components, and the
//! schema-guided recursive descent maps them onto the [`SchemaNode`] tree.
//!
//! Schema conventions:
//! - A group named like a segment ID (2-3 uppercase alphanumeric chars,
//!   starting with a letter -- `ISA`, `BEG`, `UNB`, `LIN`) whose children
//!   are scalars and/or groups-of-scalars is a **segment** matcher. Scalar
//!   children map positionally to elements 1..N; a group child is a
//!   **composite** element whose scalar children map positionally to its
//!   components. Extra elements/components in the file are ignored;
//!   missing/empty ones read as `Null`. Declaring a composite element as a
//!   plain scalar reads its raw text (components joined by the component
//!   separator).
//! - Any other group is a **loop/container**: it matches when its first
//!   segment descendant (the trigger) matches the cursor, and `repeating:
//!   true` means 0..N occurrences (also the v1 spelling for optional).
//!   Because segments are recognized by their ID-shaped names, container
//!   names must NOT look like segment IDs -- use descriptive names
//!   (`Item`, `Party`, `Loop2000A`).
//! - The schema root is always a container, whatever its name.
//! - Matching is strict and in order: every segment in the file must be
//!   consumed by the schema, and a missing non-repeating node is an error.
//!   This doubles as structural validation of the file.

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};

use crate::EdiFormatError;

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    /// One entry per element; each element is one or more components.
    pub elements: Vec<Vec<String>>,
}

/// Separators used when serializing; `release` (EDIFACT's `?`) escapes any
/// of the other three inside component text.
pub(crate) struct WriteOptions {
    pub element: char,
    pub component: char,
    pub terminator: char,
    pub release: Option<char>,
}

fn is_segment_id(name: &str) -> bool {
    (2..=3).contains(&name.len())
        && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

enum Shape<'a> {
    Segment(&'a [SchemaNode]),
    Container(&'a [SchemaNode]),
}

fn shape_of(node: &SchemaNode, is_root: bool) -> Result<Shape<'_>, EdiFormatError> {
    let SchemaKind::Group { children } = &node.kind else {
        return Err(EdiFormatError::UnsupportedSchema(node.name.clone()));
    };
    if !is_root && is_segment_id(&node.name) {
        let valid_segment = children.iter().all(|c| match &c.kind {
            SchemaKind::Scalar { .. } => true,
            SchemaKind::Group { children } => children
                .iter()
                .all(|cc| matches!(cc.kind, SchemaKind::Scalar { .. })),
        });
        if valid_segment {
            return Ok(Shape::Segment(children));
        }
        return Err(EdiFormatError::UnsupportedSchema(node.name.clone()));
    }
    if children
        .iter()
        .all(|c| matches!(c.kind, SchemaKind::Group { .. }))
    {
        Ok(Shape::Container(children))
    } else {
        Err(EdiFormatError::UnsupportedSchema(node.name.clone()))
    }
}

/// The segment ID that signals the start of `node` (for a container, its
/// first segment descendant).
fn trigger_of(node: &SchemaNode) -> Result<&str, EdiFormatError> {
    match shape_of(node, false)? {
        Shape::Segment(_) => Ok(&node.name),
        Shape::Container(children) => {
            let first = children
                .first()
                .ok_or_else(|| EdiFormatError::UnsupportedSchema(node.name.clone()))?;
            trigger_of(first)
        }
    }
}

/// The first segment ID a whole schema expects (the root is always a
/// container, whatever its name) -- used for dialect detection.
pub(crate) fn root_trigger(schema: &SchemaNode) -> Result<&str, EdiFormatError> {
    match shape_of(schema, true)? {
        Shape::Container(children) => {
            let first = children
                .first()
                .ok_or_else(|| EdiFormatError::UnsupportedSchema(schema.name.clone()))?;
            trigger_of(first)
        }
        Shape::Segment(_) => unreachable!("the root is always classified as a container"),
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

/// Maps tokenized segments onto `schema`. `component_join` is the dialect's
/// component separator, used only to reconstruct raw text when a composite
/// element is declared as a plain scalar.
pub(crate) fn read_segments(
    schema: &SchemaNode,
    segments: &[Segment],
    component_join: char,
) -> Result<Instance, EdiFormatError> {
    let mut cursor = Cursor { segments, pos: 0 };
    let instance = read_node(schema, &mut cursor, component_join, true)?;
    if let Some(segment) = cursor.peek() {
        return Err(EdiFormatError::TrailingSegment {
            index: cursor.pos,
            id: segment.id.clone(),
        });
    }
    Ok(instance)
}

fn read_node(
    node: &SchemaNode,
    cursor: &mut Cursor,
    component_join: char,
    is_root: bool,
) -> Result<Instance, EdiFormatError> {
    match shape_of(node, is_root)? {
        Shape::Segment(elements) => read_segment(node, elements, cursor, component_join),
        Shape::Container(children) => {
            let mut fields = Vec::with_capacity(children.len());
            for child in children {
                let trigger = trigger_of(child)?;
                if child.repeating {
                    let mut items = Vec::new();
                    while cursor.peek().is_some_and(|s| s.id == trigger) {
                        items.push(read_node(child, cursor, component_join, false)?);
                    }
                    fields.push((child.name.clone(), Instance::Repeated(items)));
                } else if cursor.peek().is_some_and(|s| s.id == trigger) {
                    fields.push((
                        child.name.clone(),
                        read_node(child, cursor, component_join, false)?,
                    ));
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
    component_join: char,
) -> Result<Instance, EdiFormatError> {
    let segment = cursor
        .peek()
        .expect("caller checked the trigger before consuming");
    debug_assert_eq!(segment.id, node.name);
    static EMPTY: Vec<String> = Vec::new();
    let mut fields = Vec::with_capacity(element_schemas.len());
    for (i, element_schema) in element_schemas.iter().enumerate() {
        let components = segment.elements.get(i).unwrap_or(&EMPTY);
        let instance = match &element_schema.kind {
            SchemaKind::Scalar { ty } => {
                let raw = if components.len() > 1 {
                    components.join(&component_join.to_string())
                } else {
                    components.first().cloned().unwrap_or_default()
                };
                Instance::Scalar(parse_element(&segment.id, i + 1, *ty, &raw)?)
            }
            SchemaKind::Group {
                children: component_schemas,
            } => {
                let mut parts = Vec::with_capacity(component_schemas.len());
                for (j, component_schema) in component_schemas.iter().enumerate() {
                    let SchemaKind::Scalar { ty } = component_schema.kind else {
                        unreachable!("shape_of validated composite children are scalars");
                    };
                    let raw = components.get(j).map_or("", String::as_str);
                    parts.push((
                        component_schema.name.clone(),
                        Instance::Scalar(parse_element(&segment.id, i + 1, ty, raw)?),
                    ));
                }
                Instance::Group(parts)
            }
        };
        fields.push((element_schema.name.clone(), instance));
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

/// Serializes an [`Instance`] shaped by `schema`, one segment per line.
/// Trailing empty elements/components are trimmed, except for `ISA` whose
/// 16 elements are positional by definition.
pub(crate) fn write_segments(
    schema: &SchemaNode,
    instance: &Instance,
    opts: &WriteOptions,
) -> Result<String, EdiFormatError> {
    let mut out = String::new();
    write_node(schema, instance, opts, &mut out, true)?;
    Ok(out)
}

fn write_node(
    node: &SchemaNode,
    instance: &Instance,
    opts: &WriteOptions,
    out: &mut String,
    is_root: bool,
) -> Result<(), EdiFormatError> {
    if let Instance::Repeated(items) = instance {
        for item in items {
            write_node(node, item, opts, out, is_root)?;
        }
        return Ok(());
    }
    match shape_of(node, is_root)? {
        Shape::Segment(element_schemas) => {
            let mut elements: Vec<String> = element_schemas
                .iter()
                .map(|e| write_element(e, instance.field(&e.name), opts))
                .collect();
            if node.name != "ISA" {
                while elements.last().is_some_and(String::is_empty) {
                    elements.pop();
                }
            }
            out.push_str(&node.name);
            for element in &elements {
                out.push(opts.element);
                out.push_str(element);
            }
            out.push(opts.terminator);
            out.push('\n');
        }
        Shape::Container(children) => {
            for child in children {
                if let Some(field) = instance.field(&child.name) {
                    write_node(child, field, opts, out, false)?;
                }
            }
        }
    }
    Ok(())
}

fn write_element(schema: &SchemaNode, instance: Option<&Instance>, opts: &WriteOptions) -> String {
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            escape(&format_value(instance.and_then(Instance::as_scalar)), opts)
        }
        SchemaKind::Group { children } => {
            let mut components: Vec<String> = children
                .iter()
                .map(|c| {
                    escape(
                        &format_value(
                            instance
                                .and_then(|i| i.field(&c.name))
                                .and_then(Instance::as_scalar),
                        ),
                        opts,
                    )
                })
                .collect();
            while components.last().is_some_and(String::is_empty) {
                components.pop();
            }
            components.join(&opts.component.to_string())
        }
    }
}

fn escape(text: &str, opts: &WriteOptions) -> String {
    let Some(release) = opts.release else {
        return text.to_string();
    };
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == release || c == opts.element || c == opts.component || c == opts.terminator {
            out.push(release);
        }
        out.push(c);
    }
    out
}

fn format_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Int(i)) => i.to_string(),
        Some(Value::Float(f)) => f.to_string(),
        Some(Value::String(s)) => s.clone(),
    }
}
