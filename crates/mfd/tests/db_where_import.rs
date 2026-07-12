use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("ferrule_mfd_db_where_{tag}_{}", std::process::id()));
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
            "CREATE TABLE People (Name TEXT, Department TEXT); \
             INSERT INTO People VALUES \
               ('Ada', 'Engineering'), ('Grace', 'Engineering'), \
               ('Bob', 'Sales'), ('Bex', 'Sales'), ('Bea', 'Sales');",
        )
        .unwrap();
}

fn write_design(
    path: &Path,
    condition: &str,
    order: &str,
    parameter_components: &str,
    parameter_feed: u32,
) {
    let text = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="22">
  <resources><datasources><datasource name="people">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="people.sqlite" name="people" path="people"/>
  </datasource></datasources></resources>
  <component name="defaultmap" uid="1" editable="1"><structure><children>
    <component name="database" library="db" uid="2" kind="15"><properties/><data>
      <root><header><namespaces><namespace/></namespaces></header><entry name="document">
        <entry name="People" type="table" outkey="10">
          <entry name="Name" outkey="11"/><entry name="Department" outkey="12"/>
        </entry>
      </entry></root><database ref="people"/>
    </data></component>
    {parameter_components}
    <component name="People where" library="db" uid="20" kind="21">
      <sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources>
      <targets><datapoint pos="0" key="30"/></targets>
      <data><where condition="{condition}" order="{order}"><parameters>
        <parameter name="bound" type="string"/>
      </parameters></where></data>
    </component>
    <component name="rows" library="text" uid="3" kind="16"><properties XSLTDefaultOutput="1"/><data>
      <root><header><namespaces><namespace/></namespaces></header>
        <entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="50">
          <entry name="Name" inpkey="51"/>
        </entry></entry></entry>
      </root>
      <text type="csv" outputinstance="out.csv"><settings separator="," quote="&quot;" firstrownames="true">
        <names root="rows" block="Rows"><field0 name="Name" type="string"/></names>
      </settings></text>
    </data></component>
  </children><graph directed="1"><edges/><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="{parameter_feed}"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="30"><edges><edge vertexkey="50"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="51"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#
    );
    std::fs::write(path, text).unwrap();
}

fn output_names(project: &mapping::Project, db_path: &Path) -> Vec<String> {
    let source = format_db::read_instance(db_path, &project.source).unwrap();
    engine::run(project, &source)
        .unwrap()
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            row.field("Name")
                .and_then(Instance::as_scalar)
                .and_then(|value| match value {
                    Value::String(value) => Some(value.clone()),
                    _ => None,
                })
                .unwrap()
        })
        .collect()
}

#[test]
fn qualified_equality_filters_and_descending_order_execute() {
    let dir = TempDir::new("equality");
    prepare_database(&dir.0);
    let design = dir.0.join("equality.mfd");
    write_design(
        &design,
        "people.department = :bound",
        "name DESC",
        r#"<component name="constant" library="core" uid="10" kind="2">
          <targets><datapoint pos="0" key="40"/></targets>
          <data><constant value="Engineering" datatype="string"/></data>
        </component>"#,
        40,
    );

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        output_names(&imported.project, &dir.0.join("people.sqlite")),
        ["Grace", "Ada"]
    );
}

#[test]
fn like_uses_a_bound_concat_expression_and_single_character_wildcard() {
    let dir = TempDir::new("like");
    prepare_database(&dir.0);
    let design = dir.0.join("like.mfd");
    write_design(
        &design,
        "Name LIKE :bound",
        "Name ASC",
        r#"<component name="constant" library="core" uid="10" kind="2">
          <targets><datapoint pos="0" key="40"/></targets>
          <data><constant value="B" datatype="string"/></data>
        </component>
        <component name="constant" library="core" uid="11" kind="2">
          <targets><datapoint pos="0" key="41"/></targets>
          <data><constant value="_%" datatype="string"/></data>
        </component>
        <component name="concat" library="core" uid="12" kind="5">
          <sources><datapoint pos="0" key="42"/><datapoint pos="1" key="43"/></sources>
          <targets><datapoint pos="0" key="44"/></targets>
        </component>"#,
        44,
    );
    let mut text = std::fs::read_to_string(&design).unwrap();
    text = text.replace(
        "<vertex vertexkey=\"44\">",
        "<vertex vertexkey=\"40\"><edges><edge vertexkey=\"42\"/></edges></vertex>\n\
         <vertex vertexkey=\"41\"><edges><edge vertexkey=\"43\"/></edges></vertex>\n\
         <vertex vertexkey=\"44\">",
    );
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        output_names(&imported.project, &dir.0.join("people.sqlite")),
        ["Bea", "Bex", "Bob"]
    );
}

#[test]
fn unsupported_condition_warns_once_and_does_not_pass_rows_through() {
    let dir = TempDir::new("unsupported");
    prepare_database(&dir.0);
    let design = dir.0.join("unsupported.mfd");
    write_design(
        &design,
        "Department = :bound OR Name = :bound",
        "Name",
        r#"<component name="constant" library="core" uid="10" kind="2">
          <targets><datapoint pos="0" key="40"/></targets>
          <data><constant value="Engineering" datatype="string"/></data>
        </component>"#,
        40,
    );

    let imported = mfd::import(&design).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("condition must be"));
    assert!(imported.warnings[0].contains("iteration into `` skipped"));
    assert!(imported.project.root.source.is_none());
}

#[test]
fn multiple_order_keys_warn_once_and_do_not_pass_rows_through() {
    let dir = TempDir::new("unsupported_order");
    prepare_database(&dir.0);
    let design = dir.0.join("unsupported-order.mfd");
    write_design(
        &design,
        "Department = :bound",
        "Name, Department",
        r#"<component name="constant" library="core" uid="10" kind="2">
          <targets><datapoint pos="0" key="40"/></targets>
          <data><constant value="Engineering" datatype="string"/></data>
        </component>"#,
        40,
    );

    let imported = mfd::import(&design).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("order must contain one identifier"));
    assert!(imported.project.root.source.is_none());
}

#[test]
fn database_order_combined_with_another_sort_warns_and_skips_iteration() {
    let dir = TempDir::new("combined_order");
    prepare_database(&dir.0);
    let design = dir.0.join("combined-order.mfd");
    write_design(
        &design,
        "Department = :bound",
        "Name ASC",
        r#"<component name="constant" library="core" uid="10" kind="2">
          <targets><datapoint pos="0" key="40"/></targets>
          <data><constant value="Engineering" datatype="string"/></data>
        </component>
        <component name="sort" library="core" uid="11" kind="30">
          <sources><datapoint pos="0" key="60"/><datapoint pos="1" key="61"/></sources>
          <targets><datapoint pos="0" key="62"/></targets>
          <data><sort><collation/><key direction="descending"/></sort></data>
        </component>"#,
        40,
    );
    let mut text = std::fs::read_to_string(&design).unwrap();
    text = text.replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"50\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"60\"/></edges></vertex>\n\
         <vertex vertexkey=\"62\"><edges><edge vertexkey=\"50\"/></edges></vertex>",
    );
    text = text.replace(
        "<vertex vertexkey=\"11\"><edges><edge vertexkey=\"51\"/></edges></vertex>",
        "<vertex vertexkey=\"11\"><edges><edge vertexkey=\"61\"/><edge vertexkey=\"51\"/></edges></vertex>",
    );
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("combines database ORDER"));
    assert!(imported.project.root.source.is_none());
}
