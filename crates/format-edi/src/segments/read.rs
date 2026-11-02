//! Schema-guided segment descent, including source lexical validation.
use super::*;
use crate::source_validation::SourceValidation;
use mapping::EdiValueConstraint;

struct Cursor<'a> {
    segments: &'a [Segment],
    component_joins: Option<&'a [char]>,
    pos: usize,
    validation: SourceValidation<'a>,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<&Segment> {
        self.segments.get(self.pos)
    }
}

/// Maps tokenized segments onto the existing schema grammar. Unmentioned
/// segments are skipped only when none of the current/ancestor expectations
/// can consume them. `component_join` reconstructs a scalar-declared composite.
pub(crate) fn read_segments(
    schema: &SchemaNode,
    segments: &[Segment],
    component_join: char,
    subcomponent: Option<char>,
    lenient: bool,
    constraints: &[EdiValueConstraint],
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
        None,
        constraints,
    )
}

/// EDIFACT can declare a different component separator in each interchange.
pub(crate) fn read_edifact_segments(
    schema: &SchemaNode,
    segments: &[Segment],
    component_joins: &[char],
    lenient: bool,
    constraints: &[EdiValueConstraint],
) -> Result<Instance, EdiFormatError> {
    if component_joins.len() != segments.len() {
        return Err(EdiFormatError::NotEdifact(
            "incomplete segment syntax metadata",
        ));
    }
    read_segments_with_syntax(
        schema,
        segments,
        ReadSyntax {
            component_join: ':',
            subcomponent: None,
            subcomponent_escape: None,
        },
        lenient,
        Some(component_joins),
        constraints,
    )
}

/// Scalar reconstruction uses the syntax of the supplying X12 interchange.
pub(crate) fn read_x12_segments(
    schema: &SchemaNode,
    segments: &[Segment],
    component_joins: &[char],
    lenient: bool,
    constraints: &[EdiValueConstraint],
) -> Result<Instance, EdiFormatError> {
    if component_joins.len() != segments.len() {
        return Err(EdiFormatError::NotX12("incomplete segment syntax metadata"));
    }
    read_segments_with_syntax(
        schema,
        segments,
        ReadSyntax {
            component_join: ':',
            subcomponent: None,
            subcomponent_escape: None,
        },
        lenient,
        Some(component_joins),
        constraints,
    )
}

/// HL7 literal subcomponent escapes are decoded after structural splitting.
pub(crate) fn read_segments_with_subcomponent_escape(
    schema: &SchemaNode,
    segments: &[Segment],
    component_join: char,
    subcomponent: char,
    escape: char,
    lenient: bool,
    constraints: &[EdiValueConstraint],
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
        None,
        constraints,
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
    component_joins: Option<&[char]>,
    constraints: &[EdiValueConstraint],
) -> Result<Instance, EdiFormatError> {
    let mut cursor = Cursor {
        segments,
        component_joins,
        pos: 0,
        validation: SourceValidation::new(constraints),
    };
    let instance = read_node(schema, &mut cursor, syntax, true, lenient, &[]);
    // A violation already observed on decoded wire text remains a validation
    // error even if conversion of that same field subsequently fails.
    let trailing =
        cursor
            .peek()
            .filter(|_| !lenient)
            .map(|segment| EdiFormatError::TrailingSegment {
                index: cursor.pos,
                id: segment.id.clone(),
            });
    cursor.validation.finish()?;
    let instance = instance?;
    if let Some(error) = trailing {
        return Err(error);
    }
    Ok(instance)
}

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
                // Later siblings and ancestor expectations keep leniency from
                // swallowing declared segments or the next loop iteration.
                let mut child_follow = Vec::new();
                for sibling in &children[i + 1..] {
                    child_follow.extend(leading_triggers(sibling)?);
                }
                child_follow.extend_from_slice(follow);
                let mut expectations = triggers.clone();
                expectations.extend_from_slice(&child_follow);
                if child.repeating {
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
                        let mark = cursor.validation.enter(&child.name, Some(items.len() + 1));
                        let item = read_node(child, cursor, syntax, false, lenient, &nested_follow);
                        cursor.validation.leave(mark);
                        items.push(item?);
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
                        let mark = cursor.validation.enter(&child.name, None);
                        let item = read_node(child, cursor, syntax, false, lenient, &child_follow);
                        cursor.validation.leave(mark);
                        fields.push((child.name.clone(), item?));
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
            Ok(Instance::Group(fields.into()))
        }
    }
}

fn read_segment(
    node: &SchemaNode,
    element_schemas: &[SchemaNode],
    cursor: &mut Cursor,
    syntax: ReadSyntax,
) -> Result<Instance, EdiFormatError> {
    let syntax = ReadSyntax {
        component_join: cursor
            .component_joins
            .and_then(|values| values.get(cursor.pos))
            .copied()
            .unwrap_or(syntax.component_join),
        ..syntax
    };
    let segment =
        cursor
            .segments
            .get(cursor.pos)
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
        // A non-repeating field still reads only the first wire repetition.
        let instance = if element_schema.repeating {
            let mut items = Vec::with_capacity(repeats.len());
            for (index, components) in repeats.iter().enumerate() {
                let mark = cursor
                    .validation
                    .enter(&element_schema.name, Some(index + 1));
                let item = read_one_repeat(
                    element_schema,
                    components,
                    &segment.id,
                    i,
                    syntax,
                    &mut cursor.validation,
                );
                cursor.validation.leave(mark);
                items.push(item?);
            }
            Instance::Repeated(items)
        } else {
            let components = repeats.first().unwrap_or(&EMPTY_COMPONENTS);
            let mark = cursor.validation.enter(&element_schema.name, None);
            let item = read_one_repeat(
                element_schema,
                components,
                &segment.id,
                i,
                syntax,
                &mut cursor.validation,
            );
            cursor.validation.leave(mark);
            item?
        };
        fields.push((element_schema.name.clone(), instance));
    }
    cursor.pos += 1;
    Ok(Instance::Group(fields.into()))
}

fn read_one_repeat(
    element_schema: &SchemaNode,
    components: &[String],
    segment_id: &str,
    element_index: usize,
    syntax: ReadSyntax,
    validation: &mut SourceValidation<'_>,
) -> Result<Instance, EdiFormatError> {
    match &element_schema.kind {
        SchemaKind::Scalar { ty } => {
            let raw = if components.len() > 1 {
                components.join(&syntax.component_join.to_string())
            } else {
                components.first().cloned().unwrap_or_default()
            };
            let raw = decode_preserved_subcomponent(&raw, syntax);
            validation.check(raw.as_ref());
            Ok(Instance::Scalar(parse_element(
                segment_id,
                element_index + 1,
                *ty,
                raw.as_ref(),
            )?))
        }
        SchemaKind::ScalarUnion { .. } => Err(EdiFormatError::UnsupportedSchema(format!(
            "EDI element `{}` cannot preserve a heterogeneous scalar union",
            element_schema.name
        ))),
        SchemaKind::Group { children, .. } => {
            let mut parts = Vec::with_capacity(children.len());
            for (j, component_schema) in children.iter().enumerate() {
                let raw = components.get(j).map_or("", String::as_str);
                let mark = validation.enter(&component_schema.name, None);
                let item = read_nested_component(
                    component_schema,
                    raw,
                    syntax.subcomponent,
                    syntax,
                    segment_id,
                    element_index,
                    validation,
                );
                validation.leave(mark);
                parts.push((component_schema.name.clone(), item?));
            }
            Ok(Instance::Group(parts.into()))
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
    validation: &mut SourceValidation<'_>,
) -> Result<Instance, EdiFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            let raw = decode_preserved_subcomponent(raw, syntax);
            validation.check(raw.as_ref());
            Ok(Instance::Scalar(parse_element(
                segment_id,
                element_index + 1,
                *ty,
                raw.as_ref(),
            )?))
        }
        SchemaKind::ScalarUnion { .. } => Err(EdiFormatError::UnsupportedSchema(format!(
            "EDI component `{}` cannot preserve a heterogeneous scalar union",
            schema.name
        ))),
        SchemaKind::Group { children, .. } => {
            let parts = separator
                .map(|separator| raw.split(separator).collect::<Vec<_>>())
                .unwrap_or_else(|| vec![raw]);
            let mut fields = Vec::with_capacity(children.len());
            for (index, child) in children.iter().enumerate() {
                let mark = validation.enter(&child.name, None);
                let item = read_nested_component(
                    child,
                    parts.get(index).copied().unwrap_or_default(),
                    None,
                    syntax,
                    segment_id,
                    element_index,
                    validation,
                );
                validation.leave(mark);
                fields.push((child.name.clone(), item?));
            }
            Ok(Instance::Group(fields.into()))
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
