use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use rusqlite::Connection;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_db_where_export_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn fixture(directory: &Path) -> PathBuf {
    let database = directory.join("people.sqlite");
    Connection::open(&database)
        .unwrap()
        .execute_batch(
            "CREATE TABLE People (Name TEXT, Department TEXT); \
             INSERT INTO People VALUES \
               ('Ada', 'Engineering'), ('Grace', 'Engineering'), \
               ('Bob', 'Sales'), ('Bex', 'Sales'), ('Bea', 'Sales');",
        )
        .unwrap();
    std::fs::write(
        directory.join("rows.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Rows"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
      <xs:element name="Department" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    let design = directory.join("original.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="22"><resources><datasources><datasource name="people"><database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="people.sqlite" name="people" path="people"/></datasource></datasources></resources>
<component name="defaultmap" uid="1" editable="1"><structure><children>
  <component name="database" library="db" uid="2" kind="15"><data><root><entry name="document"><entry name="People" type="table" outkey="10"><entry name="Name" outkey="11"/><entry name="Department" outkey="12"/></entry></entry></root><database ref="people"/></data></component>
  <component name="constant" library="core" uid="10" kind="2"><targets><datapoint pos="0" key="40"/></targets><data><constant value="B" datatype="string"/></data></component>
  <component name="constant" library="core" uid="11" kind="2"><targets><datapoint pos="0" key="41"/></targets><data><constant value="%" datatype="string"/></data></component>
  <component name="concat" library="core" uid="12" kind="5"><sources><datapoint pos="0" key="42"/><datapoint pos="1" key="43"/></sources><targets><datapoint pos="0" key="44"/></targets></component>
  <component name="People where" library="db" uid="20" kind="21"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="30"/></targets><data><where condition="Name LIKE :bound" order="Name DESC"><parameters><parameter name="bound" type="string"/></parameters></where></data></component>
  <component name="rows" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows"><entry name="Row" inpkey="50"><entry name="Name" inpkey="51"/><entry name="Department" inpkey="52"/></entry></entry></entry></entry></root><document schema="rows.xsd" outputinstance="out.xml" instanceroot="{}Rows"/></data></component>
</children><graph directed="1"><edges/><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="40"><edges><edge vertexkey="42"/></edges></vertex>
  <vertex vertexkey="41"><edges><edge vertexkey="43"/></edges></vertex>
  <vertex vertexkey="44"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="30"><edges><edge vertexkey="50"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="51"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="52"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
    design
}

fn names(output: &Instance) -> Vec<String> {
    output
        .field("Row")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .map(|row| {
            let Value::String(name) = row.field("Name").and_then(Instance::as_scalar).unwrap()
            else {
                panic!("expected a string Name");
            };
            name.clone()
        })
        .collect()
}

#[test]
fn direct_sqlite_like_filter_and_order_round_trip_as_native_where()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new();
    let original_path = fixture(&directory.0);
    let imported = mfd::import(&original_path)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source =
        format_db::read_instance(&directory.0.join("people.sqlite"), &imported.project.source)?;
    let original = engine::run(&imported.project, &source)?;
    assert_eq!(names(&original), ["Bob", "Bex", "Bea"]);

    let native_path = directory.0.join("native.mfd");
    let report = mfd::preflight_export(&imported.project, &native_path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(
        &imported.project,
        &native_path,
        mfd::ExportProfile::NativeMfd,
    )?;
    let xml = std::fs::read_to_string(&native_path)?;
    assert!(xml.contains("library=\"db\"") && xml.contains("kind=\"21\""));
    assert!(xml.contains("condition=\"Name LIKE :sqlparam\""));
    assert!(xml.contains("order=\"Name DESC\""));
    assert!(xml.contains("name=\"concat\" library=\"core\""));
    assert!(!xml.contains("library=\"ferrule\""));
    let reimported = mfd::import(&native_path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert!(reimported.project.graph.nodes.values().any(|node| {
        matches!(node, mapping::Node::Call { function, .. } if function == "concat")
    }));
    let roundtrip = engine::run(&reimported.project, &source)?;
    assert_eq!(roundtrip, original);
    let connection = Connection::open(directory.0.join("people.sqlite"))?;
    assert_eq!(
        connection.query_row("SELECT count(*) FROM People", [], |row| row
            .get::<_, i64>(0))?,
        5
    );
    Ok(())
}

#[test]
fn non_prefix_like_pattern_keeps_the_strict_export_blocker()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new();
    let imported = mfd::import(&fixture(&directory.0))?;
    assert!(imported.warnings.is_empty());
    let mut changed = imported.project;
    let pattern = changed
        .graph
        .nodes
        .values_mut()
        .find_map(|node| match node {
            mapping::Node::Const {
                value: Value::String(value),
            } if value == "%" => Some(value),
            _ => None,
        })
        .unwrap();
    *pattern = "_%".to_string();
    let report = mfd::preflight_export(&changed, &directory.0.join("changed.mfd"))?;
    assert!(!report.is_native_compatible());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == mfd::ExportCompatibilityFeature::FerruleComponent)
    );
    Ok(())
}

#[test]
fn shared_like_expression_is_not_absorbed_into_the_database_control()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new();
    let imported = mfd::import(&fixture(&directory.0))?;
    let mut changed = imported.project;
    let like = changed
        .graph
        .nodes
        .iter()
        .find_map(|(id, node)| match node {
            mapping::Node::Call { function, .. } if function == "sql_like" => Some(*id),
            _ => None,
        })
        .unwrap();
    changed.root.children[0].bindings[0].node = like;
    let report = mfd::preflight_export(&changed, &directory.0.join("shared.mfd"))?;
    assert!(!report.is_native_compatible());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == mfd::ExportCompatibilityFeature::FerruleComponent)
    );
    Ok(())
}

#[test]
#[ignore = "needs the local ReferenceSamples corpus; informational only"]
fn local_filter_database_records_strict_export_reimports_same_output()
-> Result<(), Box<dyn std::error::Error>> {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples/Tutorial/FilterDatabaseRecords.mfd");
    let imported = mfd::import(&sample)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let directory = TempDir::new();
    let native = directory.0.join("native.mfd");
    let report = mfd::preflight_export(&imported.project, &native)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&imported.project, &native, mfd::ExportProfile::NativeMfd)?;
    let reimported = mfd::import(&native)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let input = format_db::read_instance(
        &sample.parent().unwrap().join("Nanonull.sqlite"),
        &imported.project.source,
    )?;
    assert_eq!(
        engine::run(&reimported.project, &input)?,
        engine::run(&imported.project, &input)?
    );
    assert_eq!(
        serde_json::to_value(&reimported.project.root)?,
        serde_json::to_value(&imported.project.root)?
    );
    Ok(())
}
