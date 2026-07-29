use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};

#[test]
fn imports_executes_and_roundtrips_homogeneous_json_prefix_items()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    write_fixture(directory.path())?;
    let design = directory.path().join("mapping.mfd");

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_homogeneous_source(&imported.project.source)?;
    assert_codegen_viable(&imported.project)?;

    let source = format_json::read(
        &directory.path().join("input.json"),
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    assert_rows(&output);

    let roundtrip = directory.path().join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported_schema: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        directory.path().join("roundtrip-source.schema.json"),
    )?)?;
    assert!(
        exported_schema.get("prefixItems").is_none(),
        "{exported_schema:#}"
    );
    assert_eq!(exported_schema["minItems"], 2);
    assert_eq!(exported_schema["maxItems"], 4);
    assert_eq!(exported_schema["items"]["type"], "object");
    assert_eq!(
        exported_schema["items"]["properties"]["Code"]["type"],
        "string"
    );
    assert_eq!(
        exported_schema["items"]["properties"]["Quantity"]["type"],
        "integer"
    );

    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_homogeneous_source(&reimported.project.source)?;
    assert_codegen_viable(&reimported.project)?;
    assert_eq!(engine::run(&reimported.project, &source)?, output);
    Ok(())
}

fn assert_homogeneous_source(schema: &ir::SchemaNode) -> Result<(), Box<dyn std::error::Error>> {
    assert!(schema.repeating);
    let range = schema
        .item_count_range
        .ok_or("homogeneous prefixItems should retain its item-count range")?;
    assert_eq!(range.minimum(), 2);
    assert_eq!(range.maximum(), Some(4));
    let code = schema.child("Code").ok_or("missing Code item field")?;
    assert!(matches!(
        &code.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
    let quantity = schema
        .child("Quantity")
        .ok_or("missing Quantity item field")?;
    assert!(matches!(
        &quantity.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    Ok(())
}

fn assert_codegen_viable(project: &mapping::Project) -> Result<(), Box<dyn std::error::Error>> {
    let program = codegen::lower(project)
        .map_err(|diagnostics| format!("codegen lowering: {diagnostics:?}"))?;
    codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-prefix-items-profile".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .map_err(|error| format!("Rust emission: {error}"))?;
    codegen_csharp::emit(&program).map_err(|error| format!("C# emission: {error}"))?;
    Ok(())
}

fn assert_rows(output: &Instance) {
    let rows = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .unwrap_or_else(|| panic!("expected repeated output rows: {output:?}"));
    assert_eq!(rows.len(), 3);
    for (row, code, quantity) in [(&rows[0], "A", 2), (&rows[1], "B", 4), (&rows[2], "C", 6)] {
        assert_eq!(
            row.field("Code").and_then(Instance::as_scalar),
            Some(&Value::String(code.into()))
        );
        assert_eq!(
            row.field("Quantity").and_then(Instance::as_scalar),
            Some(&Value::Int(quantity))
        );
    }
}

fn write_fixture(directory: &Path) -> Result<(), std::io::Error> {
    std::fs::write(
        directory.join("source.schema.json"),
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"Rows",
  "type":"array",
  "minItems":2,
  "maxItems":4,
  "prefixItems":[
    {"$ref":"#/$defs/Row"},
    {"$ref":"#/$defs/Row"}
  ],
  "items":{"$ref":"#/$defs/Row"},
  "$defs":{
    "Row":{
      "type":"object",
      "properties":{
        "Code":{"type":"string"},
        "Quantity":{"type":"integer"}
      },
      "required":["Code","Quantity"],
      "additionalProperties":false
    }
  }
}"##,
    )?;
    std::fs::write(
        directory.join("input.json"),
        r#"[
  {"Code":"A","Quantity":2},
  {"Code":"B","Quantity":4},
  {"Code":"C","Quantity":6}
]"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Code" type="xs:string"/>
      <xs:element name="Quantity" type="xs:integer"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root">
      <entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="10">
        <entry name="Code" type="json-property"><entry name="string" outkey="11"/></entry>
        <entry name="Quantity" type="json-property"><entry name="integer" outkey="12"/></entry>
      </entry></entry></entry>
    </entry></entry></entry></root>
    <json schema="source.schema.json" inputinstance="input.json"/>
  </data></component>
  <component name="target" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Target"><entry name="Row" inpkey="20">
        <entry name="Code" inpkey="21"/><entry name="Quantity" inpkey="22"/>
      </entry></entry></root>
      <document schema="target.xsd" outputinstance="output.xml" instanceroot="{}Target"/>
    </data>
  </component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-prefix-items-profile-{}-{}",
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
