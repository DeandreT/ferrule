use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, NumericRange, SchemaKind};

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
      "minItems":1,
      "maxItems":2,
      "items":{"type":"object","properties":{"Id":{"type":"integer"}}}
    },
    "Amount":{
      "oneOf":[
        {"type":"number","minimum":0},
        {"type":"null"}
      ],
      "exclusiveMaximum":20
    },
    "Status":{"type":"string","const":"ready"}
  }
}"#,
    )?;
    std::fs::write(
        directory.0.join("input.json"),
        r#"{"MaybeObject":{"Code":"A","nested":{"enabled":true}},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready"}"#,
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
    let range = array
        .item_count_range
        .ok_or("missing nullable array item-count range")?;
    assert_eq!(range.minimum(), 1);
    assert_eq!(range.maximum(), Some(2));
    let amount = imported
        .project
        .source
        .child("Amount")
        .ok_or("missing nullable amount")?;
    assert!(amount.nullable);
    assert!(matches!(amount.kind, SchemaKind::Scalar { .. }));
    let Some(NumericRange::Number(amount_range)) = amount.numeric_range else {
        return Err("missing nullable amount range".into());
    };
    assert_eq!(
        amount_range.minimum().map(|bound| bound.value().get()),
        Some(0.0)
    );
    assert_eq!(
        amount_range.maximum().map(|bound| bound.value().get()),
        Some(20.0)
    );
    assert!(
        amount_range
            .maximum()
            .is_some_and(|bound| bound.is_exclusive())
    );
    assert_eq!(
        imported
            .project
            .source
            .child("Status")
            .and_then(|status| status.fixed.as_deref()),
        Some("ready")
    );

    let input = format_json::read(&directory.0.join("input.json"), &imported.project.source)?;
    assert!(matches!(input, Instance::Group(_)));
    let output = engine::run(&imported.project, &input)?;
    assert!(output.field("Amount").is_some());
    Ok(())
}

#[test]
fn package_root_confines_external_json_schema_references() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TempDir::new()?;
    let maps = directory.0.join("maps");
    let schemas = directory.0.join("schemas");
    let shared = directory.0.join("shared");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&schemas)?;
    std::fs::create_dir_all(&shared)?;
    std::fs::write(
        shared.join("types.schema.json"),
        r#"{
  "$defs":{
    "Row":{
      "type":"object",
      "properties":{"Code":{"type":"string"}},
      "required":["Code"],
      "additionalProperties":false
    }
  }
}"#,
    )?;
    std::fs::write(
        schemas.join("source.schema.json"),
        r#"{
  "title":"Envelope",
  "type":"object",
  "properties":{"Row":{"$ref":"../shared/types.schema.json#/$defs/Row"}},
  "required":["Row"],
  "additionalProperties":false
}"#,
    )?;
    std::fs::write(maps.join("input.json"), r#"{"Row":{"Code":"A"}}"#)?;
    let design = maps.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root">
      <entry name="object"><entry name="Row" type="json-property"><entry name="object">
        <entry name="Code" type="json-property"><entry name="string" outkey="10"/></entry>
      </entry></entry></entry>
    </entry></entry></entry></root>
    <json schema="../schemas/source.schema.json" inputinstance="input.json"/>
  </data></component>
  <component name="target" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Output"><entry name="Code" inpkey="20"/></entry></root>
      <document outputinstance="output.xml" instanceroot="{}Output"/>
    </data>
  </component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let options = mfd::ImportOptions::default().with_package_root(&directory.0);
    let imported = mfd::import_with_options(&design, &options)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported
            .project
            .source
            .child("Row")
            .and_then(|row| row.child("Code"))
            .is_some()
    );
    let source = format_json::read(&maps.join("input.json"), &imported.project.source)?;
    let target = engine::run(&imported.project, &source)?;
    assert_eq!(
        target.field("Code").and_then(Instance::as_scalar),
        Some(&ir::Value::String("A".into()))
    );
    Ok(())
}
