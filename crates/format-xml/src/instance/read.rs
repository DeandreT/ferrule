use std::io::Read;
use std::path::Path;

use ir::{
    Instance, SchemaKind, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_SUBSTITUTION_FIELD,
    XML_TYPE_FIELD, XmlAlternativeKind,
};
use quick_xml::events::Event;

use crate::declaration::validate_schema;

use super::generic::{read_generic_element, read_group_fields};
use super::{
    MAX_ROOT_VIEW_DOCUMENT_BYTES, MAX_XML_NODES, MAX_XML_RECURSION_DEPTH, XmlFormatError,
    element_matches_schema, expand_xml_qname, expanded_node_name, group_fields, has_xml_nil,
    parse_input_schema_scalar, resolve_recursive_schema, schema_xml_name, select_group_alternative,
    validate_alternative_fields, validate_singular_xml_choices,
};

/// Explicit XML input policies. The default preserves ordinary schema reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XmlReadOptions {
    /// Retain declared scalar attributes outside a known explicitly selected
    /// root type's member set. Nested nodes retain the ordinary strict policy.
    pub allow_inactive_root_type_members: bool,
    /// Bounded observed flat String root annotation/nil behavior, source-only.
    pub root_view_policy: bool,
}

/// Reads an XML file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    read_with_options(path, schema, &XmlReadOptions::default())
}

/// Reads one XML file with an explicit root-only input policy.
pub fn read_with_options(
    path: &Path,
    schema: &SchemaNode,
    options: &XmlReadOptions,
) -> Result<Instance, XmlFormatError> {
    let text = if options.root_view_policy {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_ROOT_VIEW_DOCUMENT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_ROOT_VIEW_DOCUMENT_BYTES {
            return Err(XmlFormatError::RootViewDocumentLimit {
                limit: MAX_ROOT_VIEW_DOCUMENT_BYTES,
            });
        }
        String::from_utf8(bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
    } else {
        std::fs::read_to_string(path)?
    };
    from_str_with_options(&text, schema, options)
}

/// Reads XML text into an [`Instance`] tree shaped by `schema` -- the
/// in-memory form of [`read`] (useful where there is no filesystem, e.g.
/// wasm).
pub fn from_str(text: &str, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    from_str_with_options(text, schema, &XmlReadOptions::default())
}

/// Reads XML text with an explicit policy for a closed flat typed root.
pub fn from_str_with_options(
    text: &str,
    schema: &SchemaNode,
    options: &XmlReadOptions,
) -> Result<Instance, XmlFormatError> {
    if options.root_view_policy
        && (!options.allow_inactive_root_type_members
            || !ir::xml_root_view_read_policy_is_supported(schema))
    {
        return Err(XmlFormatError::UnsupportedRootViewReadPolicy {
            name: schema.name.clone(),
        });
    }
    if options.root_view_policy && text.len() > MAX_ROOT_VIEW_DOCUMENT_BYTES {
        return Err(XmlFormatError::RootViewDocumentLimit {
            limit: MAX_ROOT_VIEW_DOCUMENT_BYTES,
        });
    }
    if options.allow_inactive_root_type_members
        && !ir::xml_inactive_root_type_members_are_supported(schema)
    {
        return Err(XmlFormatError::UnsupportedInactiveRootTypeMembers {
            name: schema.name.clone(),
        });
    }
    if options.root_view_policy {
        validate_root_view_encoding(text)?;
    }
    validate_schema(schema)?;
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: MAX_XML_NODES,
            ..roxmltree::ParsingOptions::default()
        },
    )?;
    let root = doc.root_element();
    if !element_matches_schema(&root, schema) {
        return Err(XmlFormatError::UnexpectedRoot {
            expected: schema_xml_name(schema),
            found: expanded_node_name(&root),
        });
    }
    read_node_with_root_policy(
        &root,
        schema,
        schema,
        0,
        options.allow_inactive_root_type_members,
        options.root_view_policy,
    )
}

pub(super) fn read_node(
    el: &roxmltree::Node,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    recursion_depth: usize,
) -> Result<Instance, XmlFormatError> {
    read_node_with_root_policy(el, schema, root_schema, recursion_depth, false, false)
}

fn read_node_with_root_policy(
    el: &roxmltree::Node,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    recursion_depth: usize,
    allow_inactive_root_type_members: bool,
    root_view_policy: bool,
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
    let xml_nil = if root_view_policy {
        validate_root_view_content(el, schema)?;
        false // The proved flat root retains attributes even for observed true nil.
    } else {
        has_xml_nil(el, schema)?
    };
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            if xml_nil {
                return Ok(Instance::Scalar(Value::xml_nil()));
            }
            let text = el.text().unwrap_or("");
            Ok(Instance::Scalar(parse_input_schema_scalar(
                schema, *ty, text,
            )?))
        }
        SchemaKind::ScalarUnion { .. } => Err(XmlFormatError::UnsupportedScalarUnion {
            name: schema.name.clone(),
        }),
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
                !schema.xml_repeating_sequences.is_empty()
                    || !schema.xml_repeating_choices.is_empty()
                    || children
                        .iter()
                        .any(|child| !child.xml_name_alternatives.is_empty()),
                root_schema,
                recursion_depth,
            )?;
            let Instance::Group(fields) = &instance else {
                unreachable!("read_group_fields always returns a group")
            };
            validate_singular_xml_choices(schema, fields)?;
            if alternatives.is_empty() {
                return Ok(instance);
            }
            if root_view_policy && attach_inactive_root_annotation(el, schema, &mut instance)? {
                return Ok(instance);
            }
            let (alternative, annotation) = input_group_alternative(
                el,
                schema,
                alternatives,
                &instance,
                allow_inactive_root_type_members,
            )?;
            let Instance::Group(fields) = &mut instance else {
                unreachable!("read_group_fields always returns a group")
            };
            if schema.xml_alternative_kind == XmlAlternativeKind::XsiType
                && annotation.is_none()
                && schema.xml_default_type.as_deref() == Some(alternative.name.as_str())
            {
                fields
                    .set_xml_type_origin(ir::XmlTypeOrigin::Absent)
                    .map_err(|source| XmlFormatError::InvalidXmlTypeOrigin {
                        name: schema.name.clone(),
                        source,
                    })?;
                return Ok(instance);
            }
            let marker = match schema.xml_alternative_kind {
                XmlAlternativeKind::XsiType => XML_TYPE_FIELD,
                XmlAlternativeKind::SubstitutionGroup => XML_SUBSTITUTION_FIELD,
            };
            fields.push((
                marker.to_string(),
                Instance::Scalar(Value::String(alternative.name.clone())),
            ));
            if schema.xml_alternative_kind == XmlAlternativeKind::XsiType {
                let origin = annotation
                    .as_deref()
                    .map_or(ir::XmlTypeOrigin::Absent, ir::XmlTypeOrigin::Explicit);
                fields.set_xml_type_origin(origin).map_err(|source| {
                    XmlFormatError::InvalidXmlTypeOrigin {
                        name: schema.name.clone(),
                        source,
                    }
                })?;
            }
            Ok(instance)
        }
    }
}

fn validate_root_view_encoding(text: &str) -> Result<(), XmlFormatError> {
    let mut reader = quick_xml::Reader::from_str(text);
    let first = reader
        .read_event()
        .map_err(|_| XmlFormatError::RootViewEncoding)?;
    if let Event::Decl(declaration) = first
        && let Some(encoding) = declaration.encoding()
        && !encoding.is_ok_and(|value| value.eq_ignore_ascii_case(b"UTF-8"))
    {
        return Err(XmlFormatError::RootViewEncoding);
    }
    Ok(())
}

fn validate_root_view_content(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<(), XmlFormatError> {
    const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    if let Some(value) = element.attribute((XSI, "nil")) {
        // Only true/false are in this observed profile; ordinary XML 1/0 stays ordinary.
        if !matches!(value, "true" | "false") {
            return Err(XmlFormatError::InvalidXmlNil {
                name: schema.name.clone(),
                value: value.to_owned(),
            });
        }
    }
    if element.children().any(|child| {
        child.is_element()
            || (child.is_text()
                && child.text().is_some_and(|text| {
                    !text
                        .chars()
                        .all(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
                }))
    }) {
        return Err(XmlFormatError::RootViewContent {
            name: schema.name.clone(),
        });
    }
    Ok(())
}

fn attach_inactive_root_annotation(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    instance: &mut Instance,
) -> Result<bool, XmlFormatError> {
    const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    let Some(literal) = element.attribute((XSI, "type")) else {
        return Ok(false);
    };
    let invalid = || XmlFormatError::InvalidXmlType {
        name: schema.name.clone(),
        value: literal.to_owned(),
    };
    if literal.len() > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES {
        return Err(invalid());
    }
    let core = literal.trim_matches([' ', '\t', '\r', '\n']);
    if core.is_empty() || core.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    match core.split_once(':') {
        Some((prefix, local))
            if ir::primary_root_ncname_is_valid(prefix)
                && ir::primary_root_ncname_is_valid(local) => {}
        Some(_) => return Err(invalid()),
        None if ir::primary_root_ncname_is_valid(core) => {}
        None => return Err(invalid()),
    }
    let (prefix, local) = core
        .split_once(':')
        .map_or((None, core), |(prefix, local)| (Some(prefix), local));
    if let Some(namespace) = element.lookup_namespace_uri(prefix)
        && !namespace.is_empty()
        && namespace
            .len()
            .checked_add(local.len())
            .and_then(|bytes| bytes.checked_add(2))
            .is_none_or(|bytes| bytes > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES)
    {
        return Err(invalid());
    }
    let resolved = expand_xml_qname(element, schema, core)?;
    if !ir::primary_root_type_identity_is_valid(&resolved) {
        return Err(invalid());
    }
    let padded = core.len() != literal.len();
    if !padded
        && schema
            .alternatives()
            .iter()
            .any(|alternative| alternative.name == resolved)
    {
        return Ok(false);
    }
    let origin = if padded {
        ir::XmlTypeOrigin::ExplicitPadded {
            literal,
            resolved_identity: &resolved,
        }
    } else {
        ir::XmlTypeOrigin::Explicit(&resolved)
    };
    let Instance::Group(fields) = instance else {
        unreachable!("flat group reader")
    };
    fields
        .set_xml_type_origin(origin)
        .map_err(|source| XmlFormatError::InvalidXmlTypeOrigin {
            name: schema.name.clone(),
            source,
        })?;
    Ok(true)
}

fn input_group_alternative<'a>(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    alternatives: &'a [ir::GroupAlternative],
    instance: &Instance,
    allow_inactive_root_type_members: bool,
) -> Result<(&'a ir::GroupAlternative, Option<String>), XmlFormatError> {
    const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    let fields = group_fields(instance);
    if schema.xml_alternative_kind == XmlAlternativeKind::SubstitutionGroup {
        let identity = expanded_node_name(element);
        let selected = alternatives
            .iter()
            .find(|alternative| alternative.name == identity)
            .ok_or_else(|| XmlFormatError::UnknownSubstitutionMember {
                name: schema.name.clone(),
                value: identity,
            })?;
        validate_alternative_fields(schema, selected, fields)?;
        return Ok((selected, None));
    }
    let (selected, annotation) = match element.attribute((XSI, "type")) {
        Some(value) => {
            let expanded = expand_xml_qname(element, schema, value)?;
            let selected = alternatives
                .iter()
                .find(|alternative| alternative.name == expanded)
                .ok_or_else(|| XmlFormatError::UnknownXmlType {
                    name: schema.name.clone(),
                    value: expanded.clone(),
                })?;
            (selected, Some(expanded))
        }
        None => (
            select_group_alternative(schema, alternatives, fields)?.ok_or_else(|| {
                XmlFormatError::NoMatchingAlternative {
                    name: schema.name.clone(),
                }
            })?,
            None,
        ),
    };
    if !allow_inactive_root_type_members || annotation.is_none() {
        validate_alternative_fields(schema, selected, fields)?;
    }
    Ok((selected, annotation))
}
