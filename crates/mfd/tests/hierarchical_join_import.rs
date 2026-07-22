use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Node, ScopeIteration};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_hierarchical_join_{}_{}",
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

fn prepare_database(dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let database = dir.join("test.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE customers (
           id INTEGER PRIMARY KEY,
           display_name TEXT NOT NULL
         );
         CREATE TABLE contacts (
           id INTEGER PRIMARY KEY,
           customer_id INTEGER NOT NULL,
           category TEXT NOT NULL,
           city TEXT NOT NULL,
           FOREIGN KEY(customer_id) REFERENCES customers(id)
         );
         INSERT INTO customers VALUES (1, 'Zulu'), (2, 'Alpha');
         INSERT INTO contacts VALUES
           (1, 1, 'home', 'North'),
           (2, 2, 'work', 'Office'),
           (3, 2, 'home', 'South');",
    )?;
    Ok(database)
}

fn write_design(path: &Path) -> Result<(), Box<dyn Error>> {
    let design = r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="26">
  <resources><datasources><datasource name="test">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="test.sqlite" name="test" path="test"/>
  </datasource></datasources></resources>
  <component name="defaultmap" uid="1" editable="1"><structure><children>
    <component name="database" library="db" uid="2" kind="15"><data>
      <root><entry name="document">
        <entry name="customers" type="table" outkey="1">
          <entry name="id" outkey="3"/>
          <entry name="display_name" outkey="12"/>
          <entry name="contacts|customer_id" type="table" outkey="2">
            <entry name="id" outkey="4"/>
            <entry name="customer_id" outkey="5"/>
            <entry name="category" outkey="22"/>
            <entry name="city" outkey="23"/>
          </entry>
        </entry>
      </entry></root><database ref="test"><data><selections>
        <selection><PathElement Name="main" Kind="Database"/><PathElement Name="customers" Kind="Table"/></selection>
        <selection><PathElement Name="main" Kind="Database"/><PathElement Name="contacts" Kind="Table"/></selection>
      </selections></data></database>
    </data></component>
    <component name="join" library="core" uid="32" kind="32"><data>
      <root><entry name="document"><entry name="tuple" outkey="90">
        <entry name="dynamic_tree_node0"><entry name="customers" inpkey="10" outkey="11">
          <entry name="display_name" outkey="12"/>
        </entry></entry>
        <entry name="dynamic_tree_node1"><entry name="contacts|customer_id" inpkey="20" outkey="21">
          <entry name="category" outkey="22"/>
          <entry name="city" outkey="23"/>
        </entry></entry>
      </entry></entry></root>
      <join><joinkeys/><keypaths><entry><condition/></entry></keypaths></join>
    </data></component>
    <component name="category" library="core" uid="40" kind="6">
      <sources><datapoint pos="0" key="51"/></sources>
      <targets><datapoint pos="0" key="50"/></targets>
      <data><input datatype="string" previewvalue="home" usepreviewvalue="1"/>
        <parameter usageKind="input" name="category"/></data>
    </component>
    <component name="Contacts where" library="db" uid="41" kind="21">
      <sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources>
      <targets><datapoint pos="0" key="32"/></targets>
      <data><where condition="contacts.category = :category" order="customers.display_name">
        <parameters><parameter name="category" type="string"/></parameters>
      </where></data>
    </component>
    <component name="rows" library="text" uid="42" kind="16"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="60">
        <entry name="Name" inpkey="61"/>
        <entry name="Category" inpkey="62"/>
        <entry name="City" inpkey="63"/>
      </entry></entry></entry></root>
      <text type="csv" outputinstance="out.csv"><settings separator="," quote="&quot;" firstrownames="true">
        <names root="rows" block="Rows">
          <field0 name="Name" type="string"/>
          <field1 name="Category" type="string"/>
          <field2 name="City" type="string"/>
        </names>
      </settings></text>
    </data></component>
  </children><graph directed="1"><vertices>
    <vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex>
    <vertex vertexkey="2"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="90"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="50"><edges><edge vertexkey="31"/></edges></vertex>
    <vertex vertexkey="32"><edges><edge vertexkey="60"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="61"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="62"/></edges></vertex>
    <vertex vertexkey="23"><edges><edge vertexkey="63"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#;
    std::fs::write(path, design)?;
    Ok(())
}

fn field_string<'a>(row: &'a Instance, name: &str) -> Option<&'a str> {
    match row.field(name).and_then(Instance::as_scalar) {
        Some(Value::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn run(project: &mapping::Project, database: &Path) -> Result<Instance, Box<dyn Error>> {
    let source = format_db::read_instance(database, &project.source)?;
    Ok(engine::run(project, &source)?)
}

fn assert_home_rows(output: &Instance) {
    let rows = output.as_repeated().expect("target rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(field_string(&rows[0], "Name"), Some("Alpha"));
    assert_eq!(field_string(&rows[0], "Category"), Some("home"));
    assert_eq!(field_string(&rows[0], "City"), Some("South"));
    assert_eq!(field_string(&rows[1], "Name"), Some("Zulu"));
    assert_eq!(field_string(&rows[1], "Category"), Some("home"));
    assert_eq!(field_string(&rows[1], "City"), Some("North"));
}

#[test]
fn keyless_join_over_hierarchical_db_relation_imports_as_plain_iteration()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let database = prepare_database(&dir.0)?;
    let design = dir.0.join("hierarchical-join.mfd");
    write_design(&design)?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported.project.root.source(),
        Some([String::from("contacts|customer_id")].as_slice())
    );
    assert!(imported.project.root.filter.is_some());
    assert!(imported.project.root.sort_by.is_some());
    assert!(!matches!(
        imported.project.root.iteration,
        ScopeIteration::InnerJoin { .. }
    ));
    assert!(imported.project.graph.nodes.values().any(
        |node| matches!(node, Node::Const { value: Value::String(value) } if value == "home")
    ));
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .all(|node| !matches!(node, Node::JoinField { .. }))
    );

    let output = run(&imported.project, &database)?;
    assert_home_rows(&output);

    let exported = dir.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported)?.is_empty());
    let roundtrip = mfd::import(&exported)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    let roundtrip_output = run(&roundtrip.project, &database)?;
    assert_eq!(roundtrip_output, output);
    assert_home_rows(&roundtrip_output);

    Ok(())
}
