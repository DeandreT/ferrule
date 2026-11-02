//! Self-authored metadata only: no reference installation or vendor body.
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

#[path = "declared_library_reconciliation/authority.rs"]
mod authority;
#[path = "declared_library_reconciliation/certification.rs"]
mod certification;
#[path = "declared_library_reconciliation/lifetimes.rs"]
mod lifetimes;
#[path = "declared_library_reconciliation/security.rs"]
mod security;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_udf_reconciliation_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        for (file, root, field) in [("in.xsd", "Input", "Text"), ("out.xsd", "Output", "Result")] {
            fixture.write(file, &format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="{field}" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#));
        }
        fixture.write("mapping.mfd", MAIN);
        fixture.write("helpers.mfd", &format!("<mapping>{IDENTITY}</mapping>"));
        fixture
    }
    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.0.join(name), text).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const MAIN: &str = r#"<mapping version="26">
<settings><libraries><import path="helpers.mfd" saverelative="1"/></libraries></settings>
<component name="main"><structure><children>
<component name="Input" library="xml" uid="1" kind="14"><data><root><entry name="Input"><entry name="Text" outkey="10"/></entry></root><document schema="in.xsd" instanceroot="{}Input"/></data></component>
<component name="Identity" library="generic" uid="2" kind="19"><data><root><entry name="value" inpkey="20" componentid="100"/></root><root rootindex="1"><entry name="result" outkey="21" componentid="101"/></root></data></component>
<component name="Output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Output"><entry name="Result" inpkey="30"/></entry></root><document schema="out.xsd" instanceroot="{}Output"/></data></component>
</children><graph directed="1"><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component></mapping>"#;

const IDENTITY: &str = r#"<component name="Identity" library="generic" inline="0"><structure><children>
<component name="value" library="core" kind="6" uid="100"><targets><datapoint pos="0" key="1000"/></targets><data><input datatype="string"/></data></component>
<component name="result" library="core" kind="7" uid="101"><sources><datapoint pos="0" key="1001"/></sources><data><output datatype="string"/></data></component>
</children><graph directed="1"><vertices><vertex vertexkey="1000"><edges><edge vertexkey="1001"/></edges></vertex></vertices></graph></structure></component>"#;

fn embedded(main: &str, definition: &str) -> String {
    main.replace("<settings><libraries><import path=\"helpers.mfd\" saverelative=\"1\"/></libraries></settings>", "")
        .replace("</mapping>", &format!("{definition}</mapping>"))
}

fn imported(fixture: &Fixture) -> mfd::Imported {
    mfd::import(&fixture.0.join("mapping.mfd")).unwrap_or_else(|error| {
        panic!(
            "{error}; design={}",
            std::fs::read_to_string(fixture.0.join("mapping.mfd")).unwrap()
        )
    })
}

fn refused(fixture: &Fixture, message: &str) {
    let actual = imported(fixture);
    assert!(
        actual.warnings.iter().any(|w| w.contains(message)),
        "expected {message}: {:?}",
        actual.warnings
    );
    assert!(
        !actual
            .project
            .graph
            .nodes
            .values()
            .any(|n| matches!(n, mapping::Node::UserFunctionCall { .. })),
        "refused call was still lowered"
    );
    assert!(
        mfd::import_with_profile(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn typed_input_and_output_adapters_keep_exact_values_and_error_categories() {
    for (ty, values) in [
        (
            "boolean",
            vec![
                ("false", Some(Value::Bool(false))),
                (" true ", Some(Value::Bool(true))),
                ("bad", None),
            ],
        ),
        (
            "integer",
            vec![
                ("10", Some(Value::Int(10))),
                (" -2 ", Some(Value::Int(-2))),
                ("2.5", None),
            ],
        ),
        (
            "double",
            vec![
                ("2.5", Some(Value::Float(2.5))),
                ("NaN", None),
                ("bad", None),
            ],
        ),
        ("string", vec![(" 10 ", Some(Value::String(" 10 ".into())))]),
    ] {
        for output_adapter in [false, true] {
            let fixture = Fixture::new();
            let definition = IDENTITY.replace(
                "<output datatype=\"string\"/>",
                &format!("<output datatype=\"{ty}\"/>"),
            );
            let definition = if output_adapter {
                definition
            } else {
                definition.replace(
                    "<input datatype=\"string\"/>",
                    &format!("<input datatype=\"{ty}\"/>"),
                )
            };
            fixture.write("helpers.mfd", &format!("<mapping>{definition}</mapping>"));
            fixture.write("embedded.mfd", &embedded(MAIN, &definition));
            let target = std::fs::read_to_string(fixture.0.join("out.xsd"))
                .unwrap()
                .replace("xs:string", &format!("xs:{ty}"));
            fixture.write("out.xsd", &target);
            let actual = imported(&fixture);
            let control = mfd::import(&fixture.0.join("embedded.mfd")).unwrap();
            assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
            assert!(control.warnings.is_empty(), "{:?}", control.warnings);
            assert_eq!(
                serde_json::to_value(&actual.project).unwrap(),
                serde_json::to_value(&control.project).unwrap()
            );
            for (text, expected) in &values {
                let input = format_xml::from_str(
                    &format!("<Input><Text>{text}</Text></Input>"),
                    &actual.project.source,
                )
                .unwrap();
                match (
                    engine::run(&control.project, &input),
                    engine::run(&actual.project, &input),
                    expected,
                ) {
                    (Ok(control), Ok(actual), Some(expected)) => {
                        assert_eq!(actual, control);
                        assert_eq!(
                            actual.field("Result").and_then(Instance::as_scalar),
                            Some(expected)
                        );
                    }
                    (Err(control), Err(actual), None) => {
                        assert_eq!(actual.to_string(), control.to_string());
                        if output_adapter {
                            assert!(matches!(
                                actual,
                                engine::EngineError::UserFunctionOutputType { .. }
                            ));
                        } else {
                            assert!(matches!(
                                actual,
                                engine::EngineError::UserFunctionParameterType { .. }
                            ));
                        }
                    }
                    result => panic!("typed adapter mismatch: {result:?}"),
                }
            }
        }
    }
}

#[test]
fn signatures_unknown_dead_components_and_effects_are_not_inferred_or_pruned() {
    for (definition, diagnostic) in [
        (IDENTITY.replace("inline=\"0\"", "inline=\"1\""), "inline"),
        (IDENTITY.replace("<input datatype=\"string\"/>", "<input/>"), "parameter type"),
        (IDENTITY.replace("<input datatype=\"string\"/>", "<input datatype=\"string\"/><parameter name=\"value\" usageKind=\"input\" optional=\"1\"/>"), "optional"),
        (IDENTITY.replace("</children>", "<component library=\"custom\" kind=\"5\" uid=\"9\" name=\"unused\"/></children>"), "unknown/effectful"),
        (IDENTITY.replace("</children>", "<component library=\"core\" kind=\"5\" uid=\"9\" name=\"now\"/></children>"), "unknown/effectful"),
        (IDENTITY.replace("</children>", "<component library=\"generic\" kind=\"19\" uid=\"9\" name=\"Identity\"/></children>"), "nested calls"),
        (IDENTITY.replace("</children>", "<component library=\"xml\" kind=\"14\" uid=\"9\" name=\"unused\"><data><document schema=\"DO-NOT-READ.xsd\"/></data></component></children>"), "unknown/effectful"),
        (IDENTITY.replace("key=\"1001\"", "key=\"1000\""), "colliding function-owned"),
        (IDENTITY.replace("vertexkey=\"1000\"", "vertexkey=\"1001\""), "owned direction"),
        (IDENTITY.replace("pos=\"0\" key=\"1000\"", "pos=\"64\" key=\"1000\""), "below 64"),
        (IDENTITY.replace("<structure>", "<structure xmlns=\"urn:unproved\">"), "namespaced declarative metadata"),
    ] {
        let fixture = Fixture::new();
        fixture.write("helpers.mfd", &format!("<mapping>{definition}</mapping>"));
        refused(&fixture, diagnostic);
    }
}

#[test]
fn component_ids_required_producers_and_call_metadata_cannot_rebind_by_name() {
    for (main, diagnostic) in [
        (MAIN.replace("componentid=\"100\"", "componentid=\"999\""), "unknown definition parameter"),
        (MAIN.replace("<edge vertexkey=\"20\"/>", ""), "required imported parameter"),
        (MAIN.replace("kind=\"19\"", "kind=\"19\" inline=\"1\""), "unknown or namespaced"),
        (MAIN.replace("<entry name=\"result\" outkey=\"21\" componentid=\"101\"/>", "<entry name=\"result\" outkey=\"21\" componentid=\"101\"/><entry name=\"other\" outkey=\"22\" componentid=\"101\"/>"), "unused call output"),
        (MAIN.replace("outkey=\"10\"", "outkey=\"21\""), "call pin collides"),
        (MAIN.replace("<edge vertexkey=\"30\"/>", ""), "unused call output"),
    ] {
        let fixture = Fixture::new();
        fixture.write("mapping.mfd", &main);
        refused(&fixture, diagnostic);
    }
}

#[test]
fn downstream_variables_filenames_and_normalized_aliases_stay_refused() {
    for (main, diagnostic) in [
        (MAIN.replace("XSLTDefaultOutput=\"1\"", "XSLTDefaultOutput=\"1\" PassThrough=\"1\""), "PassThrough"),
        (MAIN.replace("<properties XSLTDefaultOutput=\"1\"/>", "<properties XSLTDefaultOutput=\"1\"/><properties PassThrough=\"1\"/>"), "PassThrough"),
        (MAIN.replace("name=\"Result\" inpkey", "name=\"FileInstance\" inpkey"), "filename/FileInstance"),
        (MAIN.replace("name=\"Result\" inpkey", "name=\"@FileInstance\" inpkey"), "filename/FileInstance"),
        (MAIN.replace("name=\"Result\" inpkey", "name=\"2:FileInstance\" inpkey"), "filename/FileInstance"),
        (MAIN.replace("name=\"Result\" inpkey=\"30\"/>", "name=\"Result\" inpkey=\"30\" outkey=\"99\"/>"), "declares output ports"),
        (MAIN.replace("name=\"Result\" inpkey=\"30\"/>", "name=\"Result\" inpkey=\"30\"/><entry name=\"FileInstance\" ns=\"1\" inpkey=\"030\"/>"), "multiple raw"),
        (MAIN.replace("name=\"Result\" inpkey=\"30\"/>", "name=\"Ignored\"/><entry name=\"Result\" type=\"xml-type\"><entry name=\"Value\" inpkey=\"30\"/></entry>"), "ordinary XML scalar"),
    ] {
        let fixture = Fixture::new();
        // An output-bearing owner is source-shaped to the ordinary reader.
        // Retain an independent target to observe the precise role refusal.
        let main = if diagnostic == "declares output ports" {
            main.replace("</children>", "<component name=\"Audit\" library=\"xml\" kind=\"14\" uid=\"4\"><data><root><entry name=\"Output\"><entry name=\"Result\" inpkey=\"40\"/></entry></root><document schema=\"out.xsd\" instanceroot=\"{}Output\"/></data></component></children>")
                .replace("</vertices>", "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"40\"/></edges></vertex></vertices>")
        } else { main };
        fixture.write("mapping.mfd", &main);
        refused(&fixture, diagnostic);
    }
}

#[test]
fn exact_import_and_local_declaration_collisions_have_no_order_winner() {
    let fixture = Fixture::new();
    fixture.write("second.mfd", &format!("<mapping>{IDENTITY}</mapping>"));
    let ambiguous = MAIN.replace(
        "</libraries>",
        "<import path=\"second.mfd\" saverelative=\"1\"/></libraries>",
    );
    fixture.write("mapping.mfd", &ambiguous);
    refused(&fixture, "ambiguous exact function");
    // Explicit local precedence retains the ambiguity diagnostic, not an
    // import-order winner silently qualified as executable.
    fixture.write(
        "mapping.mfd",
        &ambiguous.replace("</mapping>", &format!("{IDENTITY}</mapping>")),
    );
    let local = imported(&fixture);
    assert!(local.warnings.iter().any(|w| w.contains("ambiguous")));
    assert_eq!(local.project.user_functions.len(), 1);
    assert!(
        local
            .project
            .graph
            .nodes
            .values()
            .any(|n| matches!(n, mapping::Node::UserFunctionCall { .. }))
    );
    fixture.write(
        "helpers.mfd",
        &format!("<mapping>{IDENTITY}{IDENTITY}</mapping>"),
    );
    fixture.write("mapping.mfd", MAIN);
    refused(&fixture, "conflicting exact declarations");
    fixture.write(
        "mapping.mfd",
        &embedded(MAIN, &format!("{IDENTITY}{IDENTITY}")),
    );
    refused(&fixture, "conflicting exact declarations");
}

#[test]
fn declared_libraries_refuse_byte_package_mode_without_affecting_next_import() {
    let fixture = Fixture::new();
    let package = |design: &str| {
        mfd::MfdPackage::new(
            "mapping.mfd",
            [
                ("mapping.mfd", design.as_bytes().to_vec()),
                (
                    "helpers.mfd",
                    format!("<mapping>{IDENTITY}</mapping>").into_bytes(),
                ),
                ("in.xsd", std::fs::read(fixture.0.join("in.xsd")).unwrap()),
                ("out.xsd", std::fs::read(fixture.0.join("out.xsd")).unwrap()),
            ],
        )
        .unwrap()
    };
    assert!(
        matches!(mfd::import_package(&package(MAIN)), Err(mfd::MfdError::UnsupportedImport(reason)) if reason.contains("declared mapping libraries") && reason.contains("no filesystem fallback"))
    );
    let control = mfd::import_package(&package(&embedded(MAIN, IDENTITY))).unwrap();
    assert!(control.warnings.is_empty(), "{:?}", control.warnings);
    assert_eq!(control.project.user_functions.len(), 1);
    assert!(imported(&fixture).warnings.is_empty());
}

#[test]
fn embedded_wrapper_cannot_flatten_an_imported_regular_call() {
    let fixture = Fixture::new();
    let call = r#"<component name="Identity" library="generic" kind="19" uid="102"><data><root><entry name="value" inpkey="2000" componentid="100"/></root><root rootindex="1"><entry name="result" outkey="2001" componentid="101"/></root></data></component>"#;
    let wrapper = IDENTITY.replace("name=\"Identity\"", "name=\"Wrapper\"")
        .replace("</children>", &format!("{call}</children>"))
        .replace("<vertex vertexkey=\"1000\"><edges><edge vertexkey=\"1001\"/></edges></vertex>", "<vertex vertexkey=\"1000\"><edges><edge vertexkey=\"2000\"/></edges></vertex><vertex vertexkey=\"2001\"><edges><edge vertexkey=\"1001\"/></edges></vertex>");
    fixture.write(
        "mapping.mfd",
        &MAIN
            .replace("name=\"Identity\"", "name=\"Wrapper\"")
            .replace("</mapping>", &format!("{wrapper}</mapping>")),
    );
    refused(&fixture, "nested typed/eager");
}

#[test]
fn connected_default_is_not_mistaken_for_a_mandatory_regular_signature() {
    let fixture = Fixture::new();
    let constant = r#"<component library="core" kind="2" uid="102" name="constant"><targets><datapoint key="1004"/></targets><data><constant datatype="string" value="fallback"/></data></component>"#;
    let definition = IDENTITY.replace("<targets><datapoint pos=\"0\" key=\"1000\"/>", "<sources><datapoint pos=\"0\" key=\"1003\"/></sources><targets><datapoint pos=\"0\" key=\"1000\"/>")
        .replace("</children>", &format!("{constant}</children>"))
        .replace("</vertices>", "<vertex vertexkey=\"1004\"><edges><edge vertexkey=\"1003\"/></edges></vertex></vertices>");
    fixture.write("helpers.mfd", &format!("<mapping>{definition}</mapping>"));
    refused(&fixture, "connected typed default");
}

#[test]
fn later_terminal_target_cannot_erase_a_pass_through_boundary() {
    let fixture = Fixture::new();
    let audit = r#"<component name="Audit" library="xml" kind="14" uid="4"><data><root><entry name="Output"><entry name="Result" inpkey="40"/></entry></root><document schema="out.xsd" instanceroot="{}Output"/></data></component>"#;
    let main = MAIN
        .replace(
            "XSLTDefaultOutput=\"1\"",
            "XSLTDefaultOutput=\"1\" PassThrough=\"1\"",
        )
        .replace(
            "name=\"Result\" inpkey=\"30\"/>",
            "name=\"Result\" inpkey=\"30\" outkey=\"99\"/>",
        )
        .replace("</children>", &format!("{audit}</children>"))
        .replace(
            "</vertices>",
            "<vertex vertexkey=\"99\"><edges><edge vertexkey=\"40\"/></edges></vertex></vertices>",
        );
    fixture.write("mapping.mfd", &main);
    refused(&fixture, "PassThrough XML owner");
}

#[test]
fn declared_identity_matches_embedded_control_without_name_substitution() {
    let fixture = Fixture::new();
    let embedded = MAIN.replace("<settings><libraries><import path=\"helpers.mfd\" saverelative=\"1\"/></libraries></settings>", "").replace("</mapping>", &format!("{IDENTITY}</mapping>"));
    fixture.write("embedded.mfd", &embedded);
    let control = mfd::import(&fixture.0.join("embedded.mfd")).unwrap();
    assert!(control.warnings.is_empty(), "{:?}", control.warnings);
    let actual = mfd::import(&fixture.0.join("mapping.mfd")).unwrap();
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert_eq!(
        serde_json::to_value(&actual.project).unwrap(),
        serde_json::to_value(&control.project).unwrap()
    );
    let input = format_xml::from_str(
        "<Input><Text>unchanged</Text></Input>",
        &actual.project.source,
    )
    .unwrap();
    let output = engine::run(&actual.project, &input).unwrap();
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("unchanged".into()))
    );
}
