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

fn static_catalog_mapping() -> &'static str {
    r#"<mapping version="26">
  <component name="map"><structure><children>
    <component name="input" library="xml" kind="14"><data><root>
      <entry name="Input"><entry name="Needle" outkey="10"/></entry>
    </root><document schema="input.xsd" inputinstance="input.xml" instanceroot="{}Input"/></data></component>
    <component name="Find" library="user" kind="19"><data>
      <root><entry name="needle" inpkey="30" componentid="101"/></root>
      <root rootindex="1"><entry name="result" outkey="34" componentid="102"/></root>
    </data></component>
    <component name="output" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
      <entry name="Output"><entry name="Result" inpkey="40"/></entry>
    </root><document schema="output.xsd" outputinstance="output.xml" instanceroot="{}Output"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="34"><edges><edge vertexkey="40"/></edges></vertex>
  </vertices></graph></structure></component>
  <component name="Find" library="user"><structure><children>
    <component name="result" library="core" uid="102" kind="7"><sources><datapoint key="200"/></sources></component>
    <component name="needle" library="core" uid="101" kind="6"><targets><datapoint key="201"/></targets></component>
    <component name="equal" library="core" uid="103" kind="5"><sources><datapoint key="202"/><datapoint key="203"/></sources><targets><datapoint key="204"/></targets></component>
    <component name="filter" library="core" uid="105" kind="3"><sources><datapoint key="205"/><datapoint key="206"/></sources><targets><datapoint key="207"/><datapoint/></targets></component>
    <component name="Catalog" library="xml" uid="104" kind="14"><properties ParameterName="catalog"/><data><root>
      <entry name="Catalog"><entry name="Item"><entry name="Key" outkey="208"/><entry name="Value" outkey="209"/></entry></entry>
    </root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="201"><edges><edge vertexkey="202"/></edges></vertex>
    <vertex vertexkey="208"><edges><edge vertexkey="203"/></edges></vertex>
    <vertex vertexkey="204"><edges><edge vertexkey="206"/></edges></vertex>
    <vertex vertexkey="209"><edges><edge vertexkey="205"/></edges></vertex>
    <vertex vertexkey="207"><edges><edge vertexkey="200"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#
}

fn computed_catalog_mapping() -> &'static str {
    r#"<mapping version="26">
  <component name="map"><structure><children>
    <component name="input" library="xml" kind="14"><data><root>
      <entry name="Input"><entry name="Needle" outkey="10"/></entry>
    </root><document schema="input.xsd" inputinstance="input.xml" instanceroot="{}Input"/></data></component>
    <component name="FindComputed" library="user" kind="19"><data>
      <root><entry name="needle" inpkey="30" componentid="101"/></root>
      <root rootindex="1"><entry name="result" outkey="34" componentid="102"/></root>
    </data></component>
    <component name="output" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
      <entry name="Output"><entry name="Result" inpkey="40"/></entry>
    </root><document schema="output.xsd" outputinstance="output.xml" instanceroot="{}Output"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="34"><edges><edge vertexkey="40"/></edges></vertex>
  </vertices></graph></structure></component>
  <component name="FindComputed" library="user"><structure><children>
    <component name="result" library="core" uid="102" kind="7"><sources><datapoint key="200"/></sources></component>
    <component name="needle" library="core" uid="101" kind="6"><targets><datapoint key="201"/></targets></component>
    <component name="equal" library="core" uid="103" kind="5"><sources><datapoint key="202"/><datapoint key="203"/></sources><targets><datapoint key="204"/></targets></component>
    <component name="filter" library="core" uid="105" kind="3"><sources><datapoint key="205"/><datapoint key="206"/></sources><targets><datapoint key="207"/><datapoint/></targets></component>
    <component name="Catalog" library="xml" uid="104" kind="14"><properties ParameterName="catalog"/><data><root>
      <entry name="Catalog"><entry name="Item"><entry name="Key" outkey="208"/><entry name="Value" outkey="209"/><entry name="Extra" outkey="210"/></entry></entry>
    </root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
    <component name="active" library="core" uid="106" kind="2"><targets><datapoint key="211"/></targets><data><constant value="active" datatype="string"/></data></component>
    <component name="equal" library="core" uid="107" kind="5"><sources><datapoint key="212"/><datapoint key="213"/></sources><targets><datapoint key="214"/></targets></component>
    <component name="logical-and" library="core" uid="108" kind="5"><sources><datapoint key="215"/><datapoint key="216"/></sources><targets><datapoint key="217"/></targets></component>
    <component name="separator" library="core" uid="109" kind="2"><targets><datapoint key="221"/></targets><data><constant value=":" datatype="string"/></data></component>
    <component name="concat" library="core" uid="110" kind="5"><sources><datapoint key="218"/><datapoint key="219"/><datapoint key="220"/></sources><targets><datapoint key="222"/></targets></component>
  </children><graph><vertices>
    <vertex vertexkey="201"><edges><edge vertexkey="202"/></edges></vertex>
    <vertex vertexkey="208"><edges><edge vertexkey="203"/></edges></vertex>
    <vertex vertexkey="210"><edges><edge vertexkey="212"/><edge vertexkey="220"/></edges></vertex>
    <vertex vertexkey="211"><edges><edge vertexkey="213"/></edges></vertex>
    <vertex vertexkey="204"><edges><edge vertexkey="215"/></edges></vertex>
    <vertex vertexkey="214"><edges><edge vertexkey="216"/></edges></vertex>
    <vertex vertexkey="217"><edges><edge vertexkey="206"/></edges></vertex>
    <vertex vertexkey="209"><edges><edge vertexkey="218"/></edges></vertex>
    <vertex vertexkey="221"><edges><edge vertexkey="219"/></edges></vertex>
    <vertex vertexkey="222"><edges><edge vertexkey="205"/></edges></vertex>
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

fn generic_attribute_setup() -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("input.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Meta" type="xs:anyType"/><xs:element name="Wanted" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("meta.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Meta" type="xs:anyType"/></xs:schema>"#,
    );
    write(
        &dir.0.join("output.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26">
  <component name="map"><structure><children>
    <component name="input" library="xml" kind="14"><data><root>
      <entry name="Input"><entry name="Meta" outkey="10"/><entry name="Wanted" outkey="11"/></entry>
    </root><document schema="input.xsd" instanceroot="{}Input"/></data></component>
    <component name="FindAttribute" library="user" kind="19"><data>
      <root><entry name="Meta" componentid="501"><entry name="Meta" inpkey="20"/></entry><entry name="Wanted" inpkey="21" componentid="502"/></root>
      <root rootindex="1"><entry name="Result" outkey="22" componentid="500"/></root>
    </data></component>
    <component name="output" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
      <entry name="Output"><entry name="Result" inpkey="30"/></entry>
    </root><document schema="output.xsd" instanceroot="{}Output"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex>
  </vertices></graph></structure></component>
  <component name="FindAttribute" library="user"><structure><children>
    <component name="Result" library="core" uid="500" kind="7"><sources><datapoint key="200"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Result"/></data></component>
    <component name="Wanted" library="core" uid="502" kind="6"><targets><datapoint key="201"/></targets></component>
    <component name="Meta" library="xml" uid="501" kind="14"><properties UsageKind="input"/><data><root>
      <entry name="Meta"><entry name="element()" outkey="208"><entry name="LocalName" outkey="209"/><entry name="anyType" type="xml-type"><entry name="attribute()" type="attribute" outkey="210"><entry name="LocalName" outkey="211"/></entry></entry></entry></entry>
    </root><document schema="meta.xsd" instanceroot="{}Meta"/><parameter usageKind="input" name="Meta"><root><entry name="Meta"/></root></parameter></data></component>
    <component name="name" library="core" uid="503" kind="2"><targets><datapoint key="212"/></targets><data><constant value="name" datatype="string"/></data></component>
    <component name="equal" library="core" uid="504" kind="5"><sources><datapoint key="213"/><datapoint key="214"/></sources><targets><datapoint key="215"/></targets></component>
    <component name="equal" library="core" uid="505" kind="5"><sources><datapoint key="216"/><datapoint key="217"/></sources><targets><datapoint key="218"/></targets></component>
    <component name="logical-and" library="core" uid="506" kind="5"><sources><datapoint key="219"/><datapoint key="220"/></sources><targets><datapoint key="221"/></targets></component>
    <component name="attribute()" library="core" uid="507" kind="3"><sources><datapoint key="222"/><datapoint key="223"/></sources><targets><datapoint key="224"/><datapoint/></targets></component>
  </children><graph><vertices>
    <vertex vertexkey="201"><edges><edge vertexkey="213"/></edges></vertex>
    <vertex vertexkey="209"><edges><edge vertexkey="214"/></edges></vertex>
    <vertex vertexkey="211"><edges><edge vertexkey="216"/></edges></vertex>
    <vertex vertexkey="212"><edges><edge vertexkey="217"/></edges></vertex>
    <vertex vertexkey="215"><edges><edge vertexkey="219"/></edges></vertex>
    <vertex vertexkey="218"><edges><edge vertexkey="220"/></edges></vertex>
    <vertex vertexkey="221"><edges><edge vertexkey="223"/></edges></vertex>
    <vertex vertexkey="210"><edges><edge vertexkey="222"/></edges></vertex>
    <vertex vertexkey="224"><edges><edge vertexkey="200"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    );
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
    let dir = setup(&mapping().replace("name=\"equal\"", "name=\"unsupported-predicate\""));
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

#[test]
fn static_catalog_filter_udf_imports_as_an_executable_named_source_lookup() {
    let dir = setup(static_catalog_mapping());
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.extra_sources.len(), 1);
    assert_eq!(imported.project.extra_sources[0].name, "Catalog");
    assert_eq!(imported.project.extra_sources[0].path, "catalog.xml");
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::Lookup { collection, key, value, .. }
            if collection == &["Catalog", "Item"] && key == &["Key"] && value == &["Value"]
    )));
    assert!(engine::validate(&imported.project).is_empty());

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
fn computed_catalog_filter_imports_composite_predicate_and_value_expression() {
    let dir = setup(computed_catalog_mapping());
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::CollectionFind { collection, .. }
            if collection == &["Catalog", "Item"]
    )));
    assert!(engine::validate(&imported.project).is_empty());

    let row = |key: &str, value: &str, extra: &str| {
        Instance::Group(vec![
            ("Key".into(), Instance::Scalar(Value::String(key.into()))),
            (
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            ),
            (
                "Extra".into(),
                Instance::Scalar(Value::String(extra.into())),
            ),
        ])
    };
    let catalog = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![
            row("A", "wrong", "inactive"),
            row("A", "chosen", "active"),
        ]),
    )]);
    let input = Instance::Group(vec![(
        "Needle".into(),
        Instance::Scalar(Value::String("A".into())),
    )]);
    let output =
        engine::run_with_sources(&imported.project, &input, vec![("Catalog".into(), catalog)])
            .unwrap();
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("chosen:active".into()))
    );
}

#[test]
fn structured_parameter_attribute_filter_imports_and_executes() {
    let dir = generic_attribute_setup();
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::CollectionFind { collection, .. }
            if collection == &["Meta", "element()", "attribute()"]
    )));
    assert!(engine::validate(&imported.project).is_empty());

    let input = format_xml::from_str(
        "<Input><Meta><Field name=\"FirstName\" type=\"string\"/></Meta><Wanted>Field</Wanted></Input>",
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("FirstName".into()))
    );
}

#[test]
fn static_catalog_lookup_rejects_non_equality_predicates() {
    let near_miss = static_catalog_mapping().replace("name=\"equal\"", "name=\"greater-than\"");
    let dir = setup(&near_miss);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning.contains(
            "skipped user-defined function `Find`: definition uses sequence operation `filter`"
        )),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.extra_sources.is_empty());
    assert!(
        !imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Lookup { .. }))
    );
}
