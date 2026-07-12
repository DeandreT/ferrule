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
    schema_segment_id(name).is_some()
}

fn schema_segment_id(name: &str) -> Option<&str> {
    let candidate = name.strip_prefix("MF_").unwrap_or(name);
    if is_edifact_segment_group(candidate) {
        return None;
    }
    (2..=3).contains(&candidate.len()).then_some(())?;
    candidate
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
        .then_some(())?;
    candidate
        .chars()
        .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        .then_some(candidate)
}

fn is_edifact_segment_group(name: &str) -> bool {
    name.strip_prefix("SG").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
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
            let trigger = trigger_of(first)?;
            schema_segment_id(&trigger.name)
                .ok_or_else(|| EdiFormatError::UnsupportedSchema(trigger.name.clone()))
        }
        Shape::Segment(_) => Err(EdiFormatError::UnsupportedSchema(schema.name.clone())),
    }
}

/// Whether `segment` satisfies a segment schema: the IDs must agree and
/// every `fixed` element/component constraint must hold. Fixed values are
/// what disambiguate qualifier-driven loops (e.g. `HL` with `HL03` fixed
/// to `20` vs `22`, or repeated `NM1`s told apart by `NM101`).
fn segment_matches(trigger: &SchemaNode, segment: &Segment) -> bool {
    if schema_segment_id(&trigger.name) != Some(segment.id.as_str()) {
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
    if schema_segment_id(&node.name) != Some(segment.id.as_str()) {
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
        ScalarType::Float => {
            let value = raw.parse::<f64>().map_err(|_| bad())?;
            if !value.is_finite() {
                return Err(bad());
            }
            Value::Float(value)
        }
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
    validate_instance_shape(schema, instance)?;
    let mut out = String::new();
    write_node(schema, instance, opts, &mut out, true)?;
    Ok(out)
}

pub(crate) fn validate_instance_shape(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), EdiFormatError> {
    if schema.repeating {
        let Instance::Repeated(items) = instance else {
            return Err(instance_shape_error(schema, "repeating values", instance));
        };
        for item in items {
            validate_single_instance(schema, item)?;
        }
        return Ok(());
    }
    if matches!(instance, Instance::Repeated(_)) {
        return Err(instance_shape_error(schema, "one value", instance));
    }
    validate_single_instance(schema, instance)
}

fn validate_single_instance(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), EdiFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            let Instance::Scalar(value) = instance else {
                return Err(instance_shape_error(schema, "a scalar", instance));
            };
            scalar_or_fixed(schema, Some(value)).map(|_| ())
        }
        SchemaKind::Group { children } => {
            let Instance::Group(fields) = instance else {
                return Err(instance_shape_error(schema, "a group", instance));
            };
            validate_group_fields(schema, children, fields)?;
            for child in children {
                if let Some((_, value)) = fields.iter().find(|(name, _)| name == &child.name) {
                    validate_instance_shape(child, value)?;
                }
            }
            Ok(())
        }
    }
}

fn validate_group_fields(
    schema: &SchemaNode,
    children: &[SchemaNode],
    fields: &[(String, Instance)],
) -> Result<(), EdiFormatError> {
    for (index, (name, _)) in fields.iter().enumerate() {
        if !children.iter().any(|child| child.name == *name) {
            return Err(EdiFormatError::UnexpectedField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
        if fields[..index].iter().any(|(previous, _)| previous == name) {
            return Err(EdiFormatError::DuplicateField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
    }
    Ok(())
}

fn instance_shape_error(
    schema: &SchemaNode,
    expected: &'static str,
    instance: &Instance,
) -> EdiFormatError {
    let got = match instance {
        Instance::Scalar(_) => "a scalar",
        Instance::Group(_) => "a group",
        Instance::Repeated(_) => "repeating values",
    };
    EdiFormatError::InstanceShape {
        name: schema.name.clone(),
        expected,
        got,
    }
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
            let segment_id = schema_segment_id(&node.name)
                .ok_or_else(|| EdiFormatError::UnsupportedSchema(node.name.clone()))?;
            let mut elements = element_schemas
                .iter()
                .enumerate()
                .map(|(index, element)| {
                    validate_isa_separator(
                        segment_id,
                        index,
                        element,
                        instance.field(&element.name),
                        opts,
                    )?;
                    let allowed_reserved = match (segment_id, index) {
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
            if segment_id != "ISA" {
                while elements.last().is_some_and(String::is_empty) {
                    elements.pop();
                }
            }
            out.push_str(segment_id);
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
    let text = scalar_or_fixed(schema, instance.and_then(Instance::as_scalar))?;
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
        SchemaKind::Scalar { .. } => {
            let text = scalar_or_fixed(schema, instance.and_then(Instance::as_scalar))?;
            escape(&text, &schema.name, opts, allowed_reserved)
        }
        SchemaKind::Group { children } => {
            let mut components: Vec<String> = children
                .iter()
                .map(|c| {
                    let text = scalar_or_fixed(
                        c,
                        instance
                            .and_then(|i| i.field(&c.name))
                            .and_then(Instance::as_scalar),
                    )?;
                    escape(&text, &c.name, opts, None)
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
fn scalar_or_fixed(schema: &SchemaNode, value: Option<&Value>) -> Result<String, EdiFormatError> {
    let missing = value.is_none_or(|value| {
        matches!(value, Value::Null) || matches!(value, Value::String(text) if text.is_empty())
    });
    let Some(fixed) = &schema.fixed else {
        if missing {
            return Ok(String::new());
        }
        let Some(value) = value else {
            return Ok(String::new());
        };
        return format_value(schema, value);
    };

    let normalized_fixed = format_value(schema, &Value::String(fixed.clone()))?;
    if missing {
        return Ok(fixed.clone());
    }
    let Some(value) = value else {
        return Ok(fixed.clone());
    };
    let normalized_value = format_value(schema, value)?;
    if semantically_equal(schema, &normalized_fixed, &normalized_value) {
        Ok(fixed.clone())
    } else {
        Err(EdiFormatError::FixedValueMismatch {
            element: schema.name.clone(),
            expected: fixed.clone(),
            found: normalized_value,
        })
    }
}

fn semantically_equal(schema: &SchemaNode, left: &str, right: &str) -> bool {
    match schema.kind {
        SchemaKind::Scalar {
            ty: ScalarType::Float,
        } => left
            .parse::<f64>()
            .ok()
            .zip(right.parse::<f64>().ok())
            .is_some_and(|(left, right)| left == right),
        SchemaKind::Scalar { .. } => left == right,
        SchemaKind::Group { .. } => false,
    }
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

fn format_value(schema: &SchemaNode, value: &Value) -> Result<String, EdiFormatError> {
    let SchemaKind::Scalar { ty } = schema.kind else {
        return Err(EdiFormatError::UnsupportedSchema(schema.name.clone()));
    };
    let incompatible = |got| EdiFormatError::ValueType {
        element: schema.name.clone(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (_, Value::Null) => Ok(String::new()),
        (ScalarType::String, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::String, Value::Float(_)) => Err(EdiFormatError::NonFiniteFloat {
            element: schema.name.clone(),
        }),
        (ScalarType::String, Value::String(value)) => Ok(value.clone()),
        (ScalarType::Int, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::Int, Value::String(value)) => value
            .trim()
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| incompatible("string")),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::Float, Value::Float(_)) => Err(EdiFormatError::NonFiniteFloat {
            element: schema.name.clone(),
        }),
        (ScalarType::Float, Value::Int(value)) if exact_f64(*value).is_some() => {
            Ok(value.to_string())
        }
        (ScalarType::Float, Value::Int(_)) => Err(incompatible("int outside the exact f64 range")),
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("string")),
        (ScalarType::Bool, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::Bool, Value::String(value)) => value
            .trim()
            .parse::<bool>()
            .map(|value| value.to_string())
            .map_err(|_| incompatible("string")),
        (_, other) => Err(incompatible(other.type_name())),
    }
}

fn exact_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return Some(0.0);
    }
    let significant_bits = u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros();
    (significant_bits <= f64::MANTISSA_DIGITS).then_some(value as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_edifact_segment_groups_are_containers() {
        let schema = SchemaNode::group(
            "EDIFACT",
            vec![SchemaNode::group(
                "SG2",
                vec![SchemaNode::group(
                    "NAD",
                    vec![SchemaNode::scalar("3035", ScalarType::String)],
                )],
            )],
        );

        assert_eq!(root_trigger(&schema).unwrap(), "NAD");
    }

    #[test]
    fn mapforce_acknowledgement_prefix_is_not_part_of_the_segment_id() {
        let schema = SchemaNode::group(
            "X12",
            vec![SchemaNode::group(
                "ParserErrors",
                vec![SchemaNode::group(
                    "MF_AK9",
                    vec![SchemaNode::scalar("715", ScalarType::String)],
                )],
            )],
        );

        assert_eq!(root_trigger(&schema).unwrap(), "AK9");
    }
}
