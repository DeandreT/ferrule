use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};

const VALID_INPUT: &str = r#"[
  {"Reference":"P-100","Weight":11},
  {"Reference":"P-200","Weight":22},
  {"Reference":"P-300","Weight":33},
  {"Reference":"P-400","Weight":44}
]"#;

#[test]
fn imports_executes_and_roundtrips_homogeneous_legacy_tuple_items()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    write_fixture(directory.path())?;
    let design = directory.path().join("mapping.mfd");

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_homogeneous_source(&imported.project.source)?;
    assert_legacy_tuple_boundaries(&imported.project.source);
    assert_codegen_viable(&imported.project)?;

    let source = format_json::from_str(VALID_INPUT, &imported.project.source)?;
    let output = engine::run(&imported.project, &source)?;
    assert_parcels(&output);

    let roundtrip = directory.path().join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported_schema: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        directory.path().join("roundtrip-source.schema.json"),
    )?)?;
    assert!(exported_schema["items"].is_object(), "{exported_schema:#}");
    assert!(
        exported_schema.get("additionalItems").is_none(),
        "{exported_schema:#}"
    );
    assert_eq!(exported_schema["minItems"], 2);
    assert_eq!(exported_schema["maxItems"], 5);
    assert_eq!(
        exported_schema["items"]["properties"]["Reference"]["type"],
        "string"
    );
    assert_eq!(
        exported_schema["items"]["properties"]["Weight"]["type"],
        "integer"
    );

    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_homogeneous_source(&reimported.project.source)?;
    assert_legacy_tuple_boundaries(&reimported.project.source);
    assert_codegen_viable(&reimported.project)?;
    assert_eq!(engine::run(&reimported.project, &source)?, output);
    Ok(())
}

fn assert_homogeneous_source(schema: &ir::SchemaNode) -> Result<(), Box<dyn std::error::Error>> {
    assert!(schema.repeating);
    let range = schema
        .item_count_range
        .ok_or("legacy tuple should retain its exact item-count range")?;
    assert_eq!(range.minimum(), 2);
    assert_eq!(range.maximum(), Some(5));
    let reference = schema
        .child("Reference")
        .ok_or("missing Reference item field")?;
    assert!(matches!(
        &reference.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
    let weight = schema.child("Weight").ok_or("missing Weight item field")?;
    assert!(matches!(
        &weight.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    Ok(())
}

fn assert_legacy_tuple_boundaries(schema: &ir::SchemaNode) {
    assert!(format_json::from_str(VALID_INPUT, schema).is_ok());
    assert!(format_json::from_str(r#"[{"Reference":"only-one","Weight":1}]"#, schema).is_err());
    assert!(
        format_json::from_str(
            r#"[
  {"Reference":"A","Weight":1},
  {"Reference":"B","Weight":2},
  {"Reference":"C","Weight":3},
  {"Reference":"D","Weight":4},
  {"Reference":"E","Weight":5},
  {"Reference":"F","Weight":6}
]"#,
            schema,
        )
        .is_err()
    );
    assert!(
        format_json::from_str(
            r#"[
  {"Reference":"A","Weight":1},
  {"Reference":"B","Weight":2},
  {"Reference":"tail","Weight":"not-an-integer"}
]"#,
            schema,
        )
        .is_err(),
        "the schema-valued additionalItems tail must retain the tuple item type"
    );
}

fn assert_codegen_viable(project: &mapping::Project) -> Result<(), Box<dyn std::error::Error>> {
    let program = codegen::lower(project)
        .map_err(|diagnostics| format!("codegen lowering: {diagnostics:?}"))?;
    codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-legacy-tuple-profile".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .map_err(|error| format!("Rust emission: {error}"))?;
    codegen_csharp::emit(&program).map_err(|error| format!("C# emission: {error}"))?;
    Ok(())
}

fn assert_parcels(output: &Instance) {
    let parcels = output
        .field("Parcel")
        .and_then(Instance::as_repeated)
        .unwrap_or_else(|| panic!("expected repeated parcels: {output:?}"));
    assert_eq!(parcels.len(), 4);
    for (parcel, reference, weight) in [
        (&parcels[0], "P-100", 11),
        (&parcels[1], "P-200", 22),
        (&parcels[2], "P-300", 33),
        (&parcels[3], "P-400", 44),
    ] {
        assert_eq!(
            parcel.field("Reference").and_then(Instance::as_scalar),
            Some(&Value::String(reference.into()))
        );
        assert_eq!(
            parcel.field("Weight").and_then(Instance::as_scalar),
            Some(&Value::Int(weight))
        );
    }
}

fn write_fixture(directory: &Path) -> Result<(), std::io::Error> {
    std::fs::write(
        directory.join("shipments.schema.json"),
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"Shipments",
  "type":"array",
  "minItems":2,
  "maxItems":5,
  "items":[
    {"$ref":"#/definitions/Parcel"},
    {"$ref":"#/definitions/Parcel"}
  ],
  "additionalItems":{"$ref":"#/definitions/Parcel"},
  "definitions":{
    "Parcel":{
      "type":"object",
      "properties":{
        "Reference":{"type":"string"},
        "Weight":{"type":"integer"}
      },
      "required":["Reference","Weight"],
      "additionalProperties":false
    }
  }
}"##,
    )?;
    std::fs::write(directory.join("shipments.json"), VALID_INPUT)?;
    std::fs::write(
        directory.join("manifest.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Manifest"><xs:complexType><xs:sequence>
    <xs:element name="Parcel" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Reference" type="xs:string"/>
      <xs:element name="Weight" type="xs:integer"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="shipments" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root">
      <entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="10">
        <entry name="Reference" type="json-property"><entry name="string" outkey="11"/></entry>
        <entry name="Weight" type="json-property"><entry name="integer" outkey="12"/></entry>
      </entry></entry></entry>
    </entry></entry></entry></root>
    <json schema="shipments.schema.json" inputinstance="shipments.json"/>
  </data></component>
  <component name="manifest" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Manifest"><entry name="Parcel" inpkey="20">
        <entry name="Reference" inpkey="21"/><entry name="Weight" inpkey="22"/>
      </entry></entry></root>
      <document schema="manifest.xsd" outputinstance="manifest.xml" instanceroot="{}Manifest"/>
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
            "ferrule-mfd-legacy-tuple-profile-{}-{}",
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
