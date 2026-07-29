use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, JsonSchemaPredicate, SchemaNode, Value};

const PRESENT_INPUT: &str = r#"{"Expedite":true,"Service":"priority","Amount":12}"#;
const ABSENT_INPUT: &str = r#"{"Service":"standard","Amount":7}"#;
const NULL_TRIGGER_INPUT: &str = r#"{"Expedite":null,"Service":"priority","Amount":9}"#;

#[test]
fn imports_executes_and_roundtrips_exact_object_if_then() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TempDir::new()?;
    write_fixture(directory.path())?;
    let design = directory.path().join("mapping.mfd");

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_conditional_schema(&imported.project.source)?;
    assert_valid_and_invalid_boundaries(&imported.project.source);
    assert_codegen_viable(&imported.project)?;

    let mut original_outputs = Vec::new();
    for (document, expected) in [
        (PRESENT_INPUT, 12),
        (ABSENT_INPUT, 7),
        (NULL_TRIGGER_INPUT, 9),
    ] {
        let source = format_json::from_str(document, &imported.project.source)?;
        let output = engine::run(&imported.project, &source)?;
        assert_amount(&output, expected);
        original_outputs.push((document, source, output));
    }

    let roundtrip = directory.path().join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported_schema: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        directory.path().join("roundtrip-source.schema.json"),
    )?)?;
    assert!(exported_schema.get("if").is_none(), "{exported_schema:#}");
    assert!(exported_schema.get("then").is_none(), "{exported_schema:#}");
    assert_eq!(
        exported_schema["dependentSchemas"]["Expedite"]["required"],
        serde_json::json!(["Service"])
    );
    assert_eq!(
        exported_schema["dependentSchemas"]["Expedite"]["properties"]["Service"]["const"],
        "priority"
    );

    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_conditional_schema(&reimported.project.source)?;
    assert_valid_and_invalid_boundaries(&reimported.project.source);
    assert_codegen_viable(&reimported.project)?;
    for (document, _, expected) in original_outputs {
        let source = format_json::from_str(document, &reimported.project.source)?;
        assert_eq!(engine::run(&reimported.project, &source)?, expected);
    }
    Ok(())
}

fn assert_conditional_schema(schema: &SchemaNode) -> Result<(), Box<dyn std::error::Error>> {
    let constraint = schema
        .json_dependent_schemas
        .as_ref()
        .and_then(|constraints| {
            constraints
                .as_slice()
                .iter()
                .find(|constraint| constraint.trigger() == "Expedite")
        })
        .ok_or("if/then should normalize to an Expedite dependent schema")?;
    let predicate = match constraint.predicate() {
        JsonSchemaPredicate::Schema { schema } => schema,
        JsonSchemaPredicate::Never => {
            return Err("then predicate unexpectedly never matches".into());
        }
    };
    assert_eq!(predicate.required_fields(), ["Service"]);
    assert_eq!(
        predicate
            .child("Service")
            .and_then(|service| service.fixed.as_deref()),
        Some("priority")
    );
    Ok(())
}

fn assert_valid_and_invalid_boundaries(schema: &SchemaNode) {
    for valid in [PRESENT_INPUT, ABSENT_INPUT, NULL_TRIGGER_INPUT] {
        assert!(format_json::from_str(valid, schema).is_ok(), "{valid}");
    }
    for invalid in [
        r#"{"Expedite":true,"Amount":1}"#,
        r#"{"Expedite":true,"Service":"economy","Amount":2}"#,
        r#"{"Expedite":null,"Amount":3}"#,
    ] {
        let Err(error) = format_json::from_str(invalid, schema) else {
            panic!("conditional input unexpectedly passed: {invalid}");
        };
        assert!(
            error.to_string().contains("Expedite")
                && error.to_string().contains("dependent schema"),
            "unexpected conditional diagnostic: {error}"
        );
    }
}

fn assert_codegen_viable(project: &mapping::Project) -> Result<(), Box<dyn std::error::Error>> {
    let program = codegen::lower(project)
        .map_err(|diagnostics| format!("codegen lowering: {diagnostics:?}"))?;
    codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-if-then-profile".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .map_err(|error| format!("Rust emission: {error}"))?;
    codegen_csharp::emit(&program).map_err(|error| format!("C# emission: {error}"))?;
    Ok(())
}

fn assert_amount(output: &Instance, expected: i64) {
    assert_eq!(
        output.field("Amount").and_then(Instance::as_scalar),
        Some(&Value::Int(expected))
    );
}

fn write_fixture(directory: &Path) -> Result<(), std::io::Error> {
    std::fs::write(
        directory.join("orders.schema.json"),
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"ConditionalOrder",
  "type":"object",
  "properties":{
    "Expedite":{"type":["boolean","null"]},
    "Service":{"type":"string"},
    "Amount":{"type":"integer"}
  },
  "required":["Amount"],
  "additionalProperties":false,
  "if":{"required":["Expedite"]},
  "then":{
    "properties":{"Service":{"const":"priority"}},
    "required":["Service"]
  }
}"#,
    )?;
    std::fs::write(directory.join("order.json"), PRESENT_INPUT)?;
    std::fs::write(
        directory.join("output.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Amount" type="xs:integer"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="order" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root">
      <entry name="object">
        <entry name="Amount" type="json-property"><entry name="integer" outkey="10"/></entry>
      </entry>
    </entry></entry></entry></root>
    <json schema="orders.schema.json" inputinstance="order.json"/>
  </data></component>
  <component name="output" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Output"><entry name="Amount" inpkey="20"/></entry></root>
      <document schema="output.xsd" outputinstance="output.xml" instanceroot="{}Output"/>
    </data>
  </component>
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
            "ferrule-mfd-if-then-profile-{}-{}",
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
