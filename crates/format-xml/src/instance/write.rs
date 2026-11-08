use std::io::{Cursor, Write};
use std::path::Path;

use ir::{
    Instance, ScalarType, SchemaKind, SchemaNode, Value, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD,
    XML_TYPE_FIELD, XmlAlternativeKind, XmlNamespace, parse_exact_decimal_i64,
};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use crate::declaration::validate_schema;

use super::generic::{GenericSiblings, push_generic_attributes, write_generic_element};
use super::{
    MAX_XML_RECURSION_DEPTH, OrderedContentLayout, XmlFormatError, group_has_serialized_content,
    instance_kind, parse_scalar, resolve_recursive_schema, scalar_output, schema_hints,
    select_group_alternative, shape_error, split_expanded_name, validate_group_fields,
    write_ordered_mixed_content, xml_boolean,
};

pub(super) fn format_schema_scalar(
    schema: &SchemaNode,
    ty: ScalarType,
    value: &Value,
) -> Result<String, XmlFormatError> {
    let text = format_scalar(&schema.name, ty, value)?;
    if let Some(expected) = schema.fixed.as_deref() {
        let expected_value = parse_scalar(&schema.name, ty, expected)?;
        let actual_value = parse_scalar(&schema.name, ty, &text)?;
        if actual_value != expected_value {
            return Err(XmlFormatError::FixedValue {
                name: schema.name.clone(),
                expected: expected.to_string(),
                got: text,
            });
        }
        return Ok(expected.to_string());
    }
    Ok(text)
}

/// Writes an [`Instance`] tree shaped by `schema` to an XML file.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), XmlFormatError> {
    std::fs::write(path, to_string(schema, instance)?)?;
    Ok(())
}

/// Writes XML with explicit document-output options.
pub fn write_with_options(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &XmlWriteOptions,
) -> Result<(), XmlFormatError> {
    let text = to_string_with_options(schema, instance, options)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Controls document-level details when rendering an XML instance in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlWriteOptions {
    pub scalar_output: Option<ir::XmlScalarOutputPolicy>,
    pub declaration: bool,
    pub indent: bool,
    pub default_namespace: Option<String>,
    pub schema_hints: Option<ir::XmlSchemaHints>,
}

impl Default for XmlWriteOptions {
    fn default() -> Self {
        Self {
            scalar_output: None,
            declaration: true,
            indent: true,
            default_namespace: None,
            schema_hints: None,
        }
    }
}

/// Renders an [`Instance`] tree shaped by `schema` as XML text -- the
/// in-memory form of [`write`].
pub fn to_string(schema: &SchemaNode, instance: &Instance) -> Result<String, XmlFormatError> {
    to_string_with_options(schema, instance, &XmlWriteOptions::default())
}

/// Renders XML with explicit declaration and root default-namespace policy.
/// The namespace is declared only on the document element; ordinary child
/// elements inherit it according to XML namespace rules.
/// Rendering errors precede the final scan for forbidden XML 1.0 characters.
/// No successful result contains those characters; bytes are never sanitized.
pub fn to_string_with_options(
    schema: &SchemaNode,
    instance: &Instance,
    options: &XmlWriteOptions,
) -> Result<String, XmlFormatError> {
    let xml = render_with_options(schema, instance, options)?;
    validate_output_characters(&xml)?;
    Ok(xml)
}

pub(super) fn validate_output_characters(xml: &str) -> Result<(), XmlFormatError> {
    for (byte_offset, character) in xml.char_indices() {
        if !matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
        {
            return Err(XmlFormatError::InvalidXmlCharacter {
                codepoint: character as u32,
                byte_offset,
            });
        }
    }
    Ok(())
}

// Rendering remains private: every public String-returning writer finalizes it.
pub(super) fn render_with_options(
    schema: &SchemaNode,
    instance: &Instance,
    options: &XmlWriteOptions,
) -> Result<String, XmlFormatError> {
    if let Some(policy) = &options.scalar_output {
        let preservation = policy
            .validate_schema(schema)
            .map_err(XmlFormatError::InvalidScalarOutput)?;
        // Schema hints only annotate the projected document's root.
        if options
            .default_namespace
            .as_ref()
            .is_some_and(|namespace| !namespace.is_empty())
        {
            return Err(XmlFormatError::InvalidScalarOutput(
                ir::XmlScalarOutputError::UnsupportedSchema,
            ));
        }
        let (projected_schema, projected_instance) =
            scalar_output::project(schema, instance, preservation)?;
        let mut strict = options.clone();
        strict.scalar_output = None;
        return render_with_options(&projected_schema, &projected_instance, &strict);
    }
    validate_schema(schema)?;
    if let Some(hints) = &options.schema_hints {
        hints
            .validate()
            .map_err(XmlFormatError::InvalidSchemaHints)?;
    }
    let cursor = Cursor::new(Vec::new());
    let mut writer = if options.indent {
        Writer::new_with_indent(cursor, b' ', 2)
    } else {
        Writer::new(cursor)
    };
    if options.declaration {
        writer.write_event(Event::Decl(quick_xml::events::BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            None,
        )))?;
    }
    write_node(
        &mut writer,
        schema,
        schema,
        instance,
        true,
        NodeWriteContext {
            recursion_depth: 0,
            inherited_namespace: None,
            legacy_root_namespace: options.default_namespace.as_deref(),
            schema_hints: options.schema_hints.as_ref(),
        },
    )?;
    let bytes = writer.into_inner().into_inner();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[derive(Clone, Copy)]
pub(crate) struct NodeWriteContext<'a> {
    pub(crate) recursion_depth: usize,
    pub(crate) inherited_namespace: Option<&'a str>,
    pub(crate) legacy_root_namespace: Option<&'a str>,
    pub(crate) schema_hints: Option<&'a ir::XmlSchemaHints>,
}

pub(super) fn write_node<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    instance: &Instance,
    is_root: bool,
    context: NodeWriteContext<'_>,
) -> Result<(), XmlFormatError> {
    let NodeWriteContext {
        recursion_depth,
        inherited_namespace,
        legacy_root_namespace,
        schema_hints,
    } = context;
    let resolved;
    let schema = if let Some(anchor) = &schema.recursive_ref {
        if recursion_depth >= MAX_XML_RECURSION_DEPTH {
            return Err(XmlFormatError::RecursionLimit {
                limit: MAX_XML_RECURSION_DEPTH,
            });
        }
        resolved = resolve_recursive_schema(schema, root_schema, anchor)?;
        &resolved
    } else {
        schema
    };
    if schema.name == XML_ELEMENTS_FIELD && !is_root {
        if !schema.repeating {
            write_generic_element(
                writer,
                schema,
                root_schema,
                instance,
                recursion_depth,
                inherited_namespace,
                GenericSiblings::default(),
            )?;
            return Ok(());
        }
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            other => return Err(shape_error(schema, "generic XML elements", other)),
        };
        for item in items {
            write_generic_element(
                writer,
                schema,
                root_schema,
                item,
                recursion_depth,
                inherited_namespace,
                GenericSiblings::default(),
            )?;
        }
        return Ok(());
    }
    if let Instance::MappedSequence(items) = instance {
        if is_root || schema.repeating || !matches!(&schema.kind, SchemaKind::Group { .. }) {
            let expected = if is_root {
                "one document root"
            } else {
                "one non-repeating element group"
            };
            return Err(shape_error(schema, expected, instance));
        }
        for item in items {
            write_single_node(
                writer,
                schema,
                root_schema,
                item,
                recursion_depth,
                inherited_namespace,
                (None, None),
            )?;
        }
        return Ok(());
    }
    if schema.repeating && !is_root {
        let Instance::Repeated(items) = instance else {
            return Err(shape_error(schema, "repeating elements", instance));
        };
        for item in items {
            write_single_node(
                writer,
                schema,
                root_schema,
                item,
                recursion_depth,
                inherited_namespace,
                (None, None),
            )?;
        }
        return Ok(());
    }
    if matches!(instance, Instance::Repeated(_)) {
        let expected = if is_root {
            "one document root"
        } else {
            "one element"
        };
        return Err(shape_error(schema, expected, instance));
    }
    write_single_node(
        writer,
        schema,
        root_schema,
        instance,
        recursion_depth,
        inherited_namespace,
        (
            is_root.then_some(legacy_root_namespace).flatten(),
            is_root.then_some(schema_hints).flatten(),
        ),
    )
}

pub(super) fn write_single_node<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    instance: &Instance,
    recursion_depth: usize,
    inherited_namespace: Option<&str>,
    document_options: (Option<&str>, Option<&ir::XmlSchemaHints>),
) -> Result<(), XmlFormatError> {
    let (legacy_namespace, schema_hints) = document_options;
    if !schema.xml_name_alternatives.is_empty() {
        return Err(XmlFormatError::AmbiguousXmlName {
            name: schema.name.clone(),
        });
    }
    let default_namespace = match &schema.xml_namespace {
        Some(XmlNamespace::Qualified(namespace)) => Some(namespace.as_str()),
        Some(XmlNamespace::Unqualified) => None,
        None => legacy_namespace.or(inherited_namespace),
    };
    let namespace_changed = default_namespace != inherited_namespace;
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
            if value.is_xml_nil() {
                if !crate::declaration::declaration(schema)?.nil {
                    return Err(XmlFormatError::UnexpectedXmlNil {
                        name: schema.name.clone(),
                    });
                }
                let mut start = BytesStart::new(schema.name.clone());
                push_element_namespace(&mut start, default_namespace, namespace_changed);
                start.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
                start.push_attribute(("xsi:nil", "true"));
                schema_hints::push_root_hints(&mut start, schema_hints)?;
                writer.write_event(Event::Empty(start))?;
                return Ok(());
            }
            let mut start = BytesStart::new(schema.name.clone());
            push_element_namespace(&mut start, default_namespace, namespace_changed);
            schema_hints::push_root_hints(&mut start, schema_hints)?;
            writer.write_event(Event::Start(start))?;
            let text = format_schema_scalar(schema, *ty, value)?;
            writer.write_event(Event::Text(xml_text(&text)))?;
            writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
            Ok(())
        }
        (SchemaKind::ScalarUnion { .. }, _) => Err(XmlFormatError::UnsupportedScalarUnion {
            name: schema.name.clone(),
        }),
        (
            SchemaKind::Group {
                children,
                alternatives,
                ..
            },
            Instance::Group(fields),
        ) => {
            validate_group_fields(schema, children, alternatives, fields)?;
            let selected = select_group_alternative(schema, alternatives, fields)?;
            let (element_namespace, element_name) =
                if schema.xml_alternative_kind == XmlAlternativeKind::SubstitutionGroup {
                    let Some(alternative) = selected else {
                        return Err(XmlFormatError::NoMatchingAlternative {
                            name: schema.name.clone(),
                        });
                    };
                    split_expanded_name(&alternative.name)
                } else {
                    (default_namespace, schema.name.as_str())
                };
            let namespace_changed = element_namespace != inherited_namespace;
            let mut start = BytesStart::new(element_name);
            push_element_namespace(&mut start, element_namespace, namespace_changed);
            if schema.xml_alternative_kind == XmlAlternativeKind::XsiType
                && let Some(alternative) = selected
                && (schema.xml_default_type.as_deref() != Some(alternative.name.as_str())
                    || fields.iter().any(|(name, _)| name == XML_TYPE_FIELD))
            {
                start.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
                let (namespace, local) = split_expanded_name(&alternative.name);
                let type_name = match namespace {
                    Some(namespace) => {
                        start.push_attribute(("xmlns:ft", namespace));
                        format!("ft:{local}")
                    }
                    None => local.to_string(),
                };
                start.push_attribute(("xsi:type", type_name.as_str()));
            }
            let mut attribute_namespaces = Vec::<&str>::new();
            if let Some(attribute_schema) = children
                .iter()
                .find(|child| child.name == XML_ATTRIBUTES_FIELD)
                && let Some((_, attributes)) = fields
                    .iter()
                    .find(|(field, _)| field == XML_ATTRIBUTES_FIELD)
            {
                push_generic_attributes(&mut start, attribute_schema, attributes, children)?;
            }
            for child_schema in children.iter().filter(|child| child.attribute) {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(name, _)| name == &child_schema.name)
                {
                    let Instance::Scalar(value) = child_instance else {
                        return Err(shape_error(
                            child_schema,
                            "an attribute scalar",
                            child_instance,
                        ));
                    };
                    if !matches!(value, Value::Null | Value::JsonNull(_)) {
                        let SchemaKind::Scalar { ty } = child_schema.kind else {
                            return Err(shape_error(
                                child_schema,
                                "an attribute scalar",
                                child_instance,
                            ));
                        };
                        let text = format_schema_scalar(child_schema, ty, value)?;
                        push_schema_attribute(
                            &mut start,
                            child_schema,
                            &text,
                            &mut attribute_namespaces,
                        );
                    }
                }
            }
            schema_hints::push_root_hints(&mut start, schema_hints)?;
            // The selected type determines whether indentation text is allowed.
            let mut selected_children = children.iter().filter(|child| {
                selected.is_none_or(|alternative| alternative.members.contains(&child.name))
            });
            if (selected_children.clone().any(|child| child.text)
                || selected_children
                    .all(|child| child.attribute || child.name == XML_ATTRIBUTES_FIELD))
                && !group_has_serialized_content(children, fields)
            {
                writer.write_event(Event::Empty(start))?;
                return Ok(());
            }
            writer.write_event(Event::Start(start))?;
            if write_ordered_mixed_content(
                writer,
                schema,
                children,
                root_schema,
                fields,
                recursion_depth,
                element_namespace,
                OrderedContentLayout::for_children(children),
            )? {
                writer.write_event(Event::End(BytesEnd::new(element_name)))?;
                return Ok(());
            }
            for child_schema in children.iter().filter(|child| child.text) {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(name, _)| name == &child_schema.name)
                {
                    let Instance::Scalar(value) = child_instance else {
                        return Err(shape_error(child_schema, "a text scalar", child_instance));
                    };
                    if !matches!(value, Value::Null | Value::JsonNull(_)) {
                        let SchemaKind::Scalar { ty } = child_schema.kind else {
                            return Err(shape_error(child_schema, "a text scalar", child_instance));
                        };
                        let text = format_schema_scalar(child_schema, ty, value)?;
                        writer.write_event(Event::Text(xml_text(&text)))?;
                    }
                }
            }
            match OrderedContentLayout::for_children(children) {
                OrderedContentLayout::Indented => write_typed_element_children(
                    writer,
                    schema,
                    children,
                    root_schema,
                    fields,
                    recursion_depth,
                    element_namespace,
                )?,
                OrderedContentLayout::Verbatim => {
                    // The reader keeps every text node of a mixed parent, so
                    // typed children get no added layout, as in its ordered
                    // content stream.
                    let output: &mut dyn Write = writer.get_mut();
                    write_typed_element_children(
                        &mut Writer::new(output),
                        schema,
                        children,
                        root_schema,
                        fields,
                        recursion_depth,
                        element_namespace,
                    )?;
                    // Keep the indentation layer from breaking the line
                    // before the close tag.
                    writer.write_event(Event::Text(BytesText::new("")))?;
                }
            }
            writer.write_event(Event::End(BytesEnd::new(element_name)))?;
            Ok(())
        }
        (SchemaKind::Scalar { .. }, other) => Err(shape_error(schema, "a scalar", other)),
        (SchemaKind::Group { .. }, other) => Err(shape_error(schema, "an element group", other)),
    }
}

/// Writes the element children of a group from its typed fields.
fn write_typed_element_children<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    children: &[SchemaNode],
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
    inherited_namespace: Option<&str>,
) -> Result<(), XmlFormatError> {
    if !schema.xml_repeating_sequences.is_empty() {
        return write_repeating_sequence_children(
            writer,
            schema,
            children,
            root_schema,
            fields,
            recursion_depth,
            inherited_namespace,
        );
    }
    for child_schema in children
        .iter()
        .filter(|child| !child.attribute && !child.text)
    {
        write_group_child(
            writer,
            child_schema,
            children,
            root_schema,
            fields,
            recursion_depth,
            inherited_namespace,
        )?;
    }
    Ok(())
}

fn write_group_child<W: std::io::Write>(
    writer: &mut Writer<W>,
    child_schema: &SchemaNode,
    siblings: &[SchemaNode],
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
    inherited_namespace: Option<&str>,
) -> Result<(), XmlFormatError> {
    if child_schema.name == XML_ATTRIBUTES_FIELD {
        return Ok(());
    }
    let Some((_, child_instance)) = fields.iter().find(|(name, _)| name == &child_schema.name)
    else {
        return Ok(());
    };
    if !child_schema.repeating
        && matches!(&child_schema.kind, SchemaKind::Scalar { .. })
        && matches!(
            child_instance,
            Instance::Scalar(Value::Null | Value::JsonNull(_))
        )
    {
        return Ok(());
    }
    if child_schema.name == XML_ELEMENTS_FIELD {
        let items = match child_instance {
            Instance::Repeated(items) | Instance::MappedSequence(items)
                if child_schema.repeating =>
            {
                items.as_slice()
            }
            instance if !child_schema.repeating => std::slice::from_ref(instance),
            other => return Err(shape_error(child_schema, "generic XML elements", other)),
        };
        let written = GenericSiblings::written_before_wildcard(siblings, fields);
        for item in items {
            write_generic_element(
                writer,
                child_schema,
                root_schema,
                item,
                recursion_depth,
                inherited_namespace,
                GenericSiblings {
                    elements: siblings,
                    written: &written,
                },
            )?;
        }
        return Ok(());
    }
    write_node(
        writer,
        child_schema,
        root_schema,
        child_instance,
        false,
        NodeWriteContext {
            recursion_depth: recursion_depth + usize::from(child_schema.recursive_ref.is_some()),
            inherited_namespace,
            legacy_root_namespace: None,
            schema_hints: None,
        },
    )
}

fn write_repeating_sequence_children<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    children: &[SchemaNode],
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
    inherited_namespace: Option<&str>,
) -> Result<(), XmlFormatError> {
    for child in children
        .iter()
        .filter(|child| !child.attribute && !child.text)
    {
        let sequence = schema.xml_repeating_sequences.iter().find(|sequence| {
            sequence
                .members
                .first()
                .is_some_and(|member| member.name == child.name)
        });
        if let Some(sequence) = sequence {
            write_constructed_repeating_sequence(
                writer,
                schema,
                children,
                sequence,
                root_schema,
                fields,
                SequenceWriteContext {
                    recursion_depth,
                    inherited_namespace,
                },
            )?;
            continue;
        }
        if schema.xml_repeating_sequences.iter().any(|sequence| {
            sequence
                .members
                .iter()
                .skip(1)
                .any(|member| member.name == child.name)
        }) {
            continue;
        }
        write_group_child(
            writer,
            child,
            children,
            root_schema,
            fields,
            recursion_depth,
            inherited_namespace,
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SequenceWriteContext<'a> {
    recursion_depth: usize,
    inherited_namespace: Option<&'a str>,
}

fn write_constructed_repeating_sequence<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    children: &[SchemaNode],
    sequence: &ir::XmlRepeatingSequence,
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    context: SequenceWriteContext<'_>,
) -> Result<(), XmlFormatError> {
    let SequenceWriteContext {
        recursion_depth,
        inherited_namespace,
    } = context;
    let items_for = |name: &str| -> Result<&[Instance], XmlFormatError> {
        let Some(instance) = fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, instance)| instance)
        else {
            return Ok(&[]);
        };
        instance.as_repeated().ok_or_else(|| XmlFormatError::Shape {
            name: name.to_string(),
            expected: "repeating elements",
            got: instance_kind(instance),
        })
    };
    let anchors = sequence
        .members
        .iter()
        .filter(|member| member.required && !member.repeating)
        .collect::<Vec<_>>();
    let cycles = match anchors.first() {
        Some(anchor) => items_for(&anchor.name)?.len(),
        None => {
            let has_values = sequence.members.iter().try_fold(false, |found, member| {
                items_for(&member.name).map(|items| found || !items.is_empty())
            })?;
            // Without a singular anchor, one cycle is the only observable
            // construction when all populated members are themselves
            // repeating. Multiple indistinguishable cycles serialize to the
            // same element run, so emit the canonical single-cycle form.
            usize::from(has_values)
        }
    };
    for anchor in anchors.iter().skip(1) {
        if items_for(&anchor.name)?.len() != cycles {
            return Err(XmlFormatError::AmbiguousRepeatingSequence {
                group: schema.name.clone(),
                reason: "required members have different occurrence counts".to_string(),
            });
        }
    }
    for member in &sequence.members {
        let count = items_for(&member.name)?.len();
        let unambiguous = if member.repeating {
            if cycles == 1 {
                !member.required || count > 0
            } else if member.required {
                count == cycles
            } else {
                count == 0
            }
        } else if member.required {
            count == cycles
        } else {
            count == 0 || count == cycles
        };
        if !unambiguous {
            return Err(XmlFormatError::AmbiguousRepeatingSequence {
                group: schema.name.clone(),
                reason: format!(
                    "member `{}` has {count} values across {cycles} sequence iterations",
                    member.name
                ),
            });
        }
    }
    for cycle in 0..cycles {
        for member in &sequence.members {
            let child = children
                .iter()
                .find(|child| child.name == member.name)
                .ok_or_else(|| XmlFormatError::AmbiguousRepeatingSequence {
                    group: schema.name.clone(),
                    reason: format!("member `{}` has no schema child", member.name),
                })?;
            let items = items_for(&member.name)?;
            if member.repeating {
                if cycles == 1 {
                    for item in items {
                        write_sequence_item(
                            writer,
                            child,
                            root_schema,
                            item,
                            recursion_depth,
                            inherited_namespace,
                        )?;
                    }
                } else if member.required {
                    write_sequence_item(
                        writer,
                        child,
                        root_schema,
                        &items[cycle],
                        recursion_depth,
                        inherited_namespace,
                    )?;
                }
            } else if let Some(item) = items.get(cycle) {
                write_sequence_item(
                    writer,
                    child,
                    root_schema,
                    item,
                    recursion_depth,
                    inherited_namespace,
                )?;
            }
        }
    }
    Ok(())
}

fn write_sequence_item<W: std::io::Write>(
    writer: &mut Writer<W>,
    child: &SchemaNode,
    root_schema: &SchemaNode,
    item: &Instance,
    recursion_depth: usize,
    inherited_namespace: Option<&str>,
) -> Result<(), XmlFormatError> {
    let child_depth = recursion_depth + usize::from(child.recursive_ref.is_some());
    let resolved;
    let child = if let Some(anchor) = &child.recursive_ref {
        if child_depth >= MAX_XML_RECURSION_DEPTH {
            return Err(XmlFormatError::RecursionLimit {
                limit: MAX_XML_RECURSION_DEPTH,
            });
        }
        resolved = resolve_recursive_schema(child, root_schema, anchor)?;
        &resolved
    } else {
        child
    };
    write_single_node(
        writer,
        child,
        root_schema,
        item,
        child_depth,
        inherited_namespace,
        (None, None),
    )
}

pub(super) fn xml_text(value: &str) -> BytesText<'_> {
    if value.contains('\r') {
        // Literal carriage returns are normalized to line feeds by XML readers.
        BytesText::from_escaped(quick_xml::escape::escape(value).replace('\r', "&#xD;"))
    } else {
        BytesText::new(value)
    }
}

pub(super) fn push_attribute(start: &mut BytesStart<'_>, name: &str, value: &str) {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '"' => escaped.push_str("&quot;"),
            '\t' => escaped.push_str("&#x9;"),
            '\n' => escaped.push_str("&#xA;"),
            '\r' => escaped.push_str("&#xD;"),
            _ => escaped.push(character),
        }
    }
    start.push_attribute((name.as_bytes(), escaped.as_bytes()));
}

pub(crate) fn push_schema_attribute<'a>(
    start: &mut BytesStart<'_>,
    schema: &'a SchemaNode,
    value: &str,
    namespaces: &mut Vec<&'a str>,
) {
    let name = match &schema.xml_namespace {
        Some(XmlNamespace::Qualified(namespace))
            if namespace.as_str() == "http://www.w3.org/XML/1998/namespace" =>
        {
            format!("xml:{}", schema.name)
        }
        Some(XmlNamespace::Qualified(namespace)) => {
            let (prefix_index, new_namespace) = namespaces
                .iter()
                .position(|existing| *existing == namespace.as_str())
                .map_or_else(
                    || {
                        namespaces.push(namespace.as_str());
                        (namespaces.len() - 1, true)
                    },
                    |index| (index, false),
                );
            let prefix = format!("fns{}", prefix_index + 1);
            if new_namespace {
                let declaration = format!("xmlns:{prefix}");
                push_attribute(start, &declaration, namespace.as_str());
            }
            format!("{prefix}:{}", schema.name)
        }
        Some(XmlNamespace::Unqualified) | None => schema.name.clone(),
    };
    push_attribute(start, &name, value);
}

pub(super) fn push_element_namespace(
    start: &mut BytesStart<'_>,
    default_namespace: Option<&str>,
    changed: bool,
) {
    if changed {
        start.push_attribute(("xmlns", default_namespace.unwrap_or_default()));
    }
}

fn format_scalar(name: &str, ty: ScalarType, value: &Value) -> Result<String, XmlFormatError> {
    let incompatible = |got| XmlFormatError::ValueType {
        name: name.to_string(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (_, Value::Null) => Err(incompatible("null")),
        (_, Value::JsonNull(_)) => Err(incompatible("json null")),
        (ScalarType::String, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::String, Value::Float(_)) => Err(incompatible("non-finite float")),
        (ScalarType::String, Value::String(value)) => Ok(value.clone()),
        (ScalarType::Int, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::Int, Value::Float(value)) => integral_i64(*value)
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("non-integral float")),
        (ScalarType::Int, Value::String(value)) => parse_exact_decimal_i64(value)
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("string")),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::Float, Value::Float(_)) => Err(incompatible("non-finite float")),
        (ScalarType::Float, Value::Int(value)) if ir::i64_to_exact_f64(*value).is_some() => {
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
        (ScalarType::Bool, Value::String(value)) => xml_boolean(value)
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("string")),
        (_, other) => Err(incompatible(other.type_name())),
    }
}

fn integral_i64(value: f64) -> Option<i64> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value < -(i64::MIN as f64))
        .then_some(value as i64)
}
