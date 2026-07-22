use std::path::Path;

use ir::{Instance, SchemaKind, SchemaNode};

use super::{MAX_XML_NODES, XmlFormatError, read_node};

const SOAP_11_ENVELOPE: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const SOAP_12_ENVELOPE: &str = "http://www.w3.org/2003/05/soap-envelope";

/// Reads a WSDL message preview. Direct XML messages remain accepted, while
/// SOAP 1.1 and 1.2 envelopes are projected into the synthetic message and
/// part groups used by imported WSDL components.
pub fn read_wsdl_message(
    path: &Path,
    schema: &SchemaNode,
    operation: &str,
) -> Result<Instance, XmlFormatError> {
    let text = std::fs::read_to_string(path)?;
    from_wsdl_message_str(&text, schema, operation)
}

/// In-memory counterpart to [`read_wsdl_message`].
pub fn from_wsdl_message_str(
    text: &str,
    schema: &SchemaNode,
    operation: &str,
) -> Result<Instance, XmlFormatError> {
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: MAX_XML_NODES,
            ..roxmltree::ParsingOptions::default()
        },
    )?;
    let root = document.root_element();
    if root.tag_name().name() != "Envelope" {
        if root.tag_name().name() != schema.name {
            return Err(XmlFormatError::UnexpectedRoot {
                expected: schema.name.clone(),
                found: root.tag_name().name().to_string(),
            });
        }
        return read_node(&root, schema, schema, 0);
    }

    let namespace = root.tag_name().namespace().unwrap_or_default();
    if !matches!(namespace, SOAP_11_ENVELOPE | SOAP_12_ENVELOPE) {
        return Err(XmlFormatError::InvalidSoapEnvelope(format!(
            "Envelope uses unsupported namespace `{namespace}`"
        )));
    }
    let mut bodies = root.children().filter(|node| {
        node.is_element()
            && node.tag_name().name() == "Body"
            && node.tag_name().namespace() == Some(namespace)
    });
    let body = bodies
        .next()
        .ok_or_else(|| XmlFormatError::InvalidSoapEnvelope("missing Body element".into()))?;
    if bodies.next().is_some() {
        return Err(XmlFormatError::InvalidSoapEnvelope(
            "contains more than one Body element".into(),
        ));
    }

    project_body(&body, schema, operation)
}

fn project_body(
    body: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    operation: &str,
) -> Result<Instance, XmlFormatError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(XmlFormatError::IncompatibleSoapBody {
            schema: schema.name.clone(),
        });
    };
    let message_children = children
        .iter()
        .filter(|child| !child.attribute && !child.text)
        .collect::<Vec<_>>();

    if let [part] = message_children.as_slice()
        && !part.repeating
        && let SchemaKind::Group {
            children: part_children,
            ..
        } = &part.kind
        && let Some(container) = part_container(body, part, part_children, operation)
    {
        let part_instance = read_node(&container, part, schema, 0)?;
        let root = read_node(body, schema, schema, 0)?;
        return replace_group_field(root, children, &part.name, part_instance);
    }

    if let [part] = message_children.as_slice()
        && part.name == "parameters"
        && matches!(part.kind, SchemaKind::Scalar { .. })
        && body.children().any(|node| {
            node.is_element()
                && node.tag_name().name() == expanded_local_name(operation)
                && !node.children().any(|child| child.is_element())
                && node.text().is_none_or(|text| text.trim().is_empty())
        })
    {
        return read_node(body, schema, schema, 0);
    }

    let body_names = body
        .children()
        .filter(|node| node.is_element())
        .map(|node| node.tag_name().name())
        .collect::<Vec<_>>();
    let has_direct_part = message_children
        .iter()
        .any(|child| body_names.contains(&child.name.as_str()));
    if has_direct_part || body_names.is_empty() && message_children.is_empty() {
        return read_node(body, schema, schema, 0);
    }

    Err(XmlFormatError::IncompatibleSoapBody {
        schema: schema.name.clone(),
    })
}

fn part_container<'a, 'input>(
    body: &roxmltree::Node<'a, 'input>,
    part: &SchemaNode,
    part_children: &[SchemaNode],
    operation: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    let body_elements = body.children().filter(|node| node.is_element());
    if let Some(element) = body_elements
        .clone()
        .find(|node| node.tag_name().name() == part.name)
    {
        return Some(element);
    }

    let part_field_names = part_children
        .iter()
        .filter(|child| !child.attribute && !child.text)
        .map(|child| child.name.as_str())
        .collect::<Vec<_>>();
    if body_elements
        .clone()
        .any(|node| part_field_names.contains(&node.tag_name().name()))
    {
        return Some(*body);
    }

    let operation = expanded_local_name(operation);
    let payload = body_elements
        .clone()
        .find(|node| node.tag_name().name() == operation)?;
    let payload_matches = payload
        .children()
        .filter(|node| node.is_element())
        .any(|node| part_field_names.contains(&node.tag_name().name()));
    (payload_matches || part.name == "parameters" || part_field_names.is_empty()).then_some(payload)
}

fn expanded_local_name(value: &str) -> &str {
    let expanded = value.rsplit_once('}').map_or(value, |(_, local)| local);
    expanded
        .rsplit_once(':')
        .map_or(expanded, |(_, local)| local)
}

fn replace_group_field(
    root: Instance,
    schema_children: &[SchemaNode],
    name: &str,
    value: Instance,
) -> Result<Instance, XmlFormatError> {
    let Instance::Group(mut fields) = root else {
        return Err(XmlFormatError::IncompatibleSoapBody {
            schema: name.to_string(),
        });
    };
    fields.retain(|(field, _)| field != name);
    let schema_index = schema_children
        .iter()
        .position(|child| child.name == name)
        .unwrap_or(schema_children.len());
    let insert_at = fields
        .iter()
        .position(|(field, _)| {
            schema_children
                .iter()
                .position(|child| child.name == *field)
                .is_some_and(|index| index > schema_index)
        })
        .unwrap_or(fields.len());
    fields.insert(insert_at, (name.to_string(), value));
    Ok(Instance::Group(fields))
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, Value};

    use super::*;

    fn value_at<'a>(instance: &'a Instance, path: &[&str]) -> Option<&'a Instance> {
        path.iter().try_fold(instance, |current, segment| {
            let Instance::Group(fields) = current else {
                return None;
            };
            fields
                .iter()
                .find_map(|(name, value)| (name == segment).then_some(value))
        })
    }

    #[test]
    fn soap_11_document_element_projects_into_named_part() {
        let schema = SchemaNode::group(
            "LookupSoapIn",
            vec![SchemaNode::group(
                "parameters",
                vec![SchemaNode::scalar("city", ScalarType::String)],
            )],
        );
        let xml = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
            <s:Body><m:Lookup xmlns:m="urn:test"><m:city>Boston</m:city></m:Lookup></s:Body>
        </s:Envelope>"#;

        let instance = from_wsdl_message_str(xml, &schema, "{urn:test}Lookup")
            .unwrap_or_else(|error| panic!("SOAP projection failed: {error}"));
        assert_eq!(
            value_at(&instance, &["parameters", "city"]),
            Some(&Instance::Scalar(Value::String("Boston".into())))
        );
    }

    #[test]
    fn soap_12_wrapped_and_bare_parts_project_into_message_groups() {
        let wrapped = SchemaNode::group(
            "Lookup",
            vec![SchemaNode::group(
                "Lookup",
                vec![SchemaNode::scalar("city", ScalarType::String)],
            )],
        );
        let wrapped_xml = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
            <s:Body><Lookup><city>Oslo</city></Lookup></s:Body>
        </s:Envelope>"#;
        let instance = from_wsdl_message_str(wrapped_xml, &wrapped, "Lookup")
            .unwrap_or_else(|error| panic!("wrapped SOAP projection failed: {error}"));
        assert_eq!(
            value_at(&instance, &["Lookup", "city"]),
            Some(&Instance::Scalar(Value::String("Oslo".into())))
        );

        let bare = SchemaNode::group(
            "LookupRequest",
            vec![SchemaNode::group(
                "LookupRequest",
                vec![SchemaNode::scalar("Query", ScalarType::String)],
            )],
        );
        let bare_xml = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
            <s:Body><Query>Iceland</Query></s:Body>
        </s:Envelope>"#;
        let instance = from_wsdl_message_str(bare_xml, &bare, "Lookup")
            .unwrap_or_else(|error| panic!("bare SOAP projection failed: {error}"));
        assert_eq!(
            value_at(&instance, &["LookupRequest", "Query"]),
            Some(&Instance::Scalar(Value::String("Iceland".into())))
        );
    }

    #[test]
    fn direct_message_xml_remains_supported() {
        let schema = SchemaNode::group(
            "Request",
            vec![SchemaNode::scalar("Query", ScalarType::String)],
        );
        let instance = from_wsdl_message_str(
            "<Request><Query>direct</Query></Request>",
            &schema,
            "Lookup",
        )
        .unwrap_or_else(|error| panic!("direct message read failed: {error}"));
        assert_eq!(
            value_at(&instance, &["Query"]),
            Some(&Instance::Scalar(Value::String("direct".into())))
        );
    }

    #[test]
    fn empty_document_element_accepts_legacy_scalar_parameters_port() {
        let schema = SchemaNode::group(
            "PingSoapIn",
            vec![SchemaNode::scalar("parameters", ScalarType::String)],
        );
        let xml = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
            <s:Body><Ping/></s:Body>
        </s:Envelope>"#;

        let instance = from_wsdl_message_str(xml, &schema, "{urn:test}Ping")
            .unwrap_or_else(|error| panic!("empty SOAP projection failed: {error}"));
        assert_eq!(
            value_at(&instance, &["parameters"]),
            Some(&Instance::Scalar(Value::Null))
        );
    }

    #[test]
    fn envelope_namespace_and_body_are_validated() {
        let schema = SchemaNode::group("Request", Vec::new());
        for xml in [
            "<Envelope xmlns=\"urn:not-soap\"><Body/></Envelope>",
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"/>",
        ] {
            assert!(matches!(
                from_wsdl_message_str(xml, &schema, "Lookup"),
                Err(XmlFormatError::InvalidSoapEnvelope(_))
            ));
        }
    }
}
