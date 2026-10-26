#[allow(dead_code, unused_imports)]
mod support;

use ir::{Instance, Value};
use support::{ConnectionStyle, ScalarContext, ScalarLiteral as Literal, ScalarMfdBuilder};

#[test]
fn renamed_literals_keep_typed_values_in_main_and_scalar_functions() {
    let cases = [
        (
            "string",
            "concat",
            vec![Literal::string("kept"), Literal::string("")],
            Value::String("kept".into()),
        ),
        (
            "integer",
            "add",
            vec![Literal::Integer(29), Literal::Integer(0)],
            Value::Int(29),
        ),
        (
            "decimal",
            "add",
            vec![Literal::Decimal(1.5), Literal::Decimal(0.0)],
            Value::Float(1.5),
        ),
        (
            "boolean",
            "logical-not",
            vec![Literal::Boolean(false)],
            Value::Bool(true),
        ),
    ];
    for context in [
        ScalarContext::Main,
        ScalarContext::UserDefined,
        ScalarContext::NestedUserDefined,
    ] {
        for style in [ConnectionStyle::Graph, ConnectionStyle::Legacy] {
            for (datatype, function, arguments, value) in &cases {
                let fixture = ScalarMfdBuilder::new(
                    "renamed_constant",
                    *function,
                    "core",
                    arguments.clone(),
                    *datatype,
                )
                .context(context)
                .connection_style(style)
                .reverse_components(true)
                .write()
                .unwrap();
                let original = mfd::import_with_profile(
                    fixture.design(),
                    &Default::default(),
                    mfd::ImportProfile::Executable,
                )
                .unwrap();
                let xml = std::fs::read_to_string(fixture.design()).unwrap();
                let renamed = xml.replace("name=\"constant\"", "name=\"SecondValue\"");
                assert_ne!(renamed, xml);
                std::fs::write(fixture.design(), renamed).unwrap();
                let imported = mfd::import_with_profile(
                    fixture.design(),
                    &Default::default(),
                    mfd::ImportProfile::Executable,
                )
                .unwrap();
                assert_eq!(
                    serde_json::to_value(&imported.imported.project).unwrap(),
                    serde_json::to_value(&original.imported.project).unwrap()
                );
                let project = imported.imported.project;
                let input =
                    format_xml::from_str("<Source><Seed>seed</Seed></Source>", &project.source)
                        .unwrap();
                let expected = Instance::Group(
                    vec![("Result".into(), Instance::Scalar(value.clone()))].into(),
                );
                assert_eq!(engine::run(&project, &input).unwrap(), expected);
                let output = fixture.design().with_file_name("roundtrip.mfd");
                assert!(mfd::export(&project, &output).unwrap().is_empty());
                let restored = mfd::import_with_profile(
                    &output,
                    &Default::default(),
                    mfd::ImportProfile::Executable,
                )
                .unwrap();
                assert_eq!(
                    engine::run(&restored.imported.project, &input).unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn renamed_malformed_literal_is_not_silently_imported_as_null() {
    let fixture = ScalarMfdBuilder::new(
        "malformed_renamed_constant",
        "add",
        "core",
        vec![Literal::Integer(29), Literal::Integer(0)],
        "integer",
    )
    .write()
    .unwrap();
    let original = std::fs::read_to_string(fixture.design()).unwrap();
    let malformed = original
        .replace("name=\"constant\"", "name=\"SecondValue\"")
        .replacen("value=\"29\"", "value=\"bad\"", 1);
    std::fs::write(fixture.design(), malformed).unwrap();
    assert!(matches!(
        mfd::import_with_profile(
            fixture.design(),
            &Default::default(),
            mfd::ImportProfile::Executable
        ),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
}
