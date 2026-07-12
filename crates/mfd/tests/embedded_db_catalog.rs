use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_embedded_db_{}_{}",
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
    let connection = Connection::open(dir.join("company.sqlite")).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON; \
             CREATE TABLE Department (Id INTEGER PRIMARY KEY, Label TEXT); \
             CREATE TABLE Staff (Id INTEGER PRIMARY KEY, DepartmentId INTEGER REFERENCES Department(Id), First TEXT, Title TEXT); \
             INSERT INTO Department VALUES (10, 'Platform'), (20, 'Operations'), (30, 'Research'); \
             INSERT INTO Staff VALUES \
               (1, 10, 'Ari', 'Team Lead'), (2, 10, 'Bea', 'Engineer'), \
               (3, 20, 'Cy', 'Lead'), (4, 30, 'Dee', 'Scientist');",
        )
        .unwrap();
}

fn design() -> &'static str {
    r#"<mapping version="26"><resources><datasources><datasource name="company">
      <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="company.sqlite" name="company">
        <LocalViewStorage><LocalViewElement SQL="SELECT &quot;First&quot;, &quot;Title&quot; FROM &quot;Staff&quot; WHERE &quot;DepartmentId&quot; = :DepartmentId AND &quot;Title&quot; LIKE '%Lead%'">
          <PathElement Name="main" Kind="Database"/><PathElement Name="StaffLeadsByDepartment" Kind="Select Statement"/>
          <Parameters><Parameter name="DepartmentId" type="integer"/></Parameters>
        </LocalViewElement></LocalViewStorage>
        <LocalRelationsStorage><LocalRelationElement name="department-leads">
          <SourceTable><PathElement Name="main" Kind="Database"/><PathElement Name="StaffLeadsByDepartment" Kind="Select Statement"/></SourceTable>
          <SourceColumns><Column name="DepartmentId" kind="Parameter"/></SourceColumns>
          <DestinationTable><PathElement Name="main" Kind="Database"/><PathElement Name="Department" Kind="Table"/></DestinationTable>
          <DestinationColumns><Column name="Id" kind="Column"/></DestinationColumns>
        </LocalRelationElement></LocalRelationsStorage>
      </database_connection></datasource></datasources></resources>
      <component name="map" uid="1"><structure><children>
        <component name="catalog" library="db" uid="2" kind="15"><data><root><entry name="document"><entry name="Department" type="table">
          <entry name="Label" outkey="20"/>
          <entry name="StaffLeadsByDepartment|DepartmentId" type="routine" displayselectionmode="selection"/>
          <entry name="StaffLeadsByDepartment|DepartmentId" type="routine" outkey="21"><entry name="StaffLeadsByDepartment" type="table">
            <entry name="First" outkey="22"/><entry name="Title" outkey="23"/>
          </entry></entry>
        </entry></entry></root><database ref="company"><data><selections>
          <selection><PathElement Name="main" Kind="Database"/><PathElement Name="StaffLeadsByDepartment" Kind="Select Statement"/></selection>
          <selection><PathElement Name="main" Kind="Database"/><PathElement Name="Department" Kind="Table"/></selection>
        </selections></data></database></data></component>
        <component name="rows" library="text" uid="3" kind="16"><properties XSLTDefaultOutput="1"/><data>
          <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="30">
            <entry name="Department" inpkey="31"/><entry name="First" inpkey="32"/><entry name="Title" inpkey="33"/>
          </entry></entry></entry></root>
          <text type="csv" outputinstance="out.csv"><settings separator="," firstrownames="true"><names block="Rows">
            <field0 name="Department" type="string"/><field1 name="First" type="string"/><field2 name="Title" type="string"/>
          </names></settings></text>
        </data></component>
      </children><graph><vertices>
        <vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex>
        <vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex>
        <vertex vertexkey="22"><edges><edge vertexkey="32"/></edges></vertex>
        <vertex vertexkey="23"><edges><edge vertexkey="33"/></edges></vertex>
      </vertices></graph></structure></component></mapping>"#
}

fn output_rows(output: &Instance) -> Vec<[String; 3]> {
    output
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            ["Department", "First", "Title"].map(|field| {
                row.field(field)
                    .and_then(Instance::as_scalar)
                    .and_then(|value| match value {
                        Value::String(value) => Some(value.clone()),
                        _ => None,
                    })
                    .unwrap()
            })
        })
        .collect()
}

#[test]
fn inline_correlated_query_imports_and_executes_without_a_query_component() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let path = dir.0.join("department-leads.mfd");
    std::fs::write(&path, design()).unwrap();

    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let department = imported.project.source.child("Department").unwrap();
    assert!(department.child("Staff|DepartmentId").is_some());
    let input =
        format_db::read_instance(&dir.0.join("company.sqlite"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    assert_eq!(
        output_rows(&output),
        [
            ["Platform", "Ari", "Team Lead"],
            ["Operations", "Cy", "Lead"]
        ]
    );
}

#[test]
fn unsupported_multiple_inline_queries_fall_back_with_an_actionable_warning() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let path = dir.0.join("ambiguous-catalog.mfd");
    let text = design().replace(
        "<entry name=\"Label\" outkey=\"20\"/>",
        "<entry name=\"Label\" outkey=\"20\"/><entry name=\"OtherQuery|Id\" type=\"routine\" outkey=\"24\"><entry name=\"OtherQuery\" type=\"table\"/></entry>",
    );
    std::fs::write(&path, text).unwrap();

    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("unsupported inline query")
            && warning.contains("exactly one connected inline query result")
    }));
    assert!(imported.project.source.child("Label").is_some());
}
