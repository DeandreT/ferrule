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
