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

fn write_balanced_fixture(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let connection = Connection::open(dir.join("records.sqlite"))?;
    connection.execute_batch("CREATE TABLE Records (Value TEXT);")?;
    drop(connection);

    let design = dir.join("balanced.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26">
  <resources><datasources><datasource name="records">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="records.sqlite" name="records" path="records"/>
  </datasource></datasources></resources>
  <component name="map"><structure><children>
    <component name="records" library="db" kind="15">
      <properties XSLTDefaultOutput="1"/><data>
        <root><entry name="document">
          <entry name="Records" type="table" inpkey="20" outkey="10">
            <entry name="Value" inpkey="21" outkey="11"/>
          </entry>
        </entry></root>
        <database ref="records"/>
      </data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn balanced_database_ports_classify_one_component_as_source_and_target()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_balanced_fixture(&dir.0)?)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("records.sqlite")
    );
    assert_eq!(
        imported.project.target_path.as_deref(),
        Some("records.sqlite")
    );
    assert_eq!(imported.project.source, imported.project.target);
    assert!(imported.project.extra_sources.is_empty());
    assert!(imported.project.extra_targets.is_empty());
    assert!(engine::validate(&imported.project).is_empty());

    let exported = dir.0.join("balanced-roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported)?.is_empty());
    let text = std::fs::read_to_string(&exported)?;
    let document = roxmltree::Document::parse(&text)?;
    let records = document
        .descendants()
        .filter(|entry| entry.has_tag_name("entry") && entry.attribute("name") == Some("Records"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    assert!(records[0].attribute("outkey").is_some());
    assert!(records[0].attribute("inpkey").is_some());
    let value = records[0]
        .children()
        .find(|entry| entry.has_tag_name("entry") && entry.attribute("name") == Some("Value"))
        .ok_or("missing balanced database Value entry")?;
    assert!(value.attribute("outkey").is_some());
    assert!(value.attribute("inpkey").is_some());
    assert_eq!(value.attribute("datatype"), Some("string"));

    let roundtrip = mfd::import(&exported)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.source, imported.project.source);
    assert_eq!(roundtrip.project.target, imported.project.target);
    assert!(engine::validate(&roundtrip.project).is_empty());
    Ok(())
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
    assert_eq!(imported.project.extra_targets[0].schema, database.schema);
    assert_eq!(
        imported.project.extra_targets[0].path.as_deref(),
        Some(database.path.as_str())
    );
    assert_eq!(imported.project.extra_targets[0].options, database.options);

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

    let exported = dir.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported)?.is_empty());
    let design = std::fs::read_to_string(&exported)?;
    let document = roxmltree::Document::parse(&design)?;
    let database_components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("db"))
        .collect::<Vec<_>>();
    assert_eq!(database_components.len(), 1);
    let database_component = database_components[0];
    assert!(database_component.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry.attribute("name") == Some("Updates")
            && entry.attribute("inpkey").is_some()
    }));
    assert!(database_component.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry.attribute("name") == Some("Journal")
            && entry.attribute("outkey").is_some()
    }));
    assert!(database_component.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry.attribute("name") == Some("Count")
            && entry.attribute("datatype") == Some("integer")
    }));

    let roundtrip = mfd::import(&exported)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    let [roundtrip_database] = roundtrip.project.extra_sources.as_slice() else {
        return Err("roundtrip did not retain the database source".into());
    };
    assert_eq!(roundtrip.project.extra_targets.len(), 1);
    let roundtrip_ledger =
        format_db::read_instance(&dir.0.join("ledger.sqlite"), &roundtrip_database.schema)?;
    let roundtrip_target = engine::run_with_sources(
        &roundtrip.project,
        &source,
        vec![(roundtrip_database.name.clone(), roundtrip_ledger)],
    )?;
    assert_eq!(roundtrip_target, target);

    let detached = dir.0.join("detached");
    std::fs::create_dir(&detached)?;
    let detached_design = detached.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &detached_design)?.is_empty());
    let detached_roundtrip = mfd::import(&detached_design)?;
    assert!(
        detached_roundtrip.warnings.is_empty(),
        "{:?}",
        detached_roundtrip.warnings
    );
    let [detached_database] = detached_roundtrip.project.extra_sources.as_slice() else {
        return Err("detached roundtrip did not retain the database source".into());
    };
    assert_eq!(detached_database.schema, database.schema);
    assert_eq!(
        detached_roundtrip.project.extra_targets[0].schema,
        database.schema
    );
    Ok(())
}
