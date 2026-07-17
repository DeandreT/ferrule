use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_external_udf_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> Option<&'a Value> {
    instance.field(field).and_then(Instance::as_scalar)
}

#[test]
fn opaque_json_udf_result_becomes_an_executable_external_source()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Category" type="xs:string"/>
      <xs:element name="Name" type="xs:string"/>
      <xs:element name="Quantity" type="xs:integer"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26">
  <component name="map"><structure><children>
    <component name="FetchInventory" library="user" kind="19"><data>
      <root><entry name="result" componentid="23"><entry name="object">
        <entry name="groups" type="json-property"><entry name="array">
          <entry name="item" type="json-item"><entry name="object">
            <entry name="category" type="json-property"><entry name="string" outkey="10"/></entry>
            <entry name="items" type="json-property"><entry name="array">
              <entry name="item" type="json-item"><entry name="object" outkey="11">
                <entry name="name" type="json-property"><entry name="string" outkey="12"/></entry>
                <entry name="quantity" type="json-property"><entry name="integer" outkey="13"/></entry>
              </entry></entry>
            </entry></entry>
          </entry></entry>
        </entry></entry>
      </entry></entry></root>
    </data></component>
    <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Target">
        <entry name="Row" inpkey="20"><entry name="Category" inpkey="21"/><entry name="Name" inpkey="22"/><entry name="Quantity" inpkey="23"/></entry>
      </entry></entry></entry></root>
      <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
    </data></component>
  </children></structure><connections>
    <edge from="11" to="20"/><edge from="10" to="21"/><edge from="12" to="22"/><edge from="13" to="23"/>
  </connections></component>
  <component name="FetchInventory" library="user"><structure><children>
    <component name="opaque-runtime" library="custom" kind="99"/>
  </children></structure></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&dir.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.source_options.external_source.is_some());
    assert!(engine::validate(&imported.project).is_empty());

    let groups = imported
        .project
        .source
        .child("groups")
        .ok_or_else(|| std::io::Error::other("missing groups schema"))?;
    let items = groups
        .child("items")
        .ok_or_else(|| std::io::Error::other("missing items schema"))?;
    let quantity = items
        .child("quantity")
        .ok_or_else(|| std::io::Error::other("missing quantity schema"))?;
    assert!(groups.repeating);
    assert!(items.repeating);
    assert!(matches!(
        quantity.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));

    let source = format_json::from_str(
        r#"{"groups":[{"category":"hardware","items":[{"name":"hammer","quantity":2},{"name":"nails","quantity":50}]},{"category":"garden","items":[{"name":"spade","quantity":1}]}]}"#,
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    let rows = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .ok_or_else(|| std::io::Error::other("missing output rows"))?;

    assert_eq!(rows.len(), 3);
    assert_eq!(
        scalar(&rows[0], "Category"),
        Some(&Value::String("hardware".to_owned()))
    );
    assert_eq!(
        scalar(&rows[0], "Name"),
        Some(&Value::String("hammer".to_owned()))
    );
    assert_eq!(scalar(&rows[1], "Quantity"), Some(&Value::Int(50)));
    assert_eq!(
        scalar(&rows[2], "Category"),
        Some(&Value::String("garden".to_owned()))
    );
    assert_eq!(
        scalar(&rows[2], "Name"),
        Some(&Value::String("spade".to_owned()))
    );
    Ok(())
}

#[test]
fn ordinary_source_takes_precedence_over_an_opaque_udf_candidate()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26">
  <component name="map"><structure><children>
    <component name="source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Value" outkey="40"/></entry></entry></entry></root>
      <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="FetchUnused" library="user" kind="19"><data><root>
      <entry name="result" componentid="23"><entry name="object"><entry name="value" type="json-property"><entry name="string" outkey="10"/></entry></entry></entry>
    </root></data></component>
    <component name="exists" library="core" kind="5"><sources><datapoint pos="0" key="30"/></sources><targets><datapoint pos="0" key="31"/></targets></component>
    <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Value" inpkey="20"/></entry></entry></entry></root>
      <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
    </data></component>
  </children></structure><connections><edge from="10" to="30"/><edge from="40" to="20"/></connections></component>
  <component name="FetchUnused" library="user"><structure><children>
    <component name="opaque-runtime" library="custom" kind="99"/>
  </children></structure></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&dir.0.join("mapping.mfd"))?;
    assert_eq!(imported.project.source.name, "Source");
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].starts_with("skipped user-defined function `FetchUnused`"));
    assert!(!imported.warnings[0].contains("external source"));
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Value".to_owned(),
        Instance::Scalar(Value::String("ordinary".to_owned())),
    )]);
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        scalar(&output, "Value"),
        Some(&Value::String("ordinary".to_owned()))
    );
    Ok(())
}
