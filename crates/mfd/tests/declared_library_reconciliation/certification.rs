use super::*;
use mfd::{ImportOptions, ResourceBindingKindV1, ResourceBindingV1, ResourceReadSetV1};
use sha2::{Digest, Sha256};

const CATALOG_SCHEMA: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;

fn lookup(key: u32) -> String {
    format!(
        r#"<component name="UnusedLookup" library="local"><structure><children>
<component name="needle" library="core" kind="6" uid="901"><targets><datapoint key="501"/></targets><data><input datatype="string"/></data></component>
<component name="result" library="core" kind="7" uid="902"><sources><datapoint key="500"/></sources><data><output datatype="string"/></data></component>
<component name="equal" library="core" kind="5" uid="903"><sources><datapoint pos="0" key="502"/><datapoint pos="1" key="503"/></sources><targets><datapoint key="504"/></targets></component>
<component name="Catalog" library="xml" kind="14" uid="904"><properties ParameterName="catalog"/><data><root><entry name="Catalog"><entry name="Item"><entry name="Key" outkey="{key}"/><entry name="Value" outkey="509"/></entry></entry></root><document schema="catalog.xsd" inputinstance="not-loaded.xml" instanceroot="{{}}Catalog"/></data></component>
<component name="filter" library="core" kind="3" uid="905"><sources><datapoint pos="0" key="505"/><datapoint pos="1" key="506"/></sources><targets><datapoint pos="0" key="507"/><datapoint pos="1"/></targets></component>
</children><graph><vertices><vertex vertexkey="501"><edges><edge vertexkey="502"/></edges></vertex><vertex vertexkey="{key}"><edges><edge vertexkey="503"/></edges></vertex><vertex vertexkey="504"><edges><edge vertexkey="506"/></edges></vertex><vertex vertexkey="509"><edges><edge vertexkey="505"/></edges></vertex><vertex vertexkey="507"><edges><edge vertexkey="500"/></edges></vertex></vertices></graph></structure></component>"#
    )
}

fn constant_main(first: bool, literal: &str) -> String {
    let main = MAIN.replace(
        r#"<entry name="Text" outkey="10"/>"#,
        r#"<entry name="Text" outkey="12"/>"#,
    );
    let constant = format!(
        r#"<component name="constant" library="core" kind="2" uid="5"><targets><datapoint key="10"/></targets><data><constant value="{literal}" datatype="string"/></data></component>"#
    );
    if first {
        main.replace(
            "<structure><children>",
            &format!("<structure><children>{constant}"),
        )
    } else {
        main.replace("</children>", &format!("{constant}</children>"))
    }
}

fn with_definition(main: &str, definition: &str) -> String {
    main.replace("</mapping>", &format!("{definition}</mapping>"))
}

fn hard_refusal(fixture: &Fixture, main: &str, role: &str, key: u32) -> String {
    fixture.write("mapping.mfd", main);
    fixture.write("published.xml", "unchanged");
    let error = match mfd::import(&fixture.0.join("mapping.mfd")) {
        Err(mfd::MfdError::UnsupportedImport(error)) => error,
        Err(error) => panic!("unexpected error: {error:?}"),
        Ok(imported) => panic!("certificate published a project: {:?}", imported.warnings),
    };
    assert!(
        error.contains("post-normalization ownership certificate")
            && error.contains(&format!("{role} `{key}`")),
        "{error}"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("published.xml")).unwrap(),
        "unchanged"
    );
    error
}

#[test]
fn promoted_catalog_cannot_shadow_arguments_in_any_main_order_or_value() {
    for first in [false, true] {
        for preview in [false, true] {
            for route in ["direct", "uppercase", "unused"] {
                for literal in ["constant-choice", "catalog-choice"] {
                    let fixture = Fixture::new();
                    fixture.write("catalog.xsd", CATALOG_SCHEMA);
                    let mut main = constant_main(first, literal);
                    if preview {
                        main = main.replace(
                            "schema=\"in.xsd\"",
                            "schema=\"in.xsd\" inputinstance=\"not-loaded-input.xml\"",
                        );
                    }
                    if route == "uppercase" {
                        let function = r#"<component name="uppercase" library="core" kind="5" uid="6"><sources><datapoint key="40"/></sources><targets><datapoint key="41"/></targets></component>"#;
                        main = main.replace("</children>", &format!("{function}</children>"))
                            .replace("<edge vertexkey=\"20\"/>", "<edge vertexkey=\"40\"/>")
                            .replace("</vertices>", "<vertex vertexkey=\"41\"><edges><edge vertexkey=\"20\"/></edges></vertex></vertices>");
                    }
                    if route == "unused" {
                        let body = r#"<component name="constant" library="core" kind="2" uid="102"><targets><datapoint key="1002"/></targets><data><constant value="kept" datatype="string"/></data></component>"#;
                        fixture.write(
                            "helpers.mfd",
                            &format!(
                                "<mapping>{}</mapping>",
                                IDENTITY
                                    .replace("</children>", &format!("{body}</children>"))
                                    .replace("vertexkey=\"1000\"", "vertexkey=\"1002\"")
                            ),
                        );
                    }
                    hard_refusal(
                        &fixture,
                        &with_definition(&main, &lookup(10)),
                        "argument feed",
                        10,
                    );
                }
            }
        }
    }
}

#[test]
fn normalized_declared_outputs_are_checked_even_with_clean_arguments() {
    let fixture = Fixture::new();
    fixture.write("catalog.xsd", CATALOG_SCHEMA);
    hard_refusal(
        &fixture,
        &with_definition(MAIN, &lookup(21)),
        "declared output",
        21,
    );
}

#[test]
fn a_currently_winning_main_source_is_not_an_ownership_certificate() {
    for other_primary in [false, true] {
        let fixture = Fixture::new();
        fixture.write("catalog.xsd", CATALOG_SCHEMA);
        let other = r#"<component name="Other" library="xml" uid="6" kind="14"><data><root><entry name="Input"><entry name="Text" outkey="13"/></entry></root><document schema="in.xsd" inputinstance="other.xml" instanceroot="{}Input"/></data></component>"#;
        let mut main = MAIN.replace("</children>", &format!("{other}</children>"));
        if other_primary {
            main = main.replace(
                "<mapping version=\"26\">",
                "<mapping version=\"26\" ferrule-primary-source=\"6\">",
            );
        }
        hard_refusal(
            &fixture,
            &with_definition(&main, &lookup(10)),
            "argument feed",
            10,
        );
    }
}

#[test]
fn parameter_catalog_and_nonpromoted_local_ids_stay_local() {
    let fixture = Fixture::new();
    let parameter = lookup(10)
        .replace("ParameterName=\"catalog\"", "UsageKind=\"input\"")
        .replace(" inputinstance=\"not-loaded.xml\"", "");
    fixture.write(
        "mapping.mfd",
        &with_definition(&constant_main(false, "constant-choice"), &parameter),
    );
    let actual = imported(&fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert!(actual.project.extra_sources.is_empty());
    let input = format_xml::from_str(
        "<Input><Text>ordinary</Text></Input>",
        &actual.project.source,
    )
    .unwrap();
    assert_eq!(
        engine::run(&actual.project, &input)
            .unwrap()
            .field("Result")
            .and_then(Instance::as_scalar),
        Some(&Value::String("constant-choice".into()))
    );
}

#[test]
fn unrelated_promoted_collisions_do_not_create_a_global_gate() {
    let fixture = Fixture::new();
    fixture.write("catalog.xsd", CATALOG_SCHEMA);
    let main = constant_main(false, "unused").replace(
        "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
        "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
    );
    fixture.write("mapping.mfd", &with_definition(&main, &lookup(10)));
    let actual = imported(&fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    let input = format_xml::from_str(
        "<Input><Text>independent</Text></Input>",
        &actual.project.source,
    )
    .unwrap();
    let catalog = format_xml::from_str(
        "<Catalog><Item><Key>other</Key><Value>other</Value></Item></Catalog>",
        &actual.project.extra_sources[0].schema,
    )
    .unwrap();
    assert_eq!(
        engine::run_with_sources(
            &actual.project,
            &input,
            vec![(actual.project.extra_sources[0].name.clone(), catalog)]
        )
        .unwrap()
        .field("Result")
        .and_then(Instance::as_scalar),
        Some(&Value::String("independent".into()))
    );
}

#[test]
fn source_node_rules_from_foreign_scopes_block_only_affected_keys() {
    for key in [10, 21, 999] {
        let fixture = Fixture::new();
        let local = IDENTITY
            .replace("generic", "local")
            .replace("Identity", "Unused");
        let rule = format!(
            r#"<data><root><entry name="Foreign" outkey="{key}"><outputnodefunctions><rule><default value="foreign"/></rule></outputnodefunctions></entry></root></data>"#
        );
        // The extra data belongs to the unused local function, not main scope.
        let local = local.replace("<structure>", &format!("{rule}<structure>"));
        let main = with_definition(MAIN, &local);
        if key == 999 {
            fixture.write("mapping.mfd", &main);
            assert!(imported(&fixture).warnings.is_empty());
        } else {
            hard_refusal(
                &fixture,
                &main,
                if key == 21 {
                    "declared output"
                } else {
                    "argument feed"
                },
                key,
            );
        }
    }
}

#[test]
fn certificate_performs_no_additional_resource_acquisition() {
    let fixture = Fixture::new();
    fixture.write("catalog.xsd", CATALOG_SCHEMA);
    let main = with_definition(&constant_main(false, "constant"), &lookup(10));
    fixture.write("mapping.mfd", &main);
    let readset = ResourceReadSetV1::default();
    let binding = ResourceBindingV1::new(
        ResourceBindingKindV1::XmlSchema,
        fixture.0.join("mapping.mfd"),
        "catalog.xsd",
        &fixture.0,
        "catalog.xsd",
        Sha256::digest(CATALOG_SCHEMA.as_bytes()).into(),
    )
    .unwrap();
    let options = ImportOptions::default()
        .with_resource_bindings_v1([binding])
        .unwrap()
        .with_resource_readset_v1(readset.clone());
    assert!(
        matches!(mfd::import_with_options(&fixture.0.join("mapping.mfd"), &options), Err(mfd::MfdError::UnsupportedImport(error)) if error.contains("post-normalization"))
    );
    let records = readset.records();
    assert_eq!(
        records
            .iter()
            .filter(|r| r.kind == ResourceBindingKindV1::XmlSchema)
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.kind == ResourceBindingKindV1::MappingLibrary)
            .count(),
        1
    );
    assert!(records.iter().all(|r| r.delivered));
}

#[test]
fn structured_catalog_promotion_is_also_an_unproved_competing_owner() {
    let fixture = Fixture::new();
    fixture.write("catalog.xsd", CATALOG_SCHEMA);
    fixture.write("record.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Record"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    let definition = r#"<component name="UnusedStructured" library="local"><structure><children>
<component name="needle" library="core" kind="6" uid="901"><targets><datapoint key="501"/></targets><data><input datatype="string"/></data></component>
<component name="Catalog" library="xml" kind="14" uid="902"><data><root><entry name="Catalog"><entry name="Item" outkey="600"><entry name="Key" outkey="10"/><entry name="Value" outkey="601"/></entry></entry></root><document schema="catalog.xsd" inputinstance="not-loaded.xml" instanceroot="{}Catalog"/></data></component>
<component name="Record" library="xml" kind="14" uid="903"><properties UsageKind="output"/><data><root><entry name="Record" inpkey="610"><entry name="Value" inpkey="611"/></entry></root><document schema="record.xsd" instanceroot="{}Record"/><parameter name="Record" usageKind="output"/></data></component>
<component name="equal" library="core" kind="5" uid="904"><sources><datapoint pos="0" key="620"/><datapoint pos="1" key="621"/></sources><targets><datapoint key="622"/></targets></component>
<component name="filter" library="core" kind="3" uid="905"><sources><datapoint pos="0" key="630"/><datapoint pos="1" key="631"/></sources><targets><datapoint key="632"/><datapoint/></targets></component>
</children><graph><vertices><vertex vertexkey="600"><edges><edge vertexkey="630"/></edges></vertex><vertex vertexkey="501"><edges><edge vertexkey="620"/></edges></vertex><vertex vertexkey="10"><edges><edge vertexkey="621"/></edges></vertex><vertex vertexkey="622"><edges><edge vertexkey="631"/></edges></vertex><vertex vertexkey="632"><edges><edge vertexkey="610"/></edges></vertex><vertex vertexkey="601"><edges><edge vertexkey="611"/></edges></vertex></vertices></graph></structure></component>"#;
    hard_refusal(
        &fixture,
        &with_definition(&constant_main(false, "constant"), definition),
        "argument feed",
        10,
    );
}

#[test]
fn normalized_path_projections_are_not_assumed_to_be_raw_scalar_ports() {
    let fixture = Fixture::new();
    let projected = MAIN.replace(
        r#"<entry name="Text" outkey="10"/>"#,
        r#"<entry name="Transparent" type="xml-type"><entry name="Text" outkey="10"/></entry>"#,
    );
    hard_refusal(&fixture, &projected, "argument feed", 10);
    // The existing decimal namespace-slot normalization has exact shared path
    // correspondence and needs no display-name or schema fallback.
    fixture.write(
        "mapping.mfd",
        &MAIN.replace("name=\"Text\" outkey", "name=\"0:Text\" outkey"),
    );
    let actual = imported(&fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    let input =
        format_xml::from_str("<Input><Text>exact</Text></Input>", &actual.project.source).unwrap();
    assert_eq!(
        engine::run(&actual.project, &input)
            .unwrap()
            .field("Result")
            .and_then(Instance::as_scalar),
        Some(&Value::String("exact".into()))
    );
}

#[test]
fn actual_specialized_recipe_domains_block_declared_argument_certification() {
    for domain in [
        "json-parser",
        "json-serializer",
        "xml-serializer",
        "fixed-parser",
        "external-scalar",
        "external-xslt",
        "rejected-join",
        "intermediate",
    ] {
        let fixture = Fixture::new();
        fixture.write(
            "payload.schema.json",
            r#"{"type":"object","properties":{"Value":{"type":"string"}}}"#,
        );
        fixture.write("record.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Record"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
        let producer = match domain {
            "json-parser" => {
                r#"<component name="Parser" library="json" kind="31" uid="6"><data><root><entry name="FileInstance" inpkey="40"><entry name="document"><entry name="root"><entry name="object"><entry name="Value" type="json-property"><entry name="string" outkey="10"/></entry></entry></entry></entry></entry></root><parameter usageKind="stringparse"/><json schema="payload.schema.json"/></data></component>"#
            }
            "json-serializer" => {
                r#"<component name="Serializer" library="json" kind="31" uid="6"><data><root><entry name="FileInstance" outkey="10"><entry name="document"><entry name="root"><entry name="object"><entry name="Value" type="json-property"><entry name="string" inpkey="40"/></entry></entry></entry></entry></entry></root><parameter usageKind="stringserialize"/><json schema="payload.schema.json"/></data></component>"#
            }
            "xml-serializer" => {
                r#"<component name="Serializer" library="xml" kind="14" uid="6"><data><root><entry name="Record" inpkey="40"><entry name="Value"/></entry><entry name="Serialized" outkey="10"/></root><parameter usageKind="stringserialize"/><document schema="record.xsd" instanceroot="{}Record"/></data></component>"#
            }
            "fixed-parser" => {
                r#"<component name="Parser" library="text" kind="4" uid="6"><data><root><entry name="FileInstance" inpkey="40"><entry name="document"><entry name="Record" outkey="50"><entry name="Value" outkey="10"/></entry></entry></entry></root><parameter usageKind="stringparse"/><text type="flf"><settings><names block="Record"><field0 name="Value" length="5" type="string"/></names></settings></text></data></component>"#
            }
            "external-scalar" => {
                r#"<component name="Numeric.Format" library="Owned" kind="5" uid="6"><sources><datapoint key="40"/></sources><targets><datapoint key="10"/></targets></component>"#
            }
            "external-xslt" => {
                r#"<component name="Count" library="owned-xslt" kind="5" uid="6"><sources><datapoint key="40"/></sources><targets><datapoint key="10"/></targets></component>"#
            }
            "rejected-join" => {
                r#"<component name="Join" library="core" kind="32" uid="6"><data><root><entry name="document"><entry name="tuple" outkey="10"/></entry></root></data></component>"#
            }
            _ => {
                r#"<component name="Variable" library="xml" kind="14" uid="6"><properties PassThrough="1"/><data><root><entry name="Input" inpkey="40"><entry name="Text" outkey="10"/></entry></root><document schema="in.xsd" instanceroot="{}Input"/></data></component>"#
            }
        };
        if domain == "external-scalar" {
            std::fs::create_dir(fixture.0.join("Owned")).unwrap();
            fixture.write("Owned/Numeric.cs", r#"class Numeric { static string Format(decimal value) { return value.ToString("0", System.Globalization.CultureInfo.InvariantCulture); } }"#);
        }
        fixture.write("owned-xslt.xsl", r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0"><xsl:template name="Count"><xsl:param name="items"/><xsl:value-of select="count($items/Item)"/></xsl:template></xsl:stylesheet>"#);
        let main = MAIN.replace("outkey=\"10\"", "outkey=\"12\"")
            .replace("<component name=\"main\">", "<component name=\"main\"><properties SelectedLanguage=\"cs\"/>")
            .replace("</children>", &format!("{producer}</children>"))
            .replace("</vertices>", "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"40\"/></edges></vertex></vertices>");
        let error = hard_refusal(&fixture, &main, "argument feed", 10);
        let expected = match domain {
            "json-parser" => "JSON parser",
            "json-serializer" => "JSON serializer",
            "xml-serializer" => "XML serializer",
            "fixed-parser" => "FlexText/fixed parser",
            "external-scalar" => "external scalar",
            "external-xslt" => "external XSLT",
            "rejected-join" => "join/hierarchical/refused output",
            _ => "dynamic/transparent intermediate projection",
        };
        assert!(error.contains(expected), "{domain}: {error}");
    }
}

fn rows(fixture: &Fixture, explicit: bool) -> String {
    for (file, root, field) in [("in.xsd", "Input", "Text"), ("out.xsd", "Output", "Result")] {
        fixture.write(file, &format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="{field}" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#));
    }
    let main = MAIN
        .replace(
            r#"<entry name="Text" outkey="10"/>"#,
            r#"<entry name="Row" outkey="11"><entry name="Text" outkey="10"/></entry>"#,
        )
        .replace(
            r#"<entry name="Result" inpkey="30"/>"#,
            &format!(
                r#"<entry name="Row" {}><entry name="Result" inpkey="30"/></entry>"#,
                if explicit { r#"inpkey="31""# } else { "" }
            ),
        );
    if explicit {
        main.replace(
            "</vertices>",
            r#"<vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex></vertices>"#,
        )
    } else {
        main
    }
}

#[test]
fn explicit_scope_drivers_are_in_the_same_frozen_producer_certificate() {
    let fixture = Fixture::new();
    let main = rows(&fixture, true);
    fixture.write("catalog.xsd", CATALOG_SCHEMA);
    hard_refusal(
        &fixture,
        &with_definition(&main, &lookup(11)),
        "scope driver",
        11,
    );
}

#[test]
fn unproved_implicit_receiving_scopes_refuse_even_for_constant_arguments() {
    let fixture = Fixture::new();
    let main = rows(&fixture, false).replace("outkey=\"10\"", "outkey=\"12\"");
    let constant = r#"<component name="constant" library="core" kind="2" uid="5"><targets><datapoint key="10"/></targets><data><constant value="same" datatype="string"/></data></component>"#;
    hard_refusal(
        &fixture,
        &main.replace("</children>", &format!("{constant}</children>")),
        "declared output",
        21,
    );
}

#[test]
fn db_computed_projection_is_an_actual_competing_resolver_domain() {
    let fixture = Fixture::new();
    let db = rusqlite::Connection::open(fixture.0.join("synthetic.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE A (Id INTEGER PRIMARY KEY, Cost REAL); CREATE TABLE B (Id INTEGER PRIMARY KEY, AId INTEGER, Units INTEGER, FOREIGN KEY (AId) REFERENCES A(Id)); INSERT INTO A VALUES (1, 2.5); INSERT INTO B VALUES (1, 1, 3);").unwrap();
    drop(db);
    let resources = r#"<resources><datasources><datasource name="synthetic"><database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="synthetic.sqlite" name="synthetic"><LocalViewStorage><LocalViewElement SQL="SELECT (Units * Cost) AS Total, B.Id, B.Units, A.Cost FROM B INNER JOIN A ON B.AId = A.Id WHERE B.Units &gt; 0"><PathElement Name="main" Kind="Database"/><PathElement Name="Totals" Kind="Select Statement"/></LocalViewElement></LocalViewStorage></database_connection></datasource></datasources></resources>"#;
    let producers = r#"<component name="catalog" library="db" kind="15" uid="7"><data><root><entry name="document"><entry name="Totals" type="routine" outkey="50"/></entry></root><database ref="synthetic"><data><selections><selection><PathElement Name="main" Kind="Database"/><PathElement Name="Totals" Kind="Select Statement"/></selection></selections></data></database></data></component><component name="Totals" library="db" kind="28" uid="8"><data><root><entry name="procedure" inpkey="40"/></root><root><entry name="Totals" outkey="60"><entry name="Totals"><entry name="Total" type="attribute" outkey="10"/><entry name="Id" type="attribute" outkey="61"/><entry name="Units" type="attribute" outkey="62"/><entry name="Cost" type="attribute" outkey="63"/></entry></entry></root></data></component>"#;
    let main = MAIN
        .replace(
            "<component name=\"main\">",
            &format!("{resources}<component name=\"main\">"),
        )
        .replace("outkey=\"10\"", "outkey=\"12\"")
        .replace(
            "schema=\"in.xsd\"",
            "schema=\"in.xsd\" inputinstance=\"input.xml\"",
        )
        .replace("</children>", &format!("{producers}</children>"))
        .replace(
            "</vertices>",
            "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"40\"/></edges></vertex></vertices>",
        );
    // Establish that the fixture really registers computed projection lowering
    // in the unchanged ordinary profile, rather than only a missing producer.
    fixture.write("ordinary.mfd", &embedded(&main, IDENTITY));
    let ordinary = mfd::import(&fixture.0.join("ordinary.mfd")).unwrap();
    assert!(ordinary.project.graph.nodes.values().any(|node| matches!(node, mapping::Node::Call { function, .. } if function == "sqlite_multiply")), "{:?}", ordinary.warnings);
    hard_refusal(&fixture, &main, "argument feed", 10);
}

#[test]
fn attribute_identity_is_not_inferred_from_a_coalesced_path() {
    let fixture = Fixture::new();
    fixture.write("in.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:attribute name="Text" type="xs:string"/></xs:complexType></xs:element></xs:schema>"#);
    hard_refusal(&fixture, MAIN, "argument feed", 10);
    fixture.write(
        "mapping.mfd",
        &MAIN.replace("name=\"Text\" outkey", "name=\"@Text\" outkey"),
    );
    let actual = imported(&fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    let input =
        format_xml::from_str("<Input Text=\"attribute\"/>", &actual.project.source).unwrap();
    assert_eq!(
        engine::run(&actual.project, &input)
            .unwrap()
            .field("Result")
            .and_then(Instance::as_scalar),
        Some(&Value::String("attribute".into()))
    );
}

#[test]
fn filename_driven_sources_are_not_certified_as_ordinary_scalar_readers() {
    let fixture = Fixture::new();
    let root = r#"<root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1" inpkey="40"><entry name="document" ns="1"><entry name="Input" ns="0"><entry name="Text" outkey="10"/></entry></entry></entry></root>"#;
    let constant = r#"<component name="constant" library="core" kind="2" uid="6"><targets><datapoint key="99"/></targets><data><constant value="not-loaded.xml" datatype="string"/></data></component>"#;
    let main = MAIN
        .replace(
            r#"<root><entry name="Input"><entry name="Text" outkey="10"/></entry></root>"#,
            root,
        )
        .replace("</children>", &format!("{constant}</children>"))
        .replace(
            "</vertices>",
            "<vertex vertexkey=\"99\"><edges><edge vertexkey=\"40\"/></edges></vertex></vertices>",
        );
    hard_refusal(&fixture, &main, "argument feed", 10);
}
