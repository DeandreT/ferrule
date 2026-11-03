use super::*;

fn assert_control(
    fixture: &Fixture,
    main: &str,
    definition: &str,
) -> (mapping::Project, mapping::Project) {
    fixture.write("mapping.mfd", main);
    fixture.write("helpers.mfd", &format!("<mapping>{definition}</mapping>"));
    fixture.write("embedded.mfd", &embedded(main, definition));
    let actual = imported(fixture);
    let control = mfd::import(&fixture.0.join("embedded.mfd")).unwrap();
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert!(control.warnings.is_empty(), "{:?}", control.warnings);
    assert_guarded_control(&actual.project, &control.project);
    (actual.project, control.project)
}

#[test]
fn owned_function_calls_keep_per_row_and_per_run_lifetimes_after_library_removal() {
    let fixture = Fixture::new();
    let main = MAIN
        .replace(
            "<entry name=\"Text\" outkey=\"10\"/>",
            "<entry name=\"Row\" outkey=\"11\"><entry name=\"Text\" outkey=\"10\"/></entry>",
        )
        .replace(
            "<entry name=\"Result\" inpkey=\"30\"/>",
            "<entry name=\"Row\" inpkey=\"31\"><entry name=\"Result\" inpkey=\"30\"/></entry>",
        )
        .replace(
            "</vertices>",
            "<vertex vertexkey=\"11\"><edges><edge vertexkey=\"31\"/></edges></vertex></vertices>",
        );
    for (file, root, field) in [("in.xsd", "Input", "Text"), ("out.xsd", "Output", "Result")] {
        fixture.write(file, &format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="{field}" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#));
    }
    let (actual, control) = assert_control(&fixture, &main, IDENTITY);
    std::fs::remove_file(fixture.0.join("helpers.mfd")).unwrap();
    for values in [["first", "second"], ["third", "fourth"]] {
        let input = format_xml::from_str(
            &format!(
                "<Input><Row><Text>{}</Text></Row><Row><Text>{}</Text></Row></Input>",
                values[0], values[1]
            ),
            &actual.source,
        )
        .unwrap();
        let output = engine::run(&actual, &input).unwrap();
        assert_eq!(output, engine::run(&control, &input).unwrap());
        let expected = Instance::Group(
            vec![(
                "Row".into(),
                Instance::Repeated(
                    values
                        .into_iter()
                        .map(|value| {
                            Instance::Group(
                                vec![(
                                    "Result".into(),
                                    Instance::Scalar(Value::String(value.into())),
                                )]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )]
            .into(),
        );
        assert_eq!(output, expected);
    }
    // Guarded persistence uses the required-capability codec, not ignorable MFD metadata.
    let encoded = mapping::project_file::encode_pretty(&actual).unwrap();
    let round_trip = mapping::project_file::decode_str(&encoded).unwrap();
    assert!(mfd::export(&actual, &fixture.0.join("owned.mfd")).is_err());
    assert!(!fixture.0.join("owned.mfd").exists());
    assert_eq!(round_trip.user_functions.len(), 1);
    let input = format_xml::from_str(
        "<Input><Row><Text>after-export</Text></Row></Input>",
        &actual.source,
    )
    .unwrap();
    assert_eq!(
        engine::run(&actual, &input).unwrap(),
        engine::run(&round_trip, &input).unwrap()
    );
}

#[test]
fn unused_typed_argument_is_still_consumed_eagerly() {
    let fixture = Fixture::new();
    let constant = r#"<component library="core" kind="2" uid="102" name="constant"><targets><datapoint key="1002"/></targets><data><constant datatype="string" value="kept"/></data></component>"#;
    let definition = IDENTITY
        .replace(
            "<input datatype=\"string\"/>",
            "<input datatype=\"integer\"/>",
        )
        .replace("</children>", &format!("{constant}</children>"))
        .replace("vertexkey=\"1000\"", "vertexkey=\"1002\"");
    let (actual, control) = assert_control(&fixture, MAIN, &definition);
    for text in ["1", "bad", "2"] {
        let input = format_xml::from_str(
            &format!("<Input><Text>{text}</Text></Input>"),
            &actual.source,
        )
        .unwrap();
        match (engine::run(&actual, &input), engine::run(&control, &input)) {
            (Ok(actual), Ok(control)) => {
                assert_ne!(text, "bad");
                assert_eq!(actual, control);
                assert_eq!(
                    actual.field("Result").and_then(Instance::as_scalar),
                    Some(&Value::String("kept".into()))
                );
            }
            (Err(actual), Err(control)) => {
                assert_eq!(text, "bad");
                assert_eq!(actual.to_string(), control.to_string());
                assert!(matches!(
                    actual,
                    engine::EngineError::UserFunctionParameterType { .. }
                ));
            }
            result => panic!("eager invocation mismatch: {result:?}"),
        }
    }
}

#[test]
fn body_lazy_branches_preserve_complete_results_and_malformed_input_errors() {
    let fixture = Fixture::new();
    let body = r#"
<component library="core" kind="2" uid="102" name="constant"><targets><datapoint key="1010"/></targets><data><constant datatype="string" value="skip"/></data></component>
<component library="core" kind="2" uid="103" name="constant"><targets><datapoint key="1011"/></targets><data><constant datatype="string" value="safe"/></data></component>
<component library="core" kind="2" uid="104" name="constant"><targets><datapoint key="1012"/></targets><data><constant datatype="string" value="[Y]-[M]-[D]"/></data></component>
<component library="core" kind="5" uid="105" name="equal"><sources><datapoint pos="0" key="1020"/><datapoint pos="1" key="1021"/></sources><targets><datapoint key="1022"/></targets></component>
<component library="core" kind="5" uid="106" name="parse-date"><sources><datapoint pos="0" key="1030"/><datapoint pos="1" key="1031"/></sources><targets><datapoint key="1032"/></targets></component>
<component library="core" kind="4" uid="107" name="if-else"><sources><datapoint pos="0" key="1040"/><datapoint pos="1" key="1041"/><datapoint pos="2" key="1042"/></sources><targets><datapoint key="1043"/></targets></component>"#;
    let edges = r#"<vertex vertexkey="1000"><edges><edge vertexkey="1020"/><edge vertexkey="1030"/></edges></vertex>
<vertex vertexkey="1010"><edges><edge vertexkey="1021"/></edges></vertex>
<vertex vertexkey="1011"><edges><edge vertexkey="1041"/></edges></vertex>
<vertex vertexkey="1012"><edges><edge vertexkey="1031"/></edges></vertex>
<vertex vertexkey="1022"><edges><edge vertexkey="1040"/></edges></vertex>
<vertex vertexkey="1032"><edges><edge vertexkey="1042"/></edges></vertex>
<vertex vertexkey="1043"><edges><edge vertexkey="1001"/></edges></vertex>"#;
    let definition = IDENTITY
        .replace("</children>", &format!("{body}</children>"))
        .replace(
            "<vertex vertexkey=\"1000\"><edges><edge vertexkey=\"1001\"/></edges></vertex>",
            edges,
        );
    let (actual, control) = assert_control(&fixture, MAIN, &definition);
    for (text, expected) in [
        ("skip", Some("safe")),
        ("2026-11-02", Some("2026-11-02")),
        ("malformed", None),
    ] {
        let input = format_xml::from_str(
            &format!("<Input><Text>{text}</Text></Input>"),
            &actual.source,
        )
        .unwrap();
        match (
            engine::run(&actual, &input),
            engine::run(&control, &input),
            expected,
        ) {
            (Ok(actual), Ok(control), Some(expected)) => {
                assert_eq!(actual, control);
                assert_eq!(
                    actual.field("Result").and_then(Instance::as_scalar),
                    Some(&Value::String(expected.into()))
                );
            }
            (Err(actual), Err(control), None) => {
                assert_eq!(actual.to_string(), control.to_string());
            }
            result => panic!("lazy invocation mismatch: {result:?}"),
        }
    }
}
