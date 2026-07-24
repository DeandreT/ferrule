use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, SchemaKind};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-json-profile-{}-{}",
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

#[test]
fn imports_nullable_and_open_json_schema_without_fallback_warnings()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("source.schema.json"),
        r#"{
  "title":"Envelope",
  "type":"object",
  "additionalProperties":false,
  "properties":{
    "MaybeObject":{
      "anyOf":[
        {
          "type":"object",
          "properties":{"Code":{"type":"string"}},
          "additionalProperties":true
        },
        {"type":"null"}
      ]
    },
    "MaybeArray":{
      "type":["array","null"],
      "items":{"type":"object","properties":{"Id":{"type":"integer"}}}
    },
    "Amount":{
      "oneOf":[
        {"type":"number","minimum":0},
        {"type":"null"}
      ]
    }
  }
}"#,
    )?;
    std::fs::write(
        directory.0.join("input.json"),
        r#"{"MaybeObject":{"Code":"A","nested":{"enabled":true}},"MaybeArray":[],"Amount":12.5}"#,
    )?;
    let design = directory.0.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">
      <entry name="Amount" type="json-property"><entry name="number" outkey="10"/></entry>
    </entry></entry></entry></entry></root>
    <json schema="source.schema.json" inputinstance="input.json"/>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Amount" inpkey="20"/></entry></root>
    <document outputinstance="output.xml" instanceroot="{}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let object = imported
        .project
        .source
        .child("MaybeObject")
        .ok_or("missing nullable object")?;
    assert!(object.container_nullable);
    let dynamic = object.dynamic_fields().ok_or("missing dynamic value")?;
    assert!(dynamic.json_any);
    let array = imported
        .project
        .source
        .child("MaybeArray")
        .ok_or("missing nullable array")?;
    assert!(array.container_nullable);
    assert!(array.repeating);
    let amount = imported
        .project
        .source
        .child("Amount")
        .ok_or("missing nullable amount")?;
    assert!(amount.nullable);
    assert!(matches!(amount.kind, SchemaKind::Scalar { .. }));

    let input = format_json::read(&directory.0.join("input.json"), &imported.project.source)?;
    assert!(matches!(input, Instance::Group(_)));
    let output = engine::run(&imported.project, &input)?;
    assert!(output.field("Amount").is_some());
    Ok(())
}
