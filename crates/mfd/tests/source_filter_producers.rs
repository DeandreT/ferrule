use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "filter-producers-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: impl AsRef<[u8]>) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn import(&self) -> mfd::Imported {
        mfd::import(&self.0.join("map.mfd")).unwrap()
    }

    fn execute(&self, imported: &mfd::Imported, xml: &str) -> Instance {
        assert!(
            engine::validate(&imported.project).is_empty(),
            "{:?}",
            engine::validate(&imported.project)
        );
        let input = format_xml::from_str(xml, &imported.project.source).unwrap();
        engine::run(&imported.project, &input).unwrap()
    }

    fn assert_refused(&self) {
        assert!(
            mfd::import_with_profile(
                &self.0.join("map.mfd"),
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scalar<'a>(value: &'a Instance, field: &str) -> Option<&'a Value> {
    value.field(field).and_then(Instance::as_scalar)
}

#[test]
fn external_scalar_recipes_never_reuse_a_filtered_variables_first_row() {
    for (language, library, function, file, source) in [
        (
            "cs",
            "Review, Version=1.0.0.0, Culture=neutral, PublicKeyToken=null",
            "NumberText.Render",
            "Review/NumberText.cs",
            r#"using System.Globalization; public class NumberText { public static string Render(decimal amount) { return amount.ToString("0.0", CultureInfo.InvariantCulture); } }"#,
        ),
        (
            "java",
            "review.NumberText",
            "review.NumberText.Render",
            "review/NumberText.java",
            r#"package review; public class NumberText { public static String Render(java.math.BigDecimal amount) { java.text.NumberFormat formatter = new java.text.DecimalFormat("0.0"); return formatter.format(amount.doubleValue()); } }"#,
        ),
        (
            "xquery",
            "review",
            "r:scale",
            "review.xq",
            r#"xquery version "1.0"; module namespace r="urn:review"; declare function r:scale($amount as xs:decimal) as xs:decimal { $amount * 2 };"#,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write(file, source);
        fixture.write("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element><xs:element name="Note" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
        fixture.write("target.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/><xs:element name="Echo" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element><xs:element name="Note" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
        let xml = "<Source><Row><Value>10</Value></Row><Row><Value>20</Value></Row><Note>untouched</Note></Source>";
        for filtered in [false, true] {
            let input = if filtered { 32 } else { 11 };
            fixture.write("map.mfd", format!(r#"<mapping><component name="map"><properties SelectedLanguage="{language}"/><structure><children>
              <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Value" outkey="11"/></entry><entry name="Note" outkey="12"/></entry></root><document schema="source.xsd" instanceroot="{{}}Source"/></data></component>
              <component name="constant" library="core" kind="2"><targets><datapoint key="20"/></targets><data><constant value="true" datatype="boolean"/></data></component>
              <component name="filter" library="core" kind="3"><sources><datapoint key="21"/><datapoint key="22"/></sources><targets><datapoint key="23"/><datapoint key="24"/></targets></component>
              <component name="variable" library="xml" kind="14"><data><root><entry name="Row" inpkey="30" outkey="31"><entry name="Value" outkey="32"/></entry></root><parameter usageKind="variable"/><document schema="source.xsd" instanceroot="{{}}Source/{{}}Row"/></data></component>
              <component name="{function}" library="{library}" kind="5"><sources><datapoint key="50"/></sources><targets><datapoint key="51"/></targets></component>
              <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="40"><entry name="Value" inpkey="41"/><entry name="Echo" inpkey="42"/></entry><entry name="Note" inpkey="43"/></entry></root><document schema="target.xsd" instanceroot="{{}}Target"/></data></component>
              </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="22"/></edges></vertex><vertex vertexkey="23"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="31"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="{input}"><edges><edge vertexkey="50"/></edges></vertex><vertex vertexkey="51"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="42"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="43"/></edges></vertex></vertices></graph></structure></component></mapping>"#));
            let imported = fixture.import();
            let output = fixture.execute(&imported, xml);
            let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
            assert_eq!(rows.len(), 2);
            assert_ne!(scalar(&rows[0], "Echo"), scalar(&rows[1], "Echo"));
            assert_eq!(
                scalar(&output, "Note"),
                Some(&Value::String("untouched".into()))
            );
            if filtered {
                assert!(
                    imported
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("external scalar function")
                            && warning.contains("affected binding skipped")),
                    "{language}: {:?}",
                    imported.warnings
                );
                assert!(
                    rows.iter().all(|row| row.field("Value").is_none()),
                    "{language}: {output:?}"
                );
                fixture.assert_refused();
            } else {
                assert!(
                    imported.warnings.is_empty(),
                    "{language}: {:?}",
                    imported.warnings
                );
                assert!(rows.iter().all(|row| row.field("Value").is_some()));
                assert_ne!(scalar(&rows[0], "Value"), scalar(&rows[1], "Value"));
            }
        }
    }
}

const LOOKUP: &str = r#"<component name="Find" library="review" inline="1"><structure><children>
  <component name="needle" library="core" uid="101" kind="6"><targets><datapoint key="201"/></targets></component>
  <component name="result" library="core" uid="102" kind="7"><sources><datapoint key="200"/></sources></component>
  <component name="equal" library="core" uid="103" kind="5"><sources><datapoint key="202"/><datapoint key="203"/></sources><targets><datapoint key="204"/></targets></component>
  <component name="Catalog" library="xml" uid="104" kind="14"><properties UsageKind="input"/><data><root><entry name="Catalog"><entry name="Item"><entry name="Key" outkey="208"/><entry name="Value" outkey="209"/></entry></entry></root><document schema="catalog.xsd" instanceroot="{}Catalog"/></data></component>
  <component name="filter" library="core" uid="105" kind="3"><sources><datapoint key="205"/><datapoint key="206"/></sources><targets><datapoint key="207"/><datapoint/></targets></component>
  </children><graph><vertices><vertex vertexkey="201"><edges><edge vertexkey="202"/></edges></vertex><vertex vertexkey="208"><edges><edge vertexkey="203"/></edges></vertex><vertex vertexkey="204"><edges><edge vertexkey="206"/></edges></vertex><vertex vertexkey="209"><edges><edge vertexkey="205"/></edges></vertex><vertex vertexkey="207"><edges><edge vertexkey="200"/></edges></vertex></vertices></graph></structure></component>"#;

#[test]
fn structured_lookup_inputs_cannot_bypass_missing_or_valid_filters() {
    let fixture = Fixture::new();
    fixture.write("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Needle" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    fixture.write("catalog.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    fixture.write("target.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string" minOccurs="0"/><xs:element name="Echo" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    fixture.write("catalog.xml", "<Catalog><Item><Key>a</Key><Value>first</Value></Item><Item><Key>b</Key><Value>second</Value></Item></Catalog>");
    for (mode, predicate) in [
        ("plain", true),
        ("variable", true),
        ("variable", false),
        ("direct", true),
        ("direct", false),
    ] {
        for default in [false, true] {
            let filtered = mode != "plain";
            let (key, value) = match mode {
                "variable" => (32, 33),
                "direct" => (74, 78),
                _ => (11, 12),
            };
            let predicate_edge = if predicate {
                r#"<vertex vertexkey="63"><edges><edge vertexkey="22"/><edge vertexkey="73"/><edge vertexkey="77"/></edges></vertex>"#
            } else {
                ""
            };
            let direct_edges = if mode == "direct" {
                r#"<vertex vertexkey="11"><edges><edge vertexkey="72"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="76"/></edges></vertex>"#
            } else {
                ""
            };
            let result_edges = if default {
                r#"<vertex vertexkey="53"><edges><edge vertexkey="80"/></edges></vertex><vertex vertexkey="81"><edges><edge vertexkey="40"/></edges></vertex>"#
            } else {
                r#"<vertex vertexkey="53"><edges><edge vertexkey="40"/></edges></vertex>"#
            };
            fixture.write("map.mfd", format!(r#"<mapping><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Needle" outkey="13"/></entry></root><document schema="source.xsd" instanceroot="{{}}Source"/></data></component>
          <component name="Catalog" library="xml" kind="14"><data><root><entry name="Catalog"><entry name="Item" outkey="10"><entry name="Key" outkey="11"/><entry name="Value" outkey="12"/></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{{}}Catalog"/></data></component>
          <component name="filter" library="core" kind="3"><sources><datapoint key="21"/><datapoint key="22"/></sources><targets><datapoint key="23"/><datapoint key="24"/></targets></component>
          <component name="constant" library="core" kind="2"><targets><datapoint key="60"/></targets><data><constant value="a" datatype="string"/></data></component>
          <component name="equal" library="core" kind="5"><sources><datapoint key="61"/><datapoint key="62"/></sources><targets><datapoint key="63"/></targets></component>
          <component name="variable" library="xml" kind="14"><data><root><entry name="Item" inpkey="30" outkey="31"><entry name="Key" outkey="32"/><entry name="Value" outkey="33"/></entry></root><parameter usageKind="variable"/><document schema="catalog.xsd" instanceroot="{{}}Catalog/{{}}Item"/></data></component>
          <component name="key-selection" library="core" kind="3"><sources><datapoint key="72"/><datapoint key="73"/></sources><targets><datapoint key="74"/><datapoint key="75"/></targets></component>
          <component name="value-selection" library="core" kind="3"><sources><datapoint key="76"/><datapoint key="77"/></sources><targets><datapoint key="78"/><datapoint key="79"/></targets></component>
          <component name="Find" library="review" kind="19"><data><root><entry name="needle" inpkey="52" componentid="101"/><entry name="Catalog" componentid="104"><entry name="Catalog"><entry name="Item"><entry name="Key" inpkey="50"/><entry name="Value" inpkey="51"/></entry></entry></entry></root><root rootindex="1"><entry name="result" outkey="53" componentid="102"/></root></data></component>
          <component name="value-map" library="core" kind="5"><sources><datapoint key="80"/></sources><targets><datapoint key="81"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable/><input name="input" type="String"/><result name="result" type="String" defaultValue="wrong-default"/></valuemap></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="40"/><entry name="Echo" inpkey="41"/></entry></root><document schema="target.xsd" instanceroot="{{}}Target"/></data></component>
          </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="61"/></edges></vertex><vertex vertexkey="60"><edges><edge vertexkey="62"/></edges></vertex>{predicate_edge}{direct_edges}<vertex vertexkey="23"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="{key}"><edges><edge vertexkey="50"/></edges></vertex><vertex vertexkey="{value}"><edges><edge vertexkey="51"/></edges></vertex><vertex vertexkey="13"><edges><edge vertexkey="52"/><edge vertexkey="41"/></edges></vertex>{result_edges}</vertices></graph></structure></component>{LOOKUP}</mapping>"#));
            let imported = fixture.import();
            let input = format_xml::from_str(
                "<Source><Needle>b</Needle></Source>",
                &imported.project.source,
            )
            .unwrap();
            let catalog = format_xml::from_str("<Catalog><Item><Key>a</Key><Value>first</Value></Item><Item><Key>b</Key><Value>second</Value></Item></Catalog>", &imported.project.extra_sources[0].schema).unwrap();
            let output = engine::run_with_sources(
                &imported.project,
                &input,
                vec![(imported.project.extra_sources[0].name.clone(), catalog)],
            )
            .unwrap();
            assert_eq!(scalar(&output, "Echo"), Some(&Value::String("b".into())));
            if filtered {
                assert_eq!(scalar(&output, "Result"), None, "{output:?}");
                assert!(
                    imported
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("structured UDF input")
                            && warning.contains("filtered")
                            && warning.contains("affected binding skipped")),
                    "{:?}",
                    imported.warnings
                );
                fixture.assert_refused();
            } else {
                assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
                assert_eq!(
                    scalar(&output, "Result"),
                    Some(&Value::String(
                        if default { "wrong-default" } else { "second" }.into()
                    ))
                );
            }
        }
    }
}

#[test]
fn refused_projection_does_not_poison_siblings_in_either_branch_or_binding_order() {
    let fixture = Fixture::new();
    fixture.write("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Tags" type="xs:string" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element><xs:element name="Wanted" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    fixture.write("target.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="GoodTrue" type="xs:string" minOccurs="0"/><xs:element name="GoodFalse" type="xs:string" minOccurs="0"/><xs:element name="BadTrue" type="xs:string" minOccurs="0"/><xs:element name="BadFalse" type="xs:string" minOccurs="0"/><xs:element name="Echo" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    for (bad_first, defaults) in [(false, false), (true, false), (false, true), (true, true)] {
        for valid_predicate in [true, false] {
            let (good_true, good_false, bad_true, bad_false) = if bad_first {
                (42, 43, 40, 41)
            } else {
                (40, 41, 42, 43)
            };
            let predicate = if valid_predicate {
                r#"<vertex vertexkey="20"><edges><edge vertexkey="22"/></edges></vertex>"#
            } else {
                ""
            };
            let (bad_true_input, bad_false_input) = if defaults {
                (70, 72)
            } else {
                (bad_true, bad_false)
            };
            let default_edges = if defaults {
                format!(
                    r#"<vertex vertexkey="71"><edges><edge vertexkey="{bad_true}"/></edges></vertex><vertex vertexkey="73"><edges><edge vertexkey="{bad_false}"/></edges></vertex>"#
                )
            } else {
                String::new()
            };
            fixture.write("map.mfd", format!(r#"<mapping><component name="map"><structure><children>
              <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Item" outkey="10"><entry name="Value" outkey="11"/><entry name="Tags"/></entry><entry name="Wanted" outkey="12"/></entry></root><document schema="source.xsd" instanceroot="{{}}Source"/></data></component>
              <component name="equal" library="core" kind="5"><sources><datapoint key="18"/><datapoint key="19"/></sources><targets><datapoint key="20"/></targets></component>
              <component name="filter" library="core" kind="3"><sources><datapoint key="21"/><datapoint key="22"/></sources><targets><datapoint key="23"/><datapoint key="24"/></targets></component>
              <component name="yes" library="xml" kind="14"><data><root><entry name="Item" inpkey="30" outkey="31"><entry name="Value" outkey="32"/><entry name="Tags" outkey="33"/></entry></root><parameter usageKind="variable"/><document schema="source.xsd" instanceroot="{{}}Source/{{}}Item"/></data></component>
              <component name="no" library="xml" kind="14"><data><root><entry name="Item" inpkey="60" outkey="61"><entry name="Value" outkey="62"/><entry name="Tags" outkey="63"/></entry></root><parameter usageKind="variable"/><document schema="source.xsd" instanceroot="{{}}Source/{{}}Item"/></data></component>
              <component name="value-map" library="core" kind="5"><sources><datapoint key="70"/></sources><targets><datapoint key="71"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable/><input name="input" type="String"/><result name="result" type="String" defaultValue="bad"/></valuemap></data></component>
              <component name="value-map" library="core" kind="5"><sources><datapoint key="72"/></sources><targets><datapoint key="73"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable/><input name="input" type="String"/><result name="result" type="String" defaultValue="bad"/></valuemap></data></component>
              <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="GoodTrue" inpkey="{good_true}"/><entry name="GoodFalse" inpkey="{good_false}"/><entry name="BadTrue" inpkey="{bad_true}"/><entry name="BadFalse" inpkey="{bad_false}"/><entry name="Echo" inpkey="44"/></entry></root><document schema="target.xsd" instanceroot="{{}}Target"/></data></component>
              </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="18"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="19"/><edge vertexkey="44"/></edges></vertex>{predicate}<vertex vertexkey="23"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="24"><edges><edge vertexkey="60"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="{good_true}"/></edges></vertex><vertex vertexkey="33"><edges><edge vertexkey="{bad_true_input}"/></edges></vertex><vertex vertexkey="62"><edges><edge vertexkey="{good_false}"/></edges></vertex><vertex vertexkey="63"><edges><edge vertexkey="{bad_false_input}"/></edges></vertex>{default_edges}</vertices></graph></structure></component></mapping>"#));
            let imported = fixture.import();
            let output = fixture.execute(&imported, "<Source><Item><Value>alpha</Value><Tags>one</Tags><Tags>two</Tags></Item><Item><Value>beta</Value><Tags>three</Tags></Item><Wanted>alpha</Wanted></Source>");
            assert_eq!(scalar(&output, "BadTrue"), None, "{output:?}");
            assert_eq!(scalar(&output, "BadFalse"), None, "{output:?}");
            assert_eq!(
                scalar(&output, "Echo"),
                Some(&Value::String("alpha".into()))
            );
            if valid_predicate {
                assert_eq!(
                    scalar(&output, "GoodTrue"),
                    Some(&Value::String("alpha".into())),
                    "bad_first={bad_first}: {output:?}"
                );
                assert_eq!(
                    scalar(&output, "GoodFalse"),
                    Some(&Value::String("beta".into())),
                    "bad_first={bad_first}: {output:?}"
                );
            } else {
                assert_eq!(scalar(&output, "GoodTrue"), None);
                assert_eq!(scalar(&output, "GoodFalse"), None);
            }
            fixture.assert_refused();
        }
    }
}
