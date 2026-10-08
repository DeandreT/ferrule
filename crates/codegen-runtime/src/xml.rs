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
    let xml = format_xml::to_string_with_options_and_finalizer(
        &schema,
        instance,
        &options,
        |xml, _preview| {
            if xml.len() > MAX_SERIALIZED_XML_BYTES {
                return Err(format_xml::XmlWriteFinalizationError::Policy(error(
                    node,
                    format!(
                        "serialized output is {} bytes; maximum is {MAX_SERIALIZED_XML_BYTES}",
                        xml.len()
                    ),
                )));
            }
            Ok(xml)
        },
    )
    .map_err(|source| match source {
        format_xml::XmlWriteFinalizationError::Format(source) => error(node, source.to_string()),
        format_xml::XmlWriteFinalizationError::Policy(error) => error,
    })?;
    Ok(Value::String(xml))
}

fn error(node: u32, message: String) -> RuntimeError {
    RuntimeError::XmlSerialization { node, message }
}

#[cfg(test)]
mod strict_character_tests {
    use super::*;

    #[test]
    fn legacy_node_serializer_rejects_small_invalid_xml_but_keeps_size_first() {
        let schema = ir::SchemaNode::group(
            "Root",
            vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String)],
        );
        let descriptor = serde_json::to_string(&schema).unwrap();
        let data = |text: String| {
            Instance::Group(vec![("Code".into(), Instance::Scalar(Value::String(text)))].into())
        };
        let error =
            serialize_xml(17, &descriptor, &data("\u{1}".into()), false, false, None).unwrap_err();
        assert!(
            matches!(error, RuntimeError::XmlSerialization { node: 17, message }
            if message == "XML output contains forbidden XML 1.0 character U+0001 at UTF-8 byte 12")
        );
        let framing = format_xml::to_string_with_options(
            &schema,
            &data(String::new()),
            &XmlWriteOptions {
                declaration: false,
                indent: false,
                default_namespace: None,
                schema_hints: None,
            },
        )
        .unwrap()
        .len();
        let mut text = "x".repeat(MAX_SERIALIZED_XML_BYTES + 1 - framing - 1);
        text.push('\u{1}');
        let error = serialize_xml(17, &descriptor, &data(text), false, false, None).unwrap_err();
        assert!(
            matches!(error, RuntimeError::XmlSerialization { node: 17, message }
            if message == "serialized output is 67108865 bytes; maximum is 67108864")
        );
    }
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
        let high = f64::from_bits(0x0031_fa18_2c40_c60e);
        let amount: SchemaNode = serde_json::from_str(
            r#"{"name":"Amount","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.0000000000000001e-307}}},"kind":{"kind":"scalar","ty":"float"}}"#,
        )
        .unwrap();
        let explicit_v2 = |schema: &SchemaNode| {
            let mut payload = serde_json::to_value(schema).unwrap();
            let slot = payload
                .pointer_mut("/kind/children/0/numeric_range/bounds/minimum/value")
                .expect("literal Amount minimum");
            assert_eq!(slot.as_f64().unwrap().to_bits(), 0x0031_fa18_2c40_c60e);
            *slot = serde_json::Value::String(format!(
                "{}0031fa182c40c60e",
                codegen_schema::FLOAT_BITS_MARKER_PREFIX,
            ));
            let descriptor = format!(
                "{}{}",
                codegen_schema::V2_PREFIX,
                serde_json::to_string(&payload).unwrap(),
            );
            let decoded = codegen_schema::decode(&descriptor, usize::MAX).unwrap();
            assert_eq!(
                serde_json::to_string(&decoded).unwrap(),
                serde_json::to_string(schema).unwrap(),
            );
            descriptor
        };
        let mut padding = SchemaNode::scalar("Padding", ScalarType::String);
        padding.fixed = Some(String::new());
        let base = SchemaNode::group("Item", vec![amount, padding]);
        let overhead = explicit_v2(&base).len();
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
            let descriptor = explicit_v2(&schema);
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
                let descriptor = explicit_v2(&schema);
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
