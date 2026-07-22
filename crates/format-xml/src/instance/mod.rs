mod generic;

use std::io::Cursor;
use std::path::Path;

use ir::{
    Instance, ScalarType, SchemaKind, SchemaNode, Value, XML_ELEMENTS_FIELD,
    XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
    XML_TYPE_FIELD,
};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use thiserror::Error;

use generic::{read_generic_element, read_group_fields, write_generic_element};

const MAX_XML_RECURSION_DEPTH: usize = 64;
const MAX_XML_NODES: u32 = 1_000_000;

#[derive(Debug, Error)]
pub enum XmlFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml parse error: {0}")]
    Parse(#[from] roxmltree::Error),
    #[error("xml write error: {0}")]
    Write(#[from] quick_xml::Error),
    #[error("expected root element `{expected}`, found `{found}`")]
    UnexpectedRoot { expected: String, found: String },
    #[error("missing required element `{0}`")]
    MissingElement(String),
    #[error("cannot parse `{value}` as {ty:?} for element `{name}`")]
    ScalarParse {
        name: String,
        ty: ScalarType,
        value: String,
    },
    #[error("element `{name}` expected {expected}, got {got}")]
    Shape {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("element `{name}` expected {expected:?}, got {got}")]
    ValueType {
        name: String,
        expected: ScalarType,
        got: &'static str,
    },
    #[error("element `{group}` has unexpected field `{field}`")]
    UnexpectedField { group: String, field: String },
    #[error("element `{group}` has duplicate field `{field}`")]
    DuplicateField { group: String, field: String },
    #[error("element `{group}` has invalid mixed-content metadata: {reason}")]
    InvalidMixedContent { group: String, reason: String },
    #[error("element `{group}` cannot reconstruct its repeating XML sequence: {reason}")]
    AmbiguousRepeatingSequence { group: String, reason: String },
    #[error("element `{name}` matches no declared schema alternative")]
    NoMatchingAlternative { name: String },
    #[error("element `{name}` matches more than one declared schema alternative")]
    AmbiguousAlternative { name: String },
    #[error("element `{name}` uses xsi:nil but its schema is not nillable")]
    UnexpectedXmlNil { name: String },
    #[error("element `{name}` with xsi:nil cannot contain a value")]
    XmlNilWithContent { name: String },
    #[error("nilled group element `{name}` cannot be represented yet")]
    UnsupportedXmlNilGroup { name: String },
    #[error("element `{name}` has invalid xsi:nil value `{value}`")]
    InvalidXmlNil { name: String, value: String },
    #[error(
        "repeating xs:{compositor} with {element_count} non-unique element members cannot preserve occurrence identity"
    )]
    UnsupportedRepeatingParticle {
        compositor: String,
        element_count: usize,
    },
    #[error(
        "repeating xs:sequence contains nested xs:{compositor}, whose occurrence choices cannot be preserved"
    )]
    UnsupportedRepeatingSequenceCompositor { compositor: String },
    #[error(
        "repeating xs:sequence contains a nested repeating sequence with {element_count} members, whose tuple identity cannot be preserved"
    )]
    UnsupportedNestedRepeatingSequence { element_count: usize },
    #[error("XSD expansion exceeds the {limit}-element materialization limit")]
    SchemaMaterializationLimit { limit: usize },
    #[error("schema node `{node}` cannot be both XML text and an attribute")]
    ConflictingSchemaRoles { node: String },
    #[error("schema {kind} `{node}` cannot be serialized as XML {role}")]
    UnsupportedSchemaRole {
        node: String,
        role: &'static str,
        kind: &'static str,
    },
    #[error("schema {role} `{node}` cannot repeat")]
    RepeatingSchemaRole { node: String, role: &'static str },
    #[error("schema group `{group}` has {count} XML text fields; at most one is supported")]
    MultipleTextFields { group: String, count: usize },
    #[error("schema group `{group}` mixes XML text with child elements")]
    MixedContent { group: String },
    #[error("schema group `{group}` has alternatives that XSD export cannot preserve")]
    UnsupportedGroupAlternatives { group: String },
    #[error("schema group `{group}` has invalid XML repeating-sequence metadata")]
    InvalidRepeatingSequenceSchema { group: String },
    #[error(
        "schema group `{group}` has alternatives whose xsi:type identity XML input cannot preserve"
    )]
    UnsupportedAlternativeRead { group: String },
    #[error("element `{name}` has invalid xsi:type QName `{value}`")]
    InvalidXmlType { name: String, value: String },
    #[error("element `{name}` has undeclared xsi:type `{value}`")]
    UnknownXmlType { name: String, value: String },
    #[error("generic XML element item has no non-empty LocalName or NodeName field")]
    MissingGenericElementName,
    #[error("recursive schema reference `{node}` has no unique concrete group anchor `{anchor}`")]
    UnsupportedRecursiveAnchor { node: String, anchor: String },
    #[error("XML recursion exceeds the {limit}-element depth limit")]
    RecursionLimit { limit: usize },
}

/// Reads an XML file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    let text = std::fs::read_to_string(path)?;
    from_str(&text, schema)
}

/// Reads XML text into an [`Instance`] tree shaped by `schema` -- the
/// in-memory form of [`read`] (useful where there is no filesystem, e.g.
/// wasm).
pub fn from_str(text: &str, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: MAX_XML_NODES,
            ..roxmltree::ParsingOptions::default()
        },
    )?;
    let root = doc.root_element();
    if root.tag_name().name() != schema.name {
        return Err(XmlFormatError::UnexpectedRoot {
            expected: schema.name.clone(),
            found: root.tag_name().name().to_string(),
        });
    }
    read_node(&root, schema, schema, 0)
}

fn read_node(
    el: &roxmltree::Node,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    recursion_depth: usize,
) -> Result<Instance, XmlFormatError> {
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
    if schema.name == XML_ELEMENTS_FIELD {
        return read_generic_element(el, schema, root_schema, recursion_depth);
    }
    let xml_nil = has_xml_nil(el, schema)?;
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            if xml_nil {
                return Ok(Instance::Scalar(Value::xml_nil()));
            }
            let text = el.text().unwrap_or("");
            Ok(Instance::Scalar(parse_scalar(&schema.name, *ty, text)?))
        }
        SchemaKind::Group {
            children,
            alternatives,
            ..
        } => {
            if xml_nil {
                return Err(XmlFormatError::UnsupportedXmlNilGroup {
                    name: schema.name.clone(),
                });
            }
            let mut instance = read_group_fields(
                el,
                children,
                false,
                !schema.xml_repeating_sequences.is_empty(),
                root_schema,
                recursion_depth,
            )?;
            if alternatives.is_empty() {
                return Ok(instance);
            }
            let alternative = input_group_alternative(el, schema, alternatives, &instance)?;
            let Instance::Group(fields) = &mut instance else {
                unreachable!("read_group_fields always returns a group")
            };
            fields.push((
                XML_TYPE_FIELD.to_string(),
                Instance::Scalar(Value::String(alternative.name.clone())),
            ));
            Ok(instance)
        }
    }
}

fn input_group_alternative<'a>(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    alternatives: &'a [ir::GroupAlternative],
    instance: &Instance,
) -> Result<&'a ir::GroupAlternative, XmlFormatError> {
    const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    let fields = group_fields(instance);
    let selected = match element.attribute((XSI, "type")) {
        Some(value) => {
            let expanded = expand_xml_qname(element, schema, value)?;
            alternatives
                .iter()
                .find(|alternative| alternative.name == expanded)
                .ok_or_else(|| XmlFormatError::UnknownXmlType {
                    name: schema.name.clone(),
                    value: expanded,
                })?
        }
        None => select_group_alternative(schema, alternatives, fields)?.ok_or_else(|| {
            XmlFormatError::NoMatchingAlternative {
                name: schema.name.clone(),
            }
        })?,
    };
    validate_alternative_fields(schema, selected, fields)?;
    Ok(selected)
}

fn expand_xml_qname(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    value: &str,
) -> Result<String, XmlFormatError> {
    let invalid = || XmlFormatError::InvalidXmlType {
        name: schema.name.clone(),
        value: value.to_string(),
    };
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let (prefix, local) = match value.split_once(':') {
        Some((prefix, local))
            if !prefix.is_empty() && !local.is_empty() && !local.contains(':') =>
        {
            (Some(prefix), local)
        }
        Some(_) => return Err(invalid()),
        None => (None, value),
    };
    Ok(match element.lookup_namespace_uri(prefix) {
        Some(namespace) if !namespace.is_empty() => format!("{{{namespace}}}{local}"),
        Some(_) | None if prefix.is_none() => local.to_string(),
        Some(_) | None => return Err(invalid()),
    })
}

fn group_fields(instance: &Instance) -> &[(String, Instance)] {
    let Instance::Group(fields) = instance else {
        unreachable!("read_group_fields always returns a group")
    };
    fields
}

fn validate_alternative_fields(
    schema: &SchemaNode,
    alternative: &ir::GroupAlternative,
    fields: &[(String, Instance)],
) -> Result<(), XmlFormatError> {
    if fields.iter().any(|(name, instance)| {
        !is_xml_metadata_field(name)
            && instance_has_value(instance)
            && !alternative.members.contains(name)
    }) || alternative.required.iter().any(|required| {
        !fields
            .iter()
            .any(|(name, instance)| name == required && instance_has_value(instance))
    }) {
        return Err(XmlFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        });
    }
    Ok(())
}

fn resolve_recursive_schema(
    occurrence: &SchemaNode,
    root: &SchemaNode,
    anchor: &str,
) -> Result<SchemaNode, XmlFormatError> {
    let mut resolved = find_concrete_group(root, anchor)
        .cloned()
        .ok_or_else(|| XmlFormatError::MissingElement(format!("recursive anchor `{anchor}`")))?;
    resolved.name.clone_from(&occurrence.name);
    resolved.repeating = occurrence.repeating;
    resolved.nillable = occurrence.nillable;
    Ok(resolved)
}

fn find_concrete_group<'a>(schema: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if schema.recursive_ref.is_none()
        && schema.name == anchor
        && matches!(schema.kind, SchemaKind::Group { .. })
    {
        return Some(schema);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_group(child, anchor))
}

fn has_xml_nil(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<bool, XmlFormatError> {
    const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    let Some(value) = element.attribute((XSI, "nil")) else {
        return Ok(false);
    };
    let is_nil = match value {
        "true" | "1" => true,
        "false" | "0" => false,
        _ => {
            return Err(XmlFormatError::InvalidXmlNil {
                name: schema.name.clone(),
                value: value.to_string(),
            });
        }
    };
    if !is_nil {
        return Ok(false);
    }
    if !schema.nillable {
        return Err(XmlFormatError::UnexpectedXmlNil {
            name: schema.name.clone(),
        });
    }
    if element
        .children()
        .any(|child| child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return Err(XmlFormatError::XmlNilWithContent {
            name: schema.name.clone(),
        });
    }
    Ok(true)
}

fn parse_scalar(name: &str, ty: ScalarType, text: &str) -> Result<Value, XmlFormatError> {
    let bad = || XmlFormatError::ScalarParse {
        name: name.to_string(),
        ty,
        value: text.to_string(),
    };
    Ok(match ty {
        ScalarType::String => Value::String(text.to_string()),
        ScalarType::Int => Value::Int(text.trim().parse().map_err(|_| bad())?),
        ScalarType::Float => {
            let value = text.trim().parse::<f64>().map_err(|_| bad())?;
            if !value.is_finite() {
                return Err(bad());
            }
            Value::Float(value)
        }
        ScalarType::Bool => Value::Bool(text.trim().parse().map_err(|_| bad())?),
    })
}

/// Writes an [`Instance`] tree shaped by `schema` to an XML file.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), XmlFormatError> {
    std::fs::write(path, to_string(schema, instance)?)?;
    Ok(())
}

/// Controls document-level details when rendering an XML instance in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlWriteOptions {
    pub declaration: bool,
    pub indent: bool,
    pub default_namespace: Option<String>,
}

impl Default for XmlWriteOptions {
    fn default() -> Self {
        Self {
            declaration: true,
            indent: true,
            default_namespace: None,
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
pub fn to_string_with_options(
    schema: &SchemaNode,
    instance: &Instance,
    options: &XmlWriteOptions,
) -> Result<String, XmlFormatError> {
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
        0,
        options.default_namespace.as_deref(),
    )?;
    let bytes = writer.into_inner().into_inner();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn write_node<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    instance: &Instance,
    is_root: bool,
    recursion_depth: usize,
    root_namespace: Option<&str>,
) -> Result<(), XmlFormatError> {
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
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            other => return Err(shape_error(schema, "generic XML elements", other)),
        };
        for item in items {
            write_generic_element(writer, schema, root_schema, item, recursion_depth)?;
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
            write_single_node(writer, schema, root_schema, item, recursion_depth, None)?;
        }
        return Ok(());
    }
    if schema.repeating && !is_root {
        let Instance::Repeated(items) = instance else {
            return Err(shape_error(schema, "repeating elements", instance));
        };
        for item in items {
            write_single_node(writer, schema, root_schema, item, recursion_depth, None)?;
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
        is_root.then_some(root_namespace).flatten(),
    )
}

fn write_single_node<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    instance: &Instance,
    recursion_depth: usize,
    default_namespace: Option<&str>,
) -> Result<(), XmlFormatError> {
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
            if value.is_xml_nil() {
                if !schema.nillable {
                    return Err(XmlFormatError::UnexpectedXmlNil {
                        name: schema.name.clone(),
                    });
                }
                let mut start = BytesStart::new(schema.name.clone());
                if let Some(namespace) = default_namespace {
                    start.push_attribute(("xmlns", namespace));
                }
                start.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
                start.push_attribute(("xsi:nil", "true"));
                writer.write_event(Event::Empty(start))?;
                return Ok(());
            }
            let mut start = BytesStart::new(schema.name.clone());
            if let Some(namespace) = default_namespace {
                start.push_attribute(("xmlns", namespace));
            }
            writer.write_event(Event::Start(start))?;
            let text = format_scalar(&schema.name, *ty, value)?;
            writer.write_event(Event::Text(BytesText::new(&text)))?;
            writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
            Ok(())
        }
        (
            SchemaKind::Group {
                children,
                alternatives,
                ..
            },
            Instance::Group(fields),
        ) => {
            validate_group_fields(schema, children, alternatives, fields)?;
            let mut start = BytesStart::new(schema.name.clone());
            if let Some(namespace) = default_namespace {
                start.push_attribute(("xmlns", namespace));
            }
            if let Some(alternative) = select_group_alternative(schema, alternatives, fields)? {
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
                    if !matches!(value, Value::Null) {
                        let SchemaKind::Scalar { ty } = child_schema.kind else {
                            return Err(shape_error(
                                child_schema,
                                "an attribute scalar",
                                child_instance,
                            ));
                        };
                        let text = format_scalar(&child_schema.name, ty, value)?;
                        push_attribute(&mut start, &child_schema.name, &text);
                    }
                }
            }
            if children.iter().any(|child| child.text)
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
            )? {
                writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
                return Ok(());
            }
            for child_schema in children.iter().filter(|child| child.text) {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(name, _)| name == &child_schema.name)
                {
                    let Instance::Scalar(value) = child_instance else {
                        return Err(shape_error(child_schema, "a text scalar", child_instance));
                    };
                    if !matches!(value, Value::Null) {
                        let SchemaKind::Scalar { ty } = child_schema.kind else {
                            return Err(shape_error(child_schema, "a text scalar", child_instance));
                        };
                        let text = format_scalar(&child_schema.name, ty, value)?;
                        writer.write_event(Event::Text(BytesText::new(&text)))?;
                    }
                }
            }
            if schema.xml_repeating_sequences.is_empty() {
                for child_schema in children
                    .iter()
                    .filter(|child| !child.attribute && !child.text)
                {
                    write_group_child(writer, child_schema, root_schema, fields, recursion_depth)?;
                }
            } else {
                write_repeating_sequence_children(
                    writer,
                    schema,
                    children,
                    root_schema,
                    fields,
                    recursion_depth,
                )?;
            }
            writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
            Ok(())
        }
        (SchemaKind::Scalar { .. }, other) => Err(shape_error(schema, "a scalar", other)),
        (SchemaKind::Group { .. }, other) => Err(shape_error(schema, "an element group", other)),
    }
}

fn write_group_child<W: std::io::Write>(
    writer: &mut Writer<W>,
    child_schema: &SchemaNode,
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
) -> Result<(), XmlFormatError> {
    let Some((_, child_instance)) = fields.iter().find(|(name, _)| name == &child_schema.name)
    else {
        return Ok(());
    };
    if !child_schema.repeating
        && matches!(&child_schema.kind, SchemaKind::Scalar { .. })
        && matches!(child_instance, Instance::Scalar(Value::Null))
    {
        return Ok(());
    }
    write_node(
        writer,
        child_schema,
        root_schema,
        child_instance,
        false,
        recursion_depth + usize::from(child_schema.recursive_ref.is_some()),
        None,
    )
}

fn write_repeating_sequence_children<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    children: &[SchemaNode],
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
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
                recursion_depth,
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
        write_group_child(writer, child, root_schema, fields, recursion_depth)?;
    }
    Ok(())
}

fn write_constructed_repeating_sequence<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    children: &[SchemaNode],
    sequence: &ir::XmlRepeatingSequence,
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
) -> Result<(), XmlFormatError> {
    let items_for = |name: &str| -> Result<&[Instance], XmlFormatError> {
        let instance = fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, instance)| instance)
            .ok_or_else(|| XmlFormatError::AmbiguousRepeatingSequence {
                group: schema.name.clone(),
                reason: format!("member `{name}` is absent from the instance"),
            })?;
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
            if has_values {
                return Err(XmlFormatError::AmbiguousRepeatingSequence {
                    group: schema.name.clone(),
                    reason: "the sequence has no required singular member to determine iteration boundaries"
                        .to_string(),
                });
            }
            0
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
                        write_sequence_item(writer, child, root_schema, item, recursion_depth)?;
                    }
                } else if member.required {
                    write_sequence_item(
                        writer,
                        child,
                        root_schema,
                        &items[cycle],
                        recursion_depth,
                    )?;
                }
            } else if let Some(item) = items.get(cycle) {
                write_sequence_item(writer, child, root_schema, item, recursion_depth)?;
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
    write_single_node(writer, child, root_schema, item, child_depth, None)
}

fn instance_kind(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(_) => "a scalar",
        Instance::Group(_) => "an element group",
        Instance::Repeated(_) => "repeating elements",
        Instance::MappedSequence(_) => "a mapped element sequence",
        Instance::DocumentSet(_) => "a document set",
    }
}

pub(crate) fn write_ordered_mixed_content<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    children: &[SchemaNode],
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
) -> Result<bool, XmlFormatError> {
    let Some((_, mixed_content)) = fields
        .iter()
        .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
    else {
        return Ok(false);
    };
    let Instance::Repeated(items) = mixed_content else {
        return Err(invalid_mixed_content(
            schema,
            "the ordered content field must be a repeated sequence",
        ));
    };
    for (index, item) in items.iter().enumerate() {
        let Instance::Group(item_fields) = item else {
            return Err(invalid_mixed_content(
                schema,
                format!("item {index} must be a group"),
            ));
        };
        let name = item_fields
            .iter()
            .find(|(name, _)| name == XML_NODE_NAME_FIELD)
            .and_then(|(_, instance)| instance.as_scalar())
            .and_then(|value| match value {
                Value::String(name) => Some(name.as_str()),
                _ => None,
            })
            .ok_or_else(|| {
                invalid_mixed_content(schema, format!("item {index} has no string node name"))
            })?;
        if name.is_empty() {
            let text = item_fields
                .iter()
                .find(|(name, _)| name == XML_TEXT_FIELD)
                .and_then(|(_, instance)| instance.as_scalar())
                .and_then(|value| match value {
                    Value::String(text) => Some(text.as_str()),
                    _ => None,
                })
                .ok_or_else(|| {
                    invalid_mixed_content(
                        schema,
                        format!("text item {index} has no string text value"),
                    )
                })?;
            if !text.is_empty() {
                writer.write_event(Event::Text(BytesText::new(text)))?;
            }
            continue;
        }
        let child_schema = children
            .iter()
            .find(|child| !child.attribute && !child.text && child.name == name)
            .or_else(|| {
                children
                    .iter()
                    .find(|child| child.name == XML_ELEMENTS_FIELD)
            })
            .ok_or_else(|| {
                invalid_mixed_content(
                    schema,
                    format!("item {index} names undeclared child `{name}`"),
                )
            })?;
        let child_instance = item_fields
            .iter()
            .find(|(name, _)| name == XML_MIXED_CONTENT_VALUE_FIELD)
            .map(|(_, instance)| instance)
            .ok_or_else(|| {
                invalid_mixed_content(
                    schema,
                    format!("element item {index} has no typed child value"),
                )
            })?;
        if child_schema.name == XML_ELEMENTS_FIELD {
            write_generic_element(
                writer,
                child_schema,
                root_schema,
                child_instance,
                recursion_depth,
            )?;
        } else {
            let child_depth = recursion_depth + usize::from(child_schema.recursive_ref.is_some());
            let resolved_child;
            let child_schema = if let Some(anchor) = &child_schema.recursive_ref {
                if child_depth >= MAX_XML_RECURSION_DEPTH {
                    return Err(XmlFormatError::RecursionLimit {
                        limit: MAX_XML_RECURSION_DEPTH,
                    });
                }
                resolved_child = resolve_recursive_schema(child_schema, root_schema, anchor)?;
                &resolved_child
            } else {
                child_schema
            };
            write_single_node(
                writer,
                child_schema,
                root_schema,
                child_instance,
                child_depth,
                None,
            )?;
        }
    }
    Ok(true)
}

fn invalid_mixed_content(schema: &SchemaNode, reason: impl Into<String>) -> XmlFormatError {
    XmlFormatError::InvalidMixedContent {
        group: schema.name.clone(),
        reason: reason.into(),
    }
}

fn group_has_serialized_content(children: &[SchemaNode], fields: &[(String, Instance)]) -> bool {
    if fields
        .iter()
        .any(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
    {
        return true;
    }
    children
        .iter()
        .filter(|child| !child.attribute)
        .any(|child| {
            let Some((_, instance)) = fields.iter().find(|(name, _)| name == &child.name) else {
                return false;
            };
            if child.text {
                return match instance {
                    Instance::Scalar(Value::Null) => false,
                    Instance::Scalar(Value::String(value)) if value.is_empty() => false,
                    _ => true,
                };
            }
            match instance {
                Instance::Scalar(Value::Null) => false,
                Instance::Repeated(items)
                    if items.is_empty()
                        && (child.repeating || child.name == XML_ELEMENTS_FIELD) =>
                {
                    false
                }
                Instance::MappedSequence(items)
                    if items.is_empty()
                        && !child.repeating
                        && matches!(child.kind, SchemaKind::Group { .. }) =>
                {
                    false
                }
                _ => true,
            }
        })
}

fn push_attribute(start: &mut BytesStart<'_>, name: &str, value: &str) {
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

fn select_group_alternative<'a>(
    schema: &SchemaNode,
    alternatives: &'a [ir::GroupAlternative],
    fields: &[(String, Instance)],
) -> Result<Option<&'a ir::GroupAlternative>, XmlFormatError> {
    if alternatives.is_empty() {
        return Ok(None);
    }
    if let Some((_, marker)) = fields.iter().find(|(name, _)| name == XML_TYPE_FIELD) {
        let Instance::Scalar(Value::String(type_name)) = marker else {
            return Err(XmlFormatError::InvalidXmlType {
                name: schema.name.clone(),
                value: "non-string internal marker".to_string(),
            });
        };
        let selected = alternatives
            .iter()
            .find(|alternative| alternative.name == *type_name)
            .ok_or_else(|| XmlFormatError::UnknownXmlType {
                name: schema.name.clone(),
                value: type_name.clone(),
            })?;
        validate_alternative_fields(schema, selected, fields)?;
        return Ok(Some(selected));
    }
    let populated: Vec<&str> = fields
        .iter()
        .filter(|(name, instance)| !is_xml_metadata_field(name) && instance_has_value(instance))
        .map(|(name, _)| name.as_str())
        .collect();
    let matches = alternatives
        .iter()
        .filter(|alternative| {
            populated
                .iter()
                .all(|field| alternative.members.iter().any(|member| member == field))
        })
        .collect::<Vec<_>>();
    let Some(member_count) = matches
        .iter()
        .map(|alternative| alternative.members.len())
        .min()
    else {
        return Err(XmlFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        });
    };
    let mut narrowest = matches
        .into_iter()
        .filter(|alternative| alternative.members.len() == member_count);
    let Some(selected) = narrowest.next() else {
        return Err(XmlFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        });
    };
    if narrowest.next().is_some() {
        return Err(XmlFormatError::AmbiguousAlternative {
            name: schema.name.clone(),
        });
    }
    Ok(Some(selected))
}

fn instance_has_value(instance: &Instance) -> bool {
    match instance {
        Instance::Scalar(Value::Null) => false,
        Instance::Scalar(Value::XmlNil(_)) => true,
        Instance::Scalar(_) => true,
        Instance::Group(fields) => fields.iter().any(|(_, value)| instance_has_value(value)),
        Instance::Repeated(items) => items.iter().any(instance_has_value),
        Instance::MappedSequence(items) => items.iter().any(instance_has_value),
        Instance::DocumentSet(documents) => documents
            .iter()
            .any(|document| instance_has_value(document.value())),
    }
}

fn split_expanded_name(name: &str) -> (Option<&str>, &str) {
    name.strip_prefix('{')
        .and_then(|name| name.split_once('}'))
        .map_or((None, name), |(namespace, local)| (Some(namespace), local))
}

fn validate_group_fields(
    schema: &SchemaNode,
    children: &[SchemaNode],
    alternatives: &[ir::GroupAlternative],
    fields: &[(String, Instance)],
) -> Result<(), XmlFormatError> {
    for (index, (name, _)) in fields.iter().enumerate() {
        let xml_type_marker = name == XML_TYPE_FIELD && !alternatives.is_empty();
        let mixed_content_marker = name == XML_MIXED_CONTENT_FIELD
            && (children.iter().any(|child| child.text)
                || !schema.xml_repeating_sequences.is_empty());
        if !xml_type_marker
            && !mixed_content_marker
            && !children.iter().any(|child| child.name == *name)
        {
            return Err(XmlFormatError::UnexpectedField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
        if fields[..index].iter().any(|(previous, _)| previous == name) {
            return Err(XmlFormatError::DuplicateField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
    }
    Ok(())
}

fn is_xml_metadata_field(name: &str) -> bool {
    matches!(name, XML_TYPE_FIELD | XML_MIXED_CONTENT_FIELD)
}

fn shape_error(schema: &SchemaNode, expected: &'static str, instance: &Instance) -> XmlFormatError {
    let got = match instance {
        Instance::Scalar(_) => "a scalar",
        Instance::Group(_) => "an element group",
        Instance::Repeated(_) => "repeating elements",
        Instance::MappedSequence(_) => "a mapped element sequence",
        Instance::DocumentSet(_) => "a document set",
    };
    XmlFormatError::Shape {
        name: schema.name.clone(),
        expected,
        got,
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
        (ScalarType::String, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::String, Value::Float(_)) => Err(incompatible("non-finite float")),
        (ScalarType::String, Value::String(value)) => Ok(value.clone()),
        (ScalarType::Int, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::Int, Value::Float(value)) => integral_i64(*value)
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("non-integral float")),
        (ScalarType::Int, Value::String(value)) => lexical_i64(value)
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("string")),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::Float, Value::Float(_)) => Err(incompatible("non-finite float")),
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

fn integral_i64(value: f64) -> Option<i64> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value < -(i64::MIN as f64))
        .then_some(value as i64)
}

/// Parses an integer-valued decimal lexical without routing through `f64`,
/// which could silently round values above its exact-integer range.
fn lexical_i64(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Ok(value) = value.parse::<i64>() {
        return Some(value);
    }
    let (negative, unsigned) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        Some(_) => (false, value),
        None => return None,
    };
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(index) => {
            let exponent = unsigned.get(index + 1..)?.parse::<i64>().ok()?;
            if unsigned[index + 1..].contains(['e', 'E']) {
                return None;
            }
            (&unsigned[..index], exponent)
        }
        None => (unsigned, 0),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) if !fraction.contains('.') => (whole, fraction),
        Some(_) => return None,
        None => (mantissa, ""),
    };
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }

    let mut digits = format!("{whole}{fraction}");
    let first_nonzero = digits.bytes().position(|byte| byte != b'0');
    let Some(first_nonzero) = first_nonzero else {
        return Some(0);
    };
    digits.drain(..first_nonzero);
    let scale = i128::try_from(fraction.len()).ok()? - i128::from(exponent);
    if scale > 0 {
        let scale = usize::try_from(scale).ok()?;
        let integer_length = digits.len().checked_sub(scale)?;
        if !digits[integer_length..].bytes().all(|byte| byte == b'0') {
            return None;
        }
        digits.truncate(integer_length);
    } else if scale < 0 {
        let zeros = usize::try_from(-scale).ok()?;
        if digits.len().checked_add(zeros)? > 19 {
            return None;
        }
        digits.extend(std::iter::repeat_n('0', zeros));
    }
    if digits.is_empty() {
        return Some(0);
    }
    if negative {
        digits.insert(0, '-');
    }
    digits.parse::<i64>().ok()
}

#[cfg(test)]
mod tests;
