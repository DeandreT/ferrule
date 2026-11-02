use super::*;

#[path = "lifecycle_controls.rs"]
mod controls;

fn two_collections(fixture: &Fixture) -> String {
    let main = rows(fixture, true);
    fixture.write("in.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="OtherRow" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    main.replace(r#"<entry name="Row" outkey="11"><entry name="Text" outkey="10"/></entry>"#, r#"<entry name="Row" outkey="11"><entry name="Text" outkey="10"/></entry><entry name="OtherRow" outkey="15"><entry name="Text" outkey="16"/></entry>"#)
}

const INPUT: &str = "<Input><Row><Text>A</Text></Row><OtherRow><Text>B1</Text></OtherRow><OtherRow><Text>B2</Text></OtherRow></Input>";

fn hinted(main: &str, scopes: &str) -> String {
    main.replace(
        "<structure>",
        &format!(
            r#"<structure><ferrule-scope-sources version="1">{scopes}</ferrule-scope-sources>"#
        ),
    )
}

fn refusal_with_receipt(fixture: &Fixture, name: &str, main: &str, input: &str) -> bool {
    fixture.write("mapping.mfd", main);
    let result = mfd::import(&fixture.0.join("mapping.mfd"));
    let root = std::env::var_os("FERRULE_LIFECYCLE_EVIDENCE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| fixture.0.clone());
    std::fs::create_dir_all(&root).unwrap();
    let receipt = root.join(format!("{}-{name}.json", std::process::id()));
    let (record, refused) = match result {
        Ok(actual) => {
            let output = match format_xml::from_str(input, &actual.project.source) {
                Ok(input) => format!("{:?}", engine::run(&actual.project, &input)),
                Err(error) => format!("input decode error: {error:?}"),
            };
            let strict = mfd::import_with_profile(
                &fixture.0.join("mapping.mfd"),
                &ImportOptions::default(),
                mfd::ImportProfile::Executable,
            );
            println!(
                "{name}: published=true, strict={}, warnings={:?}, output={output}",
                strict.is_ok(),
                actual.warnings
            );
            (
                serde_json::json!({"case": name, "mapping": main, "input": input, "published": true, "warnings": actual.warnings,
                "strict_accepted": strict.is_ok(), "strict_error": strict.err().map(|error| format!("{error:?}")),
                "validation": format!("{:?}", engine::validate(&actual.project)), "whole_output_or_error": output,
                "project": serde_json::to_value(&actual.project).unwrap()}),
                false,
            )
        }
        Err(error) => {
            let refused = matches!(&error, mfd::MfdError::UnsupportedImport(message) if message.contains("post-normalization ownership certificate"));
            println!("{name}: error={error:?}");
            (
                serde_json::json!({"case": name, "mapping": main, "input": input, "published": false, "error": format!("{error:?}")}),
                refused,
            )
        }
    };
    std::fs::write(receipt, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    refused
}

#[test]
fn affected_scope_hint_cannot_replace_a_certified_driver() {
    let fixture = Fixture::new();
    let main = two_collections(&fixture);
    let main = hinted(&main, r#"<scope targetkey="31" source='["OtherRow"]'/>"#);
    assert!(refusal_with_receipt(
        &fixture,
        "scope-override",
        &main,
        INPUT
    ));
}

#[test]
fn sibling_receiver_cannot_borrow_another_boundarys_argument_frame() {
    let fixture = Fixture::new();
    let main = two_collections(&fixture);
    fixture.write("out.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Other" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    let main = main.replace(r#"<entry name="Row" inpkey="31"><entry name="Result" inpkey="30"/></entry>"#, r#"<entry name="Row" inpkey="31"><entry name="Result" inpkey="30"/></entry><entry name="Other" inpkey="41"><entry name="Result" inpkey="42"/></entry>"#)
        .replace("</vertices>", r#"<vertex vertexkey="15"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="42"/></edges></vertex></vertices>"#);
    assert!(refusal_with_receipt(
        &fixture,
        "sibling-frames",
        &main,
        INPUT
    ));
}

#[test]
fn ancestor_attributes_and_qualified_or_undefined_slots_do_not_lose_identity() {
    let mut failures = Vec::new();
    for case in ["ancestor-attribute", "qualified-slot", "undefined-slot"] {
        let fixture = Fixture::new();
        let (main, input) = if case == "ancestor-attribute" {
            fixture.write("in.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Group"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
            (
                MAIN.replace(
                    r#"<entry name="Text" outkey="10"/>"#,
                    r#"<entry name="@Group"><entry name="Text" outkey="10"/></entry>"#,
                ),
                "<Input><Group><Text>group-value</Text></Group></Input>",
            )
        } else {
            let header = if case == "qualified-slot" {
                r#"<header><namespaces><namespace/><namespace uid="urn:qualified"/></namespaces></header>"#
            } else {
                r#"<header><namespaces><namespace/></namespaces></header>"#
            };
            (
                MAIN.replace(
                    r#"<root><entry name="Input">"#,
                    &format!(r#"<root>{header}<entry name="Input">"#),
                )
                .replace(r#"name="Text" outkey="10""#, r#"name="1:Text" outkey="10""#),
                "<Input><Text>slot-value</Text></Input>",
            )
        };
        if !refusal_with_receipt(&fixture, case, &main, input) {
            failures.push(case);
        }
    }
    assert!(
        failures.is_empty(),
        "identity cases published: {failures:?}"
    );
}
