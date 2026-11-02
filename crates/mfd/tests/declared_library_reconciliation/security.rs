//! Original adversarial metadata: raw identities must be proved before lowering.
use super::*;
use mfd::{ImportOptions, ResourceBindingKindV1, ResourceBindingV1, ResourceReadSetV1};
use sha2::{Digest, Sha256};

const PRODUCER_REFUSAL: &str = "imported argument feed `10` has ambiguous raw producer ownership (2 declarations); no producer-order winner is selected";

fn source_fields(fixture: &Fixture, reverse: bool) -> String {
    let fields = if reverse {
        ["Other", "Text"]
    } else {
        ["Text", "Other"]
    };
    fixture.write("in.xsd", &format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence>{}</xs:sequence></xs:complexType></xs:element></xs:schema>"#, fields.iter().map(|name| format!(r#"<xs:element name="{name}" type="xs:string"/>"#)).collect::<String>()));
    MAIN.replace(
        r#"<entry name="Text" outkey="10"/>"#,
        &fields
            .iter()
            .map(|name| format!(r#"<entry name="{name}" outkey="10"/>"#))
            .collect::<String>(),
    )
}

fn unused_typed_definition() -> String {
    let constant = r#"<component library="core" kind="2" uid="102" name="constant"><targets><datapoint key="1002"/></targets><data><constant datatype="string" value="kept"/></data></component>"#;
    IDENTITY
        .replace(
            r#"<input datatype="string"/>"#,
            r#"<input datatype="integer"/>"#,
        )
        .replace("</children>", &format!("{constant}</children>"))
        .replace(r#"vertexkey="1000""#, r#"vertexkey="1002""#)
}

fn through_uppercase(main: &str) -> String {
    let function = r#"<component library="core" kind="5" uid="4" name="uppercase"><sources><datapoint key="40"/></sources><targets><datapoint key="41"/></targets></component>"#;
    main.replace("</children>", &format!("{function}</children>"))
        .replace(r#"<edge vertexkey="20"/>"#, r#"<edge vertexkey="40"/>"#)
        .replace(
            "</vertices>",
            r#"<vertex vertexkey="41"><edges><edge vertexkey="20"/></edges></vertex></vertices>"#,
        )
}

#[test]
fn duplicate_raw_argument_producers_are_refused_in_both_orders_and_through_functions() {
    let mut failures = Vec::new();
    for reverse in [false, true] {
        for route in ["direct", "known-function", "unused-typed"] {
            let fixture = Fixture::new();
            let main = source_fields(&fixture, reverse);
            let main = if route == "known-function" {
                through_uppercase(&main)
            } else {
                main
            };
            if route == "unused-typed" {
                fixture.write(
                    "helpers.mfd",
                    &format!("<mapping>{}</mapping>", unused_typed_definition()),
                );
            }
            fixture.write("mapping.mfd", &main);
            let actual = imported(&fixture);
            let input_text = if reverse {
                "<Input><Other>2</Other><Text>1</Text></Input>"
            } else {
                "<Input><Text>1</Text><Other>2</Other></Input>"
            };
            let input = format_xml::from_str(input_text, &actual.project.source).unwrap();
            let output = engine::run(&actual.project, &input);
            let has_call = actual
                .project
                .graph
                .nodes
                .values()
                .any(|n| matches!(n, mapping::Node::UserFunctionCall { .. }));
            let strict_refuses = mfd::import_with_profile(
                &fixture.0.join("mapping.mfd"),
                &ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )
            .is_err();
            if !actual.warnings.iter().any(|w| w.contains(PRODUCER_REFUSAL))
                || has_call
                || !strict_refuses
            {
                failures.push(format!("reverse={reverse}, route={route}, warnings={:?}, has_call={has_call}, strict_refuses={strict_refuses}, output={output:?}", actual.warnings));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "raw producer admission failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_single_raw_producer_can_fan_out_to_multiple_library_arguments() {
    let fixture = Fixture::new();
    let parameter = r#"<component name="unused" library="core" kind="6" uid="102"><targets><datapoint key="1002"/></targets><data><input datatype="string"/></data></component>"#;
    fixture.write(
        "helpers.mfd",
        &format!(
            "<mapping>{}</mapping>",
            IDENTITY.replace("</children>", &format!("{parameter}</children>"))
        ),
    );
    let main = MAIN.replace(r#"<entry name="value" inpkey="20" componentid="100"/>"#, r#"<entry name="value" inpkey="20" componentid="100"/><entry name="unused" inpkey="22" componentid="102"/>"#)
        .replace(r#"<edge vertexkey="20"/>"#, r#"<edge vertexkey="20"/><edge vertexkey="22"/>"#);
    fixture.write("mapping.mfd", &main);
    let actual = imported(&fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    let input = format_xml::from_str(
        "<Input><Text>fan-out</Text></Input>",
        &actual.project.source,
    )
    .unwrap();
    let output = engine::run(&actual.project, &input).unwrap();
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("fan-out".into()))
    );
}

fn replace_call(main: &str, replacement: &str) -> String {
    let document = roxmltree::Document::parse(main).unwrap();
    let range = document
        .descendants()
        .find(|n| {
            n.has_tag_name("component")
                && n.attribute("library") == Some("generic")
                && n.attribute("uid") == Some("2")
        })
        .unwrap()
        .range();
    let mut text = main.to_string();
    text.replace_range(range, replacement);
    text
}

#[test]
fn declared_kind19_numeric_spellings_refuse_before_schema_builtin_or_module_dispatch() {
    let mut failures = Vec::new();
    for kind in ["19", "019", "+19"] {
        for owner in ["xml", "core", "unsupported-module"] {
            let fixture = Fixture::new();
            let probe_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Probe"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
            fixture.write("probe.xsd", probe_schema);
            // An original inert module sentinel: no vendor source or execution.
            fixture.write("unsupported-module.xsl", "<not-a-callable-module/>");
            let call = match owner {
                "xml" => format!(
                    r#"<component name="Probe" library="xml" kind="{kind}" uid="2"><data><root><entry name="Probe"><entry name="Value" outkey="21"/></entry></root><document schema="probe.xsd" instanceroot="{{}}Probe"/></data></component>"#
                ),
                _ => format!(
                    r#"<component name="uppercase" library="{owner}" kind="{kind}" uid="2"><sources><datapoint key="20"/></sources><targets><datapoint key="21"/></targets></component>"#
                ),
            };
            fixture.write("mapping.mfd", &replace_call(MAIN, &call));
            let readset = ResourceReadSetV1::default();
            let binding = ResourceBindingV1::new(
                ResourceBindingKindV1::XmlSchema,
                fixture.0.join("mapping.mfd"),
                "probe.xsd",
                &fixture.0,
                "probe.xsd",
                Sha256::digest(probe_schema.as_bytes()).into(),
            )
            .unwrap();
            let options = ImportOptions::default()
                .with_resource_bindings_v1([binding])
                .unwrap()
                .with_resource_readset_v1(readset.clone());
            let actual =
                mfd::import_with_options(&fixture.0.join("mapping.mfd"), &options).unwrap();
            let strict_refuses = mfd::import_with_profile(
                &fixture.0.join("mapping.mfd"),
                &options,
                mfd::ImportProfile::Executable,
            )
            .is_err();
            let schema_reads = readset
                .records()
                .iter()
                .filter(|r| r.kind == ResourceBindingKindV1::XmlSchema)
                .count();
            let boundary_refusal = actual.warnings.iter().any(|w| {
                if kind != "19" {
                    w.contains("noncanonical kind")
                        && w.contains("refused before built-in/schema/HTTP/module dispatch")
                } else if owner == "unsupported-module" {
                    w.contains("no module discovery or name fallback is permitted")
                } else {
                    w.contains("reserved owner")
                        && w.contains("refused before built-in/schema/HTTP dispatch")
                }
            });
            let builtin_lowered =
                actual.project.graph.nodes.values().any(
                    |n| matches!(n, mapping::Node::Call { function, .. } if function == "upper"),
                );
            if !boundary_refusal || schema_reads != 0 || builtin_lowered || !strict_refuses {
                failures.push(format!("kind={kind}, owner={owner}, warnings={:?}, schema_reads={schema_reads}, builtin_lowered={builtin_lowered}, strict_refuses={strict_refuses}", actual.warnings));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "numeric-kind boundary failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn other_numeric_kinds_and_ordinary_profiles_keep_their_existing_admission() {
    for declared in [false, true] {
        let fixture = Fixture::new();
        let main = MAIN.replacen(r#"kind="14""#, r#"kind="014""#, 1);
        fixture.write(
            "mapping.mfd",
            &if declared {
                main
            } else {
                embedded(&main, IDENTITY)
            },
        );
        let actual = imported(&fixture);
        assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
        let input = format_xml::from_str(
            "<Input><Text>unchanged</Text></Input>",
            &actual.project.source,
        )
        .unwrap();
        assert_eq!(
            engine::run(&actual.project, &input)
                .unwrap()
                .field("Result")
                .and_then(Instance::as_scalar),
            Some(&Value::String("unchanged".into()))
        );
    }
    // This is profile-local, not a global rewrite of the permissive builtin
    // reader's existing numeric-kind behavior.
    for kind in ["19", "019", "+19"] {
        let fixture = Fixture::new();
        let call = format!(
            r#"<component name="uppercase" library="core" kind="{kind}" uid="2"><sources><datapoint key="20"/></sources><targets><datapoint key="21"/></targets></component>"#
        );
        fixture.write(
            "mapping.mfd",
            &embedded(&replace_call(MAIN, &call), IDENTITY),
        );
        let actual = imported(&fixture);
        assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
        let input = format_xml::from_str(
            "<Input><Text>unchanged</Text></Input>",
            &actual.project.source,
        )
        .unwrap();
        assert_eq!(
            engine::run(&actual.project, &input)
                .unwrap()
                .field("Result")
                .and_then(Instance::as_scalar),
            Some(&Value::String("UNCHANGED".into()))
        );
    }
}

#[test]
fn duplicate_function_output_and_source_owners_have_no_priority() {
    let constant = r#"<component library="core" name="constant" kind="2" uid="4"><targets><datapoint key="10"/></targets><data><constant value="not-an-owner-choice" datatype="string"/></data></component>"#;
    for first in [false, true] {
        let fixture = Fixture::new();
        let main = if first {
            MAIN.replace(
                "<structure><children>",
                &format!("<structure><children>{constant}"),
            )
        } else {
            MAIN.replace("</children>", &format!("{constant}</children>"))
        };
        fixture.write("mapping.mfd", &main);
        refused(&fixture, PRODUCER_REFUSAL);
    }
}

#[test]
fn missing_argument_producer_cannot_use_a_schema_or_name_fallback() {
    let fixture = Fixture::new();
    fixture.write(
        "mapping.mfd",
        &MAIN.replace(r#"<vertex vertexkey="10">"#, r#"<vertex vertexkey="999">"#),
    );
    refused(
        &fixture,
        "imported argument feed `999` has no raw producer declaration; schema/name fallback is not permitted",
    );
}

#[test]
fn numeric_raw_producer_aliases_do_not_hide_duplicate_ownership() {
    for reverse in [false, true] {
        for alias in ["010", "+10"] {
            let fixture = Fixture::new();
            let main = source_fields(&fixture, reverse).replace(
                r#"<entry name="Other" outkey="10"/>"#,
                &format!(r#"<entry name="Other" outkey="{alias}"/>"#),
            );
            fixture.write("mapping.mfd", &main);
            refused(&fixture, PRODUCER_REFUSAL);
        }
    }
}

#[test]
fn unsupported_library_format_does_not_restore_kind19_module_fallback() {
    for kind in ["19", "019", "+19"] {
        let fixture = Fixture::new();
        let call = format!(
            r#"<component name="uppercase" library="unsupported-module" kind="{kind}" uid="2"><sources><datapoint key="20"/></sources><targets><datapoint key="21"/></targets></component>"#
        );
        fixture.write("unsupported-module.xsl", "<not-a-callable-module/>");
        fixture.write(
            "mapping.mfd",
            &replace_call(MAIN, &call).replace("helpers.mfd", "helpers.mff"),
        );
        let actual = imported(&fixture);
        assert!(
            actual.warnings.iter().any(|w| w.contains("bounded .mfd")),
            "{:?}",
            actual.warnings
        );
        refused(
            &fixture,
            if kind == "19" {
                "no module discovery or name fallback is permitted"
            } else {
                "noncanonical kind"
            },
        );
    }
}

#[test]
fn other_builtin_numeric_kinds_remain_admitted_in_both_profiles() {
    for kind in ["5", "005", "+5"] {
        for declared in [false, true] {
            let fixture = Fixture::new();
            let main = through_uppercase(MAIN).replace(r#"kind="5""#, &format!(r#"kind="{kind}""#));
            fixture.write(
                "mapping.mfd",
                &if declared {
                    main
                } else {
                    embedded(&main, IDENTITY)
                },
            );
            let actual = imported(&fixture);
            assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
            let input = format_xml::from_str(
                "<Input><Text>unchanged</Text></Input>",
                &actual.project.source,
            )
            .unwrap();
            assert_eq!(
                engine::run(&actual.project, &input)
                    .unwrap()
                    .field("Result")
                    .and_then(Instance::as_scalar),
                Some(&Value::String("UNCHANGED".into()))
            );
        }
    }
}
