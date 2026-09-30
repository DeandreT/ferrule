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
fn optional_prefix_and_computed_projection_round_trip_with_host_overrides()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new();
    let imported = mfd::import(&fixture(&directory.0))?;
    let mut project = imported.project;
    let default = project.graph.nodes.keys().max().copied().unwrap() + 1;
    let prefix = project
        .graph
        .nodes
        .iter()
        .find_map(|(id, node)| {
            matches!(node, mapping::Node::Const { value: Value::String(value) } if value == "B")
                .then_some(*id)
        })
        .unwrap();
    project.graph.nodes.insert(
        default,
        mapping::Node::Const {
            value: Value::String("B".into()),
        },
    );
    project.graph.nodes.insert(
        prefix,
        mapping::Node::RuntimeParameterDefault {
            name: "NamePrefix".into(),
            ty: ir::ScalarType::String,
            default,
            preview: None,
        },
    );
    // A computed row binding should retain its normal native graph wiring.
    let department = project.root.children[0].bindings[1].node;
    let label = default + 1;
    project.graph.nodes.insert(
        label,
        mapping::Node::Call {
            function: "concat".into(),
            args: vec![department, department],
        },
    );
    project.root.children[0].bindings[1].node = label;
    assert!(engine::validate(&project).is_empty());
    let source = format_db::read_instance(&directory.0.join("people.sqlite"), &project.source)?;
    let native = directory.0.join("optional.mfd");
    let report = mfd::preflight_export(&project, &native)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &native, mfd::ExportProfile::NativeMfd)?;
    let restored = mfd::import(&native)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    assert!(engine::validate(&restored.project).is_empty());
    assert_eq!(
        names(&engine::run(&restored.project, &source)?),
        ["Bob", "Bex", "Bea"]
    );
    for prefix in ["G", "b", "_", "%", "B%\0", "B\0"] {
        let mut parameters = engine::RuntimeParameters::new();
        parameters.insert("NamePrefix", Value::String(prefix.into()))?;
        let context = engine::ExecutionContext::new(&native).with_parameters(&parameters);
        let before = engine::run_with_context(&project, &source, &context)?;
        let after = engine::run_with_context(&restored.project, &source, &context)?;
        assert_eq!(after, before, "prefix {prefix:?}");
        let connection = Connection::open(directory.0.join("people.sqlite"))?;
        let sql_names = connection
            .prepare("SELECT Name FROM People WHERE Name LIKE ?1 ORDER BY Name DESC")?
            .query_map([format!("{prefix}%")], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(names(&after), sql_names, "prefix {prefix:?}");
    }
    let mut too_long = engine::RuntimeParameters::new();
    let prefix = "B".repeat(50_000);
    too_long.insert("NamePrefix", Value::String(prefix.clone()))?;
    let context = engine::ExecutionContext::new(&native).with_parameters(&too_long);
    assert!(engine::run_with_context(&project, &source, &context).is_err());
    assert!(engine::run_with_context(&restored.project, &source, &context).is_err());
    let connection = Connection::open(directory.0.join("people.sqlite"))?;
    assert!(
        connection
            .query_row(
                "SELECT count(*) FROM People WHERE Name LIKE ?1",
                [format!("{prefix}%")],
                |row| row.get::<_, i64>(0),
            )
            .is_err()
    );
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("NamePrefix", Value::Null)?;
    let context = engine::ExecutionContext::new(&native).with_parameters(&parameters);
    assert_eq!(
        engine::run_with_context(&restored.project, &source, &context)?,
        engine::run_with_context(&project, &source, &context)?,
    );
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn local_phone_list_native_roundtrip_keeps_optional_prefix_and_related_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let imported = mfd::import(&samples.join("DB_PhoneList.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source =
        format_db::read_instance(&samples.join("Altova.sqlite"), &imported.project.source)?;
    let directory = TempDir::new();
    let native = directory.0.join("phone-list.mfd");
    let report = mfd::preflight_export(&imported.project, &native)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&imported.project, &native, mfd::ExportProfile::NativeMfd)?;
    let restored = mfd::import(&native)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    assert!(engine::validate(&restored.project).is_empty());
    let last_names = |output: &Instance| {
        output
            .field("Person")
            .and_then(Instance::as_repeated)
            .unwrap()
            .iter()
            .map(|person| {
                person
                    .field("Last")
                    .and_then(Instance::as_scalar)
                    .unwrap()
                    .clone()
            })
            .collect::<Vec<_>>()
    };
    let before = engine::run(&imported.project, &source)?;
    assert_eq!(
        last_names(&before),
        ["Bander", "Bass", "Bone", "Butler"].map(|name| Value::String(name.into()))
    );
    let after = engine::run(&restored.project, &source)?;
    let xml_bytes = |project: &mapping::Project, value: &Instance, name: &str| {
        let path = directory.0.join(name);
        format_xml::write(&path, &project.target, value)?;
        std::fs::read(path).map_err(Box::<dyn std::error::Error>::from)
    };
    assert_eq!(
        xml_bytes(&restored.project, &after, "default-after.xml")?,
        xml_bytes(&imported.project, &before, "default-before.xml")?,
    );
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("NamePrefix", Value::String("F".into()))?;
    let context = engine::ExecutionContext::new(&native).with_parameters(&parameters);
    let before = engine::run_with_context(&imported.project, &source, &context)?;
    assert_eq!(
        last_names(&before),
        ["Firstbread", "Franken", "Further"].map(|name| Value::String(name.into()))
    );
    let after = engine::run_with_context(&restored.project, &source, &context)?;
    assert_eq!(
        xml_bytes(&restored.project, &after, "override-after.xml")?,
        xml_bytes(&imported.project, &before, "override-before.xml")?,
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
    let preview =
        engine::ExecutionContext::new(&sample).with_purpose(engine::ExecutionPurpose::Preview);
    assert_eq!(
        engine::run_with_context(&reimported.project, &input, &preview)?,
        engine::run_with_context(&imported.project, &input, &preview)?
    );
    assert_eq!(
        serde_json::to_value(&reimported.project.root)?,
        serde_json::to_value(&imported.project.root)?
    );
    Ok(())
}
