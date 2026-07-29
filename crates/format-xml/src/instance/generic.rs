use std::collections::BTreeSet;

use ir::{
    Instance, SchemaKind, SchemaNode, Value, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD,
    XML_LOCAL_NAME_FIELD, XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD,
    XML_NAMESPACE_URI_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use super::{
    NodeWriteContext, XmlFormatError, attribute_value, element_matches_schema,
    format_schema_scalar, parse_input_schema_scalar, parse_schema_scalar, push_attribute,
    push_element_namespace, push_schema_attribute, read_node, shape_error, validate_group_fields,
    write_node, write_ordered_mixed_content,
};

pub(super) fn read_generic_element(
    element: &roxmltree::Node,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    recursion_depth: usize,
) -> Result<Instance, XmlFormatError> {
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &schema.kind
    else {
        return Err(XmlFormatError::UnsupportedSchemaRole {
            node: schema.name.clone(),
            role: "generic elements",
            kind: "scalar",
        });
    };
    if let Some(constraint) = &schema.xml_wildcard_namespace
        && !constraint.allows(element.tag_name().namespace())
    {
        return Err(wildcard_namespace_mismatch(
            element.tag_name().name(),
            element.tag_name().namespace(),
        ));
    }
    if schema.xml_wildcard_namespace.is_none()
        && schema.xml_namespace == Some(ir::XmlNamespace::Unqualified)
        && element
            .tag_name()
            .namespace()
            .is_some_and(|namespace| !namespace.is_empty())
    {
        return Err(XmlFormatError::UnsupportedXmlWildcard {
            reason: "the supported ##local wildcard profile cannot retain qualified descendant elements",
        });
    }
    if schema.xml_wildcard_namespace.is_none()
        && schema.xml_namespace == Some(ir::XmlNamespace::Unqualified)
        && element.attributes().any(|attribute| {
            attribute
                .namespace()
                .is_some_and(|namespace| !namespace.is_empty())
        })
    {
        return Err(XmlFormatError::UnsupportedXmlWildcard {
            reason: "the supported ##local wildcard profile cannot retain qualified descendant attributes",
        });
    }
    if !alternatives.is_empty() {
        return Err(XmlFormatError::UnsupportedAlternativeRead {
            group: schema.name.clone(),
        });
    }
    read_group_fields(
        element,
        children,
        true,
        !schema.xml_repeating_sequences.is_empty(),
        root_schema,
        recursion_depth,
    )
}

pub(super) fn read_group_fields(
    element: &roxmltree::Node,
    children: &[SchemaNode],
    generic_element: bool,
    retain_element_order: bool,
    root_schema: &SchemaNode,
    recursion_depth: usize,
) -> Result<Instance, XmlFormatError> {
    let mut fields = Vec::with_capacity(children.len());
    for child in children {
        if generic_element
            && matches!(
                child.name.as_str(),
                XML_LOCAL_NAME_FIELD | XML_NODE_NAME_FIELD
            )
        {
            fields.push((
                child.name.clone(),
                Instance::Scalar(Value::String(element.tag_name().name().to_string())),
            ));
        } else if generic_element && child.name == XML_NAMESPACE_URI_FIELD {
            fields.push((
                child.name.clone(),
                Instance::Scalar(
                    element
                        .tag_name()
                        .namespace()
                        .map_or(Value::Null, |namespace| {
                            Value::String(namespace.to_string())
                        }),
                ),
            ));
        } else if generic_element
            && child.name == element.tag_name().name()
            && children.iter().any(|candidate| candidate.text)
        {
            let SchemaKind::Scalar { ty } = child.kind else {
                return Err(XmlFormatError::MissingElement(child.name.clone()));
            };
            let text = element_string_value(element);
            fields.push((
                child.name.clone(),
                Instance::Scalar(parse_input_schema_scalar(child, ty, &text)?),
            ));
        } else if child.attribute {
            let value = match attribute_value(element, child) {
                Some(text) => {
                    let SchemaKind::Scalar { ty } = child.kind else {
                        return Err(XmlFormatError::MissingElement(child.name.clone()));
                    };
                    parse_schema_scalar(child, ty, text)?
                }
                None => match child.default.as_deref() {
                    Some(default) => {
                        let SchemaKind::Scalar { ty } = child.kind else {
                            return Err(XmlFormatError::MissingElement(child.name.clone()));
                        };
                        parse_schema_scalar(child, ty, default)?
                    }
                    None => Value::Null,
                },
            };
            fields.push((child.name.clone(), Instance::Scalar(value)));
        } else if child.text {
            let SchemaKind::Scalar { ty } = child.kind else {
                return Err(XmlFormatError::MissingElement(child.name.clone()));
            };
            let text = direct_text_value(element);
            let value = parse_input_schema_scalar(child, ty, &text)?;
            fields.push((child.name.clone(), Instance::Scalar(value)));
        } else if child.name == XML_ELEMENTS_FIELD {
            let items = element
                .children()
                .filter(|node| node.is_element())
                .map(|element| {
                    read_node(
                        &element,
                        child,
                        root_schema,
                        recursion_depth + usize::from(child.recursive_ref.is_some()),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            fields.push((child.name.clone(), Instance::Repeated(items)));
        } else if child.name == XML_ATTRIBUTES_FIELD {
            let items = element
                .attributes()
                .map(|attribute| {
                    let namespace_aware = child.child(XML_NAMESPACE_URI_FIELD).is_some();
                    if !namespace_aware
                        && child.xml_namespace == Some(ir::XmlNamespace::Unqualified)
                        && attribute
                            .namespace()
                            .is_some_and(|namespace| !namespace.is_empty())
                    {
                        return Err(XmlFormatError::UnsupportedXmlAttributeWildcard {
                            reason: "the supported ##local wildcard profile cannot retain qualified attributes",
                        });
                    }
                    let mut fields = vec![
                        (
                            XML_LOCAL_NAME_FIELD.to_string(),
                            Instance::Scalar(Value::String(attribute.name().to_string())),
                        ),
                    ];
                    if namespace_aware {
                        fields.push((
                            XML_NAMESPACE_URI_FIELD.to_string(),
                            Instance::Scalar(attribute.namespace().map_or(
                                Value::Null,
                                |namespace| Value::String(namespace.to_string()),
                            )),
                        ));
                    }
                    fields.push((
                            XML_TEXT_FIELD.to_string(),
                            Instance::Scalar(Value::String(attribute.value().to_string())),
                        ));
                    Ok(Instance::Group(fields))
                })
                .collect::<Result<Vec<_>, _>>()?;
            fields.push((child.name.clone(), Instance::Repeated(items)));
        } else if child.repeating {
            let items = element
                .children()
                .filter(|node| node.is_element() && element_matches_schema(node, child))
                .map(|element| {
                    read_node(
                        &element,
                        child,
                        root_schema,
                        recursion_depth + usize::from(child.recursive_ref.is_some()),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            fields.push((child.name.clone(), Instance::Repeated(items)));
        } else {
            let matched = element
                .children()
                .find(|node| node.is_element() && element_matches_schema(node, child));
            match matched {
                Some(element) => {
                    fields.push((
                        child.name.clone(),
                        read_node(
                            &element,
                            child,
                            root_schema,
                            recursion_depth + usize::from(child.recursive_ref.is_some()),
                        )?,
                    ));
                }
                None if matches!(child.kind, SchemaKind::Scalar { .. }) => {
                    fields.push((child.name.clone(), Instance::Scalar(Value::Null)));
                }
                // A missing field and a present empty group must remain
                // distinguishable so an XML read/write round trip does not
                // invent absent choice branches or lose `<Empty/>` elements.
                None => {}
            }
        }
    }
    let mixed = children.iter().any(|child| child.text);
    if (mixed || retain_element_order) && element.children().any(|node| node.is_element()) {
        fields.push((
            XML_MIXED_CONTENT_FIELD.to_string(),
            Instance::Repeated(mixed_content_items(element, children, &fields, mixed)),
        ));
    }
    Ok(Instance::Group(fields))
}

fn element_string_value(element: &roxmltree::Node<'_, '_>) -> String {
    element
        .descendants()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .collect()
}

fn direct_text_value(element: &roxmltree::Node<'_, '_>) -> String {
    element
        .children()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .filter(|text| {
            !text.trim().is_empty()
                || !text
                    .chars()
                    .any(|character| matches!(character, '\n' | '\r'))
        })
        .collect()
}

fn mixed_content_items(
    element: &roxmltree::Node<'_, '_>,
    children: &[SchemaNode],
    fields: &[(String, Instance)],
    include_text: bool,
) -> Vec<Instance> {
    let mut occurrence = std::collections::BTreeMap::<&str, usize>::new();
    let mut generic_index = 0usize;
    element
        .children()
        .filter_map(|node| {
            if node.is_text() && !include_text {
                return None;
            }
            let (name, text, value) = if node.is_text() {
                (
                    String::new(),
                    node.text().unwrap_or_default().to_string(),
                    Instance::Scalar(Value::String(node.text().unwrap_or_default().to_string())),
                )
            } else if node.is_element() {
                let name = node.tag_name().name().to_string();
                let text = element_string_value(&node);
                let value = children
                    .iter()
                    .find(|child| child.name == name && element_matches_schema(&node, child))
                    .and_then(|child| {
                        let instance = fields
                            .iter()
                            .find(|(field, _)| field == &child.name)
                            .map(|(_, instance)| instance)?;
                        if child.repeating {
                            let index = occurrence.entry(child.name.as_str()).or_default();
                            let value = instance.as_repeated()?.get(*index)?.clone();
                            *index += 1;
                            Some(value)
                        } else {
                            Some(instance.clone())
                        }
                    })
                    .or_else(|| {
                        let value = fields
                            .iter()
                            .find(|(field, _)| field == XML_ELEMENTS_FIELD)
                            .and_then(|(_, instance)| instance.as_repeated())?
                            .get(generic_index)?
                            .clone();
                        generic_index += 1;
                        Some(value)
                    })
                    .unwrap_or_else(|| Instance::Scalar(Value::String(text.clone())));
                (name, text, value)
            } else {
                return None;
            };
            Some(Instance::Group(vec![
                (
                    XML_NODE_NAME_FIELD.to_string(),
                    Instance::Scalar(Value::String(name)),
                ),
                (
                    XML_TEXT_FIELD.to_string(),
                    Instance::Scalar(Value::String(text)),
                ),
                (XML_MIXED_CONTENT_VALUE_FIELD.to_string(), value),
            ]))
        })
        .collect()
}

pub(super) fn write_generic_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    instance: &Instance,
    recursion_depth: usize,
    inherited_namespace: Option<&str>,
) -> Result<(), XmlFormatError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(shape_error(schema, "a generic XML element group", instance));
    };
    let Instance::Group(fields) = instance else {
        return Err(shape_error(schema, "a generic XML element group", instance));
    };
    validate_group_fields(schema, children, &[], fields)?;
    let name = generic_element_name(fields)?;
    if is_qualified_name(name) {
        let reason = if schema.xml_namespace == Some(ir::XmlNamespace::Unqualified) {
            "the supported ##local wildcard profile requires unqualified runtime element names"
        } else {
            "generic element LocalName values cannot contain namespace syntax"
        };
        return Err(XmlFormatError::UnsupportedXmlWildcard { reason });
    }

    let namespace_aware = schema.xml_wildcard_namespace.is_some()
        || children
            .iter()
            .any(|child| child.name == XML_NAMESPACE_URI_FIELD);
    let runtime_namespace = namespace_aware
        .then(|| generic_namespace_uri(children, fields))
        .transpose()?
        .flatten();
    if let Some(constraint) = &schema.xml_wildcard_namespace
        && !constraint.allows(runtime_namespace)
    {
        return Err(wildcard_namespace_mismatch(name, runtime_namespace));
    }
    let mut start = BytesStart::new(name);
    let element_namespace = if namespace_aware {
        runtime_namespace
    } else {
        match &schema.xml_namespace {
            Some(ir::XmlNamespace::Qualified(namespace)) => Some(namespace.as_str()),
            Some(ir::XmlNamespace::Unqualified) => None,
            None => inherited_namespace,
        }
    };
    push_element_namespace(
        &mut start,
        element_namespace,
        element_namespace != inherited_namespace,
    );
    if let Some(attribute_schema) = children
        .iter()
        .find(|child| child.name == XML_ATTRIBUTES_FIELD)
        && let Some((_, attributes)) = fields
            .iter()
            .find(|(field, _)| field == XML_ATTRIBUTES_FIELD)
    {
        push_generic_attributes(&mut start, attribute_schema, attributes)?;
    }
    let mut attribute_namespaces = Vec::<&str>::new();
    for child_schema in children.iter().filter(|child| child.attribute) {
        if let Some((_, child_instance)) =
            fields.iter().find(|(field, _)| field == &child_schema.name)
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
                push_schema_attribute(&mut start, child_schema, &text, &mut attribute_namespaces);
            }
        }
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
    )? {
        writer.write_event(Event::End(BytesEnd::new(name)))?;
        return Ok(());
    }
    for child_schema in children.iter().filter(|child| child.text) {
        if let Some((_, child_instance)) =
            fields.iter().find(|(field, _)| field == &child_schema.name)
        {
            let Instance::Scalar(value) = child_instance else {
                return Err(shape_error(child_schema, "a text scalar", child_instance));
            };
            if !matches!(value, Value::Null | Value::JsonNull(_)) {
                let SchemaKind::Scalar { ty } = child_schema.kind else {
                    return Err(shape_error(child_schema, "a text scalar", child_instance));
                };
                let text = format_schema_scalar(child_schema, ty, value)?;
                writer.write_event(Event::Text(BytesText::new(&text)))?;
            }
        }
    }
    for child_schema in children.iter().filter(|child| {
        !child.attribute
            && !child.text
            && !matches!(
                child.name.as_str(),
                XML_LOCAL_NAME_FIELD
                    | XML_NODE_NAME_FIELD
                    | XML_NAMESPACE_URI_FIELD
                    | XML_ATTRIBUTES_FIELD
            )
    }) {
        if let Some((_, child_instance)) =
            fields.iter().find(|(field, _)| field == &child_schema.name)
        {
            if !child_schema.repeating
                && matches!(&child_schema.kind, SchemaKind::Scalar { .. })
                && matches!(
                    child_instance,
                    Instance::Scalar(Value::Null | Value::JsonNull(_))
                )
            {
                continue;
            }
            write_node(
                writer,
                child_schema,
                root_schema,
                child_instance,
                false,
                NodeWriteContext {
                    recursion_depth: recursion_depth
                        + usize::from(child_schema.recursive_ref.is_some()),
                    inherited_namespace: element_namespace,
                    legacy_root_namespace: None,
                },
            )?;
        }
    }
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

fn generic_element_name(fields: &[(String, Instance)]) -> Result<&str, XmlFormatError> {
    [XML_LOCAL_NAME_FIELD, XML_NODE_NAME_FIELD]
        .into_iter()
        .find_map(|field| {
            fields
                .iter()
                .find(|(name, _)| name == field)
                .and_then(|(_, value)| value.as_scalar())
                .and_then(|value| match value {
                    Value::String(name) if !name.is_empty() => Some(name.as_str()),
                    _ => None,
                })
        })
        .ok_or(XmlFormatError::MissingGenericElementName)
}

fn generic_namespace_uri<'a>(
    children: &[SchemaNode],
    fields: &'a [(String, Instance)],
) -> Result<Option<&'a str>, XmlFormatError> {
    let Some(namespace_schema) = children
        .iter()
        .find(|child| child.name == XML_NAMESPACE_URI_FIELD)
    else {
        return Ok(None);
    };
    let Some((_, namespace)) = fields
        .iter()
        .find(|(name, _)| name == XML_NAMESPACE_URI_FIELD)
    else {
        return Ok(None);
    };
    match namespace {
        Instance::Scalar(Value::Null) => Ok(None),
        Instance::Scalar(Value::String(namespace)) if namespace.is_empty() => Ok(None),
        Instance::Scalar(Value::String(namespace)) => Ok(Some(namespace.as_str())),
        instance => Err(shape_error(
            namespace_schema,
            "an optional namespace URI string",
            instance,
        )),
    }
}

fn wildcard_namespace_mismatch(name: &str, namespace: Option<&str>) -> XmlFormatError {
    XmlFormatError::XmlWildcardNamespaceMismatch {
        name: name.to_string(),
        namespace: namespace
            .filter(|namespace| !namespace.is_empty())
            .unwrap_or("##local")
            .to_string(),
    }
}

pub(super) fn push_generic_attributes<'a>(
    start: &mut BytesStart<'a>,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), XmlFormatError> {
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &schema.kind
    else {
        return Err(shape_error(
            schema,
            "a generic XML attribute group",
            instance,
        ));
    };
    let Instance::Repeated(attributes) = instance else {
        return Err(shape_error(schema, "generic XML attributes", instance));
    };
    let mut names = BTreeSet::new();
    let namespace_aware = children
        .iter()
        .any(|child| child.name == XML_NAMESPACE_URI_FIELD);
    let mut namespaces = Vec::<&str>::new();
    for attribute in attributes {
        let Instance::Group(attribute_fields) = attribute else {
            return Err(shape_error(
                schema,
                "a generic XML attribute group",
                attribute,
            ));
        };
        validate_group_fields(schema, children, alternatives, attribute_fields)?;
        let attribute_name = attribute_fields
            .iter()
            .find(|(field, _)| field == XML_LOCAL_NAME_FIELD)
            .and_then(|(_, value)| value.as_scalar())
            .and_then(|value| match value {
                Value::String(name) if !name.is_empty() => Some(name.as_str()),
                _ => None,
            })
            .ok_or(XmlFormatError::MissingGenericAttributeName)?;
        if is_qualified_name(attribute_name) {
            let reason = if schema.xml_namespace == Some(ir::XmlNamespace::Unqualified) {
                "the supported ##local wildcard profile requires unqualified runtime attribute names"
            } else {
                "generic attribute LocalName values cannot contain namespace syntax"
            };
            return Err(XmlFormatError::UnsupportedXmlAttributeWildcard { reason });
        }
        let namespace = namespace_aware
            .then(|| generic_namespace_uri(children, attribute_fields))
            .transpose()?
            .flatten();
        if !namespace_aware
            && schema.xml_namespace == Some(ir::XmlNamespace::Unqualified)
            && namespace.is_some()
        {
            return Err(XmlFormatError::UnsupportedXmlAttributeWildcard {
                reason: "the supported ##local wildcard profile requires unqualified runtime attribute names",
            });
        }
        let expanded_name = namespace.map_or_else(
            || attribute_name.to_string(),
            |namespace| format!("{{{namespace}}}{attribute_name}"),
        );
        if !names.insert(expanded_name) {
            return Err(XmlFormatError::DuplicateField {
                group: schema.name.clone(),
                field: attribute_name.to_string(),
            });
        }
        let attribute_value = attribute_fields
            .iter()
            .find(|(field, _)| field == XML_TEXT_FIELD)
            .and_then(|(_, value)| value.as_scalar())
            .and_then(|value| match value {
                Value::String(value) => Some(value.as_str()),
                _ => None,
            })
            .unwrap_or_default();
        let rendered_name = match namespace {
            Some("http://www.w3.org/XML/1998/namespace") => {
                format!("xml:{attribute_name}")
            }
            Some(namespace) => {
                let (index, new_namespace) = namespaces
                    .iter()
                    .position(|candidate| *candidate == namespace)
                    .map_or_else(
                        || {
                            namespaces.push(namespace);
                            (namespaces.len(), true)
                        },
                        |index| (index + 1, false),
                    );
                let prefix = format!("fga{index}");
                if new_namespace {
                    push_attribute(start, &format!("xmlns:{prefix}"), namespace);
                }
                format!("{prefix}:{attribute_name}")
            }
            None => attribute_name.to_string(),
        };
        push_attribute(start, &rendered_name, attribute_value);
    }
    Ok(())
}

fn is_qualified_name(name: &str) -> bool {
    name.contains(':') || name.starts_with('{')
}
