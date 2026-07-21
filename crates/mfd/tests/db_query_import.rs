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

fn prepare_joined_query(dir: &Path) -> PathBuf {
    let connection = Connection::open(dir.join("purchases.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE Item (Id INTEGER PRIMARY KEY, Label TEXT, Cost REAL); \
             CREATE TABLE Purchase (Id INTEGER PRIMARY KEY, ItemId INTEGER, Units INTEGER, \
               FOREIGN KEY (ItemId) REFERENCES Item(Id)); \
             INSERT INTO Item VALUES (1, 'Pen', 4.0), (2, 'Book', 10.0), (3, 'Unknown', NULL); \
             INSERT INTO Purchase VALUES \
               (1, 1, 2), (2, 1, 3), (3, 2, 5), (4, NULL, 6), (5, 3, 4);",
        )
        .unwrap();
    let design = dir.join("joined-query.mfd");
    let xml = r#"<mapping version="26"><resources><datasources><datasource name="purchases">
      <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="purchases.sqlite" name="purchases">
        <LocalViewStorage><LocalViewElement SQL="SELECT (Units * Cost) AS Total, Purchase.Id, Purchase.Units, Item.Label, Item.Cost FROM Purchase INNER JOIN Item ON Purchase.ItemId = Item.Id WHERE Purchase.Units &gt; :MinimumUnits">
          <PathElement Name="main" Kind="Database"/><PathElement Name="PurchasesAboveMinimum" Kind="Select Statement"/>
          <Parameters><Parameter name="MinimumUnits" type="text"/></Parameters>
        </LocalViewElement></LocalViewStorage>
      </database_connection></datasource></datasources></resources>
      <component name="map" uid="1"><structure><children>
        <component name="MinimumUnits" library="core" uid="2" kind="6">
          <sources><datapoint/></sources><targets><datapoint pos="0" key="10"/></targets>
          <data><input datatype="string" previewvalue="2" usepreviewvalue="1"/><parameter usageKind="input" name="MinimumUnits"/></data>
        </component>
        <component name="catalog" library="db" uid="3" kind="15"><data><root><entry name="document">
          <entry name="PurchasesAboveMinimum" type="routine" outkey="20"/>
        </entry></root><database ref="purchases"><data><selections><selection>
          <PathElement Name="main" Kind="Database"/><PathElement Name="PurchasesAboveMinimum" Kind="Select Statement"/>
        </selection></selections></data></database></data></component>
        <component name="PurchasesAboveMinimum" library="db" uid="4" kind="28"><data>
          <root><entry name="procedure" inpkey="21"/><entry name="PurchasesAboveMinimum"><entry name="MinimumUnits" type="attribute" inpkey="22"/></entry></root>
          <root><entry name="PurchasesAboveMinimum" outkey="30"><entry name="PurchasesAboveMinimum">
            <entry name="Total" type="attribute" outkey="31"><outputnodefunctions>
              <rule applyto="self"><default value="7"/><filter datatype="decimal"/></rule>
            </outputnodefunctions></entry><entry name="Id" type="attribute" outkey="32"/>
            <entry name="Units" type="attribute" outkey="33"/><entry name="Label" type="attribute" outkey="34"/>
            <entry name="Cost" type="attribute" outkey="35"/>
          </entry></entry></root>
        </data></component>
        <component name="report" library="text" uid="5" kind="16"><properties XSLTDefaultOutput="1"/><data>
          <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40">
            <entry name="Total" inpkey="41"/><entry name="Id" inpkey="42"/><entry name="Units" inpkey="43"/>
            <entry name="Label" inpkey="44"/><entry name="Cost" inpkey="45"/>
          </entry></entry></entry></root>
          <text type="csv" outputinstance="report.csv"><settings separator="," quote="&quot;" firstrownames="true"><names root="report" block="Rows">
            <field0 name="Total" type="string"/><field1 name="Id" type="string"/><field2 name="Units" type="string"/>
            <field3 name="Label" type="string"/><field4 name="Cost" type="string"/>
          </names></settings></text>
        </data></component>
      </children><graph><vertices>
        <vertex vertexkey="10"><edges><edge vertexkey="22"/></edges></vertex>
        <vertex vertexkey="20"><edges><edge vertexkey="21"/></edges></vertex>
        <vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex>
        <vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex>
        <vertex vertexkey="32"><edges><edge vertexkey="42"/></edges></vertex>
        <vertex vertexkey="33"><edges><edge vertexkey="43"/></edges></vertex>
        <vertex vertexkey="34"><edges><edge vertexkey="44"/></edges></vertex>
        <vertex vertexkey="35"><edges><edge vertexkey="45"/></edges></vertex>
      </vertices></graph></structure></component></mapping>"#;
    std::fs::write(&design, xml).unwrap();
    design
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
fn many_to_one_joined_query_imports_computed_projection_and_filter() {
    let dir = TempDir::new();
    let design = prepare_joined_query(&dir.0);

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source =
        format_db::read_instance(&dir.0.join("purchases.sqlite"), &imported.project.source)
            .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let rows = output.as_repeated().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(12.0))
    );
    assert_eq!(
        rows[0].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("Pen".to_string()))
    );
    assert_eq!(
        rows[1].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(50.0))
    );
    assert_eq!(
        rows[1].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("Book".to_string()))
    );
    assert_eq!(
        rows[2].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(7.0))
    );

    let exported = dir.0.join("joined-query-roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let roundtrip = mfd::import(&exported).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    let source =
        format_db::read_instance(&dir.0.join("purchases.sqlite"), &roundtrip.project.source)
            .unwrap();
    assert_eq!(engine::run(&roundtrip.project, &source).unwrap(), output);
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
fn computed_only_query_outputs_cannot_bypass_query_scope() {
    let dir = TempDir::new();
    let design = prepare_joined_query(&dir.0);
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
fn joined_integer_multiplication_matches_sqlite_overflow_promotion() {
    let dir = TempDir::new();
    let design = prepare_joined_query(&dir.0);
    let database = dir.0.join("purchases.sqlite");
    let connection = Connection::open(&database).unwrap();
    let large = 4_000_000_000_i64;
    connection
        .execute(
            "INSERT INTO Purchase (Id, ItemId, Units) VALUES (?1, 1, ?1)",
            [large],
        )
        .unwrap();
    let sqlite_product = connection
        .query_row(
            "SELECT Id * Units FROM Purchase WHERE Id = ?1",
            [large],
            |row| row.get::<_, f64>(0),
        )
        .unwrap();
    drop(connection);
    let text = std::fs::read_to_string(&design)
        .unwrap()
        .replace("(Units * Cost)", "(Purchase.Id * Purchase.Units)");
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = format_db::read_instance(&database, &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let overflow_row = output
        .as_repeated()
        .unwrap()
        .iter()
        .find(|row| row.field("Id").and_then(Instance::as_scalar) == Some(&Value::Int(large)))
        .unwrap();
    assert_eq!(
        overflow_row.field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(sqlite_product))
    );
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

fn prepare_relational_database(dir: &Path) {
    let connection = Connection::open(dir.join("company.sqlite")).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON; \
             CREATE TABLE Office (PrimaryKey INTEGER PRIMARY KEY, Name TEXT); \
             CREATE TABLE Department (PrimaryKey INTEGER PRIMARY KEY, ForeignKey INTEGER REFERENCES Office(PrimaryKey), Name TEXT); \
             CREATE TABLE Person (PrimaryKey INTEGER PRIMARY KEY, ForeignKey INTEGER REFERENCES Department(PrimaryKey), First TEXT, Title TEXT); \
             INSERT INTO Office VALUES (1, 'West'), (2, 'East'); \
             INSERT INTO Department VALUES (10, 1, 'Engineering'), (11, 1, 'Sales'), (20, 2, 'Support'); \
             INSERT INTO Person VALUES \
               (1, 10, 'Ada', 'Manager'), (2, 10, 'Linus', 'Engineer'), \
               (3, 11, 'Grace', 'Senior Manager'), (4, 20, 'Edsger', 'Manager');",
        )
        .unwrap();
}

fn run_csv_names(project: &mapping::Project, db: &Path, fields: &[&str]) -> Vec<Vec<String>> {
    let source = format_db::read_instance(db, &project.source).unwrap();
    engine::run(project, &source)
        .unwrap()
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            fields
                .iter()
                .map(|field| {
                    row.field(field)
                        .and_then(Instance::as_scalar)
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.clone()),
                            _ => None,
                        })
                        .unwrap()
                })
                .collect()
        })
        .collect()
}

#[test]
fn nested_query_correlation_preserves_parent_fields_and_controls() {
    let dir = TempDir::new();
    prepare_relational_database(&dir.0);
    let design = dir.0.join("selected-office.mfd");
    let text = r#"<mapping version="26"><resources><datasources><datasource name="company">
      <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="company.sqlite" name="company">
        <LocalViewStorage>
          <LocalViewElement SQL="SELECT &quot;PrimaryKey&quot;, &quot;ForeignKey&quot;, &quot;Name&quot; FROM &quot;Department&quot;">
            <PathElement Name="main" Kind="Database"/><PathElement Name="DepartmentByOffice" Kind="Select Statement"/>
          </LocalViewElement>
          <LocalViewElement SQL="SELECT &quot;PrimaryKey&quot;, &quot;ForeignKey&quot;, &quot;First&quot;, &quot;Title&quot; FROM &quot;Person&quot; WHERE &quot;ForeignKey&quot; = :DepartmentID AND &quot;Title&quot; LIKE '%Manager%'">
            <PathElement Name="main" Kind="Database"/><PathElement Name="ManagersByDepartment" Kind="Select Statement"/>
            <Parameters><Parameter name="DepartmentID" type="integer"/></Parameters>
          </LocalViewElement>
        </LocalViewStorage>
        <LocalRelationsStorage><LocalRelationElement name="department-managers">
          <SourceTable><PathElement Name="main" Kind="Database"/><PathElement Name="ManagersByDepartment" Kind="Select Statement"/></SourceTable>
          <SourceColumns><Column name="DepartmentID" kind="Parameter"/></SourceColumns>
          <DestinationTable><PathElement Name="main" Kind="Database"/><PathElement Name="DepartmentByOffice" Kind="Select Statement"/></DestinationTable>
          <DestinationColumns><Column name="PrimaryKey" kind="Column"/></DestinationColumns>
        </LocalRelationElement></LocalRelationsStorage>
      </database_connection></datasource></datasources></resources>
      <component name="map" uid="1"><structure><children>
        <component name="constant" library="core" uid="2" kind="2"><targets><datapoint key="10"/></targets><data><constant value="1" datatype="decimal"/></data></component>
        <component name="OfficeID" library="core" uid="3" kind="6"><sources><datapoint key="11"/></sources><targets><datapoint key="12"/></targets><data><input datatype="integer"/></data></component>
        <component name="catalog" library="db" uid="4" kind="15"><data><root><entry name="document"><entry name="DepartmentByOffice" type="routine" outkey="20"/></entry></root><database ref="company"><data><selections><selection><PathElement Name="main" Kind="Database"/><PathElement Name="DepartmentByOffice" Kind="Select Statement"/></selection><selection><PathElement Name="main" Kind="Database"/><PathElement Name="ManagersByDepartment" Kind="Select Statement"/></selection></selections></data></database></data></component>
        <component name="DepartmentByOffice" library="db" uid="5" kind="28"><data>
          <root><entry name="procedure" inpkey="21"/><entry name="DepartmentByOffice"><entry name="OfficeID" type="attribute" inpkey="22"/></entry></root>
          <root><entry name="DepartmentByOffice"><entry name="DepartmentByOffice">
            <entry name="Name" type="attribute" outkey="30"/>
            <entry name="ManagersByDepartment" outkey="31"><entry name="ManagersByDepartment"><entry name="First" type="attribute" outkey="32"/><entry name="Title" type="attribute" outkey="33"/></entry></entry>
          </entry></entry></root>
        </data></component>
        <component name="rows" library="text" uid="6" kind="16"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40"><entry name="Department" inpkey="41"/><entry name="First" inpkey="42"/><entry name="Title" inpkey="43"/></entry></entry></entry></root><text type="csv" outputinstance="out.csv"><settings separator="," firstrownames="true"><names block="Rows"><field0 name="Department" type="string"/><field1 name="First" type="string"/><field2 name="Title" type="string"/></names></settings></text></data></component>
      </children><graph><vertices>
        <vertex vertexkey="10"><edges><edge vertexkey="11"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="21"/></edges></vertex>
        <vertex vertexkey="31"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="42"/></edges></vertex><vertex vertexkey="33"><edges><edge vertexkey="43"/></edges></vertex>
      </vertices></graph></structure></component></mapping>"#;
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let department = imported.project.source.child("Department").unwrap();
    assert!(department.child("Person|ForeignKey").is_some());
    assert!(imported.project.source.child("department").is_none());
    assert_eq!(
        run_csv_names(
            &imported.project,
            &dir.0.join("company.sqlite"),
            &["Department", "First", "Title"]
        ),
        [
            ["Engineering", "Ada", "Manager"],
            ["Sales", "Grace", "Senior Manager"],
            ["Support", "Edsger", "Manager"]
        ]
    );

    let controlled = text.replace(
        "FROM &quot;Department&quot;\"",
        "FROM &quot;Department&quot; WHERE &quot;Name&quot; LIKE '%Engineering%'\"",
    );
    std::fs::write(&design, controlled).unwrap();
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        run_csv_names(
            &imported.project,
            &dir.0.join("company.sqlite"),
            &["Department", "First", "Title"]
        ),
        [["Engineering", "Ada", "Manager"]]
    );
}

#[test]
fn table_parent_correlation_uses_relation_key_and_static_child_parameter() {
    let dir = TempDir::new();
    prepare_relational_database(&dir.0);
    let design = dir.0.join("employees-by-title.mfd");
    let text = r#"<mapping version="26"><resources><datasources><datasource name="company">
      <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="company.sqlite" name="company">
        <LocalViewStorage><LocalViewElement SQL="SELECT &quot;first&quot;, &quot;title&quot; FROM &quot;person&quot; WHERE &quot;foreignkey&quot; = :DepartmentID AND &quot;title&quot; LIKE :patternForTitle"><PathElement Name="main" Kind="Database"/><PathElement Name="PersonsByDepartmentAndTitle" Kind="Select Statement"/><Parameters><Parameter name="DepartmentID" type="integer"/><Parameter name="patternForTitle" type="text"/></Parameters></LocalViewElement></LocalViewStorage>
        <LocalRelationsStorage><LocalRelationElement name="department-persons"><SourceTable><PathElement Name="main" Kind="Database"/><PathElement Name="PersonsByDepartmentAndTitle" Kind="Select Statement"/></SourceTable><SourceColumns><Column name="DepartmentID" kind="Parameter"/></SourceColumns><DestinationTable><PathElement Name="main" Kind="Database"/><PathElement Name="department" Kind="Table"/></DestinationTable><DestinationColumns><Column name="primarykey" kind="Column"/></DestinationColumns></LocalRelationElement></LocalRelationsStorage>
      </database_connection></datasource></datasources></resources>
      <component name="map" uid="1"><structure><children>
        <component name="constant" library="core" uid="2" kind="2"><targets><datapoint key="10"/></targets><data><constant value="%Engineer%" datatype="string"/></data></component>
        <component name="pattern" library="core" uid="3" kind="6"><sources><datapoint key="11"/></sources><targets><datapoint key="12"/></targets><data><input datatype="string"/></data></component>
        <component name="catalog" library="db" uid="4" kind="15"><data><root><entry name="document"><entry name="dEpArTmEnT" type="table"><entry name="nAmE" outkey="20"/><entry name="PersonsByDepartmentAndTitle|DepartmentID" type="routine" outkey="21"/></entry></entry></root><database ref="company"><data><selections><selection><PathElement Name="main" Kind="Database"/><PathElement Name="PersonsByDepartmentAndTitle" Kind="Select Statement"/></selection><selection><PathElement Name="main" Kind="Database"/><PathElement Name="department" Kind="Table"/></selection></selections></data></database></data></component>
        <component name="PersonsByDepartmentAndTitle|DepartmentID" library="db" uid="5" kind="28"><data><root><entry name="procedure" inpkey="22"/><entry name="PersonsByDepartmentAndTitle"><entry name="patternForTitle" type="attribute" inpkey="23"/></entry></root><root><entry name="PersonsByDepartmentAndTitle" outkey="30"><entry name="PersonsByDepartmentAndTitle"><entry name="First" type="attribute" outkey="31"/><entry name="Title" type="attribute" outkey="32"/></entry></entry></root></data></component>
        <component name="rows" library="text" uid="6" kind="16"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40"><entry name="Department" inpkey="41"/><entry name="First" inpkey="42"/></entry></entry></entry></root><text type="csv" outputinstance="out.csv"><settings separator="," firstrownames="true"><names block="Rows"><field0 name="Department" type="string"/><field1 name="First" type="string"/></names></settings></text></data></component>
      </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="11"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="23"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="22"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="31"><edges><edge vertexkey="42"/></edges></vertex></vertices></graph></structure></component></mapping>"#;
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let department = imported.project.source.child("Department").unwrap();
    assert!(department.child("Person|ForeignKey").is_some());
    assert!(imported.project.source.child("department").is_none());
    assert_eq!(
        run_csv_names(
            &imported.project,
            &dir.0.join("company.sqlite"),
            &["Department", "First"]
        ),
        [["Engineering", "Linus"]]
    );
}

#[test]
fn reverse_correlation_uses_the_parent_side_foreign_key_column() {
    let dir = TempDir::new();
    prepare_relational_database(&dir.0);
    let design = dir.0.join("offices-by-department.mfd");
    let text = r#"<mapping version="26"><resources><datasources><datasource name="company">
      <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="company.sqlite" name="company">
        <LocalViewStorage><LocalViewElement SQL="SELECT &quot;PrimaryKey&quot;, &quot;Name&quot; FROM &quot;Office&quot; WHERE &quot;PrimaryKey&quot; = :OfficeID"><PathElement Name="main" Kind="Database"/><PathElement Name="OfficesByDepartment" Kind="Select Statement"/><Parameters><Parameter name="OfficeID" type="integer"/></Parameters></LocalViewElement></LocalViewStorage>
        <LocalRelationsStorage><LocalRelationElement name="department-office"><SourceTable><PathElement Name="main" Kind="Database"/><PathElement Name="OfficesByDepartment" Kind="Select Statement"/></SourceTable><SourceColumns><Column name="OfficeID" kind="Parameter"/></SourceColumns><DestinationTable><PathElement Name="main" Kind="Database"/><PathElement Name="Department" Kind="Table"/></DestinationTable><DestinationColumns><Column name="ForeignKey" kind="Column"/></DestinationColumns></LocalRelationElement></LocalRelationsStorage>
      </database_connection></datasource></datasources></resources>
      <component name="map" uid="1"><structure><children>
        <component name="catalog" library="db" uid="4" kind="15"><data><root><entry name="document"><entry name="Department" type="table"><entry name="Name" outkey="20"/><entry name="OfficesByDepartment|OfficeID" type="routine" outkey="21"/></entry></entry></root><database ref="company"><data><selections><selection><PathElement Name="main" Kind="Database"/><PathElement Name="OfficesByDepartment" Kind="Select Statement"/></selection><selection><PathElement Name="main" Kind="Database"/><PathElement Name="Department" Kind="Table"/></selection></selections></data></database></data></component>
        <component name="OfficesByDepartment|OfficeID" library="db" uid="5" kind="28"><data><root><entry name="procedure" inpkey="22"/></root><root><entry name="OfficesByDepartment" outkey="30"><entry name="OfficesByDepartment"><entry name="PrimaryKey" type="attribute" outkey="31"/><entry name="Name" type="attribute" outkey="32"/></entry></entry></root></data></component>
        <component name="rows" library="text" uid="6" kind="16"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40"><entry name="Department" inpkey="41"/><entry name="Office" inpkey="42"/></entry></entry></entry></root><text type="csv" outputinstance="out.csv"><settings separator="," firstrownames="true"><names block="Rows"><field0 name="Department" type="string"/><field1 name="Office" type="string"/></names></settings></text></data></component>
      </children><graph><vertices><vertex vertexkey="21"><edges><edge vertexkey="22"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="42"/></edges></vertex></vertices></graph></structure></component></mapping>"#;
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let department = imported.project.source.child("Department").unwrap();
    assert!(department.child("Office|ForeignKey").is_some());
    assert_eq!(
        run_csv_names(
            &imported.project,
            &dir.0.join("company.sqlite"),
            &["Department", "Office"]
        ),
        [
            ["Engineering", "West"],
            ["Sales", "West"],
            ["Support", "East"]
        ]
    );
}
