include!("../../../codegen/src/tests/xml_input_emission.rs");

const XML_INPUT_LANGUAGE: &str = "rust";

fn emit_xml_input_source(program: &codegen::Program) -> String {
    let artifacts = crate::emit(
        program,
        &crate::Options {
            package_name: "xml-input-emission-fixture".into(),
            runtime_dependency: crate::RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .unwrap();
    String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap()
}
