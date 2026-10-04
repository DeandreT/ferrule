// Included by the existing shared Rust/C# emitter regression module.
mod structured_xml_transport_emission {
    use super::emit_schema_fixture;
    use codegen::{ArtifactSet, EmbeddedSchemaError, XmlInputProfile};

    mod cases {
        include!("fixtures/structured_xml_transport_cases.rs");
    }

    fn mapping_source(artifacts: &ArtifactSet) -> &str {
        let file = artifacts.files().iter().find(|file| {
            matches!(file.path.as_str(), "src/lib.rs" | "GeneratedMapping.cs")
        }).expect("one untouched generated mapping source");
        std::str::from_utf8(&file.contents).unwrap()
    }

    fn has_xml_entry_points(source: &str) -> bool {
        source.contains("pub fn execute_xml(") || source.contains("public static string ExecuteXml(")
    }

    #[test]
    fn structured_xml_emission_never_returns_artifacts_with_unreadable_depth_descriptors() {
        for depth in cases::DEPTHS {
            let program = codegen::lower(&cases::project_with_source(cases::source_at_depth(depth)))
                .expect("closed source lowers independently of descriptor emission");
            match emit_schema_fixture(&program) {
                Ok(artifacts) => {
                    assert!(depth <= 42, "unreadable depth {depth} unexpectedly emitted");
                    assert_eq!(program.xml_boundary.as_ref().unwrap().input.profile(),
                        Some(XmlInputProfile::Structured));
                    let descriptor = codegen::serialize_embedded_schema(&program.source,
                        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
                    let literal = serde_json::to_string(&descriptor).unwrap();
                    let source = mapping_source(&artifacts);
                    assert!(source.contains(&literal), "exact checked source descriptor is embedded");
                    assert!(has_xml_entry_points(source));
                    assert!(source.contains("parse_structured_xml(") ||
                        source.contains("FerruleXml.ParseStructuredEmbedded("));
                    assert!(source.contains("parse_structured_xml_bytes(") ||
                        source.contains("FerruleXml.ParseStructuredEmbeddedBytes("));
                }
                Err(error) => {
                    assert!(depth >= 43, "unexpected emission refusal at depth {depth}: {error}");
                    assert!(matches!(error, EmbeddedSchemaError::Deserialization { ref schema, .. }
                        if schema == "N1"), "exact source transport refusal required: {error}");
                }
            }
        }
    }

    #[test]
    fn unsupported_ordinary_reader_sources_emit_existing_core_apis_without_xml_entry_points() {
        for (case, source) in cases::unsupported_reader_sources() {
            let program = codegen::lower(&cases::project_with_source(source))
                .unwrap_or_else(|error| panic!("{case}: {error}"));
            assert!(program.xml_boundary.is_none(), "{case}");
            let artifacts = emit_schema_fixture(&program)
                .unwrap_or_else(|error| panic!("core-only {case} must still emit: {error}"));
            let source = mapping_source(&artifacts);
            assert!(!has_xml_entry_points(source), "{case} gained an unsupported XML reader");
            assert!(source.contains("pub fn execute(") ||
                source.contains("public static global::Ferrule.Runtime.FerruleInstance Execute("));
        }
    }
}
