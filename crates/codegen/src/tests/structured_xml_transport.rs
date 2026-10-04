#[path = "fixtures/structured_xml_transport_cases.rs"]
mod cases;

#[test]
fn structured_lowering_and_plain_or_v2_descriptor_transport_are_distinct() {
    for depth in cases::DEPTHS {
        let source = cases::source_at_depth(depth);
        assert_eq!(
            ir::xml_structured_document_input_is_supported(&source),
            depth <= 64
        );
        let program = crate::lower(&cases::project_with_source(source.clone()))
            .expect("closed unused source retains valid core lowering");
        assert_eq!(
            program
                .xml_boundary
                .as_ref()
                .map(|policy| policy.input.profile()),
            (depth <= 64).then_some(Some(crate::XmlInputProfile::Structured)),
            "logical schema depth {depth}",
        );

        let value = serde_json::to_value(&source).unwrap();
        assert_eq!(
            json_container_depth(&value),
            3 * depth - 1,
            "depth fixture {depth}"
        );
        let plain = serde_json::to_string(&source).unwrap();
        let v2 = format!("{}{plain}", codegen_schema::V2_PREFIX);
        for (transport, descriptor) in [("plain", plain.as_str()), ("V2", v2.as_str())] {
            match codegen_schema::decode(descriptor, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES) {
                Ok(decoded) => {
                    assert!(
                        depth <= 42,
                        "unexpectedly readable {transport} depth {depth}"
                    );
                    assert_eq!(decoded, source);
                }
                Err(error) => {
                    assert!(depth >= 43, "unexpected {transport} depth {depth}: {error}");
                    assert!(
                        matches!(error, codegen_schema::CodecError::Deserialization(_)),
                        "transport refusal must be typed: {error}"
                    );
                }
            }
        }

        match crate::serialize_embedded_schema(&source, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES) {
            Ok(descriptor) => {
                assert!(depth <= 42, "unreadable depth {depth} must not encode");
                assert_eq!(
                    codegen_schema::decode(&descriptor, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
                        .unwrap(),
                    source
                );
            }
            Err(error) => {
                assert!(
                    depth >= 43,
                    "unexpected encode error at depth {depth}: {error}"
                );
                assert!(
                    matches!(error, crate::EmbeddedSchemaError::Deserialization { .. }),
                    "depth transport must reach typed deserialization refusal: {error}"
                );
            }
        }
    }
}

#[test]
fn unsupported_ordinary_readers_keep_core_lowering_and_observed_policies_remain_strict() {
    for (case, source) in cases::unsupported_reader_sources() {
        assert!(
            !ir::xml_structured_document_input_is_supported(&source),
            "{case}"
        );
        let mut project = cases::project_with_source(source);
        let program = crate::lower(&project).unwrap_or_else(|error| panic!("{case}: {error}"));
        assert!(
            program.xml_boundary.is_none(),
            "unsupported reader {case} gained XML APIs"
        );
        crate::validate_program(&program).unwrap();
        project.source_options.xml_allow_inactive_root_type_members = true;
        project.source_options.xml_root_view_read_policy = true;
        assert!(
            crate::lower(&project).is_err(),
            "observed policy {case} must not fall back"
        );
    }
}

fn json_container_depth(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(fields) => {
            1 + fields.values().map(json_container_depth).max().unwrap_or(0)
        }
        serde_json::Value::Array(items) => {
            1 + items.iter().map(json_container_depth).max().unwrap_or(0)
        }
        _ => 0,
    }
}
