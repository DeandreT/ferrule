use ir::{
    Instance, SchemaKind, SchemaNode, Value, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD,
    XML_LOCAL_NAME_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use super::{
    XmlFormatError, format_scalar, parse_scalar, read_node, shape_error, validate_group_fields,
    write_node,
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
    if !alternatives.is_empty() {
        return Err(XmlFormatError::UnsupportedAlternativeRead {
            group: schema.name.clone(),
        });
    }
    read_group_fields(element, children, true, root_schema, recursion_depth)
}

pub(super) fn read_group_fields(
    element: &roxmltree::Node,
    children: &[SchemaNode],
    generic_element: bool,
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
        } else if child.attribute {
            let value = match element.attribute(child.name.as_str()) {
                Some(text) => {
                    let SchemaKind::Scalar { ty } = child.kind else {
                        return Err(XmlFormatError::MissingElement(child.name.clone()));
                    };
                    parse_scalar(&child.name, ty, text)?
                }
                None => Value::Null,
            };
            fields.push((child.name.clone(), Instance::Scalar(value)));
        } else if child.text {
            let SchemaKind::Scalar { ty } = child.kind else {
                return Err(XmlFormatError::MissingElement(child.name.clone()));
            };
            let value = parse_scalar(&child.name, ty, element.text().unwrap_or(""))?;
            fields.push((child.name.clone(), Instance::Scalar(value)));
        } else if child.name == XML_ELEMENTS_FIELD {
            let items = element
                .children()
                .filter(|node| node.is_element())
                .map(|element| read_generic_element(&element, child, root_schema, recursion_depth))
                .collect::<Result<Vec<_>, _>>()?;
            fields.push((child.name.clone(), Instance::Repeated(items)));
        } else if child.name == XML_ATTRIBUTES_FIELD {
            let items = element
                .attributes()
                .map(|attribute| {
                    Instance::Group(vec![
                        (
                            XML_LOCAL_NAME_FIELD.to_string(),
                            Instance::Scalar(Value::String(attribute.name().to_string())),
                        ),
                        (
                            XML_TEXT_FIELD.to_string(),
                            Instance::Scalar(Value::String(attribute.value().to_string())),
                        ),
                    ])
                })
                .collect();
            fields.push((child.name.clone(), Instance::Repeated(items)));
        } else if child.repeating {
            let items = element
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == child.name)
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
                .find(|node| node.is_element() && node.tag_name().name() == child.name);
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
    Ok(Instance::Group(fields))
}

pub(super) fn write_generic_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    root_schema: &SchemaNode,
    instance: &Instance,
    recursion_depth: usize,
) -> Result<(), XmlFormatError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(shape_error(schema, "a generic XML element group", instance));
    };
    let Instance::Group(fields) = instance else {
        return Err(shape_error(schema, "a generic XML element group", instance));
    };
    validate_group_fields(schema, children, &[], fields)?;
    let name = generic_element_name(fields)?;

    let mut start = BytesStart::new(name);
    if let Some(attribute_schema) = children
        .iter()
        .find(|child| child.name == XML_ATTRIBUTES_FIELD)
        && let Some((_, Instance::Repeated(attributes))) = fields
            .iter()
            .find(|(field, _)| field == XML_ATTRIBUTES_FIELD)
    {
        for attribute in attributes {
            let Instance::Group(attribute_fields) = attribute else {
                return Err(shape_error(
                    attribute_schema,
                    "a generic XML attribute group",
                    attribute,
                ));
            };
            let attribute_name = attribute_fields
                .iter()
                .find(|(field, _)| field == XML_LOCAL_NAME_FIELD)
                .and_then(|(_, value)| value.as_scalar())
                .and_then(|value| match value {
                    Value::String(name) if !name.is_empty() => Some(name.as_str()),
                    _ => None,
                })
                .ok_or(XmlFormatError::MissingGenericElementName)?;
            let attribute_value = attribute_fields
                .iter()
                .find(|(field, _)| field == XML_TEXT_FIELD)
                .and_then(|(_, value)| value.as_scalar())
                .and_then(|value| match value {
                    Value::String(value) => Some(value.as_str()),
                    _ => None,
                })
                .unwrap_or_default();
            start.push_attribute((attribute_name, attribute_value));
        }
    }
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
            if !matches!(value, Value::Null) {
                let SchemaKind::Scalar { ty } = child_schema.kind else {
                    return Err(shape_error(
                        child_schema,
                        "an attribute scalar",
                        child_instance,
                    ));
                };
                let text = format_scalar(&child_schema.name, ty, value)?;
                start.push_attribute((child_schema.name.as_str(), text.as_str()));
            }
        }
    }
    writer.write_event(Event::Start(start))?;
    for child_schema in children.iter().filter(|child| child.text) {
        if let Some((_, child_instance)) =
            fields.iter().find(|(field, _)| field == &child_schema.name)
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
    for child_schema in children.iter().filter(|child| {
        !child.attribute
            && !child.text
            && !matches!(
                child.name.as_str(),
                XML_LOCAL_NAME_FIELD | XML_NODE_NAME_FIELD | XML_ATTRIBUTES_FIELD
            )
    }) {
        if let Some((_, child_instance)) =
            fields.iter().find(|(field, _)| field == &child_schema.name)
        {
            if !child_schema.repeating
                && matches!(&child_schema.kind, SchemaKind::Scalar { .. })
                && matches!(child_instance, Instance::Scalar(Value::Null))
            {
                continue;
            }
            write_node(
                writer,
                child_schema,
                root_schema,
                child_instance,
                false,
                recursion_depth + usize::from(child_schema.recursive_ref.is_some()),
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
