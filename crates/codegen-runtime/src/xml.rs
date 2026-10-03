use format_xml::XmlWriteOptions;

use crate::{Instance, RuntimeError, Value};

pub const MAX_EMBEDDED_XML_SCHEMA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SERIALIZED_XML_BYTES: usize = 64 * 1024 * 1024;

/// Serializes one already-resolved structured source with its generated,
/// immutable schema snapshot.
pub fn serialize_xml(
    node: u32,
    schema_json: &str,
    instance: &Instance,
    declaration: bool,
    indent: bool,
    namespace: Option<&str>,
) -> Result<Value, RuntimeError> {
    if schema_json.len() > MAX_EMBEDDED_XML_SCHEMA_BYTES {
        return Err(error(
            node,
            format!(
                "embedded schema is {} bytes; maximum is {MAX_EMBEDDED_XML_SCHEMA_BYTES}",
                schema_json.len()
            ),
        ));
    }
    let schema = codegen_schema::decode(schema_json, MAX_EMBEDDED_XML_SCHEMA_BYTES)
        .map_err(|source| error(node, format!("embedded schema is invalid: {source}")))?;
    let options = XmlWriteOptions {
        declaration,
        indent,
        default_namespace: namespace.map(str::to_owned),
        schema_hints: None,
    };
    let xml = format_xml::to_string_with_options(&schema, instance, &options)
        .map_err(|source| error(node, source.to_string()))?;
    if xml.len() > MAX_SERIALIZED_XML_BYTES {
        return Err(error(
            node,
            format!(
                "serialized output is {} bytes; maximum is {MAX_SERIALIZED_XML_BYTES}",
                xml.len()
            ),
        ));
    }
    Ok(Value::String(xml))
}

fn error(node: u32, message: String) -> RuntimeError {
    RuntimeError::XmlSerialization { node, message }
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode};

    use super::*;
    use crate::{field, group, scalar, string};

    #[test]
    fn serializes_embedded_schema_with_exact_document_options() {
        let schema = SchemaNode::group(
            "Item",
            vec![
                SchemaNode::scalar("id", ScalarType::String).attribute(),
                SchemaNode::scalar("Name", ScalarType::String),
            ],
        );
        let schema = serde_json::to_string(&schema).expect("schema serializes");
        let instance = group([
            field("id", scalar(string("A&1"))),
            field("Name", scalar(string("Alpha"))),
        ]);

        assert_eq!(
            serialize_xml(7, &schema, &instance, false, false, Some("urn:test")),
            Ok(Value::String(
                "<Item xmlns=\"urn:test\" id=\"A&amp;1\"><Name>Alpha</Name></Item>".into()
            ))
        );
    }

    #[test]
    fn embedded_schema_lossless_xml_uses_its_own_prefixed_byte_cap() {
        let amount: SchemaNode = serde_json::from_str(
            r#"{"name":"Amount","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307}}},"kind":{"kind":"scalar","ty":"float"}}"#,
        )
        .unwrap();
        let high: f64 = serde_json::from_str("1e-307").unwrap();
        let mut padding = SchemaNode::scalar("Padding", ScalarType::String);
        padding.fixed = Some(String::new());
        let base = SchemaNode::group("Item", vec![amount, padding]);
        let overhead = codegen_schema::encode(&base, usize::MAX).unwrap().len();
        let instance = group([field("Amount", scalar(Value::Float(high)))]);
        let options = XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
            schema_hints: None,
        };
        for bytes in [1024 * 1024 + 1, MAX_EMBEDDED_XML_SCHEMA_BYTES] {
            let mut schema = base.clone();
            let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
                unreachable!()
            };
            children[1].fixed = Some("x".repeat(bytes - overhead));
            let descriptor = codegen_schema::encode(&schema, usize::MAX).unwrap();
            assert!(descriptor.starts_with(codegen_schema::V2_PREFIX));
            assert_eq!(descriptor.len(), bytes);
            let expected =
                format_xml::to_string_with_options(&schema, &instance, &options).unwrap();
            assert_eq!(
                serialize_xml(19, &descriptor, &instance, false, false, None),
                Ok(Value::String(expected))
            );
            if bytes == MAX_EMBEDDED_XML_SCHEMA_BYTES {
                let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
                    unreachable!()
                };
                children[1].fixed.as_mut().unwrap().push('x');
                let descriptor = codegen_schema::encode(&schema, usize::MAX).unwrap();
                assert!(
                    matches!(serialize_xml(19, &descriptor, &instance, false, false, None),
                    Err(RuntimeError::XmlSerialization { node: 19, message })
                        if message.contains("8388609 bytes") && message.contains("8388608"))
                );
            }
        }
        assert!(
            matches!(serialize_xml(19, "FERRULE-EMBEDDED-SCHEMA/3\n{}", &instance, false, false, None),
            Err(RuntimeError::XmlSerialization { node: 19, message }) if message.contains("unsupported"))
        );
    }
}
