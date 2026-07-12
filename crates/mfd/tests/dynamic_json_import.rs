use std::path::{Path, PathBuf};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_dynamic_json_{}_{}",
            std::process::id(),
            label
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

fn write_fixture(dir: &Path) -> PathBuf {
    std::fs::write(
        dir.join("departments.schema.json"),
        r#"{
  "title": "Company",
  "type": "object",
  "properties": {
    "Department": { "type": "array", "items": {
      "type": "object",
      "properties": {
        "Name": { "type": "string" },
        "Person": { "type": "array", "items": {
          "type": "object",
          "properties": {
            "First": { "type": "string" },
            "Title": { "type": "string" }
          }
        } }
      }
    } }
  }
}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("departments.json"),
        r#"{"Department":[
  {"Name":"Engineering","Person":[{"First":"Ada","Title":"Manager"},{"First":"Linus","Title":"Engineer"}]},
  {"Name":"Sales","Person":[{"First":"Grace","Title":"Director"}]}
]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("object.schema.json"),
        r#"{"title":"Object","type":"object"}"#,
    )
    .unwrap();

    let design = dir.join("dynamic.mfd");
    std::fs::write(
        &design,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="26"><component name="map" editable="1"><structure><children>
  <component name="constant" library="core" kind="2"><targets><datapoint key="40"/></targets><data><constant value="Name" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="41"/></targets><data><constant value="Details" datatype="string"/></data></component>
  <component name="group-by" library="core" kind="5">
    <sources><datapoint pos="0" key="10"/><datapoint pos="1" key="11"/></sources>
    <targets><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></targets>
  </component>
  <component name="source" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">
    <entry name="Department" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="1">
      <entry name="Name" type="json-property"><entry name="string" outkey="2"/></entry>
      <entry name="Person" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object">
        <entry name="First" type="json-property"><entry name="string" outkey="3"/></entry>
        <entry name="Title" type="json-property"><entry name="string" outkey="4"/></entry>
      </entry></entry></entry></entry>
    </entry></entry></entry></entry>
  </entry></entry></entry></entry></root><json schema="departments.schema.json" inputinstance="departments.json"/></data></component>
  <component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">
    <entry name="property" type="json-property" inpkey="30"><entry name="name" type="json-propertyname" inpkey="31"/><entry name="array"><entry name="item" type="json-item" inpkey="32"><entry name="object">
      <entry name="property" type="json-property"><entry name="name" type="json-propertyname" inpkey="33"/><entry name="string" inpkey="34"/></entry>
      <entry name="property" type="json-property" clone="1"><entry name="name" type="json-propertyname" inpkey="35"/><entry name="string" inpkey="36"/></entry>
    </entry></entry></entry></entry>
  </entry></entry></entry></entry></root><json schema="object.schema.json" outputinstance="out.json"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="11"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="3"><edges><edge vertexkey="32"/><edge vertexkey="34"/></edges></vertex>
  <vertex vertexkey="4"><edges><edge vertexkey="36"/></edges></vertex>
  <vertex vertexkey="40"><edges><edge vertexkey="33"/></edges></vertex>
  <vertex vertexkey="41"><edges><edge vertexkey="35"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
    design
}

#[test]
fn imports_executes_and_serializes_computed_json_properties_in_order() {
    let dir = TempDir::new("executes");
    let design = write_fixture(&dir.0);
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.target.dynamic_fields().is_some());

    let source =
        format_json::read(&dir.0.join("departments.json"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let output_path = dir.0.join("out.json");
    format_json::write(&output_path, &imported.project.target, &output).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output_path).unwrap()).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(
        object.keys().map(String::as_str).collect::<Vec<_>>(),
        ["Engineering", "Sales"]
    );
    assert_eq!(object["Engineering"][0]["Name"], "Ada");
    assert_eq!(object["Engineering"][0]["Details"], "Manager");
    assert_eq!(object["Sales"][0]["Name"], "Grace");
}

#[test]
fn late_dynamic_resolution_failure_leaves_root_scope_unchanged() {
    let dir = TempDir::new("atomic");
    let design = write_fixture(&dir.0);
    let text = std::fs::read_to_string(&design).unwrap().replace(
        r#"<vertex vertexkey="4"><edges><edge vertexkey="36"/></edges></vertex>"#,
        r#"<vertex vertexkey="4"><edges><edge vertexkey="999"/></edges></vertex>"#,
    );
    std::fs::write(&design, text).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("dynamic JSON target")
            && warning.contains("computed item property value input is not connected")
    }));
    let root = &imported.project.root;
    assert!(root.source.is_none());
    assert!(root.group_by.is_none());
    assert!(!root.merge_dynamic_fields);
    assert!(root.dynamic_bindings.is_empty());
    assert!(root.dynamic_children.is_empty());
}

#[test]
fn export_rejects_dynamic_mapping_without_publishing_artifacts() {
    let dir = TempDir::new("export");
    let project = mfd::import(&write_fixture(&dir.0)).unwrap().project;
    let output = dir.0.join("dynamic-export.mfd");

    assert!(matches!(
        mfd::export(&project, &output),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("computed JSON property mappings")
    ));
    assert!(!output.exists());
}
