use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_db_query_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn prepare_database(dir: &Path) {
    let connection = Connection::open(dir.join("people.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE Person (Name TEXT, Title TEXT, DepartmentID INTEGER); \
             INSERT INTO Person VALUES \
               ('Ada', 'Manager', 1), ('Grace', 'Senior Manager', 1), \
               ('Linus', 'Engineer', 1), ('Bob', 'Manager', 2);",
        )
        .unwrap();
}

fn write_design(path: &Path) {
    let design = r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="26">
  <resources><datasources><datasource name="people">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="people.sqlite" name="people" path="people">
      <LocalViewStorage>
        <LocalViewElement SQL="SELECT &quot;Name&quot;, &quot;Title&quot; FROM &quot;Person&quot; WHERE &quot;DepartmentID&quot; = :DepartmentID AND &quot;Title&quot; LIKE '%Manager%'">
          <PathElement Name="main" Kind="Database"/>
          <PathElement Name="ManagersByDepartment" Kind="Select Statement"/>
          <Parameters><Parameter name="DepartmentID" type="integer" null="Yes"/></Parameters>
        </LocalViewElement>
      </LocalViewStorage>
    </database_connection>
  </datasource></datasources></resources>
  <component name="defaultmap" uid="1" editable="1"><structure><children>
    <component name="constant" library="core" uid="2" kind="2">
      <targets><datapoint pos="0" key="10"/></targets>
      <data><constant value="1" datatype="decimal"/></data>
    </component>
    <component name="DepartmentID" library="core" uid="3" kind="6">
      <sources><datapoint pos="0" key="11"/></sources>
      <targets><datapoint pos="0" key="12"/></targets>
      <data><input datatype="integer"/><parameter usageKind="input" name="DepartmentID"/></data>
    </component>
    <component name="database" library="db" uid="4" kind="15"><data>
      <root><entry name="document"><entry name="ManagersByDepartment" type="routine" outkey="20"/></entry></root>
      <database ref="people"><data><selections><selection>
        <PathElement Name="main" Kind="Database"/>
        <PathElement Name="ManagersByDepartment" Kind="Select Statement"/>
      </selection></selections></data></database>
    </data></component>
    <component name="ManagersByDepartment" library="db" uid="5" kind="28"><data>
      <root>
        <entry name="procedure" inpkey="21"/>
        <entry name="ManagersByDepartment"><entry name="DepartmentID" type="attribute" inpkey="22"/></entry>
      </root>
      <root>
        <entry name="ManagersByDepartment" outkey="30"><entry name="ManagersByDepartment">
          <entry name="Name" type="attribute" outkey="31"/>
          <entry name="Title" type="attribute" outkey="32"/>
        </entry></entry>
      </root>
    </data></component>
    <component name="rows" library="text" uid="6" kind="16"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40">
        <entry name="Name" inpkey="41"/>
      </entry></entry></entry></root>
      <text type="csv" outputinstance="out.csv"><settings separator="," quote="&quot;" firstrownames="true">
        <names root="rows" block="Rows"><field0 name="Name" type="string"/></names>
      </settings></text>
    </data></component>
  </children><graph directed="1"><edges/><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="11"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
    <vertex vertexkey="20"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex>
    <vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#;
    std::fs::write(path, design).unwrap();
}

#[test]
fn static_single_table_query_imports_and_executes() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let design = dir.0.join("query.mfd");
    write_design(&design);

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source.name, "Person");
    assert!(imported.project.source.child("DepartmentID").is_some());

    let source =
        format_db::read_instance(&dir.0.join("people.sqlite"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let names = output
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            row.field("Name")
                .and_then(Instance::as_scalar)
                .and_then(|value| match value {
                    Value::String(value) => Some(value.as_str()),
                    _ => None,
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["Ada", "Grace"]);
}

#[test]
fn scalar_only_query_outputs_are_skipped_once() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let design = dir.0.join("scalar-query.mfd");
    write_design(&design);
    let text = std::fs::read_to_string(&design).unwrap().replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges/></vertex>",
    );
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("used only through scalar outputs"));
    assert!(imported.project.root.bindings.is_empty());
}

#[test]
fn query_output_casing_is_canonicalized_to_the_sqlite_schema() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let design = dir.0.join("query-casing.mfd");
    write_design(&design);
    let text = std::fs::read_to_string(&design)
        .unwrap()
        .replace(
            "SELECT &quot;Name&quot;, &quot;Title&quot;",
            "SELECT &quot;name&quot;, &quot;title&quot;",
        )
        .replace(
            "<entry name=\"Name\" type=\"attribute\" outkey=\"31\"/>",
            "<entry name=\"NAME\" type=\"attribute\" outkey=\"31\"/>",
        );
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.source.child("Name").is_some());
    assert!(imported.project.source.child("NAME").is_none());
    assert!(
        imported.project.graph.nodes.values().any(
            |node| matches!(node, mapping::Node::SourceField { path, .. } if path == &["Name"])
        )
    );
}
