use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_db_substitute_null_{}_{}",
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

fn row(present: &str, optional: Value) -> Instance {
    Instance::Group(vec![
        (
            "Present".into(),
            Instance::Scalar(Value::String(present.into())),
        ),
        ("Optional".into(), Instance::Scalar(optional)),
    ])
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> &'a Value {
    instance.field(field).and_then(Instance::as_scalar).unwrap()
}

fn write_design(path: &Path) {
    std::fs::write(
        path,
        r#"<mapping version="26">
  <resources><datasources><datasource name="people">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="people.sqlite" name="people" path="people"/>
  </datasource></datasources></resources>
  <component name="map"><structure><children>
    <component name="database" library="db" kind="15"><data>
      <root><entry name="document"><entry name="People" type="table" outkey="10">
        <entry name="Present" outkey="11"/><entry name="Optional" outkey="12"/>
      </entry></entry></root><database ref="people"/>
    </data></component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint key="20"/></targets>
      <data><constant value="fallback-present" datatype="string"/></data>
    </component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint key="21"/></targets>
      <data><constant value="fallback-null" datatype="string"/></data>
    </component>
    <component name="substitute-null" library="db" kind="5">
      <sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources>
      <targets><datapoint pos="0" key="32"/></targets>
    </component>
    <component name="substitute-null" library="db" kind="5">
      <sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources>
      <targets><datapoint pos="0" key="42"/></targets>
    </component>
    <component name="rows" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="50">
        <entry name="PresentResult" inpkey="51"/><entry name="OptionalResult" inpkey="52"/>
      </entry></entry></entry></root>
      <text type="csv"><settings separator="," quote="&quot;" firstrownames="true">
        <names root="rows" block="Rows"><field0 name="PresentResult" type="string"/><field1 name="OptionalResult" type="string"/></names>
      </settings></text>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="50"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex>
    <vertex vertexkey="32"><edges><edge vertexkey="51"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="40"/></edges></vertex>
    <vertex vertexkey="21"><edges><edge vertexkey="41"/></edges></vertex>
    <vertex vertexkey="42"><edges><edge vertexkey="52"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )
    .unwrap();
}

#[test]
fn database_substitute_null_preserves_values_and_replaces_nulls() {
    let dir = TempDir::new();
    let schema = SchemaNode::group(
        "People",
        vec![
            SchemaNode::scalar("Present", ScalarType::String),
            SchemaNode::scalar("Optional", ScalarType::String),
        ],
    )
    .repeating();
    format_db::write(
        &dir.0.join("people.sqlite"),
        &schema,
        &[
            row("kept-1", Value::String("also-present".into())),
            row("kept-2", Value::Null),
        ],
    )
    .unwrap();
    let design = dir.0.join("mapping.mfd");
    write_design(&design);

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source =
        format_db::read_instance(&dir.0.join("people.sqlite"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let rows = target.as_repeated().unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(
        scalar(&rows[0], "PresentResult"),
        &Value::String("kept-1".into())
    );
    assert_eq!(
        scalar(&rows[0], "OptionalResult"),
        &Value::String("also-present".into())
    );
    assert_eq!(
        scalar(&rows[1], "PresentResult"),
        &Value::String("kept-2".into())
    );
    assert_eq!(
        scalar(&rows[1], "OptionalResult"),
        &Value::String("fallback-null".into())
    );
}
