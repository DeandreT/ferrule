use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_mixed_db_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn write_fixture(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let connection = Connection::open(dir.join("ledger.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE Updates (Value TEXT, Note TEXT); \
         CREATE TABLE Journal (Count INTEGER); \
         INSERT INTO Journal VALUES (7);",
    )?;
    drop(connection);

    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
      <xs:element name="Note" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;

    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26">
  <resources><datasources><datasource name="ledger">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="ledger.sqlite" name="ledger" path="ledger"/>
  </datasource></datasources></resources>
  <component name="map"><structure><children>
    <component name="source" library="xml" kind="14"><data>
      <root><entry name="Source"><entry name="Item" outkey="10">
        <entry name="Value" outkey="11"/><entry name="Note" outkey="12"/>
      </entry></entry></root>
      <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="ledger" library="db" kind="15"><data>
      <root><entry name="document">
        <entry name="Updates" type="table" inpkey="20">
          <entry name="Value" inpkey="21"/><entry name="Note" inpkey="22"/>
        </entry>
        <entry name="Journal" type="table" outkey="30">
          <entry name="Count" outkey="31"/>
        </entry>
      </entry></root>
      <database ref="ledger"/>
    </data></component>
    <component name="rows" library="text" kind="16">
      <properties XSLTDefaultOutput="1"/><data>
        <root><entry name="FileInstance"><entry name="document">
          <entry name="Rows" inpkey="40"><entry name="Count" inpkey="41"/></entry>
        </entry></entry></root>
        <text type="csv" outputinstance="out.csv"><settings separator="," firstrownames="true">
          <names block="Rows"><field0 name="Count" type="int"/></names>
        </settings></text>
      </data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="40"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
    <vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn mixed_database_component_is_both_a_named_source_and_target()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let [database] = imported.project.extra_sources.as_slice() else {
        panic!("expected the mixed database component as an extra source");
    };
    assert_eq!(database.path, "ledger.sqlite");
    assert!(database.dynamic_path.is_none());
    assert_eq!(imported.project.extra_targets.len(), 1);
    assert_eq!(imported.project.extra_targets[0].name, database.name);

    let source = format_xml::from_str(
        "<Source><Item><Value>A</Value><Note>first</Note></Item><Item><Value>B</Value><Note>second</Note></Item></Source>",
        &imported.project.source,
    )?;
    let ledger = format_db::read_instance(&dir.0.join("ledger.sqlite"), &database.schema)?;
    let target = engine::run_with_sources(
        &imported.project,
        &source,
        vec![(database.name.clone(), ledger)],
    )?;
    let rows = target
        .as_repeated()
        .ok_or("CSV target did not produce rows")?;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter().all(|row| {
            row.field("Count").and_then(Instance::as_scalar) == Some(&Value::Int(7))
        })
    );
    Ok(())
}
