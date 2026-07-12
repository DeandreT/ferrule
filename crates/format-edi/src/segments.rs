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
//! - Matching is strict and in order by default: every segment in the file
//!   must be consumed by the schema, and a missing non-repeating node is an
//!   error. This doubles as structural validation of the file. In lenient
//!   mode (see [`read_segments`]) unmentioned segments are skipped instead,
//!   so a schema only needs to declare what it binds.

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};

use crate::EdiFormatError;

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    /// One entry per element; each element is one or more repeats (X12
    /// 5010's repetition separator -- exactly one repeat when the dialect
    /// or file has no repetition); each repeat is one or more components.
    pub elements: Vec<Vec<Vec<String>>>,
}

/// Separators used when serializing; `release` (EDIFACT's `?`) escapes the
/// other separators inside component text, and `repetition` (X12 5010's
/// `^`) joins the occurrences of a `repeating` element.
#[derive(Clone, Copy)]
pub(crate) struct WriteOptions {
    pub element: char,
    pub component: char,
    pub terminator: char,
    pub release: Option<char>,
    pub repetition: Option<char>,
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

/// The segment schema that signals the start of `node` (for a container,
/// its first segment descendant).
fn trigger_of(node: &SchemaNode) -> Result<&SchemaNode, EdiFormatError> {
    match shape_of(node, false)? {
        Shape::Segment(_) => Ok(node),
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
            Ok(&trigger_of(first)?.name)
        }
        Shape::Segment(_) => Err(EdiFormatError::UnsupportedSchema(schema.name.clone())),
    }
}

/// Whether `segment` satisfies a segment schema: the IDs must agree and
/// every `fixed` element/component constraint must hold. Fixed values are
/// what disambiguate qualifier-driven loops (e.g. `HL` with `HL03` fixed
/// to `20` vs `22`, or repeated `NM1`s told apart by `NM101`).
fn segment_matches(trigger: &SchemaNode, segment: &Segment) -> bool {
    if trigger.name != segment.id {
        return false;
    }
    let SchemaKind::Group { children } = &trigger.kind else {
        return false;
    };
    children.iter().enumerate().all(|(i, child)| {
        // Constraints are checked against the first repeat.
        let components = segment.elements.get(i).and_then(|repeats| repeats.first());
        match &child.kind {
            SchemaKind::Scalar { .. } => fixed_holds(child, components.and_then(|c| c.first())),
            SchemaKind::Group {
                children: component_schemas,
            } => component_schemas
                .iter()
                .enumerate()
                .all(|(j, comp)| fixed_holds(comp, components.and_then(|c| c.get(j)))),
        }
    })
}

fn fixed_holds(schema: &SchemaNode, raw: Option<&String>) -> bool {
    schema
        .fixed
        .as_ref()
        .is_none_or(|fixed| raw.is_some_and(|raw| raw == fixed))
}

/// Human-readable description of a trigger for error messages, e.g.
/// `HL(03=22)`.
fn describe_trigger(trigger: &SchemaNode) -> String {
    let SchemaKind::Group { children } = &trigger.kind else {
        return trigger.name.clone();
    };
    let constraints: Vec<String> = children
        .iter()
        .flat_map(|child| match &child.kind {
            SchemaKind::Scalar { .. } => child
                .fixed
                .as_ref()
                .map(|f| format!("{}={f}", child.name))
                .into_iter()
                .collect::<Vec<_>>(),
            SchemaKind::Group {
                children: component_schemas,
            } => component_schemas
                .iter()
                .filter_map(|comp| {
                    comp.fixed
                        .as_ref()
                        .map(|f| format!("{}.{}={f}", child.name, comp.name))
                })
                .collect(),
        })
        .collect();
    if constraints.is_empty() {
        trigger.name.clone()
    } else {
        format!("{}({})", trigger.name, constraints.join(","))
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
///
/// With `lenient`, segments the schema doesn't mention are skipped instead
/// of erroring -- but only when they match *no* current or upcoming
/// expectation (the current trigger, any later sibling's trigger at every
/// ancestor level, or an ancestor loop's next iteration), so declared
/// segments are never swallowed. Trailing unmentioned segments are ignored
/// too.
pub(crate) fn read_segments(
    schema: &SchemaNode,
    segments: &[Segment],
    component_join: char,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    let mut cursor = Cursor { segments, pos: 0 };
    let instance = read_node(schema, &mut cursor, component_join, true, lenient, &[])?;
    if let Some(segment) = cursor.peek()
        && !lenient
    {
        return Err(EdiFormatError::TrailingSegment {
            index: cursor.pos,
            id: segment.id.clone(),
        });
    }
    Ok(instance)
}

/// Advances past segments that match none of `expectations`.
fn skip_unmatched(cursor: &mut Cursor, expectations: &[&SchemaNode]) {
    while let Some(segment) = cursor.peek() {
        if expectations.iter().any(|t| segment_matches(t, segment)) {
            return;
        }
        cursor.pos += 1;
    }
}

fn read_node(
    node: &SchemaNode,
    cursor: &mut Cursor,
    component_join: char,
    is_root: bool,
    lenient: bool,
    follow: &[&SchemaNode],
) -> Result<Instance, EdiFormatError> {
    match shape_of(node, is_root)? {
        Shape::Segment(elements) => read_segment(node, elements, cursor, component_join),
        Shape::Container(children) => {
            let mut fields = Vec::with_capacity(children.len());
            for (i, child) in children.iter().enumerate() {
                let trigger = trigger_of(child)?;
                // Triggers that may legitimately appear once this child is
                // done: later siblings here, then everything the ancestors
                // still expect.
                let mut child_follow: Vec<&SchemaNode> = children[i + 1..]
                    .iter()
                    .map(trigger_of)
                    .collect::<Result<_, _>>()?;
                child_follow.extend_from_slice(follow);

                let mut expectations = vec![trigger];
                expectations.extend_from_slice(&child_follow);

                if child.repeating {
                    // The loop's own trigger stays expected across nested
                    // reads, so leniency can't swallow the next iteration.
                    let mut nested_follow = vec![trigger];
                    nested_follow.extend_from_slice(&child_follow);
                    let mut items = Vec::new();
                    loop {
                        if lenient {
                            skip_unmatched(cursor, &expectations);
                        }
                        if !cursor.peek().is_some_and(|s| segment_matches(trigger, s)) {
                            break;
                        }
                        items.push(read_node(
                            child,
                            cursor,
                            component_join,
                            false,
                            lenient,
                            &nested_follow,
                        )?);
                    }
                    fields.push((child.name.clone(), Instance::Repeated(items)));
                } else {
                    if lenient {
                        skip_unmatched(cursor, &expectations);
                    }
                    if cursor.peek().is_some_and(|s| segment_matches(trigger, s)) {
                        fields.push((
                            child.name.clone(),
                            read_node(
                                child,
                                cursor,
                                component_join,
                                false,
                                lenient,
                                &child_follow,
                            )?,
                        ));
                    } else {
                        return Err(EdiFormatError::UnexpectedSegment {
                            index: cursor.pos,
                            expected: describe_trigger(trigger),
                            found: cursor
                                .peek()
                                .map_or_else(|| "end of interchange".to_string(), |s| s.id.clone()),
                        });
                    }
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
        .ok_or_else(|| EdiFormatError::UnexpectedSegment {
            index: cursor.pos,
            expected: describe_trigger(node),
            found: "end of interchange".to_string(),
        })?;
    if segment.id != node.name {
        return Err(EdiFormatError::UnexpectedSegment {
            index: cursor.pos,
            expected: describe_trigger(node),
            found: segment.id.clone(),
        });
    }
    static EMPTY_REPEATS: Vec<Vec<String>> = Vec::new();
    static EMPTY_COMPONENTS: Vec<String> = Vec::new();
    let mut fields = Vec::with_capacity(element_schemas.len());
    for (i, element_schema) in element_schemas.iter().enumerate() {
        let repeats = segment.elements.get(i).unwrap_or(&EMPTY_REPEATS);
        // An element child marked `repeating` collects every repeat
        // (X12 5010 repetition); otherwise only the first is read.
        let instance = if element_schema.repeating {
            let items = repeats
                .iter()
                .map(|components| {
                    read_one_repeat(element_schema, components, &segment.id, i, component_join)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Instance::Repeated(items)
        } else {
            let components = repeats.first().unwrap_or(&EMPTY_COMPONENTS);
            read_one_repeat(element_schema, components, &segment.id, i, component_join)?
        };
        fields.push((element_schema.name.clone(), instance));
    }
    cursor.pos += 1;
    Ok(Instance::Group(fields))
}

fn read_one_repeat(
    element_schema: &SchemaNode,
    components: &[String],
    segment_id: &str,
    element_index: usize,
    component_join: char,
) -> Result<Instance, EdiFormatError> {
    match &element_schema.kind {
        SchemaKind::Scalar { ty } => {
            let raw = if components.len() > 1 {
                components.join(&component_join.to_string())
            } else {
                components.first().cloned().unwrap_or_default()
            };
            Ok(Instance::Scalar(parse_element(
                segment_id,
                element_index + 1,
                *ty,
                &raw,
            )?))
        }
        SchemaKind::Group {
            children: component_schemas,
        } => {
            let mut parts = Vec::with_capacity(component_schemas.len());
            for (j, component_schema) in component_schemas.iter().enumerate() {
                let SchemaKind::Scalar { ty } = component_schema.kind else {
                    return Err(EdiFormatError::UnsupportedSchema(format!(
                        "{}/{}",
                        element_schema.name, component_schema.name
                    )));
                };
                let raw = components.get(j).map_or("", String::as_str);
                parts.push((
                    component_schema.name.clone(),
                    Instance::Scalar(parse_element(segment_id, element_index + 1, ty, raw)?),
                ));
            }
            Ok(Instance::Group(parts))
        }
    }
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
            let mut elements = element_schemas
                .iter()
                .enumerate()
                .map(|(index, element)| {
                    validate_isa_separator(
                        &node.name,
                        index,
                        element,
                        instance.field(&element.name),
                        opts,
                    )?;
                    let allowed_reserved = match (node.name.as_str(), index) {
                        ("ISA", 10) => opts.repetition,
                        ("ISA", 15) => Some(opts.component),
                        _ => None,
                    };
                    write_element(
                        element,
                        instance.field(&element.name),
                        opts,
                        allowed_reserved,
                    )
                })
                .collect::<Result<Vec<String>, _>>()?;
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

fn validate_isa_separator(
    segment: &str,
    index: usize,
    schema: &SchemaNode,
    instance: Option<&Instance>,
    opts: &WriteOptions,
) -> Result<(), EdiFormatError> {
    if segment != "ISA" {
        return Ok(());
    }
    let text = scalar_or_fixed(schema, instance.and_then(Instance::as_scalar));
    let expected = match index {
        10 => {
            let mut chars = text.chars();
            let Some(found) = chars.next() else {
                return Ok(());
            };
            // In pre-5010 X12, ISA11 is an alphanumeric standards ID rather
            // than a repetition separator.
            if chars.next().is_some() || found.is_alphanumeric() {
                return Ok(());
            }
            opts.repetition
        }
        15 => Some(opts.component),
        _ => return Ok(()),
    };
    if let Some(expected) = expected
        && text != expected.to_string()
    {
        return Err(EdiFormatError::EnvelopeSeparatorMismatch {
            element: schema.name.clone(),
            expected,
            found: text,
        });
    }
    Ok(())
}

fn write_element(
    schema: &SchemaNode,
    instance: Option<&Instance>,
    opts: &WriteOptions,
    allowed_reserved: Option<char>,
) -> Result<String, EdiFormatError> {
    if let Some(Instance::Repeated(items)) = instance {
        let Some(repetition) = opts.repetition else {
            return Err(EdiFormatError::UnsupportedSchema(format!(
                "element `{}` repeats, but this dialect has no repetition separator",
                schema.name
            )));
        };
        let repeats = items
            .iter()
            .map(|item| write_one_repeat(schema, Some(item), opts, allowed_reserved))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(repeats.join(&repetition.to_string()));
    }
    write_one_repeat(schema, instance, opts, allowed_reserved)
}

fn write_one_repeat(
    schema: &SchemaNode,
    instance: Option<&Instance>,
    opts: &WriteOptions,
    allowed_reserved: Option<char>,
) -> Result<String, EdiFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { .. } => escape(
            &scalar_or_fixed(schema, instance.and_then(Instance::as_scalar)),
            &schema.name,
            opts,
            allowed_reserved,
        ),
        SchemaKind::Group { children } => {
            let mut components: Vec<String> = children
                .iter()
                .map(|c| {
                    escape(
                        &scalar_or_fixed(
                            c,
                            instance
                                .and_then(|i| i.field(&c.name))
                                .and_then(Instance::as_scalar),
                        ),
                        &c.name,
                        opts,
                        None,
                    )
                })
                .collect::<Result<_, _>>()?;
            while components.last().is_some_and(String::is_empty) {
                components.pop();
            }
            Ok(components.join(&opts.component.to_string()))
        }
    }
}

/// The serialized text for one element/component: the instance value, or
/// the schema's `fixed` value when the instance doesn't provide one -- so
/// qualifier elements need no explicit bindings in a mapping.
fn scalar_or_fixed(schema: &SchemaNode, value: Option<&Value>) -> String {
    let text = format_value(value);
    if text.is_empty()
        && let Some(fixed) = &schema.fixed
    {
        return fixed.clone();
    }
    text
}

fn escape(
    text: &str,
    element: &str,
    opts: &WriteOptions,
    allowed_reserved: Option<char>,
) -> Result<String, EdiFormatError> {
    let Some(release) = opts.release else {
        if text.chars().count() == 1 && text.chars().next() == allowed_reserved {
            return Ok(text.to_string());
        }
        if let Some(delimiter) = text.chars().find(|character| {
            *character == opts.element
                || *character == opts.component
                || *character == opts.terminator
                || opts.repetition == Some(*character)
        }) {
            return Err(EdiFormatError::UnescapableDelimiter {
                element: element.to_string(),
                delimiter,
            });
        }
        return Ok(text.to_string());
    };
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == release
            || c == opts.element
            || c == opts.component
            || c == opts.terminator
            || opts.repetition == Some(c)
        {
            out.push(release);
        }
        out.push(c);
    }
    Ok(out)
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
