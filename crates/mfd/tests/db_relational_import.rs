use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaKind, Value, ValueGeneration};
use mapping::Node;
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_relational_{tag}_{}",
            std::process::id()
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

fn write_design(path: &Path, table_entries: &str, source_key: u32) {
    let text = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="22">
  <resources><datasources><datasource name="test-db">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="test.sqlite" name="test-db" path="test-db"/>
  </datasource></datasources></resources>
  <component name="defaultmap" uid="1" editable="1">
    <structure><children>
      <component name="database" library="db" uid="2" kind="15">
        <properties/><data>
          <root><header><namespaces><namespace/></namespaces></header>
            <entry name="document" expanded="1">{table_entries}</entry>
          </root>
          <database ref="test-db"/>
        </data>
      </component>
      <component name="Report" library="xml" uid="3" kind="14">
        <properties XSLTDefaultOutput="1"/><data>
          <root><header><namespaces><namespace/></namespaces></header>
            <entry name="document" expanded="1">
              <entry name="Report" expanded="1"><entry name="Value" inpkey="900"/></entry>
            </entry>
          </root>
        </data>
      </component>
    </children>
    <graph directed="1"><edges/><vertices>
      <vertex vertexkey="{source_key}"><edges><edge vertexkey="900"/></edges></vertex>
    </vertices></graph></structure>
  </component>
</mapping>"#
    );
    std::fs::write(path, text).unwrap();
}

fn write_relational_target_design(path: &Path) {
    let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="22">
  <resources><datasources><datasource name="test-db">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="test.sqlite" name="test-db" path="test-db"/>
  </datasource></datasources></resources>
  <component name="defaultmap" uid="1" editable="1">
    <structure><children>
      <component name="Source" library="xml" uid="2" kind="14">
        <properties/><data><root><header><namespaces><namespace/></namespaces></header>
          <entry name="document"><entry name="Source"><entry name="Value" outkey="10"/></entry></entry>
        </root></data>
      </component>
      <component name="database" library="db" uid="3" kind="15">
        <properties XSLTDefaultOutput="1"/><data>
          <root><header><namespaces><namespace/></namespaces></header>
            <entry name="document"><entry name="departments" type="table" inpkey="20">
              <entry name="id" inpkey="21"/>
              <entry name="people|department_id" type="table" inpkey="22">
                <entry name="name" inpkey="23"/>
              </entry>
            </entry></entry>
          </root><database ref="test-db"/>
        </data>
      </component>
    </children><graph directed="1"><edges/><vertices>
      <vertex vertexkey="10"><edges><edge vertexkey="23"/></edges></vertex>
    </vertices></graph></structure>
  </component>
</mapping>"#;
    std::fs::write(path, text).unwrap();
}

fn field<'a>(instance: &'a Instance, name: &str) -> &'a Instance {
    instance
        .field(name)
        .unwrap_or_else(|| panic!("missing field `{name}`"))
}

#[test]
fn imports_nested_tables_with_relational_names_ports_and_types() {
    let dir = TempDir::new("nested");
    let db_path = dir.0.join("test.sqlite");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON; \
             CREATE TABLE departments (id INTEGER PRIMARY KEY, name TEXT); \
             CREATE TABLE people (id INTEGER PRIMARY KEY, department_id INTEGER, name TEXT, \
                 FOREIGN KEY(department_id) REFERENCES departments(id)); \
             INSERT INTO departments VALUES (1, 'Engineering'), (2, 'Sales'); \
             INSERT INTO people VALUES (10, 1, 'Ada'), (11, 1, 'Grace'), (12, 2, 'Linus');",
        )
        .unwrap();
    drop(connection);
    let design = dir.0.join("nested.mfd");
    write_design(
        &design,
        r#"<entry name="departments" type="table" outkey="1" expanded="1">
          <entry name="id" outkey="2"/><entry name="name" outkey="3"/>
          <entry name="people|department_id" type="table" outkey="4" expanded="1">
            <entry name="id" outkey="5"/><entry name="department_id" outkey="6"/>
            <entry name="name" outkey="7"/>
          </entry>
        </entry>"#,
        7,
    );

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = &imported.project.source;
    assert_eq!(source.name, "departments");
    assert!(source.repeating);
    assert!(matches!(
        source.child("id").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    let people = source.child("people|department_id").unwrap();
    assert!(people.repeating);
    assert!(matches!(
        people.child("name").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(node, Node::SourceField { path, .. }
            if path == &["people|department_id".to_string(), "name".to_string()])
    }));

    let instance = format_db::read_instance(&db_path, source).unwrap();
    let departments = instance.as_repeated().unwrap();
    let engineering = field(&departments[0], "people|department_id")
        .as_repeated()
        .unwrap();
    assert_eq!(engineering.len(), 2);
    assert_eq!(
        field(&engineering[0], "name").as_scalar(),
        Some(&Value::String("Ada".into()))
    );
    let mut project = imported.project;
    project
        .root
        .set_source(Some(vec!["people|department_id".into()]));
    assert!(engine::validate(&project).is_empty());
    let output = engine::run(&project, &instance).unwrap();
    let rows = output.as_repeated().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        field(&rows[0], "Value").as_scalar(),
        Some(&Value::String("Ada".into()))
    );
    assert_eq!(
        field(&rows[1], "Value").as_scalar(),
        Some(&Value::String("Grace".into()))
    );
    assert_eq!(
        field(&rows[2], "Value").as_scalar(),
        Some(&Value::String("Linus".into()))
    );
}

#[test]
fn imports_multiple_top_level_tables_below_a_composite_root() {
    let dir = TempDir::new("composite");
    let db_path = dir.0.join("test.sqlite");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE departments (id INTEGER, name TEXT); \
             CREATE TABLE offices (id INTEGER, city TEXT); \
             INSERT INTO departments VALUES (1, 'Engineering'); \
             INSERT INTO offices VALUES (7, 'Seattle');",
        )
        .unwrap();
    drop(connection);
    let design = dir.0.join("composite.mfd");
    write_design(
        &design,
        r#"<entry name="departments" type="table" outkey="1" expanded="1">
          <entry name="id" outkey="2"/><entry name="name" outkey="3"/>
        </entry>
        <entry name="offices" type="table" outkey="4" expanded="1">
          <entry name="id" outkey="5"/><entry name="city" outkey="6"/>
        </entry>"#,
        6,
    );

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = &imported.project.source;
    assert_eq!(source.name, "database");
    assert!(!source.repeating);
    let departments = source.child("departments").unwrap();
    let offices = source.child("offices").unwrap();
    assert!(departments.repeating);
    assert!(offices.repeating);
    assert!(matches!(
        offices.child("id").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(node, Node::SourceField { path, .. }
            if path == &["offices".to_string(), "city".to_string()])
    }));

    let instance = format_db::read_instance(&db_path, source).unwrap();
    let office_rows = field(&instance, "offices").as_repeated().unwrap();
    assert_eq!(office_rows.len(), 1);
    assert_eq!(
        field(&office_rows[0], "city").as_scalar(),
        Some(&Value::String("Seattle".into()))
    );
}

#[test]
fn ignores_disconnected_selected_tables_when_connected_tables_exist() {
    let dir = TempDir::new("disconnected_table");
    let db_path = dir.0.join("test.sqlite");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE unused (id INTEGER); \
             CREATE TABLE departments (id INTEGER, name TEXT); \
             INSERT INTO departments VALUES (1, 'Engineering');",
        )
        .unwrap();
    drop(connection);
    let design = dir.0.join("disconnected-table.mfd");
    write_design(
        &design,
        r#"<entry name="unused" type="table"/>
        <entry name="departments" type="table" outkey="1">
          <entry name="name" outkey="2"/>
        </entry>"#,
        2,
    );

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source.name, "departments");
    assert!(imported.project.source.child("name").is_some());
    assert!(imported.project.source.child("unused").is_none());
}

#[test]
fn missing_untyped_database_keeps_the_fallback_warning() {
    let dir = TempDir::new("missing_untyped");
    let design = dir.0.join("missing-untyped.mfd");
    write_design(
        &design,
        r#"<entry name="departments" type="table" outkey="1">
          <entry name="id" outkey="2"/>
        </entry>"#,
        2,
    );

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("database `test.sqlite` not found next to the design")
            && warning.contains("falling back to untyped columns")
    }));
    assert!(matches!(
        imported.project.source.child("id").map(|field| &field.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
}

#[test]
fn warns_when_nested_relationship_metadata_is_missing() {
    let dir = TempDir::new("missing_relation");
    let db_path = dir.0.join("test.sqlite");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE departments (id INTEGER PRIMARY KEY, name TEXT); \
             CREATE TABLE people (id INTEGER PRIMARY KEY, department_id INTEGER, name TEXT);",
        )
        .unwrap();
    drop(connection);
    let design = dir.0.join("missing-relation.mfd");
    write_design(
        &design,
        r#"<entry name="departments" type="table" outkey="1">
          <entry name="id" outkey="2"/>
          <entry name="people|department_id" type="table" outkey="3">
            <entry name="name" outkey="4"/>
          </entry>
        </entry>"#,
        4,
    );

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("relational schema does not match SQLite foreign-key metadata")
            && warning.contains("people")
            && warning.contains("department_id")
    }));
}

#[test]
fn relational_database_targets_use_the_executable_writer() {
    let dir = TempDir::new("target");
    let db_path = dir.0.join("test.sqlite");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON; \
             CREATE TABLE departments (id INTEGER PRIMARY KEY); \
             CREATE TABLE people (id INTEGER PRIMARY KEY, department_id INTEGER, name TEXT, \
                 FOREIGN KEY(department_id) REFERENCES departments(id));",
        )
        .unwrap();
    drop(connection);
    let design = dir.0.join("target.mfd");
    write_relational_target_design(&design);

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.iter().all(|warning| {
        !warning.contains("relational database target component `database` is non-executable")
            && !warning.contains("cannot write")
    }));
}

#[test]
fn database_max_number_columns_roundtrip_as_generated_metadata() {
    let dir = TempDir::new("max_number");
    let db_path = dir.0.join("test.sqlite");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch("CREATE TABLE People (Id INTEGER PRIMARY KEY, Name TEXT NOT NULL);")
        .unwrap();
    drop(connection);
    let design = dir.0.join("generated.mfd");
    std::fs::write(
        &design,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="26">
  <resources><datasources><datasource name="test-db">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="test.sqlite" name="test-db" path="test-db"/>
  </datasource></datasources></resources>
  <component name="defaultmap" uid="1" editable="1"><structure><children>
    <component name="Source" library="xml" uid="2" kind="14"><data>
      <root><header><namespaces><namespace/></namespaces></header>
        <entry name="document"><entry name="Rows" outkey="10">
          <entry name="Name" outkey="11"/>
        </entry></entry>
      </root>
    </data></component>
    <component name="database" library="db" uid="3" kind="15">
      <properties XSLTDefaultOutput="1"/><data>
        <root><header><namespaces><namespace/></namespaces></header>
          <entry name="document"><entry name="People" type="table" inpkey="20">
            <entry name="Id" valuekeygeneration="maxnumber"/>
            <entry name="Name" inpkey="21"/>
          </entry></entry>
        </root><database ref="test-db"/>
      </data>
    </component>
  </children><graph directed="1"><edges/><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported
            .project
            .target
            .child("Id")
            .and_then(|column| column.value_generation),
        Some(ValueGeneration::MaxNumber)
    );

    let exported_path = dir.0.join("generated-roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&exported_path).unwrap();
    assert!(text.contains(r#"name="Id" valuekeygeneration="maxnumber""#));

    let reimported = mfd::import(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported
            .project
            .target
            .child("Id")
            .and_then(|column| column.value_generation),
        Some(ValueGeneration::MaxNumber)
    );
}
