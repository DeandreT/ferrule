include!("../../codegen/src/tests/xml_input_emission.rs");

const XML_INPUT_LANGUAGE: &str = "csharp";

fn emit_xml_input_source(program: &codegen::Program) -> String {
    let artifacts = crate::emit(program).unwrap();
    assert!(
        artifacts
            .files()
            .iter()
            .any(|file| file.path.as_str() == "Runtime/FerruleXml.InputSet.cs")
    );
    String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap()
}

#[test]
fn selected_context_helper_follows_the_xml_emission_plan_for_every_option() {
    let decode = |source: &str| mapping::project_file::decode_str(source).unwrap();
    let legacy = decode(include_str!(
        "../../../conformance/basic-owned-group-adjacent-by/legacy-project.json"
    ));
    let mut primary_xml = legacy.clone();
    primary_xml.source_options.xml_document = true;
    primary_xml.target_options.xml_document = true;
    let cases = [
        ("legacy", legacy, false),
        ("primary XML", primary_xml, true),
        (
            "static named XML inputs",
            decode(include_str!(
                "../../codegen/src/tests/fixtures/static_named_xml_inputs.json"
            )),
            true,
        ),
        (
            "observed root view",
            decode(include_str!(
                "../../codegen/src/tests/fixtures/static_named_xml_rootview.json"
            )),
            false,
        ),
        (
            "dynamic XML input",
            decode(include_str!(
                "../../codegen/src/tests/fixtures/dynamic_named_xml_input_mixed.json"
            )),
            false,
        ),
        (
            "XML document set",
            decode(include_str!(
                "../../codegen/src/tests/fixtures/dynamic_primary_xml_documents.json"
            )),
            false,
        ),
    ];
    for (name, project, selected_xml) in cases {
        let program = codegen::lower(&project).unwrap();
        for companions in [
            codegen::CompanionSelection::Complete,
            codegen::CompanionSelection::TypedOnly,
        ] {
            for configured in [false, true] {
                let options = if configured {
                    crate::Options {
                        identity: crate::Identity::new(
                            "Example.Mapping",
                            "Example.Generated",
                            "ExampleMapping",
                        )
                        .unwrap(),
                        manifest: codegen::ManifestSelection::Included,
                    }
                } else {
                    crate::Options::default()
                };
                let artifacts = crate::Generator::new(&options)
                    .emit_with_companions(&program, companions)
                    .unwrap();
                let source = std::str::from_utf8(
                    &artifacts
                        .files()
                        .iter()
                        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
                        .unwrap()
                        .contents,
                )
                .unwrap();
                let expected = selected_xml && companions == codegen::CompanionSelection::Complete;
                for marker in [
                    "private static SelectedTargetOutput ExecuteSelectedTargetFromContext(",
                    "return ExecuteSelectedTargetFromContext(context, selected);",
                    "mapped = ExecuteSelectedTargetFromContext(context, selected);",
                    "public static SelectedXmlBytesTargetOutput ExecuteXmlBytesSelectedTargetWithHost(",
                ] {
                    assert_eq!(
                        source.contains(marker),
                        expected,
                        "{name}/{companions:?}/configured={configured}: {marker}"
                    );
                }
                assert!(
                    source.contains(
                        "public static SelectedTargetOutput ExecuteSelectedTargetWithHost("
                    )
                );
            }
        }
    }
}
