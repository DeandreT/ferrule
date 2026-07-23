//! The dialect-agnostic core shared by X12 and EDIFACT: a tokenized
//! [`Segment`] carries elements that are lists of components, and the
//! schema-guided recursive descent maps them onto the [`SchemaNode`] tree.
//!
//! Schema conventions:
//! - A group named like a segment ID (2-30 uppercase alphanumeric chars,
//!   starting with a letter -- `ISA`, `BEG`, `UNB`, `LIN`) whose children
//!   are scalars and/or groups-of-scalars is a **segment** matcher. Scalar
//!   children map positionally to elements 1..N; a group child is a
//!   **composite** element whose scalar children map positionally to its
//!   components. Extra elements/components in the file are ignored;
//!   missing/empty ones read as `Null`. Declaring a composite element as a
//!   plain scalar reads its raw text (components joined by the component
//!   separator).
//! - Any other group is a **loop/container**: it matches when one of its
//!   leading segment descendants matches the cursor. Leading children marked
//!   `repeating: true` may be absent, so a later required child can also start
//!   the container. `repeating: true` means 0..N occurrences (also the v1
//!   spelling for optional).
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

mod value;

use value::{escape, scalar_or_fixed, write_component};

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    /// One entry per element; each element is one or more repeats (X12
    /// 5010's repetition separator -- exactly one repeat when the dialect
    /// or file has no repetition); each repeat is one or more components.
    pub elements: Vec<Vec<Vec<String>>>,
}

/// Structural spelling used around and inside serialized segments.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStyle {
    /// `ID+element`-style syntax used by X12 and EDIFACT.
    Delimited,
    /// `ID=element`-style syntax used by TRADACOMS.
    Assigned,
    /// HL7 v2 header and escape spelling, including one subcomponent level.
    Hl7 { subcomponent: char },
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
    pub style: WriteStyle,
    /// Five ASCII digits used only when ISA12 is absent from the instance.
    pub interchange_version: Option<[u8; 5]>,
}

fn schema_segment_id(name: &str) -> Option<&str> {
    let candidate = name.strip_prefix("MF_").unwrap_or(name);
    let candidate = candidate
        .rsplit_once('_')
        .filter(|(_, suffix)| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map_or(candidate, |(base, _)| base);
    if is_edifact_segment_group(candidate) {
        return None;
    }
    (2..=30).contains(&candidate.len()).then_some(())?;
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
    let SchemaKind::Group { children, .. } = &node.kind else {
        return Err(EdiFormatError::UnsupportedSchema(node.name.clone()));
    };
    let segment_id = (!is_root).then(|| schema_segment_id(&node.name)).flatten();
    // Configured message bodies can have an all-uppercase type name such as
    // `INVOICE`, while holding only segment groups. Supported interchange
    // segment IDs are at most three characters; longer ID-shaped names with
    // only group children are therefore containers. Long flat names remain
    // valid for IDoc record schemas, which share this shape validator.
    if segment_id.is_some_and(|id| id.len() > 3)
        && children
            .iter()
            .all(|child| matches!(child.kind, SchemaKind::Group { .. }))
    {
        return Ok(Shape::Container(children));
    }
    if segment_id.is_some() {
        let valid_segment = children.iter().all(is_scalar_tree);
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

fn is_scalar_tree(node: &SchemaNode) -> bool {
    match &node.kind {
        SchemaKind::Scalar { .. } => true,
        SchemaKind::Group { children, .. } => children.iter().all(is_scalar_tree),
    }
}

/// Segment schemas that can begin `node`. A repeating leading child is also
/// the IR spelling for an optional child, so every following sibling remains
/// a possible start until the first required child is reached.
fn leading_triggers(node: &SchemaNode) -> Result<Vec<&SchemaNode>, EdiFormatError> {
    let mut triggers = Vec::new();
    collect_leading_triggers(node, false, &mut triggers)?;
    if triggers.is_empty() {
        return Err(EdiFormatError::UnsupportedSchema(node.name.clone()));
    }
    Ok(triggers)
}

fn collect_leading_triggers<'a>(
    node: &'a SchemaNode,
    is_root: bool,
    triggers: &mut Vec<&'a SchemaNode>,
) -> Result<(), EdiFormatError> {
    match shape_of(node, is_root)? {
        Shape::Segment(_) => triggers.push(node),
        Shape::Container(children) => {
            if children.is_empty() {
                return Err(EdiFormatError::UnsupportedSchema(node.name.clone()));
            }
            for child in children {
                collect_leading_triggers(child, false, triggers)?;
                if !child.repeating {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// The first segment ID a whole schema expects (the root is always a
/// container, whatever its name) -- used for dialect detection.
pub(crate) fn root_trigger(schema: &SchemaNode) -> Result<&str, EdiFormatError> {
    match shape_of(schema, true)? {
        Shape::Container(children) => {
            let mut triggers = Vec::new();
            for child in children {
                collect_leading_triggers(child, false, &mut triggers)?;
                if !child.repeating {
                    break;
                }
            }
            let trigger = triggers
                .first()
                .ok_or_else(|| EdiFormatError::UnsupportedSchema(schema.name.clone()))?;
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
    let SchemaKind::Group { children, .. } = &trigger.kind else {
        return false;
    };
    children.iter().enumerate().all(|(i, child)| {
        // Constraints are checked against the first repeat.
        let components = segment.elements.get(i).and_then(|repeats| repeats.first());
        match &child.kind {
            SchemaKind::Scalar { .. } => fixed_holds(child, components.and_then(|c| c.first())),
            SchemaKind::Group {
                children: component_schemas,
                ..
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
    let SchemaKind::Group { children, .. } = &trigger.kind else {
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
                ..
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
    subcomponent: Option<char>,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    read_segments_with_syntax(
        schema,
        segments,
        ReadSyntax {
            component_join,
            subcomponent,
            subcomponent_escape: None,
        },
        lenient,
    )
}

/// Reads segments whose subcomponent delimiter can also occur as an encoded
/// literal. HL7 represents that case as `<escape>T<escape>`; decoding must
/// happen after the structural subcomponent split.
pub(crate) fn read_segments_with_subcomponent_escape(
    schema: &SchemaNode,
    segments: &[Segment],
    component_join: char,
    subcomponent: char,
    escape: char,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    read_segments_with_syntax(
        schema,
        segments,
        ReadSyntax {
            component_join,
            subcomponent: Some(subcomponent),
            subcomponent_escape: Some(escape),
        },
        lenient,
    )
}

#[derive(Clone, Copy)]
struct ReadSyntax {
    component_join: char,
    subcomponent: Option<char>,
    subcomponent_escape: Option<char>,
}

fn read_segments_with_syntax(
    schema: &SchemaNode,
    segments: &[Segment],
    syntax: ReadSyntax,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    let mut cursor = Cursor { segments, pos: 0 };
    let instance = read_node(schema, &mut cursor, syntax, true, lenient, &[])?;
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
    syntax: ReadSyntax,
    is_root: bool,
    lenient: bool,
    follow: &[&SchemaNode],
) -> Result<Instance, EdiFormatError> {
    match shape_of(node, is_root)? {
        Shape::Segment(elements) => read_segment(node, elements, cursor, syntax),
        Shape::Container(children) => {
            let mut fields = Vec::with_capacity(children.len());
            for (i, child) in children.iter().enumerate() {
                let triggers = leading_triggers(child)?;
                // Triggers that may legitimately appear once this child is
                // done: later siblings here, then everything the ancestors
                // still expect.
                let mut child_follow = Vec::new();
                for sibling in &children[i + 1..] {
                    child_follow.extend(leading_triggers(sibling)?);
                }
                child_follow.extend_from_slice(follow);

                let mut expectations = triggers.clone();
                expectations.extend_from_slice(&child_follow);

                if child.repeating {
                    // Every possible start of this loop stays expected across
                    // nested reads, so leniency can't swallow the next
                    // iteration when an optional prefix is absent.
                    let mut nested_follow = triggers.clone();
                    nested_follow.extend_from_slice(&child_follow);
                    let mut items = Vec::new();
                    loop {
                        if lenient {
                            skip_unmatched(cursor, &expectations);
                        }
                        if !cursor.peek().is_some_and(|segment| {
                            triggers
                                .iter()
                                .any(|trigger| segment_matches(trigger, segment))
                        }) {
                            break;
                        }
                        items.push(read_node(
                            child,
                            cursor,
                            syntax,
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
                    if cursor.peek().is_some_and(|segment| {
                        triggers
                            .iter()
                            .any(|trigger| segment_matches(trigger, segment))
                    }) {
                        fields.push((
                            child.name.clone(),
                            read_node(child, cursor, syntax, false, lenient, &child_follow)?,
                        ));
                    } else {
                        return Err(EdiFormatError::UnexpectedSegment {
                            index: cursor.pos,
                            expected: triggers
                                .iter()
                                .map(|trigger| describe_trigger(trigger))
                                .collect::<Vec<_>>()
                                .join(" or "),
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
    syntax: ReadSyntax,
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
                    read_one_repeat(element_schema, components, &segment.id, i, syntax)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Instance::Repeated(items)
        } else {
            let components = repeats.first().unwrap_or(&EMPTY_COMPONENTS);
            read_one_repeat(element_schema, components, &segment.id, i, syntax)?
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
    syntax: ReadSyntax,
) -> Result<Instance, EdiFormatError> {
    match &element_schema.kind {
        SchemaKind::Scalar { ty } => {
            let raw = if components.len() > 1 {
                components.join(&syntax.component_join.to_string())
            } else {
                components.first().cloned().unwrap_or_default()
            };
            let raw = decode_preserved_subcomponent(&raw, syntax);
            Ok(Instance::Scalar(parse_element(
                segment_id,
                element_index + 1,
                *ty,
                raw.as_ref(),
            )?))
        }
        SchemaKind::Group { children, .. } => {
            let mut parts = Vec::with_capacity(children.len());
            for (j, component_schema) in children.iter().enumerate() {
                let raw = components.get(j).map_or("", String::as_str);
                parts.push((
                    component_schema.name.clone(),
                    read_nested_component(
                        component_schema,
                        raw,
                        syntax.subcomponent,
                        syntax,
                        segment_id,
                        element_index,
                    )?,
                ));
            }
            Ok(Instance::Group(parts))
        }
    }
}

fn read_nested_component(
    schema: &SchemaNode,
    raw: &str,
    separator: Option<char>,
    syntax: ReadSyntax,
    segment_id: &str,
    element_index: usize,
) -> Result<Instance, EdiFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            let raw = decode_preserved_subcomponent(raw, syntax);
            Ok(Instance::Scalar(parse_element(
                segment_id,
                element_index + 1,
                *ty,
                raw.as_ref(),
            )?))
        }
        SchemaKind::Group { children, .. } => {
            let parts = separator
                .map(|separator| raw.split(separator).collect::<Vec<_>>())
                .unwrap_or_else(|| vec![raw]);
            let fields = children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    Ok((
                        child.name.clone(),
                        read_nested_component(
                            child,
                            parts.get(index).copied().unwrap_or_default(),
                            None,
                            syntax,
                            segment_id,
                            element_index,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, EdiFormatError>>()?;
            Ok(Instance::Group(fields))
        }
    }
}

fn decode_preserved_subcomponent(raw: &str, syntax: ReadSyntax) -> std::borrow::Cow<'_, str> {
    let (Some(separator), Some(escape)) = (syntax.subcomponent, syntax.subcomponent_escape) else {
        return std::borrow::Cow::Borrowed(raw);
    };
    let encoded = format!("{escape}T{escape}");
    if raw.contains(&encoded) {
        std::borrow::Cow::Owned(raw.replace(&encoded, &separator.to_string()))
    } else {
        std::borrow::Cow::Borrowed(raw)
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

/// Serializes already-tokenized segments with the same escaping rules as the
/// schema-guided writer. This is used after bounded envelope completion has
/// inserted or filled control segments.
pub(crate) fn serialize_segments(
    segments: &[Segment],
    opts: &WriteOptions,
) -> Result<String, EdiFormatError> {
    let mut out = String::new();
    for segment in segments {
        let mut serialized = Vec::with_capacity(segment.elements.len());
        for (index, element) in segment.elements.iter().enumerate() {
            if matches!(opts.style, WriteStyle::Hl7 { .. })
                && matches!(segment.id.as_str(), "FHS" | "BHS" | "MSH")
                && index < 2
            {
                serialized.push(
                    element
                        .first()
                        .and_then(|parts| parts.first())
                        .cloned()
                        .unwrap_or_default(),
                );
                continue;
            }
            let allowed_reserved = match (segment.id.as_str(), index) {
                ("ISA", 10) => opts.repetition,
                ("ISA", 15) => Some(opts.component),
                _ => None,
            };
            let repeats = element
                .iter()
                .map(|components| {
                    components
                        .iter()
                        .map(|component| escape(component, &segment.id, opts, allowed_reserved))
                        .collect::<Result<Vec<_>, _>>()
                        .map(|components| components.join(&opts.component.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if repeats.len() > 1 && opts.repetition.is_none() {
                return Err(EdiFormatError::UnsupportedSchema(format!(
                    "segment `{}` contains repeated elements, but this dialect has no repetition separator",
                    segment.id
                )));
            }
            serialized.push(repeats.join(&opts.repetition.unwrap_or_default().to_string()));
        }
        serialize_segment(&segment.id, &serialized, opts, &mut out);
    }
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
        SchemaKind::Group { children, .. } => {
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
        Instance::MappedSequence(_) => "a mapped sequence",
        Instance::DocumentSet(_) => "a document set",
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
            if matches!(opts.style, WriteStyle::Hl7 { .. })
                && matches!(segment_id, "FHS" | "BHS" | "MSH")
                && element_schemas.len() < 2
            {
                return Err(EdiFormatError::UnsupportedSchema(format!(
                    "HL7 header `{segment_id}` must declare its separator and encoding fields"
                )));
            }
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
                    let separator_default = match (segment_id, index) {
                        ("ISA", 10) => opts.repetition.map(|value| value.to_string()),
                        ("ISA", 11) => opts
                            .interchange_version
                            .map(|value| String::from_utf8_lossy(&value).into_owned()),
                        ("ISA", 15) => Some(opts.component.to_string()),
                        _ => None,
                    };
                    if matches!(opts.style, WriteStyle::Hl7 { .. })
                        && matches!(segment_id, "FHS" | "BHS" | "MSH")
                        && index < 2
                    {
                        return write_hl7_header_element(element, instance, index, opts);
                    }
                    write_element(
                        element,
                        instance.field(&element.name),
                        opts,
                        allowed_reserved,
                        separator_default.as_deref(),
                    )
                })
                .collect::<Result<Vec<String>, _>>()?;
            if segment_id != "ISA" {
                while elements.last().is_some_and(String::is_empty) {
                    elements.pop();
                }
            }
            serialize_segment(segment_id, &elements, opts, out);
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

fn serialize_segment(segment_id: &str, elements: &[String], opts: &WriteOptions, out: &mut String) {
    out.push_str(segment_id);
    match opts.style {
        WriteStyle::Delimited => {
            for element in elements {
                out.push(opts.element);
                out.push_str(element);
            }
        }
        WriteStyle::Assigned => {
            if let Some((first, rest)) = elements.split_first() {
                out.push('=');
                out.push_str(first);
                for element in rest {
                    out.push(opts.element);
                    out.push_str(element);
                }
            }
        }
        WriteStyle::Hl7 { .. } if matches!(segment_id, "FHS" | "BHS" | "MSH") => {
            if let Some(field_separator) = elements.first() {
                out.push_str(field_separator);
            }
            if let Some(encoding) = elements.get(1) {
                out.push_str(encoding);
            }
            for element in elements.iter().skip(2) {
                out.push(opts.element);
                out.push_str(element);
            }
        }
        WriteStyle::Hl7 { .. } => {
            for element in elements {
                out.push(opts.element);
                out.push_str(element);
            }
        }
    }
    out.push(opts.terminator);
    if !matches!(opts.style, WriteStyle::Hl7 { .. }) {
        out.push('\n');
    }
}

fn write_hl7_header_element(
    schema: &SchemaNode,
    instance: &Instance,
    index: usize,
    opts: &WriteOptions,
) -> Result<String, EdiFormatError> {
    let expected = match (index, opts.style) {
        (0, WriteStyle::Hl7 { .. }) => opts.element.to_string(),
        (1, WriteStyle::Hl7 { subcomponent }) => format!(
            "{}{}{}{}",
            opts.component,
            opts.repetition.unwrap_or('~'),
            opts.release.unwrap_or('\\'),
            subcomponent
        ),
        _ => return Err(EdiFormatError::UnsupportedSchema(schema.name.clone())),
    };
    let value = scalar_or_fixed(
        schema,
        instance.field(&schema.name).and_then(Instance::as_scalar),
    )?;
    if !value.is_empty() && value != expected {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: schema.name.clone(),
            value,
            reason: "value does not match the configured HL7 separators",
        });
    }
    Ok(expected)
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
        && !text.is_empty()
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
    separator_default: Option<&str>,
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
            .map(|item| {
                write_one_repeat(
                    schema,
                    Some(item),
                    opts,
                    allowed_reserved,
                    separator_default,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(repeats.join(&repetition.to_string()));
    }
    write_one_repeat(schema, instance, opts, allowed_reserved, separator_default)
}

fn write_one_repeat(
    schema: &SchemaNode,
    instance: Option<&Instance>,
    opts: &WriteOptions,
    allowed_reserved: Option<char>,
    separator_default: Option<&str>,
) -> Result<String, EdiFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            let mut text = scalar_or_fixed(schema, instance.and_then(Instance::as_scalar))?;
            if text.is_empty()
                && let Some(default) = separator_default
            {
                text.push_str(default);
            }
            escape(&text, &schema.name, opts, allowed_reserved)
        }
        SchemaKind::Group { children, .. } => {
            let mut components: Vec<String> = children
                .iter()
                .map(|child| {
                    write_component(child, instance.and_then(|i| i.field(&child.name)), opts)
                })
                .collect::<Result<_, _>>()?;
            while components.last().is_some_and(String::is_empty) {
                components.pop();
            }
            Ok(components.join(&opts.component.to_string()))
        }
    }
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

        let occurrence = SchemaNode::group(
            "X12",
            vec![SchemaNode::group(
                "REF_8",
                vec![SchemaNode::scalar("128", ScalarType::String)],
            )],
        );
        assert_eq!(root_trigger(&occurrence).unwrap(), "REF");
    }

    #[test]
    fn composite_fixed_values_select_a_segment() {
        let trigger = SchemaNode::group(
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
        let segment = Segment {
            id: "MHD".into(),
            elements: vec![
                vec![vec!["1".into()]],
                vec![vec!["ORDER".into(), "1".into()]],
            ],
        };

        assert!(segment_matches(&trigger, &segment));
    }
}
