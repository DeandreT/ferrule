use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, JsonSchemaPredicate, SchemaNode, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-dependent-schemas-{}-{}",
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

fn dependent_predicate<'a>(
    schema: &'a SchemaNode,
    trigger: &str,
) -> Result<&'a SchemaNode, Box<dyn std::error::Error>> {
    let constraint = schema
        .json_dependent_schemas
        .as_ref()
        .and_then(|constraints| {
            constraints
                .as_slice()
                .iter()
                .find(|constraint| constraint.trigger() == trigger)
        })
        .ok_or_else(|| format!("missing dependent schema for `{trigger}`"))?;
    match constraint.predicate() {
        JsonSchemaPredicate::Schema { schema } => Ok(schema),
        JsonSchemaPredicate::Never => Err(format!("`{trigger}` unexpectedly never matches").into()),
    }
}

fn assert_dependent_error(
    document: &str,
    schema: &SchemaNode,
    trigger: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let error = format_json::from_str(document, schema)
        .err()
        .ok_or("document unexpectedly satisfied its dependent schema")?;
    let message = error.to_string();
    assert!(
        message.contains(trigger) && message.contains("dependent schema"),
        "unexpected dependent-schema diagnostic: {message}"
    );
    Ok(())
}

#[test]
fn imports_executes_and_roundtrips_json_dependent_schemas() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("legacy.schema.json"),
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"LegacyConditional",
  "type":"object",
  "properties":{"X":{"type":"integer"},"Y":{"type":"string"}},
  "dependencies":{
    "X":{"properties":{"Y":{"type":"string","const":"legacy"}},"required":["Y"]}
  }
}"#,
    )?;
    std::fs::write(
        directory.0.join("source.schema.json"),
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"ConditionalEnvelope",
  "type":"object",
  "properties":{
    "RequiredOnlyTrigger":{"type":"string"},
    "RequiredOnlyPeer":{"type":"string"},
    "Trigger":{"type":["string","null"]},
    "Mode":{"type":"string"},
    "Code":{"type":"string"},
    "Embedded":{
      "type":"object",
      "properties":{"X":{"type":"integer"},"Y":{"type":"string"}}
    },
    "Nested":{
      "type":"object",
      "properties":{"A":{"type":"integer"},"B":{"type":"integer"}},
      "dependentSchemas":{
        "A":{"properties":{"B":{"type":"integer","minimum":1}},"required":["B"]}
      }
    },
    "Rows":{
      "type":"array",
      "items":{
        "type":"object",
        "properties":{"A":{"type":"integer"},"B":{"type":"string"}},
        "dependentSchemas":{
          "A":{"properties":{"B":{"type":"string","pattern":"^row$"}},"required":["B"]}
        }
      }
    },
    "Maybe":{
      "anyOf":[
        {
          "type":"object",
          "properties":{"A":{"type":"integer"},"B":{"type":"string"}},
          "dependentSchemas":{
            "A":{"properties":{"B":{"type":"string","const":"maybe"}},"required":["B"]}
          }
        },
        {"type":"null"}
      ]
    },
    "Legacy":{"$ref":"legacy.schema.json"},
    "Amount":{"type":"integer"}
  },
  "dependentSchemas":{
    "RequiredOnlyTrigger":{"required":["RequiredOnlyPeer"]},
    "Trigger":{
      "properties":{
        "Mode":{"type":"string","const":"strict"},
        "Code":{"type":"string","pattern":"^OK$"},
        "Embedded":{
          "type":"object",
          "properties":{"X":{"type":"integer"},"Y":{"type":"string"}},
          "dependentSchemas":{
            "X":{"properties":{"Y":{"type":"string","const":"nested"}},"required":["Y"]}
          }
        }
      },
      "required":["Mode","Code","Embedded"]
    }
  }
}"#,
    )?;
    let input_document = r#"{
  "RequiredOnlyTrigger":"present",
  "RequiredOnlyPeer":"peer",
  "Trigger":null,
  "Mode":"strict",
  "Code":"OK",
  "Embedded":{"X":1,"Y":"nested"},
  "Nested":{"A":1,"B":2},
  "Rows":[{"A":1,"B":"row"},{}],
  "Maybe":null,
  "Legacy":{"X":1,"Y":"legacy"},
  "Amount":7
}"#;
    std::fs::write(directory.0.join("input.json"), input_document)?;
    let design = directory.0.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">
      <entry name="Amount" type="json-property"><entry name="integer" outkey="10"/></entry>
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
    let source = &imported.project.source;
    assert_eq!(
        source
            .json_property_dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.requirements("RequiredOnlyTrigger")),
        Some(&["RequiredOnlyPeer".to_string()][..])
    );
    let root_predicate = dependent_predicate(source, "Trigger")?;
    assert_eq!(
        root_predicate.required_fields(),
        ["Mode", "Code", "Embedded"]
    );
    assert!(
        dependent_predicate(
            root_predicate
                .child("Embedded")
                .ok_or("missing nested predicate object")?,
            "X",
        )
        .is_ok()
    );
    assert!(dependent_predicate(source.child("Nested").ok_or("missing Nested")?, "A").is_ok());
    assert!(dependent_predicate(source.child("Rows").ok_or("missing Rows")?, "A").is_ok());
    assert!(dependent_predicate(source.child("Maybe").ok_or("missing Maybe")?, "A").is_ok());
    assert!(dependent_predicate(source.child("Legacy").ok_or("missing Legacy")?, "X").is_ok());

    assert!(format_json::from_str(input_document, source).is_ok());
    assert!(
        format_json::from_str(
            r#"{"Mode":"wrong","Code":"bad","Nested":{},"Rows":[],"Maybe":null,"Amount":7}"#,
            source,
        )
        .is_ok(),
        "an absent trigger does not apply its predicate"
    );
    assert_dependent_error(
        r#"{"Trigger":null,"Mode":"strict","Nested":{},"Rows":[],"Maybe":null,"Amount":7}"#,
        source,
        "Trigger",
    )?;
    assert_dependent_error(
        r#"{"Trigger":"on","Mode":"wrong","Code":"OK","Nested":{},"Rows":[],"Maybe":null,"Amount":7}"#,
        source,
        "Trigger",
    )?;
    assert_dependent_error(
        r#"{"Trigger":null,"Mode":"strict","Code":"OK","Embedded":{"X":1,"Y":"wrong"},"Nested":{},"Rows":[],"Maybe":null,"Amount":7}"#,
        source,
        "Trigger",
    )?;
    assert_dependent_error(
        r#"{"Nested":{"A":1},"Rows":[],"Maybe":null,"Amount":7}"#,
        source,
        "A",
    )?;
    assert_dependent_error(
        r#"{"Nested":{},"Rows":[{"A":1,"B":"wrong"}],"Maybe":null,"Amount":7}"#,
        source,
        "A",
    )?;
    assert_dependent_error(
        r#"{"Nested":{},"Rows":[],"Maybe":{"A":1},"Amount":7}"#,
        source,
        "A",
    )?;
    assert_dependent_error(
        r#"{"Nested":{},"Rows":[],"Maybe":null,"Legacy":{"X":1,"Y":"wrong"},"Amount":7}"#,
        source,
        "X",
    )?;
    let property_error = format_json::from_str(
        r#"{"RequiredOnlyTrigger":"present","Nested":{},"Rows":[],"Maybe":null,"Amount":7}"#,
        source,
    )
    .err()
    .ok_or("required-only dependent schema unexpectedly passed")?;
    assert!(property_error.to_string().contains("RequiredOnlyPeer"));

    let input = format_json::from_str(input_document, source)?;
    let output = engine::run(&imported.project, &input)?;
    assert!(matches!(
        output.field("Amount"),
        Some(Instance::Scalar(Value::Int(7)))
    ));

    let roundtrip = directory.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &roundtrip)?.is_empty());
    let exported: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        directory.0.join("roundtrip-source.schema.json"),
    )?)?;
    assert_eq!(
        exported["dependentRequired"]["RequiredOnlyTrigger"],
        serde_json::json!(["RequiredOnlyPeer"])
    );
    assert_eq!(
        exported["dependentSchemas"]["Trigger"]["required"],
        serde_json::json!(["Mode", "Code", "Embedded"])
    );
    assert_eq!(
        exported["dependentSchemas"]["Trigger"]["properties"]["Mode"]["const"],
        "strict"
    );
    assert_eq!(
        exported["dependentSchemas"]["Trigger"]["properties"]["Embedded"]["dependentSchemas"]["X"]
            ["properties"]["Y"]["const"],
        "nested"
    );
    assert_eq!(
        exported["properties"]["Rows"]["items"]["dependentSchemas"]["A"]["properties"]["B"]["pattern"],
        "^row$"
    );
    assert_eq!(
        exported["properties"]["Legacy"]["dependentSchemas"]["X"]["properties"]["Y"]["const"],
        "legacy"
    );

    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert!(dependent_predicate(&reimported.project.source, "Trigger").is_ok());
    assert!(
        dependent_predicate(
            reimported
                .project
                .source
                .child("Rows")
                .ok_or("missing round-tripped Rows")?,
            "A",
        )
        .is_ok()
    );
    assert!(
        dependent_predicate(
            reimported
                .project
                .source
                .child("Legacy")
                .ok_or("missing round-tripped Legacy")?,
            "X",
        )
        .is_ok()
    );
    assert!(format_json::from_str(input_document, &reimported.project.source).is_ok());
    assert_dependent_error(
        r#"{"Trigger":null,"Mode":"strict","Code":"OK","Embedded":{"X":1,"Y":"wrong"},"Nested":{},"Rows":[],"Maybe":null,"Amount":7}"#,
        &reimported.project.source,
        "Trigger",
    )?;
    assert_dependent_error(
        r#"{"Nested":{},"Rows":[],"Maybe":null,"Legacy":{"X":1,"Y":"wrong"},"Amount":7}"#,
        &reimported.project.source,
        "X",
    )?;
    Ok(())
}
