use super::*;

#[path = "safety_guard.rs"]
mod safety_guard;

fn run(fixture: &Fixture, main: &str, input: &str) -> Instance {
    fixture.write("mapping.mfd", main);
    let actual = imported(fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert!(
        mfd::import_with_profile(
            &fixture.0.join("mapping.mfd"),
            &ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_ok()
    );
    let input = format_xml::from_str(input, &actual.project.source).unwrap();
    engine::run(&actual.project, &input).unwrap()
}

fn result(value: &str) -> Instance {
    Instance::Group(
        vec![(
            "Result".into(),
            Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )
}

#[test]
fn matching_hints_remain_valid_but_duplicate_conflicting_and_unknown_affected_hints_refuse() {
    let fixture = Fixture::new();
    let main = two_collections(&fixture);
    let good = r#"<scope targetkey="31" source='["Row"]'/>"#;
    let bad = r#"<scope targetkey="31" source='["OtherRow"]'/>"#;
    let expected =
        Instance::Group(vec![("Row".into(), Instance::Repeated(vec![result("A")]))].into());
    assert_eq!(run(&fixture, &hinted(&main, good), INPUT), expected);
    for (name, hints) in [
        ("bad-first", format!("{bad}{good}")),
        ("bad-last", format!("{good}{bad}")),
        ("same-duplicate", format!("{good}{good}")),
        (
            "unknown-attribute",
            r#"<scope targetkey="31" source='["Row"]' unknown="1"/>"#.into(),
        ),
        (
            "bad-json",
            r#"<scope targetkey="31" source="invalid"/>"#.into(),
        ),
    ] {
        assert!(refusal_with_receipt(
            &fixture,
            name,
            &hinted(&main, &hints),
            INPUT
        ));
    }
    let multiple_blocks = hinted(&main, good).replace("</ferrule-scope-sources>", &format!(r#"</ferrule-scope-sources><ferrule-scope-sources version="1">{bad}</ferrule-scope-sources>"#));
    assert!(refusal_with_receipt(
        &fixture,
        "multiple-hint-blocks",
        &multiple_blocks,
        INPUT
    ));
    assert!(refusal_with_receipt(
        &fixture,
        "unknown-hint-version",
        &hinted(&main, good).replace("version=\"1\"", "version=\"2\""),
        INPUT
    ));
    assert_eq!(
        run(
            &fixture,
            &hinted(&main, r#"<scope targetkey="999" source="invalid"/>"#),
            INPUT
        ),
        expected
    );
}

fn sibling(fixture: &Fixture, same_driver: bool) -> String {
    let main = two_collections(fixture);
    fixture.write("out.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Other" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    main.replace(r#"<entry name="Row" inpkey="31"><entry name="Result" inpkey="30"/></entry>"#, r#"<entry name="Row" inpkey="31"><entry name="Result" inpkey="30"/></entry><entry name="Other" inpkey="41"><entry name="Result" inpkey="42"/></entry>"#)
        .replace("</vertices>", &format!(r#"<vertex vertexkey="{}"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="42"/></edges></vertex></vertices>"#, if same_driver { 11 } else { 15 }))
}

#[test]
fn same_owner_sibling_fanout_and_constants_or_root_values_in_distinct_receivers_are_valid() {
    let fixture = Fixture::new();
    let main = sibling(&fixture, true);
    let expected = Instance::Group(
        vec![
            ("Row".into(), Instance::Repeated(vec![result("A")])),
            ("Other".into(), Instance::Repeated(vec![result("A")])),
        ]
        .into(),
    );
    assert_eq!(run(&fixture, &main, INPUT), expected);
    for root in [false, true] {
        let main = sibling(&fixture, false);
        let main = if root {
            let schema = std::fs::read_to_string(fixture.0.join("in.xsd")).unwrap().replacen("<xs:sequence>", "<xs:sequence><xs:element name=\"RootValue\" type=\"xs:string\"/>", 1);
            fixture.write("in.xsd", &schema);
            main.replace(r#"<entry name="Input">"#, r#"<entry name="Input"><entry name="RootValue" outkey="18"/>"#)
        } else {
            let constant = r#"<component name="constant" library="core" kind="2" uid="6"><targets><datapoint key="18"/></targets><data><constant value="CONST" datatype="string"/></data></component>"#;
            main.replace("</children>", &format!("{constant}</children>"))
        }.replace(r#"<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>"#, r#"<vertex vertexkey="18"><edges><edge vertexkey="20"/></edges></vertex>"#);
        let input = if root {
            INPUT.replace("<Input>", "<Input><RootValue>ROOT</RootValue>")
        } else {
            INPUT.into()
        };
        let value = if root { "ROOT" } else { "CONST" };
        let expected = Instance::Group(
            vec![
                ("Row".into(), Instance::Repeated(vec![result(value)])),
                (
                    "Other".into(),
                    Instance::Repeated(vec![result(value), result(value)]),
                ),
            ]
            .into(),
        );
        assert_eq!(run(&fixture, &main, &input), expected);
    }
}

#[test]
fn unrelated_receiving_scope_hints_keep_ordinary_behavior() {
    let fixture = Fixture::new();
    let main = sibling(&fixture, false);
    let constant = r#"<component name="constant" library="core" kind="2" uid="6"><targets><datapoint key="19"/></targets><data><constant value="C" datatype="string"/></data></component>"#;
    let main = main
        .replace(
            r#"<vertex vertexkey="21"><edges><edge vertexkey="42"/></edges></vertex>"#,
            r#"<vertex vertexkey="19"><edges><edge vertexkey="42"/></edges></vertex>"#,
        )
        .replace("</children>", &format!("{constant}</children>"));
    let main = hinted(&main, r#"<scope targetkey="41" source='["Row"]'/>"#);
    let expected = Instance::Group(
        vec![
            ("Row".into(), Instance::Repeated(vec![result("A")])),
            ("Other".into(), Instance::Repeated(vec![result("C")])),
        ]
        .into(),
    );
    assert_eq!(run(&fixture, &main, INPUT), expected);
}

fn nested(fixture: &Fixture) -> String {
    let main = rows(fixture, true);
    fixture.write("in.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/><xs:element name="Cell" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    fixture.write("out.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/><xs:element name="Child" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    main.replace(r#"<entry name="Text" outkey="10"/>"#, r#"<entry name="Text" outkey="10"/><entry name="Cell" outkey="17"><entry name="Text" outkey="18"/></entry>"#)
        .replace(r#"<entry name="Result" inpkey="30"/>"#, r#"<entry name="Result" inpkey="30"/><entry name="Child" inpkey="33"><entry name="Result" inpkey="34"/></entry>"#)
        .replace("</vertices>", r#"<vertex vertexkey="17"><edges><edge vertexkey="33"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="34"/></edges></vertex></vertices>"#)
}

#[test]
fn ancestor_frames_are_valid_in_children_but_children_cannot_supply_parent_frames() {
    let fixture = Fixture::new();
    let main = nested(&fixture);
    let input = "<Input><Row><Text>A</Text><Cell><Text>C1</Text></Cell><Cell><Text>C2</Text></Cell></Row></Input>";
    let expected = Instance::Group(
        vec![(
            "Row".into(),
            Instance::Repeated(vec![Instance::Group(
                vec![
                    ("Result".into(), Instance::Scalar(Value::String("A".into()))),
                    (
                        "Child".into(),
                        Instance::Repeated(vec![result("A"), result("A")]),
                    ),
                ]
                .into(),
            )]),
        )]
        .into(),
    );
    assert_eq!(run(&fixture, &main, input), expected);
    assert_eq!(
        run(
            &fixture,
            &hinted(
                &main,
                r#"<scope targetkey="31" source='["Row"]'/><scope targetkey="33" source='["Cell"]'/>"#
            ),
            input
        ),
        expected
    );
    let bad = main.replace(
        r#"<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>"#,
        r#"<vertex vertexkey="18"><edges><edge vertexkey="20"/></edges></vertex>"#,
    );
    assert!(refusal_with_receipt(
        &fixture,
        "child-borrowed-by-parent",
        &bad,
        input
    ));
    assert!(refusal_with_receipt(
        &fixture,
        "absolute-child-hint",
        &hinted(&main, r#"<scope targetkey="33" source='["Row","Cell"]'/>"#),
        input
    ));
}

#[test]
fn xml_identity_is_checked_for_driver_and_receiver_ancestors_as_well_as_arguments() {
    let fixture = Fixture::new();
    let main =
        rows(&fixture, true).replace(r#"name="Row" inpkey="31""#, r#"name="@Row" inpkey="31""#);
    assert!(refusal_with_receipt(
        &fixture,
        "attribute-receiver-ancestor",
        &main,
        "<Input><Row><Text>A</Text></Row></Input>"
    ));
    let main = rows(&fixture, true).replace(r#"name="Row" inpkey="31""#, r#"name="1:Row" inpkey="31""#)
        .replace(r#"<root><entry name="Output">"#, r#"<root><header><namespaces><namespace/><namespace uid="urn:qualified"/></namespaces></header><entry name="Output">"#);
    assert!(refusal_with_receipt(
        &fixture,
        "qualified-receiver-ancestor",
        &main,
        "<Input><Row><Text>A</Text></Row></Input>"
    ));
    let main = nested(&fixture).replace(r#"name="Row" outkey="11""#, r#"name="@Row" outkey="11""#);
    assert!(refusal_with_receipt(
        &fixture,
        "attribute-driver-ancestor",
        &main,
        "<Input><Row><Text>A</Text><Cell><Text>C</Text></Cell></Row></Input>"
    ));
    for header in ["", "<header><namespaces><namespace/></namespaces></header>"] {
        let fixture = Fixture::new();
        let main = MAIN
            .replace(r#"name="Text" outkey="10""#, r#"name="0:Text" outkey="10""#)
            .replace(
                r#"<root><entry name="Input">"#,
                &format!("<root>{header}<entry name=\"Input\">"),
            );
        let expected = Instance::Group(
            vec![(
                "Result".into(),
                Instance::Scalar(Value::String("zero".into())),
            )]
            .into(),
        );
        assert_eq!(
            run(&fixture, &main, "<Input><Text>zero</Text></Input>"),
            expected
        );
    }
}

#[test]
fn receiver_hints_cannot_make_a_sibling_frame_proof_transferable() {
    let fixture = Fixture::new();
    let main = sibling(&fixture, true);
    let changed = hinted(&main, r#"<scope targetkey="41" source='["OtherRow"]'/>"#);
    assert!(refusal_with_receipt(
        &fixture,
        "changed-second-receiver",
        &changed,
        INPUT
    ));
    // This might repair execution under the legacy resolver, but it changes
    // the proved physical driver and is outside the bounded hint profile.
    let mixed = sibling(&fixture, false);
    assert!(refusal_with_receipt(
        &fixture,
        "hint-cannot-repair-mixed-proof",
        &hinted(&mixed, r#"<scope targetkey="41" source='["Row"]'/>"#),
        INPUT
    ));
}

#[test]
fn affected_hint_paths_are_bounded_before_json_allocation() {
    let fixture = Fixture::new();
    let main = two_collections(&fixture);
    let huge = format!(
        r#"<scope targetkey="31" source='["{}"]'/>"#,
        "x".repeat(4097)
    );
    assert!(refusal_with_receipt(
        &fixture,
        "hint-path-limit",
        &hinted(&main, &huge),
        INPUT
    ));
}

#[test]
fn guarded_nested_scope_hints_persist_through_the_capability_codec_and_export_refuses() {
    let fixture = Fixture::new();
    let main = nested(&fixture);
    fixture.write("mapping.mfd", &main);
    let original = imported(&fixture);
    assert!(original.warnings.is_empty());
    let path = fixture.0.join("owned.mfd");
    assert!(mfd::export(&original.project, &path).is_err());
    assert!(!path.exists());
    let encoded = mapping::project_file::encode_pretty(&original.project).unwrap();
    let round_trip = mapping::project_file::decode_str(&encoded).unwrap();
    let input = format_xml::from_str(
        "<Input><Row><Text>A</Text><Cell><Text>C</Text></Cell></Row></Input>",
        &original.project.source,
    )
    .unwrap();
    assert_eq!(
        engine::run(&original.project, &input).unwrap(),
        engine::run(&round_trip, &input).unwrap()
    );
}
