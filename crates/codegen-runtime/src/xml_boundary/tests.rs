#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn descriptor() -> String {
        let schema: ir::SchemaNode = serde_json::from_str(r#"{
          "name":"Envelope","xml_namespace":{"kind":"unqualified"},"repeating":false,
          "xml_type_alternatives":true,"xml_default_type":"Base",
          "kind":{"kind":"group","children":[
            {"name":"Token","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}},
            {"name":"Supplement","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}
          ],"alternatives":[{"name":"Base","members":["Token"]},{"name":"Extended","members":["Token","Supplement"]}]}
        }"#).unwrap();
        codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap()
    }

    #[test]
    fn document_namespace_metadata_cannot_declare_reserved_bindings_but_xml_attributes_work() {
        let ordinary = ir::Instance::Group(
            vec![(
                "Code".into(),
                ir::Instance::Scalar(ir::Value::String("ordinary".into())),
            )]
            .into(),
        );
        let mut schemas = Vec::new();
        let mut xmlns = ir::SchemaNode::group(
            "Root",
            vec![ir::SchemaNode::scalar("xmlns", ir::ScalarType::String).attribute()],
        );
        xmlns.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new("urn:root").unwrap(),
        ));
        schemas.push(xmlns);
        for (uri, attribute) in [
            ("http://www.w3.org/2000/xmlns/", true),
            ("http://www.w3.org/XML/1998/namespace", false),
        ] {
            let mut child = ir::SchemaNode::scalar("Code", ir::ScalarType::String);
            child.attribute = attribute;
            child.xml_namespace = Some(ir::XmlNamespace::Qualified(
                ir::XmlNamespaceUri::new(uri).unwrap(),
            ));
            schemas.push(ir::SchemaNode::group("Root", vec![child]));
        }
        for uri in [
            "http://www.w3.org/XML/1998/namespace",
            "http://www.w3.org/2000/xmlns/",
        ] {
            let mut typed: ir::SchemaNode = serde_json::from_str(&descriptor()).unwrap();
            if let ir::SchemaKind::Group { alternatives, .. } = &mut typed.kind {
                alternatives[1].name = format!("{{{uri}}}Extended");
            }
            schemas.push(typed);
        }
        for schema in schemas {
            let error = serialize_xml_document(
                &serde_json::to_string(&schema).unwrap(),
                &ordinary,
                false,
                false,
                None,
                None,
            )
            .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
            assert_eq!(
                error.detail,
                "unsupported XML namespace declaration metadata in document schema"
            );
        }
        let mut field = ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute();
        field.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new("http://www.w3.org/XML/1998/namespace").unwrap(),
        ));
        let schema = ir::SchemaNode::group("Root", vec![field]);
        assert_eq!(
            serialize_xml_document(
                &serde_json::to_string(&schema).unwrap(),
                &ordinary,
                false,
                false,
                None,
                None
            )
            .unwrap(),
            "<Root xml:Code=\"ordinary\"/>"
        );
    }

    #[test]
    fn document_schema_names_refuse_malformed_children_and_preserve_qualified_unicode_text() {
        let instance = ir::Instance::Group(
            vec![(
                "Code".into(),
                ir::Instance::Scalar(ir::Value::String("ordinary".into())),
            )]
            .into(),
        );
        for (root_name, child_name, attribute) in [
            ("bad root", "Code", true),
            ("Root", "bad attr", true),
            ("Root", "bad child", false),
            ("p:Root", "Code", true),
            ("Root", "p:Code", true),
        ] {
            let schema = ir::SchemaNode::group(
                root_name,
                vec![{
                    let mut field = ir::SchemaNode::scalar(child_name, ir::ScalarType::String);
                    field.attribute = attribute;
                    field
                }],
            );
            let descriptor = serde_json::to_string(&schema).unwrap();
            let error = serialize_xml_document(&descriptor, &instance, false, false, None, None)
                .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
            assert_eq!(
                error.detail,
                "XML document schema requires local NCNames and canonical type identities"
            );
        }
        for root_name in ["根", "𐀀"] {
            let mut schema = ir::SchemaNode::group(
                root_name,
                vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
            );
            schema.xml_namespace = Some(ir::XmlNamespace::Qualified(
                ir::XmlNamespaceUri::new("urn:root").unwrap(),
            ));
            let xml = serialize_xml_document(
                &serde_json::to_string(&schema).unwrap(),
                &instance,
                false,
                false,
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                xml,
                format!("<{root_name} xmlns=\"urn:root\" Code=\"ordinary\"/>")
            );
        }
        let supplementary = ir::SchemaNode::group(
            "𐀀",
            vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
        );
        let error = serialize_xml_document(
            &serde_json::to_string(&supplementary).unwrap(),
            &instance,
            false,
            false,
            None,
            Some("{}"),
        )
        .unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
        assert!(error.detail.contains("supplementary XML names"));
        let text_schema = ir::SchemaNode::group(
            "Root",
            vec![{
                let mut text = ir::SchemaNode::scalar(ir::XML_TEXT_FIELD, ir::ScalarType::String);
                text.text = true;
                text
            }],
        );
        let text_instance = ir::Instance::Group(
            vec![(
                ir::XML_TEXT_FIELD.into(),
                ir::Instance::Scalar(ir::Value::String("ordinary".into())),
            )]
            .into(),
        );
        assert_eq!(
            serialize_xml_document(
                &serde_json::to_string(&text_schema).unwrap(),
                &text_instance,
                false,
                false,
                None,
                None
            )
            .unwrap(),
            "<Root>ordinary</Root>"
        );
        let mut typed_schema: ir::SchemaNode = serde_json::from_str(&descriptor()).unwrap();
        if let ir::SchemaKind::Group { alternatives, .. } = &mut typed_schema.kind {
            alternatives[1].name = "{urn:type}Extended".into();
        }
        let typed_instance = ir::Instance::Group(
            vec![
                (
                    ir::XML_TYPE_FIELD.into(),
                    ir::Instance::Scalar(ir::Value::String("{urn:type}Extended".into())),
                ),
                (
                    "Token".into(),
                    ir::Instance::Scalar(ir::Value::String("value".into())),
                ),
                (
                    "Supplement".into(),
                    ir::Instance::Scalar(ir::Value::String("extra".into())),
                ),
            ]
            .into(),
        );
        let xml = serialize_xml_document(
            &serde_json::to_string(&typed_schema).unwrap(),
            &typed_instance,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("xmlns:ft=\"urn:type\""));
        assert!(xml.contains("xsi:type=\"ft:Extended\""));
        assert!(xml.contains("Token=\"value\" Supplement=\"extra\""));
        let legacy = ir::SchemaNode::group(
            "bad root",
            vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
        );
        assert_eq!(
            crate::serialize_xml(
                0,
                &serde_json::to_string(&legacy).unwrap(),
                &instance,
                false,
                false,
                None
            )
            .unwrap(),
            ir::Value::String("<bad root Code=\"ordinary\"/>".into())
        );
    }

    #[test]
    fn actual_xml_input_preserves_origin_and_defers_required_fields() {
        let schema = descriptor();
        for (annotation, matches) in [
            ("", false),
            (" xsi:type=\"Extended\"", true),
            (" xsi:type=\"Other\"", false),
            (" xsi:type=\" Extended \"", false),
        ] {
            let xml = format!(
                "<Envelope xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"{annotation} Token=\"\"/>"
            );
            let input = parse_xml(&schema, &xml, true, true).unwrap();
            assert_eq!(
                ir::primary_root_xml_type_equals(Some(&input), "Extended"),
                Ok(matches)
            );
            assert_eq!(
                ir::primary_root_scalar_with_requirement(Some(&input), &["Token"], true),
                Ok(crate::Value::String(String::new()))
            );
            assert!(matches!(
                ir::primary_root_scalar_with_requirement(Some(&input), &["Supplement"], true),
                Err(ir::PrimaryRootError::MissingRequiredField { .. })
            ));
            assert_eq!(
                parse_xml_bytes(&schema, xml.as_bytes(), true, true).unwrap(),
                input
            );
        }
    }

    #[test]
    fn document_and_utf8_limits_precede_schema_and_mapping_work() {
        let error = parse_xml_bytes("bad", &[0xff], true, true).unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Utf8);
        let oversized = vec![0xff; MAX_XML_DOCUMENT_BYTES + 1];
        let error = parse_xml_bytes("bad", &oversized, true, true).unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::DocumentLimit);
        assert_eq!(error.bytes, Some(MAX_XML_DOCUMENT_BYTES + 1));
        assert_eq!(error.limit, Some(MAX_XML_DOCUMENT_BYTES));
        let error = parse_xml(
            &" ".repeat(crate::MAX_EMBEDDED_XML_SCHEMA_BYTES + 1),
            "<Envelope/>",
            true,
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
    }

    #[test]
    fn numeric_nil_is_typed_input_error_and_mapping_error_retains_original_payload() {
        let error = parse_xml(
            &descriptor(),
            r#"<Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="1"/>"#,
            true,
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Input);
        assert!(
            matches!(error.source().and_then(|source|source.downcast_ref::<format_xml::XmlFormatError>()),
            Some(format_xml::XmlFormatError::InvalidXmlNil { value, .. }) if value=="1")
        );
        let original = RuntimeError::PrimaryRoot {
            node: 9,
            source: ir::PrimaryRootError::MissingRequiredField {
                path: vec!["Supplement".into()],
            },
        };
        let error = XmlBoundaryError::from(original);
        assert_eq!(error.kind, XmlBoundaryErrorKind::Mapping);
        assert!(
            matches!(error.source().and_then(|source|source.downcast_ref::<RuntimeError>()),
            Some(RuntimeError::PrimaryRoot { node:9, source:ir::PrimaryRootError::MissingRequiredField { path } }) if path==&["Supplement".to_string()])
        );
    }

    #[test]
    fn xml_output_policy_escapes_literal_hints_and_preserves_declared_fields() {
        let schema = descriptor();
        let instance = parse_xml(&schema, r#"<Envelope Token="A&amp;B"/>"#, true, true).unwrap();
        let hints = r#"{"no_namespace_location":"schema&lt;\".xsd"}"#;
        let output =
            serialize_xml_document(&schema, &instance, false, false, None, Some(hints)).unwrap();
        assert!(!output.starts_with("<?xml"));
        assert!(output.contains("Token=\"A&amp;B\""));
        assert!(output.contains("xsi:noNamespaceSchemaLocation=\"schema&amp;lt;&quot;.xsd\""));
        assert_eq!(
            parse_xml(&schema, &output, true, true)
                .unwrap()
                .field("Token"),
            instance.field("Token")
        );
    }
}

#[cfg(test)]
#[test]
fn invalid_default_namespace_xml_character_is_typed_output_refusal() {
    let target = ir::SchemaNode::group(
        "Target",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
    );
    let descriptor = codegen_schema::encode(&target, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let instance = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("v".into())),
        )]
        .into(),
    );
    let result = serialize_xml_document(&descriptor, &instance, false, false, Some("\0"), None);
    assert!(
        matches!(
            result,
            Err(XmlBoundaryError {
                kind: XmlBoundaryErrorKind::Output,
                ..
            })
        ),
        "must refuse forbidden XML namespace character, actual: {result:?}"
    );
}

#[cfg(test)]
#[test]
fn reserved_default_namespace_is_typed_output_refusal_without_or_with_hints() {
    let target = ir::SchemaNode::group("Target", vec![]);
    let descriptor = codegen_schema::encode(&target, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let instance = Instance::Group(vec![].into());
    for namespace in [
        "http://www.w3.org/XML/1998/namespace",
        "http://www.w3.org/2000/xmlns/",
    ] {
        for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
            let error = serialize_xml_document(
                &descriptor,
                &instance,
                false,
                false,
                Some(namespace),
                hints,
            )
            .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Output);
            assert_eq!(
                error.detail,
                "reserved XML namespace cannot be the default namespace"
            );
        }
    }
}

#[cfg(test)]
#[test]
fn new_document_namespace_preserves_value_without_changing_legacy_writer() {
    use quick_xml::{Reader, XmlVersion, events::Event};
    let schema = ir::SchemaNode::group(
        "Root",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
    );
    let descriptor = codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let value = "ordinary xmlns=\"\t\" data\r\n";
    let instance = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String(value.into())),
        )]
        .into(),
    );
    for uri in ["\t", "\n", "\r", "urn:literal:\t\n\r<&\"😀"] {
        for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
            let output =
                serialize_xml_document(&descriptor, &instance, false, false, Some(uri), hints)
                    .unwrap();
            let mut reader = Reader::from_str(&output);
            let start = match reader.read_event().unwrap() {
                Event::Empty(start) => start,
                actual => panic!("unexpected root event: {actual:?}"),
            };
            let attributes = start
                .attributes()
                .map(|attribute| {
                    let attribute = attribute.unwrap();
                    (
                        String::from_utf8(attribute.key.as_ref().to_vec()).unwrap(),
                        attribute
                            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                            .unwrap()
                            .into_owned(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(attributes["xmlns"], uri);
            assert_eq!(attributes["Code"], value);
        }
        let legacy = format_xml::to_string_with_options(
            &schema,
            &instance,
            &XmlWriteOptions {
                declaration: false,
                indent: false,
                default_namespace: Some(uri.into()),
                schema_hints: None,
            },
        )
        .unwrap();
        assert!(
            legacy.contains(if uri.len() == 1 {
                uri
            } else {
                "urn:literal:\t\n\r"
            }),
            "legacy namespace whitespace bytes must remain unchanged: {legacy:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn new_document_rejects_forbidden_scalar_characters_and_preserves_legal_values() {
    for attribute in [false, true] {
        let mut field = ir::SchemaNode::scalar("Code", ir::ScalarType::String);
        field.attribute = attribute;
        let schema = ir::SchemaNode::group("Root", vec![field]);
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        for character in ['\0', '\u{1}', '\u{B}', '\t', '\n', '\r', '😀'] {
            let value = format!("a{character}b");
            let instance = Instance::Group(
                vec![(
                    "Code".into(),
                    Instance::Scalar(crate::Value::String(value.clone())),
                )]
                .into(),
            );
            for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
                let output =
                    serialize_xml_document(&descriptor, &instance, false, false, None, hints);
                if matches!(character, '\0' | '\u{1}' | '\u{B}') {
                    let error = output.unwrap_err();
                    assert_eq!(error.kind, XmlBoundaryErrorKind::Output);
                    assert_eq!(error.detail, "XML output must contain XML 1.0 characters");
                } else {
                    let output = output.unwrap();
                    let parsed = format_xml::from_str(&output, &schema).unwrap();
                    assert_eq!(
                        ir::primary_root_scalar_with_requirement(Some(&parsed), &["Code"], false),
                        Ok(crate::Value::String(value.clone()))
                    );
                }
            }
            if matches!(character, '\0' | '\u{1}' | '\u{B}') {
                // The new boundary validation must not change the legacy writer.
                let legacy = format_xml::to_string(&schema, &instance).unwrap();
                assert!(legacy.contains(character));
            }
        }
    }
}

#[cfg(test)]
#[test]
fn new_document_preserves_schema_hint_and_document_limit_error_precedence() {
    let schema = ir::SchemaNode::group(
        "Root",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String)],
    );
    let descriptor = codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let invalid = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("\0".into())),
        )]
        .into(),
    );
    assert_eq!(
        serialize_xml_document("{}", &invalid, false, false, None, None)
            .unwrap_err()
            .kind,
        XmlBoundaryErrorKind::Schema
    );
    let hint =
        serialize_xml_document(&descriptor, &invalid, false, false, None, Some("{}")).unwrap_err();
    assert_eq!(hint.kind, XmlBoundaryErrorKind::Output);
    assert_ne!(hint.detail, "XML output must contain XML 1.0 characters");
    let oversized = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("\0".repeat(MAX_XML_DOCUMENT_BYTES))),
        )]
        .into(),
    );
    let error =
        serialize_xml_document(&descriptor, &oversized, false, false, None, None).unwrap_err();
    assert_eq!(error.kind, XmlBoundaryErrorKind::DocumentLimit);
    assert!(error.bytes.unwrap() > MAX_XML_DOCUMENT_BYTES);
    assert_eq!(error.limit, Some(MAX_XML_DOCUMENT_BYTES));
}

#[cfg(test)]
#[test]
fn new_document_rejects_reserved_schema_root_namespace_without_changing_legacy() {
    for uri in [
        "http://www.w3.org/XML/1998/namespace",
        "http://www.w3.org/2000/xmlns/",
    ] {
        let mut schema = ir::SchemaNode::group("Root", vec![]);
        schema.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new(uri).unwrap(),
        ));
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        let instance = Instance::Group(vec![].into());
        for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
            let error = serialize_xml_document(&descriptor, &instance, false, false, None, hints)
                .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Output);
            assert_eq!(
                error.detail,
                "reserved XML namespace cannot be the default namespace"
            );
        }
        let legacy = format_xml::to_string(&schema, &instance).unwrap();
        assert!(legacy.contains(&format!("xmlns=\"{uri}\"")));
    }
}

#[cfg(test)]
#[test]
fn new_document_preserves_schema_root_namespace_and_explicit_unqualified_override() {
    use quick_xml::{Reader, XmlVersion, events::Event};
    let instance = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("ordinary\tdata".into())),
        )]
        .into(),
    );
    let mut schema = ir::SchemaNode::group(
        "Root",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
    );
    for character in ['\t', '\n', '\r'] {
        let uri = format!("urn:qualified:{character}value");
        schema.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new(&uri).unwrap(),
        ));
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        for policy in [None, Some("urn:ignored")] {
            for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
                let xml =
                    serialize_xml_document(&descriptor, &instance, false, false, policy, hints)
                        .unwrap();
                let mut reader = Reader::from_str(&xml);
                let start = match reader.read_event().unwrap() {
                    Event::Empty(start) => start,
                    actual => panic!("unexpected root event: {actual:?}"),
                };
                let attribute = start
                    .attributes()
                    .map(Result::unwrap)
                    .find(|attribute| attribute.key.as_ref() == b"xmlns")
                    .unwrap();
                assert_eq!(
                    attribute
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .unwrap(),
                    uri
                );
            }
        }
    }
    schema.xml_namespace = Some(ir::XmlNamespace::Unqualified);
    let descriptor = codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let xml = serialize_xml_document(
        &descriptor,
        &instance,
        false,
        false,
        Some("urn:ignored:\tvalue"),
        None,
    )
    .unwrap();
    assert!(!xml.contains("xmlns="));
}

#[cfg(test)]
mod structured_input_tests {
    use super::*;

    #[test]
    fn ignored_descriptor_properties_share_the_plain_and_v2_depth_boundary() {
        let source = ir::SchemaNode::scalar("Value", ir::ScalarType::String);
        let base = serde_json::to_string(&source).unwrap();
        for depth in [126, 127, 128, 129, 200] {
            let plain = format!(
                "{},\"ignored\":{}0{}}}",
                base.strip_suffix('}').unwrap(),
                "[".repeat(depth - 1),
                "]".repeat(depth - 1)
            );
            // Keep the general plain codec's ignored-property semantics.
            assert_eq!(
                codegen_schema::decode(&plain, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap(),
                source
            );
            let v2 = format!("{}{plain}", codegen_schema::V2_PREFIX);
            for descriptor in [&plain, &v2] {
                for result in [
                    parse_structured_xml(descriptor, "<Value>x</Value>"),
                    parse_structured_xml_bytes(descriptor, b"<Value>x</Value>"),
                ] {
                    if depth <= 127 {
                        assert_eq!(
                            result.unwrap(),
                            Instance::Scalar(ir::Value::String("x".into()))
                        );
                    } else {
                        assert_eq!(result.unwrap_err().kind, XmlBoundaryErrorKind::Schema);
                    }
                }
            }
        }
    }

    #[test]
    fn descriptor_depth_ignores_delimiters_and_escaped_quotes_in_strings() {
        let source = ir::SchemaNode::scalar("Value", ir::ScalarType::String);
        let mut value = serde_json::to_value(&source).unwrap();
        value["ignored"] = serde_json::Value::String("[ { \\\" \\".repeat(200));
        let descriptor = serde_json::to_string(&value).unwrap();
        assert_eq!(
            parse_structured_xml(&descriptor, "<Value>x</Value>").unwrap(),
            Instance::Scalar(ir::Value::String("x".into()))
        );
        // Malformed shallow JSON remains a typed schema failure.
        assert_eq!(
            parse_structured_xml("{\"name\":\"Value", "<Value/>")
                .unwrap_err()
                .kind,
            XmlBoundaryErrorKind::Schema
        );
        // Original schema bytes take precedence over the nesting scan.
        let oversized = format!(
            "{}{}",
            "[".repeat(128),
            " ".repeat(crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
        );
        let error = parse_structured_xml(&oversized, "<Value/>").unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
        assert!(error.detail.starts_with("embedded XML schema is "));
    }

    #[test]
    fn structured_parser_is_separate_from_boolean_root_policy_and_checks_utf8() {
        let source = ir::SchemaNode::group(
            "Source",
            vec![ir::SchemaNode::scalar("Value", ir::ScalarType::Int)],
        );
        let descriptor =
            codegen_schema::encode(&source, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        let xml = "<Source><Value> 42 </Value></Source>";
        let actual = parse_structured_xml(&descriptor, xml).unwrap();
        assert_eq!(actual, format_xml::from_str(xml, &source).unwrap());
        assert_eq!(
            parse_structured_xml_bytes(&descriptor, xml.as_bytes()).unwrap(),
            actual
        );
        assert_eq!(
            parse_xml(&descriptor, xml, false, false).unwrap_err().kind,
            XmlBoundaryErrorKind::Schema
        );
        assert_eq!(
            parse_structured_xml_bytes(&descriptor, &[0xff])
                .unwrap_err()
                .kind,
            XmlBoundaryErrorKind::Utf8
        );
        assert_eq!(
            parse_structured_xml(&descriptor, "<!DOCTYPE Source><Source/>")
                .unwrap_err()
                .kind,
            XmlBoundaryErrorKind::Input
        );
    }

    #[test]
    fn structured_proof_rejects_default_metadata_before_input_materialization() {
        let mut source = ir::SchemaNode::scalar("Value", ir::ScalarType::String);
        source.default = Some("fallback".into());
        let descriptor =
            codegen_schema::encode(&source, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        assert_eq!(
            parse_structured_xml(&descriptor, "<Value/>")
                .unwrap_err()
                .kind,
            XmlBoundaryErrorKind::Schema
        );
    }
    #[test]
    fn empty_qualified_namespace_wire_schema_is_refused_as_schema() {
        assert!(ir::XmlNamespace::qualified("").is_none());
        let descriptor = r#"{"name":"Value","xml_namespace":{"kind":"qualified","uri":""},"repeating":false,"kind":{"kind":"scalar","ty":"string"}}"#;
        assert_eq!(
            parse_structured_xml(descriptor, "<Value/>")
                .unwrap_err()
                .kind,
            XmlBoundaryErrorKind::Schema
        );
    }
}
