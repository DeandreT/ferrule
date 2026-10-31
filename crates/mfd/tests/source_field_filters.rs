use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct Fixture(PathBuf);

impl Fixture {
    fn new(compare: &str, branches: &[(u32, &str)], predicate: Option<u32>) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-source-filters-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("source.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Wanted" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
        let target_schema = branches
            .iter()
            .map(|(_, name)| {
                format!(r#"<xs:element name="{name}" type="xs:string" minOccurs="0"/>"#)
            })
            .collect::<String>();
        std::fs::write(path.join("target.xsd"), format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence>{target_schema}<xs:element name="Untouched" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#)).unwrap();
        let target_entries = branches
            .iter()
            .enumerate()
            .map(|(index, (_, name))| {
                format!(r#"<entry name="{name}" inpkey="{}"/>"#, 40 + index * 2)
            })
            .collect::<String>();
        let branch_edges = branches.iter().enumerate().map(|(index, (output, _))| format!(r#"<vertex vertexkey="{output}"><edges><edge vertexkey="{}"/></edges></vertex>"#, 40 + index * 2)).collect::<String>();
        let predicate_edge = predicate
            .map(|feed| {
                format!(
                    r#"<vertex vertexkey="{feed}"><edges><edge vertexkey="31"/></edges></vertex>"#
                )
            })
            .unwrap_or_default();
        std::fs::write(path.join("mapping.mfd"), format!(r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/><entry name="Wanted" outkey="11"/></entry></root><document schema="source.xsd" instanceroot="{{}}Source"/></data></component>
          <component name="{compare}" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
          <component name="source-selection" library="core" kind="3"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources><targets><datapoint pos="0" key="32"/><datapoint pos="1" key="33"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target">{target_entries}<entry name="Untouched" inpkey="41"/></entry></root><document schema="target.xsd" instanceroot="{{}}Target"/></data></component>
          </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="30"/><edge vertexkey="41"/></edges></vertex>
          <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
          {predicate_edge}{branch_edges}</vertices></graph></structure></component></mapping>"#)).unwrap();
        Self(path)
    }

    fn import(&self) -> mfd::Imported {
        mfd::import(&self.0.join("mapping.mfd")).unwrap()
    }

    fn rewrite(&self, from: &str, to: &str) {
        let path = self.0.join("mapping.mfd");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "missing replacement: {from}");
        std::fs::write(path, text.replace(from, to)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn execute(imported: &mfd::Imported, wanted: &str) -> Instance {
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    let input = format_xml::from_str(
        &format!("<Source><Value>alpha</Value><Wanted>{wanted}</Wanted></Source>"),
        &imported.project.source,
    )
    .unwrap();
    engine::run(&imported.project, &input).unwrap()
}

fn field<'a>(output: &'a Instance, name: &str) -> Option<&'a Value> {
    output.field(name).and_then(Instance::as_scalar)
}

#[test]
fn singleton_filter_branches_preserve_comparison_and_target_order() {
    for compare in ["equal", "not-equal"] {
        for branches in [
            vec![(32, "Selected")],
            vec![(33, "Rejected")],
            vec![(32, "Selected"), (33, "Rejected")],
            vec![(33, "Rejected"), (32, "Selected")],
        ] {
            let fixture = Fixture::new(compare, &branches, Some(22));
            let imported = fixture.import();
            assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
            for wanted in ["alpha", "beta"] {
                let output = execute(&imported, wanted);
                let selected = (wanted == "alpha") == (compare == "equal");
                for (branch, name) in &branches {
                    let expected = if selected == (*branch == 32) {
                        Value::String("alpha".into())
                    } else {
                        Value::Null
                    };
                    assert_eq!(
                        field(&output, name),
                        Some(&expected),
                        "{compare}, {wanted}, {branches:?}, {output:?}"
                    );
                }
                assert_eq!(
                    field(&output, "Untouched"),
                    Some(&Value::String("alpha".into()))
                );
            }
        }
    }
}

#[test]
fn unsupported_predicates_skip_only_filtered_bindings_and_refuse_executable_import() {
    for (compare, predicate) in [
        ("unrecognized-predicate", Some(22)),
        ("equal", Some(11)),
        ("equal", None),
        ("equal", Some(32)),
    ] {
        let fixture = Fixture::new(compare, &[(32, "Selected"), (33, "Rejected")], predicate);
        let imported = fixture.import();
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("filter `source-selection`")
                    && warning.contains("affected binding skipped")),
            "{:?}",
            imported.warnings
        );
        let output = execute(&imported, "alpha");
        assert_eq!(field(&output, "Selected"), None, "{output:?}");
        assert_eq!(field(&output, "Rejected"), None, "{output:?}");
        assert_eq!(
            field(&output, "Untouched"),
            Some(&Value::String("alpha".into()))
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
}

#[test]
fn singleton_filter_survives_transparent_variable_descendant_projection() {
    let fixture = Fixture::new("equal", &[(32, "Selected")], Some(22));
    fixture.rewrite(
        "<entry name=\"Source\"><entry name=\"Value\"",
        "<entry name=\"Source\" outkey=\"12\"><entry name=\"Value\"",
    );
    fixture.rewrite("<edge vertexkey=\"30\"/>", "");
    fixture.rewrite("</children>", r#"<component name="variable" library="xml" kind="14"><data><root><entry name="Source" inpkey="50" outkey="51"><entry name="Value" outkey="52"/><entry name="Wanted" outkey="53"/></entry></root><parameter usageKind="variable"><root><entry name="Source"/></root></parameter><document schema="source.xsd" instanceroot="{}Source"/></data></component></children>"#);
    fixture.rewrite(r#"<vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex>"#, r#"<vertex vertexkey="12"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="50"/></edges></vertex><vertex vertexkey="52"><edges><edge vertexkey="40"/></edges></vertex>"#);
    let imported = fixture.import();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    for wanted in ["alpha", "beta"] {
        let output = execute(&imported, wanted);
        let expected = if wanted == "alpha" {
            Value::String("alpha".into())
        } else {
            Value::Null
        };
        assert_eq!(field(&output, "Selected"), Some(&expected), "{output:?}");
    }
}

#[test]
fn named_collection_lookup_keeps_complementary_outputs_separate() {
    for reverse in [false, true] {
        let branches = if reverse {
            vec![(33, "Rejected"), (32, "Selected")]
        } else {
            vec![(32, "Selected"), (33, "Rejected")]
        };
        let fixture = Fixture::new("equal", &branches, Some(22));
        let source_schema = std::fs::read_to_string(fixture.0.join("source.xsd")).unwrap();
        std::fs::write(fixture.0.join("source.xsd"), source_schema.replace(
            r#"<xs:element name="Value" type="xs:string"/>"#,
            r#"<xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element>"#,
        )).unwrap();
        fixture.rewrite(
            r#"<entry name="Value" outkey="10"/>"#,
            r#"<entry name="Row"><entry name="Value" outkey="10"/></entry>"#,
        );
        // Keep the unrelated binding singleton, outside the selected collection.
        fixture.rewrite(r#"<edge vertexkey="41"/>"#, "");
        fixture.rewrite(
            r#"<edge vertexkey="21"/>"#,
            r#"<edge vertexkey="21"/><edge vertexkey="41"/>"#,
        );
        let imported = fixture.import();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let input = format_xml::from_str("<Source><Row><Value>beta</Value></Row><Row><Value>alpha</Value></Row><Wanted>alpha</Wanted></Source>", &imported.project.source).unwrap();
        let output = engine::run(&imported.project, &input).unwrap();
        assert_eq!(
            field(&output, "Selected"),
            Some(&Value::String("alpha".into()))
        );
        assert_eq!(
            field(&output, "Rejected"),
            Some(&Value::String("beta".into()))
        );
        assert!(
            imported
                .project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, mapping::Node::Lookup { .. }))
        );
        assert!(
            imported
                .project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, mapping::Node::CollectionFind { .. }))
        );
    }
}

#[test]
fn transposed_source_filter_is_refused_instead_of_reading_unfiltered_field() {
    let fixture = Fixture::new("equal", &[(32, "Selected")], Some(22));
    let design = include_str!("fixtures/xlsx-transposed.mfd")
        .replace(r#"<edge vertexkey="201"/>"#, r#"<edge vertexkey="20"/><edge vertexkey="30"/>"#)
        .replace("</children>", r#"
          <component name="constant" library="core" kind="2"><targets><datapoint key="24"/></targets><data><constant value="excluded" datatype="string"/></data></component>
          <component name="not-equal" library="core" kind="5"><sources><datapoint key="20"/><datapoint key="21"/></sources><targets><datapoint key="22"/></targets></component>
          <component name="source-selection" library="core" kind="3"><sources><datapoint key="30"/><datapoint key="31"/></sources><targets><datapoint key="32"/><datapoint key="33"/></targets></component>
          </children>"#)
        .replace("<vertices>", r#"<vertices>
          <vertex vertexkey="24"><edges><edge vertexkey="21"/></edges></vertex>
          <vertex vertexkey="22"><edges><edge vertexkey="31"/></edges></vertex>
          <vertex vertexkey="32"><edges><edge vertexkey="201"/></edges></vertex>"#);
    std::fs::write(fixture.0.join("mapping.mfd"), design).unwrap();
    let imported = fixture.import();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("filter `source-selection`")
                && warning.contains("affected binding skipped")),
        "{:?}",
        imported.warnings
    );
    assert!(
        !imported
            .project
            .root
            .bindings
            .iter()
            .any(|binding| binding.target_field == "Category")
    );
    assert!(
        imported
            .project
            .root
            .bindings
            .iter()
            .any(|binding| binding.target_field == "Amount")
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
fn scope_owned_filter_preserves_transparent_variable_fields() {
    let fixture = Fixture::new("equal", &[(32, "Selected")], Some(22));
    let row = r#"<xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element>"#;
    let source_schema = std::fs::read_to_string(fixture.0.join("source.xsd")).unwrap();
    std::fs::write(
        fixture.0.join("source.xsd"),
        source_schema.replace(r#"<xs:element name="Value" type="xs:string"/>"#, row),
    )
    .unwrap();
    let target_schema = std::fs::read_to_string(fixture.0.join("target.xsd")).unwrap();
    std::fs::write(fixture.0.join("target.xsd"), target_schema.replace(r#"<xs:element name="Selected" type="xs:string" minOccurs="0"/>"#, r#"<xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Selected" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element>"#)).unwrap();
    std::fs::write(
        fixture.0.join("row.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{}</xs:schema>"#,
            row.replace(" maxOccurs=\"unbounded\"", "")
        ),
    )
    .unwrap();
    fixture.rewrite(
        r#"<entry name="Value" outkey="10"/>"#,
        r#"<entry name="Row" outkey="12"><entry name="Value" outkey="10"/></entry>"#,
    );
    fixture.rewrite(r#"<edge vertexkey="30"/>"#, "");
    fixture.rewrite(r#"<edge vertexkey="41"/>"#, "");
    fixture.rewrite(
        r#"<edge vertexkey="21"/>"#,
        r#"<edge vertexkey="21"/><edge vertexkey="41"/>"#,
    );
    fixture.rewrite(
        r#"<entry name="Selected" inpkey="40"/>"#,
        r#"<entry name="Row" inpkey="44"><entry name="Selected" inpkey="40"/></entry>"#,
    );
    fixture.rewrite("</children>", r#"<component name="variable" library="xml" kind="14"><data><root><entry name="Row" inpkey="50" outkey="51"><entry name="Value" outkey="52"/></entry></root><parameter usageKind="variable"><root><entry name="Row"/></root></parameter><document schema="row.xsd" instanceroot="{}Row"/></data></component></children>"#);
    fixture.rewrite(r#"<vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex>"#, r#"<vertex vertexkey="12"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="50"/></edges></vertex><vertex vertexkey="51"><edges><edge vertexkey="44"/></edges></vertex><vertex vertexkey="52"><edges><edge vertexkey="40"/></edges></vertex>"#);
    let imported = fixture.import();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let input = format_xml::from_str("<Source><Row><Value>alpha</Value></Row><Row><Value>beta</Value></Row><Wanted>alpha</Wanted></Source>", &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 1, "{output:?}");
    assert_eq!(
        field(&rows[0], "Selected"),
        Some(&Value::String("alpha".into()))
    );
}

fn value_map(input: u32, output: u32) -> String {
    format!(
        r#"<component name="value-map" library="core" kind="5"><sources><datapoint key="{input}"/></sources><targets><datapoint key="{output}"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable/><input name="input" type="String"/><result name="result" type="String" defaultValue="fallback"/></valuemap></data></component>"#
    )
}

const NORMALIZE_DEFINITION: &str = r#"<component name="Normalize" library="helpers" inline="1"><structure><children>
  <component name="Value" library="core" uid="100" kind="6"><targets><datapoint key="1000"/></targets><data><input datatype="string"/></data></component>
  <component name="normalize-space" library="core" uid="101" kind="5"><sources><datapoint key="1001"/></sources><targets><datapoint key="1002"/></targets></component>
  <component name="Result" library="core" uid="102" kind="7"><sources><datapoint key="1003"/></sources><data><output datatype="string"/></data></component>
  </children><graph><vertices><vertex vertexkey="1000"><edges><edge vertexkey="1001"/></edges></vertex><vertex vertexkey="1002"><edges><edge vertexkey="1003"/></edges></vertex></vertices></graph></structure></component>"#;

fn consumer(kind: &str, input: u32, output: u32) -> String {
    match kind {
        "udf" => format!(
            r#"<component name="Normalize" library="helpers" kind="19"><data><root><entry name="Value" inpkey="{input}" componentid="100"/></root><root rootindex="1"><entry name="Result" outkey="{output}" componentid="102"/></root></data></component>"#
        ),
        "parser" => format!(
            r#"<component name="payload" library="json" kind="31"><data><root><entry name="FileInstance" inpkey="{input}"><entry name="document"><entry name="root"><entry name="object"><entry name="text" type="json-property"><entry name="string" outkey="{output}"/></entry></entry></entry></entry></entry></root><parameter usageKind="stringparse"/><json schema="payload.schema.json"/></data></component>"#
        ),
        "serializer" => format!(
            r#"<component name="payload" library="json" kind="31"><data><root><entry name="FileInstance" outkey="{output}"><entry name="document"><entry name="root"><entry name="object"><entry name="text" type="json-property"><entry name="string" inpkey="{input}"/></entry></entry></entry></entry></entry></root><parameter usageKind="stringserialize"/><json schema="payload.schema.json"/></data></component>"#
        ),
        _ => panic!("unknown consumer"),
    }
}

fn write_parser_schema(fixture: &Fixture) {
    std::fs::write(
        fixture.0.join("payload.schema.json"),
        r#"{"type":"object","properties":{"text":{"type":"string"}}}"#,
    )
    .unwrap();
}

#[test]
fn refused_filter_never_becomes_a_default_through_nested_consumers() {
    for kind in ["map", "nested-map", "call", "udf", "parser", "serializer"] {
        let fixture = Fixture::new("equal", &[(32, "Selected")], None);
        write_parser_schema(&fixture);
        let mut components = value_map(70, 71);
        let (middle, tail) = match kind {
            "map" => (String::new(), 71),
            "nested-map" => {
                components.push_str(&value_map(72, 73));
                (
                    r#"<vertex vertexkey="71"><edges><edge vertexkey="72"/></edges></vertex>"#
                        .into(),
                    73,
                )
            }
            "call" => {
                components.push_str(r#"<component name="uppercase" library="lang" kind="5"><sources><datapoint key="72"/></sources><targets><datapoint key="73"/></targets></component>"#);
                (
                    r#"<vertex vertexkey="71"><edges><edge vertexkey="72"/></edges></vertex>"#
                        .into(),
                    73,
                )
            }
            kind => {
                components.push_str(&consumer(kind, 72, 73));
                components.push_str(&value_map(74, 75));
                (r#"<vertex vertexkey="71"><edges><edge vertexkey="72"/></edges></vertex><vertex vertexkey="73"><edges><edge vertexkey="74"/></edges></vertex>"#.into(), 75)
            }
        };
        fixture.rewrite("</children>", &format!("{components}</children>"));
        fixture.rewrite("</mapping>", &format!("{NORMALIZE_DEFINITION}</mapping>"));
        fixture.rewrite(r#"<vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex>"#, &format!(r#"<vertex vertexkey="32"><edges><edge vertexkey="70"/></edges></vertex>{middle}<vertex vertexkey="{tail}"><edges><edge vertexkey="40"/></edges></vertex>"#));
        let imported = fixture.import();
        let output = execute(&imported, "alpha");
        assert_eq!(field(&output, "Selected"), None, "{kind}: {output:?}");
        assert_eq!(
            field(&output, "Untouched"),
            Some(&Value::String("alpha".into()))
        );
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("affected binding skipped")),
            "{kind}: {:?}",
            imported.warnings
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
    // A successfully lowered filter's absent result remains an ordinary Null:
    // defaults are still valid when there was no import refusal.
    let fixture = Fixture::new("equal", &[(32, "Selected")], Some(22));
    fixture.rewrite("</children>", &format!("{}</children>", value_map(70, 71)));
    fixture.rewrite(r#"<vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex>"#, r#"<vertex vertexkey="32"><edges><edge vertexkey="70"/></edges></vertex><vertex vertexkey="71"><edges><edge vertexkey="40"/></edges></vertex>"#);
    let imported = fixture.import();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        field(&execute(&imported, "beta"), "Selected"),
        Some(&Value::String("fallback".into()))
    );
}

#[test]
fn cyclic_filter_predicate_cannot_be_replaced_with_a_boolean_default() {
    let fixture = Fixture::new("equal", &[(32, "Selected")], None);
    fixture.rewrite("</children>", r#"<component name="value-map" library="core" kind="5"><sources><datapoint key="70"/></sources><targets><datapoint key="71"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable/><input name="input" type="String"/><result name="result" type="boolean" defaultValue="true"/></valuemap></data></component></children>"#);
    fixture.rewrite("<vertices>", r#"<vertices><vertex vertexkey="32"><edges><edge vertexkey="70"/></edges></vertex><vertex vertexkey="71"><edges><edge vertexkey="31"/></edges></vertex>"#);
    let imported = fixture.import();
    let output = execute(&imported, "alpha");
    assert_eq!(field(&output, "Selected"), None);
    assert_eq!(
        field(&output, "Untouched"),
        Some(&Value::String("alpha".into()))
    );
    assert!(!imported.warnings.is_empty());
}

#[test]
fn structural_serializer_cannot_bypass_a_filter_before_a_default() {
    let fixture = Fixture::new("equal", &[(32, "Selected")], None);
    fixture.rewrite(
        "<entry name=\"Source\"><entry name=\"Value\"",
        "<entry name=\"Source\" outkey=\"12\"><entry name=\"Value\"",
    );
    fixture.rewrite(r#"<edge vertexkey="30"/>"#, "");
    let serializer = r#"<component name="serialize" library="xml" kind="14"><data><root><entry name="FileInstance" outkey="71"><entry name="document"><entry name="Source" inpkey="70"><entry name="Value"/><entry name="Wanted"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{}Source"/><parameter usageKind="stringserialize"/></data></component>"#;
    fixture.rewrite(
        "</children>",
        &format!("{serializer}{}</children>", value_map(72, 73)),
    );
    fixture.rewrite(r#"<vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex>"#, r#"<vertex vertexkey="12"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="70"/></edges></vertex><vertex vertexkey="71"><edges><edge vertexkey="72"/></edges></vertex><vertex vertexkey="73"><edges><edge vertexkey="40"/></edges></vertex>"#);
    let imported = fixture.import();
    let output = execute(&imported, "alpha");
    assert_eq!(field(&output, "Selected"), None);
    assert_eq!(
        field(&output, "Untouched"),
        Some(&Value::String("alpha".into()))
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("filtered sequence")
                && warning.contains("affected binding skipped")),
        "{:?}",
        imported.warnings
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
fn filtered_variable_udf_and_parser_inputs_refuse_without_repeating_first_match() {
    for kind in ["udf", "parser", "serializer"] {
        let fixture = Fixture::new("contains", &[(32, "Selected")], Some(22));
        write_parser_schema(&fixture);
        let row = r#"<xs:element name="Rows" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element>"#;
        let source = std::fs::read_to_string(fixture.0.join("source.xsd")).unwrap();
        std::fs::write(
            fixture.0.join("source.xsd"),
            source.replace(r#"<xs:element name="Value" type="xs:string"/>"#, row),
        )
        .unwrap();
        std::fs::write(
            fixture.0.join("row.xsd"),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{}</xs:schema>"#,
                row.replace(" maxOccurs=\"unbounded\"", "")
            ),
        )
        .unwrap();
        std::fs::write(fixture.0.join("target.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Included" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/><xs:element name="Echo" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Excluded" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/><xs:element name="Echo" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Untouched" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
        fixture.rewrite(
            r#"<entry name="Value" outkey="10"/>"#,
            r#"<entry name="Rows" outkey="12"><entry name="Value" outkey="10"/></entry>"#,
        );
        fixture.rewrite(r#"<entry name="Selected" inpkey="40"/>"#, r#"<entry name="Included" inpkey="40"><entry name="Value" inpkey="42"/><entry name="Echo" inpkey="48"/></entry><entry name="Excluded" inpkey="44"><entry name="Value" inpkey="46"/><entry name="Echo" inpkey="49"/></entry>"#);
        fixture.rewrite(
            r#"<edge vertexkey="20"/>"#,
            r#"<edge vertexkey="20"/><edge vertexkey="48"/><edge vertexkey="49"/>"#,
        );
        fixture.rewrite(r#"<edge vertexkey="30"/>"#, "");
        fixture.rewrite(r#"<edge vertexkey="41"/>"#, "");
        fixture.rewrite(
            r#"<edge vertexkey="21"/>"#,
            r#"<edge vertexkey="21"/><edge vertexkey="41"/>"#,
        );
        let mut components = String::new();
        for (input, root, field) in [(50, 51, 52), (60, 61, 62)] {
            components.push_str(&format!(r#"<component name="variable" library="xml" kind="14"><data><root><entry name="Rows" inpkey="{input}" outkey="{root}"><entry name="Value" outkey="{field}"/></entry></root><parameter usageKind="variable"><root><entry name="Rows"/></root></parameter><document schema="row.xsd" instanceroot="{{}}Rows"/></data></component>"#));
        }
        components.push_str(&consumer(kind, 70, 71));
        components.push_str(&consumer(kind, 80, 81));
        // A default after the consumer must not turn its refusal into output.
        components.push_str(&value_map(90, 91));
        components.push_str(&value_map(92, 93));
        fixture.rewrite("</children>", &format!("{components}</children>"));
        fixture.rewrite("</mapping>", &format!("{NORMALIZE_DEFINITION}</mapping>"));
        fixture.rewrite(r#"<vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex>"#, r#"<vertex vertexkey="12"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="50"/></edges></vertex><vertex vertexkey="33"><edges><edge vertexkey="60"/></edges></vertex><vertex vertexkey="51"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="61"><edges><edge vertexkey="44"/></edges></vertex><vertex vertexkey="52"><edges><edge vertexkey="70"/></edges></vertex><vertex vertexkey="62"><edges><edge vertexkey="80"/></edges></vertex><vertex vertexkey="71"><edges><edge vertexkey="90"/></edges></vertex><vertex vertexkey="81"><edges><edge vertexkey="92"/></edges></vertex><vertex vertexkey="91"><edges><edge vertexkey="42"/></edges></vertex><vertex vertexkey="93"><edges><edge vertexkey="46"/></edges></vertex>"#);
        let imported = fixture.import();
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("filtered variable descendant")
                    && warning.contains("affected binding skipped")),
            "{kind}: {:?}",
            imported.warnings
        );
        let input = format_xml::from_str(r#"<Source><Rows><Value>{"text":"alpha"}</Value></Rows><Rows><Value>{"text":"skip1"}</Value></Rows><Rows><Value>{"text":"beta"}</Value></Rows><Rows><Value>{"text":"skip2"}</Value></Rows><Wanted>skip</Wanted></Source>"#, &imported.project.source).unwrap();
        let output = engine::run(&imported.project, &input).unwrap();
        for (name, count) in [("Included", 2), ("Excluded", 2)] {
            let rows = output.field(name).and_then(Instance::as_repeated).unwrap();
            assert_eq!(rows.len(), count, "{kind}: {output:?}");
            assert!(
                rows.iter().all(|row| row.field("Value").is_none()),
                "{kind}: {output:?}"
            );
            let expected = if name == "Included" {
                vec!["skip1", "skip2"]
            } else {
                vec!["alpha", "beta"]
            };
            for (row, expected) in rows.iter().zip(expected) {
                assert_eq!(
                    field(row, "Echo"),
                    Some(&Value::String(format!(r#"{{"text":"{expected}"}}"#)))
                );
            }
        }
        assert_eq!(
            field(&output, "Untouched"),
            Some(&Value::String("skip".into()))
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
}

#[test]
fn collection_position_filters_admit_only_the_actual_collection() {
    for position_source in [12, 13, 0] {
        let fixture = Fixture::new("greater", &[(32, "Selected"), (33, "Rejected")], Some(22));
        let source = std::fs::read_to_string(fixture.0.join("source.xsd")).unwrap();
        let rows = r#"<xs:element name="Rows" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="OtherRows" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element>"#;
        std::fs::write(
            fixture.0.join("source.xsd"),
            source.replace(r#"<xs:element name="Value" type="xs:string"/>"#, rows),
        )
        .unwrap();
        fixture.rewrite(r#"<entry name="Value" outkey="10"/>"#, r#"<entry name="Rows" outkey="12"><entry name="Value" outkey="10"/></entry><entry name="OtherRows" outkey="13"><entry name="Value"/></entry>"#);
        fixture.rewrite(r#"<edge vertexkey="20"/>"#, "");
        fixture.rewrite(r#"<edge vertexkey="21"/>"#, "");
        fixture.rewrite(r#"<edge vertexkey="41"/>"#, "");
        fixture.rewrite("</children>", r#"<component name="position" library="core" kind="5"><sources><datapoint key="24"/></sources><targets><datapoint key="25"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint key="26"/></targets><data><constant value="1" datatype="integer"/></data></component></children>"#);
        let position_edge = if position_source == 0 {
            String::new()
        } else {
            format!(
                r#"<vertex vertexkey="{position_source}"><edges><edge vertexkey="24"/></edges></vertex>"#
            )
        };
        fixture.rewrite("<vertices>", &format!(r#"<vertices>{position_edge}<vertex vertexkey="25"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="26"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="41"/></edges></vertex>"#));
        let imported = fixture.import();
        let input = format_xml::from_str("<Source><Rows><Value>alpha</Value></Rows><Rows><Value>beta</Value></Rows><Rows><Value>gamma</Value></Rows><OtherRows><Value>foreign</Value></OtherRows><Wanted>untouched</Wanted></Source>", &imported.project.source).unwrap();
        let output = engine::run(&imported.project, &input).unwrap();
        if position_source != 12 {
            assert_eq!(field(&output, "Selected"), None);
            assert_eq!(field(&output, "Rejected"), None);
            assert!(!imported.warnings.is_empty());
            assert!(
                mfd::import_with_profile(
                    &fixture.0.join("mapping.mfd"),
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable
                )
                .is_err()
            );
        } else {
            assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
            assert_eq!(
                field(&output, "Selected"),
                Some(&Value::String("beta".into()))
            );
            assert_eq!(
                field(&output, "Rejected"),
                Some(&Value::String("alpha".into()))
            );
        }
        assert_eq!(
            field(&output, "Untouched"),
            Some(&Value::String("untouched".into()))
        );
    }
}
