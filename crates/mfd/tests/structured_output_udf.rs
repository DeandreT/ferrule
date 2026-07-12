use std::fs;
use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::IterationOutput;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-structured-output-{}-{}",
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
    r#"<mapping version="26"><component name="map"><structure><children>
      <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Order" outkey="10"><entry name="Needle" outkey="11"/><entry name="Qty" outkey="12"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
      <component name="Find" library="user" kind="19"><data>
        <root><entry name="needle" inpkey="30" componentid="101"/><entry name="qty" inpkey="32" componentid="102"/></root>
        <root rootindex="1"><entry name="Record" componentid="106"><entry name="Article" outkey="31"/></entry></root>
      </data></component>
      <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="20"><entry name="Article" inpkey="21"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
    </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices>
      <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
      <vertex vertexkey="11"><edges><edge vertexkey="30"/></edges></vertex>
      <vertex vertexkey="12"><edges><edge vertexkey="32"/></edges></vertex>
      <vertex vertexkey="31"><edges><edge vertexkey="21" edgekey="90"/></edges></vertex>
    </vertices></graph></structure></component>
    <component name="Find" library="user" inline="1"><structure><children>
      <component name="needle" library="core" uid="101" kind="6"><targets><datapoint key="201"/></targets></component>
      <component name="qty" library="core" uid="102" kind="6"><targets><datapoint key="202"/></targets></component>
      <component name="Catalog" library="xml" uid="103" kind="14"><data><root><entry name="Catalog"><entry name="Item" outkey="203"><entry name="Key" outkey="204"/><entry name="Label" outkey="205"/><entry name="Price" outkey="206"/></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
      <component name="equal" library="core" uid="104" kind="5"><sources><datapoint key="207"/><datapoint key="208"/></sources><targets><datapoint key="209"/></targets></component>
      <component name="filter" library="core" uid="105" kind="3"><sources><datapoint key="210"/><datapoint key="211"/></sources><targets><datapoint key="212"/><datapoint/></targets></component>
      <component name="multiply" library="core" uid="107" kind="5"><sources><datapoint key="213"/><datapoint key="214"/></sources><targets><datapoint key="215"/></targets></component>
      <component name="Record" library="xml" uid="106" kind="14"><properties UsageKind="output"/><data><root><entry name="Article" inpkey="216"><entry name="Key" inpkey="217"/><entry name="Label" inpkey="218"/><entry name="Total" inpkey="219"/></entry></root><document schema="article.xsd" instanceroot="{}Article"/><parameter usageKind="output" name="Record"/></data></component>
    </children><graph><vertices>
      <vertex vertexkey="203"><edges><edge vertexkey="210"/></edges></vertex>
      <vertex vertexkey="204"><edges><edge vertexkey="207"/><edge vertexkey="217"/></edges></vertex>
      <vertex vertexkey="201"><edges><edge vertexkey="208"/></edges></vertex>
      <vertex vertexkey="209"><edges><edge vertexkey="211"/></edges></vertex>
      <vertex vertexkey="212"><edges><edge vertexkey="216"/></edges></vertex>
      <vertex vertexkey="205"><edges><edge vertexkey="218"/></edges></vertex>
      <vertex vertexkey="206"><edges><edge vertexkey="213"/></edges></vertex>
      <vertex vertexkey="202"><edges><edge vertexkey="214"/></edges></vertex>
      <vertex vertexkey="215"><edges><edge vertexkey="219"/></edges></vertex>
    </vertices></graph></structure></component>
  </mapping>"#
}

fn setup(mfd: &str) -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Order" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Needle" type="xs:string"/><xs:element name="Qty" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Label" type="xs:string"/><xs:element name="Price" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("article.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Article"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Label" type="xs:string"/><xs:element name="Total" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Article"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Label" type="xs:string"/><xs:element name="Total" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(&dir.0.join("mapping.mfd"), mfd);
    dir
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn integer(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}

fn order(needle: &str, qty: i64) -> Instance {
    Instance::Group(vec![
        ("Needle".into(), text(needle)),
        ("Qty".into(), integer(qty)),
    ])
}

fn item(key: &str, label: &str, price: i64) -> Instance {
    Instance::Group(vec![
        ("Key".into(), text(key)),
        ("Label".into(), text(label)),
        ("Price".into(), integer(price)),
    ])
}

#[test]
fn structured_lookup_output_emits_zero_or_many_constructed_occurrences() {
    let dir = setup(mapping());
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.extra_sources.len(), 1);
    let source = Instance::Group(vec![(
        "Order".into(),
        Instance::Repeated(vec![order("A", 2), order("B", 3)]),
    )]);
    let catalog = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![item("A", "first", 4), item("A", "second", 5)]),
    )]);
    let target = engine::run_with_sources(
        &imported.project,
        &source,
        vec![("Catalog".into(), catalog)],
    )
    .unwrap();
    let rows = target.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    let articles = rows[0]
        .field("Article")
        .and_then(Instance::as_mapped_sequence)
        .unwrap();
    assert_eq!(articles.len(), 2);
    assert_eq!(
        articles[0].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("first".into()))
    );
    assert_eq!(
        articles[0].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Int(8))
    );
    assert_eq!(
        articles[1].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Int(10))
    );
    assert!(
        rows[1]
            .field("Article")
            .and_then(Instance::as_mapped_sequence)
            .is_some_and(|items| items.is_empty())
    );
    let article_scope = &imported.project.root.children[0].children[0];
    assert_eq!(
        article_scope.iteration_output,
        IterationOutput::MappedSequence
    );
}

#[test]
fn structured_lookup_with_sequence_output_expression_warns_and_skips() {
    let dir = setup(&mapping().replace("name=\"multiply\"", "name=\"sum\""));
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning
            .contains("structured lookup uses unsupported sequence operation `sum`")),
        "{:?}",
        imported.warnings
    );
    assert!(
        imported
            .project
            .root
            .children
            .iter()
            .flat_map(|scope| &scope.children)
            .all(|scope| scope.target_field != "Article")
    );
}
