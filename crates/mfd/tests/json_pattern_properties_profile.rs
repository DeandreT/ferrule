use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, SchemaKind, Value};

#[test]
fn imports_executes_and_roundtrips_structured_pattern_properties()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    write_fixture(directory.path())?;
    let design = directory.path().join("mapping.mfd");

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_pattern_schema(&imported.project.source)?;

    let source = format_json::read(
        &directory.path().join("input.json"),
        &imported.project.source,
    )?;
    let rendered = format_json::to_string(&imported.project.source, &source)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&rendered)?,
        serde_json::from_str::<serde_json::Value>(INPUT)?
    );
    assert_native_rejections(&imported.project.source);

    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Amount").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );

    let roundtrip = directory.path().join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &roundtrip)?.is_empty());
    let exported: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        directory.path().join("roundtrip-source.schema.json"),
    )?)?;
    assert_eq!(exported["additionalProperties"], false);
    assert_eq!(exported["patternProperties"]["^list-"]["type"], "array");
    assert_eq!(
        exported["patternProperties"]["^list-"]["items"]["properties"]["Code"]["type"],
        "string"
    );
    assert_eq!(
        exported["dependentRequired"]["list-required"],
        serde_json::json!(["Amount"])
    );
    assert!(exported.pointer("/dependentRequired/impossible").is_none());
    assert!(
        exported
            .pointer("/dependentSchemas/never-selected")
            .is_none()
    );

    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_pattern_schema(&reimported.project.source)?;
    assert_native_rejections(&reimported.project.source);
    let reimported_source = format_json::from_str(INPUT, &reimported.project.source)?;
    assert_eq!(
        engine::run(&reimported.project, &reimported_source)?,
        output
    );
    Ok(())
}

fn assert_pattern_schema(schema: &ir::SchemaNode) -> Result<(), Box<dyn std::error::Error>> {
    let selectors = schema
        .json_pattern_property_names()
        .ok_or("missing pattern-property selectors")?;
    assert_eq!(selectors.sources(), ["^list-"]);
    let dynamic = schema
        .dynamic_fields()
        .ok_or("missing pattern-property value schema")?;
    assert!(dynamic.repeating);
    assert!(matches!(dynamic.kind, SchemaKind::Group { .. }));
    assert_eq!(
        dynamic
            .item_count_range
            .map(|range| (range.minimum(), range.maximum())),
        Some((1, Some(2)))
    );
    assert!(dynamic.child("Code").is_some());

    let dependencies = schema
        .json_property_dependencies
        .as_ref()
        .ok_or("missing retained pattern-property dependency")?;
    assert_eq!(
        dependencies.requirements("list-required"),
        Some(&["Amount".to_string()][..])
    );
    assert!(dependencies.requirements("impossible").is_none());
    let dependent_schemas = schema
        .json_dependent_schemas
        .as_ref()
        .ok_or("missing retained pattern-property dependent schema")?;
    assert_eq!(dependent_schemas.as_slice().len(), 1);
    assert_eq!(dependent_schemas.as_slice()[0].trigger(), "list-checked");
    Ok(())
}

fn assert_native_rejections(schema: &ir::SchemaNode) {
    assert!(format_json::from_str(r#"{"Amount":7,"list-empty":[]}"#, schema).is_err());
    assert!(format_json::from_str(r#"{"Amount":7,"list-invalid":[{"Code":1}]}"#, schema).is_err());
    assert!(format_json::from_str(r#"{"Amount":7,"other":[]}"#, schema).is_err());
    assert!(format_json::from_str(r#"{"list-required":[{"Code":"A"}]}"#, schema).is_err());
    assert!(
        format_json::from_str(r#"{"Amount":0,"list-checked":[{"Code":"A"}]}"#, schema).is_err()
    );
}

const INPUT: &str = r#"{
  "Amount":7,
  "list-known":[{"Code":"known"}],
  "list-required":[{"Code":"required"}],
  "list-checked":[{"Code":"checked"}]
}"#;

fn write_fixture(directory: &Path) -> Result<(), std::io::Error> {
    std::fs::write(
        directory.join("source.schema.json"),
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"PatternEnvelope",
  "type":"object",
  "properties":{
    "Amount":{"type":"integer"},
    "list-known":{
      "type":"array",
      "minItems":1,
      "maxItems":2,
      "items":{
        "type":"object",
        "properties":{"Code":{"type":"string"}},
        "required":["Code"],
        "additionalProperties":false
      }
    }
  },
  "patternProperties":{
    "^list-":{
      "type":"array",
      "minItems":1,
      "maxItems":2,
      "items":{
        "type":"object",
        "properties":{"Code":{"type":"string"}},
        "required":["Code"],
        "additionalProperties":false
      }
    }
  },
  "additionalProperties":false,
  "dependentRequired":{
    "impossible":["also-impossible"],
    "list-required":["Amount"]
  },
  "dependentSchemas":{
    "never-selected":false,
    "list-checked":{
      "properties":{"Amount":{"type":"integer","minimum":1}},
      "required":["Amount"]
    }
  }
}"#,
    )?;
    std::fs::write(directory.join("input.json"), INPUT)?;
    std::fs::write(
        directory.join("mapping.mfd"),
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
    )
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-pattern-properties-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
