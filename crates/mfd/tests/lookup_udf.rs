use std::fs;
use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-lookup-udf-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
}

fn mapping() -> &'static str {
    r#"<mapping version="26">
  <component name="map"><structure><children>
    <component name="input" library="xml" kind="14"><data><root>
      <entry name="Input"><entry name="Needle" outkey="10"/></entry>
    </root><document schema="input.xsd" inputinstance="input.xml" instanceroot="{}Input"/></data></component>
    <component name="Catalog" library="xml" kind="14"><data><root>
      <entry name="Catalog"><entry name="Item"><entry name="Key" outkey="20"/><entry name="Value" outkey="21"/><entry name="Extra" outkey="22"/></entry></entry>
    </root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
    <component name="Find" library="user" kind="19"><data>
      <root><entry name="needle" inpkey="30" componentid="101"/><entry name="Catalog" componentid="104"><entry name="Catalog"><entry name="Item"><entry name="Key" inpkey="31"/><entry name="Value" inpkey="32"/><entry name="Extra" inpkey="33"/></entry></entry></entry></root>
      <root rootindex="1"><entry name="result" outkey="34" componentid="102"/></root>
    </data></component>
    <component name="output" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
      <entry name="Output"><entry name="Result" inpkey="40"/></entry>
    </root><document schema="output.xsd" outputinstance="output.xml" instanceroot="{}Output"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex>
    <vertex vertexkey="21"><edges><edge vertexkey="32"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="33"/></edges></vertex>
    <vertex vertexkey="34"><edges><edge vertexkey="40"/></edges></vertex>
  </vertices></graph></structure></component>
  <component name="Find" library="user" inline="1"><structure><children>
    <component name="result" library="core" uid="102" kind="7"><sources><datapoint key="200"/></sources></component>
    <component name="needle" library="core" uid="101" kind="6"><targets><datapoint key="201"/></targets></component>
    <component name="equal" library="core" uid="103" kind="5"><sources><datapoint key="202"/><datapoint key="203"/></sources><targets><datapoint key="204"/></targets></component>
    <component name="filter" library="core" uid="105" kind="3"><sources><datapoint key="205"/><datapoint key="206"/></sources><targets><datapoint key="207"/><datapoint/></targets></component>
    <component name="Catalog" library="xml" uid="104" kind="14"><properties UsageKind="input"/><data><root>
      <entry name="Catalog"><entry name="Item"><entry name="Key" outkey="208"/><entry name="Value" outkey="209"/></entry></entry>
    </root><document schema="catalog.xsd" instanceroot="{}Catalog"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="201"><edges><edge vertexkey="202"/></edges></vertex>
    <vertex vertexkey="208"><edges><edge vertexkey="203"/></edges></vertex>
    <vertex vertexkey="204"><edges><edge vertexkey="206"/></edges></vertex>
    <vertex vertexkey="209"><edges><edge vertexkey="205"/></edges></vertex>
    <vertex vertexkey="207"><edges><edge vertexkey="200"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#
}

fn setup(mfd: &str) -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("input.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Needle" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Value" type="xs:string"/><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("output.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(&dir.0.join("mapping.mfd"), mfd);
    dir
}

fn item(key: &str, value: &str) -> Instance {
    Instance::Group(vec![
        ("Key".into(), Instance::Scalar(Value::String(key.into()))),
        (
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        ),
        ("Extra".into(), Instance::Scalar(Value::Null)),
    ])
}

#[test]
fn structured_input_filter_udf_imports_as_first_match_lookup() {
    let dir = setup(mapping());
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::Lookup { collection, key, value, .. }
            if collection == &["Catalog", "Item"] && key == &["Key"] && value == &["Value"]
    )));

    let catalog = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![item("A", "first"), item("A", "second")]),
    )]);
    for (needle, expected) in [("A", Value::String("first".into())), ("B", Value::Null)] {
        let input = Instance::Group(vec![(
            "Needle".into(),
            Instance::Scalar(Value::String(needle.into())),
        )]);
        let output = engine::run_with_sources(
            &imported.project,
            &input,
            vec![("Catalog".into(), catalog.clone())],
        )
        .unwrap();
        assert_eq!(
            output.field("Result").and_then(Instance::as_scalar),
            Some(&expected)
        );
    }
}

#[test]
fn structured_lookup_near_miss_retains_actionable_warning() {
    let dir = setup(&mapping().replace("name=\"equal\"", "name=\"not-equal\""));
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning.contains(
            "skipped user-defined function `Find`: definition uses sequence operation `filter`"
        )),
        "{:?}",
        imported.warnings
    );
    assert!(
        !imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Lookup { .. }))
    );
}
