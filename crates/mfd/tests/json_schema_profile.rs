use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, JsonAllowedValue, JsonContainsPredicate, NumericRange, SchemaKind, SchemaNode};

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
  "minProperties":6,
  "maxProperties":8,
  "dependentRequired":{
    "TypedOpen":["Status"],
    "Status":["Priority"]
  },
  "additionalProperties":false,
  "properties":{
    "MaybeObject":{
      "anyOf":[
        {
          "type":"object",
          "minProperties":1,
          "maxProperties":3,
          "properties":{"Code":{"type":"string"}},
          "additionalProperties":true
        },
        {"type":"null"}
      ]
    },
    "ImplicitOpen":{
      "type":"object",
      "properties":{
        "Known":{"type":"string"},
        "LegacyPeer":{"type":"string"}
      },
      "propertyNames":{
        "minLength":1,
        "maxLength":24,
        "pattern":"^[A-Za-z]+$",
        "format":"member-name"
      },
      "dependencies":{"Known":["LegacyPeer"]}
    },
    "TypedOpen":{
      "type":"object",
      "minProperties":2,
      "maxProperties":3,
      "properties":{"Unit":{"type":"string"}},
      "additionalProperties":{"type":"integer"}
    },
    "MaybeArray":{
      "type":["array","null"],
      "minItems":1,
      "maxItems":2,
      "uniqueItems":true,
      "items":{
        "type":"object",
        "minProperties":1,
        "maxProperties":1,
        "properties":{"Id":{"type":"integer"}}
      }
    },
    "Amount":{
      "oneOf":[
        {"type":"number","minimum":0},
        {"type":"null"}
      ],
      "exclusiveMaximum":20,
      "multipleOf":0.25
    },
    "Status":{"type":"string","const":"ready","minLength":5,"maxLength":5,"pattern":"^ready$","format":"workflow-status"},
    "Priority":{"enum":["normal","urgent",null]},
    "Tracking":{"type":"string","format":""}
  }
}"#,
    )?;
    std::fs::write(
        directory.0.join("input.json"),
        r#"{"MaybeObject":{"Code":"A","nested":{"enabled":true}},"ImplicitOpen":{"Known":"A","LegacyPeer":"B","arbitrary":{"nested":[true,null]}},"TypedOpen":{"Unit":"count","widgets":3},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"urgent"}"#,
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
    assert!(imported.project.source.dynamic_fields().is_none());
    let source_property_count = imported
        .project
        .source
        .property_count_range
        .ok_or("missing root property-count range")?;
    assert_eq!(source_property_count.minimum(), 6);
    assert_eq!(source_property_count.maximum(), Some(8));
    let source_dependencies = imported
        .project
        .source
        .json_property_dependencies
        .as_ref()
        .ok_or("missing root property dependencies")?;
    assert_eq!(
        source_dependencies.requirements("TypedOpen"),
        Some(&["Status".to_string()][..])
    );
    assert_eq!(
        source_dependencies.requirements("Status"),
        Some(&["Priority".to_string()][..])
    );

    let object = imported
        .project
        .source
        .child("MaybeObject")
        .ok_or("missing nullable object")?;
    assert!(object.container_nullable);
    let object_property_count = object
        .property_count_range
        .ok_or("missing nullable object property-count range")?;
    assert_eq!(object_property_count.minimum(), 1);
    assert_eq!(object_property_count.maximum(), Some(3));
    let dynamic = object.dynamic_fields().ok_or("missing dynamic value")?;
    assert!(dynamic.json_any);
    let implicit_open = imported
        .project
        .source
        .child("ImplicitOpen")
        .ok_or("missing default-open object")?;
    assert!(
        implicit_open
            .dynamic_fields()
            .is_some_and(|dynamic| dynamic.json_any)
    );
    assert_eq!(
        implicit_open
            .json_property_dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.requirements("Known")),
        Some(&["LegacyPeer".to_string()][..])
    );
    let implicit_names = implicit_open
        .json_property_names
        .as_ref()
        .ok_or("missing property-name constraints")?;
    assert!(implicit_names.accepts("Known"));
    assert!(implicit_names.accepts("arbitrary"));
    assert!(!implicit_names.accepts("bad-key"));
    assert_eq!(
        implicit_names
            .length()
            .map(|length| (length.minimum(), length.maximum())),
        Some((1, Some(24)))
    );
    assert_eq!(
        implicit_names
            .formats()
            .and_then(|formats| formats.as_slice().first())
            .map(String::as_str),
        Some("member-name")
    );
    let typed_open = imported
        .project
        .source
        .child("TypedOpen")
        .ok_or("missing typed-open object")?;
    assert!(matches!(
        typed_open.dynamic_fields().map(|dynamic| &dynamic.kind),
        Some(SchemaKind::Scalar {
            ty: ir::ScalarType::Int
        })
    ));
    let typed_open_property_count = typed_open
        .property_count_range
        .ok_or("missing typed-open property-count range")?;
    assert_eq!(typed_open_property_count.minimum(), 2);
    assert_eq!(typed_open_property_count.maximum(), Some(3));
    let array = imported
        .project
        .source
        .child("MaybeArray")
        .ok_or("missing nullable array")?;
    assert!(array.container_nullable);
    assert!(array.repeating);
    assert!(array.json_unique_items);
    let range = array
        .item_count_range
        .ok_or("missing nullable array item-count range")?;
    assert_eq!(range.minimum(), 1);
    assert_eq!(range.maximum(), Some(2));
    let item_property_count = array
        .property_count_range
        .ok_or("missing array-item property-count range")?;
    assert_eq!(item_property_count.minimum(), 1);
    assert_eq!(item_property_count.maximum(), Some(1));
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
    let amount_multiple_of = amount
        .json_multiple_of
        .as_ref()
        .ok_or("missing nullable amount multipleOf constraint")?;
    assert_eq!(
        amount_multiple_of
            .any_of()
            .first()
            .and_then(|terms| terms.first())
            .map(|divisor| divisor.to_decimal_lexical())
            .as_deref(),
        Some("0.25")
    );
    assert_eq!(
        imported
            .project
            .source
            .child("Status")
            .and_then(|status| status.fixed.as_deref()),
        Some("ready")
    );
    assert_eq!(
        imported
            .project
            .source
            .child("Status")
            .and_then(|status| status.json_formats.as_slice().first())
            .map(String::as_str),
        Some("workflow-status")
    );
    let status_length = imported
        .project
        .source
        .child("Status")
        .and_then(|status| status.string_length_range)
        .ok_or("missing status string-length range")?;
    assert_eq!(status_length.minimum(), 5);
    assert_eq!(status_length.maximum(), Some(5));
    assert_eq!(
        imported
            .project
            .source
            .child("Status")
            .and_then(|status| status.json_patterns.as_ref())
            .map(ir::JsonPatternConstraints::any_of),
        Some(&[vec!["^ready$".to_string()]][..])
    );
    assert_eq!(
        imported
            .project
            .source
            .child("Tracking")
            .and_then(|tracking| tracking.json_formats.as_slice().first())
            .map(String::as_str),
        Some("")
    );
    let priority = imported
        .project
        .source
        .child("Priority")
        .ok_or("missing priority enum")?;
    let priority_values = priority
        .json_allowed_values
        .as_ref()
        .ok_or("missing priority allowed values")?;
    assert_eq!(
        priority_values.values(),
        [
            JsonAllowedValue::JsonNull,
            JsonAllowedValue::String("normal".to_string()),
            JsonAllowedValue::String("urgent".to_string()),
        ]
    );
    assert!(priority.nullable);

    let input = format_json::read(&directory.0.join("input.json"), &imported.project.source)?;
    assert!(matches!(input, Instance::Group(_)));
    assert_eq!(
        input
            .field("TypedOpen")
            .and_then(|object| object.field("widgets"))
            .and_then(Instance::as_scalar),
        Some(&ir::Value::Int(3))
    );
    let arbitrary = input
        .field("ImplicitOpen")
        .and_then(|object| object.field("arbitrary"))
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            ir::Value::String(text) => Some(text),
            _ => None,
        })
        .ok_or("missing unconstrained dynamic value")?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(arbitrary)?,
        serde_json::json!({"nested":[true,null]})
    );
    assert!(matches!(
        format_json::from_str(
            r#"{"Unexpected":true,"MaybeObject":{"Code":"A"},"ImplicitOpen":{},"Amount":12.5,"Status":"ready","Priority":"normal"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::UndeclaredProperty {
            ref object,
            ref property,
        }) if object == "Envelope" && property == "Unexpected"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{"Code":"A"},"ImplicitOpen":{},"TypedOpen":{"Unit":"count","widgets":3},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal","Tracking":"opaque","Unexpected":true}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::PropertyCountMismatch {
            ref name,
            got: 9,
            ..
        }) if name == "Envelope"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{"Code":"A"},"ImplicitOpen":{"LegacyPeer":"B"},"TypedOpen":{"Unit":"count","widgets":3},"MaybeArray":[{"Id":1}],"Amount":12.5,"Priority":"normal","Tracking":"opaque"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::MissingDependentProperty {
            ref object,
            ref trigger,
            ref property,
        }) if object == "Envelope" && trigger == "TypedOpen" && property == "Status"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{"Code":"A"},"ImplicitOpen":{"Known":"A"},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal","Tracking":"opaque"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::MissingDependentProperty {
            ref object,
            ref trigger,
            ref property,
        }) if object == "ImplicitOpen" && trigger == "Known" && property == "LegacyPeer"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{"Code":"A"},"ImplicitOpen":{"Known":"A","LegacyPeer":"B","bad-key":"value"},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal","Tracking":"opaque"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::InvalidPropertyName {
            ref object,
            ref property,
        }) if object == "ImplicitOpen" && property == "bad-key"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::PropertyCountMismatch {
            ref name,
            got: 4,
            ..
        }) if name == "Envelope"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{},"ImplicitOpen":{},"TypedOpen":{"Unit":"count","widgets":3},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::PropertyCountMismatch {
            ref name,
            got: 0,
            ..
        }) if name == "MaybeObject"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{"Code":"A"},"ImplicitOpen":{},"TypedOpen":{"Unit":"count"},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::PropertyCountMismatch {
            ref name,
            got: 1,
            ..
        }) if name == "TypedOpen"
    ));
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{"Code":"A"},"ImplicitOpen":{},"TypedOpen":{"Unit":"count","widgets":3},"MaybeArray":[{}],"Amount":12.5,"Status":"ready","Priority":"normal"}"#,
            &imported.project.source,
        ),
        Err(format_json::JsonFormatError::PropertyCountMismatch {
            ref name,
            got: 0,
            ..
        }) if name == "MaybeArray"
    ));
    assert!(
        format_json::from_str(
            r#"{"TypedOpen":{"Unit":"count","widgets":"three"},"Amount":12.5,"Status":"ready","Priority":"normal"}"#,
            &imported.project.source,
        )
        .is_err()
    );
    assert!(
        format_json::from_str(
            r#"{"MaybeArray":[{"Id":1},{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal"}"#,
            &imported.project.source,
        )
        .is_err()
    );
    let output = engine::run(&imported.project, &input)?;
    assert!(output.field("Amount").is_some());

    let roundtrip_design = directory.0.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip_design)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported_schema: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        directory.0.join("roundtrip-source.schema.json"),
    )?)?;
    assert_eq!(exported_schema["additionalProperties"], false);
    assert_eq!(exported_schema["minProperties"], 6);
    assert_eq!(exported_schema["maxProperties"], 8);
    assert_eq!(
        exported_schema["dependentRequired"],
        serde_json::json!({
            "Status":["Priority"],
            "TypedOpen":["Status"],
        })
    );
    assert_eq!(
        exported_schema["properties"]["MaybeObject"]["anyOf"][0]["additionalProperties"],
        serde_json::json!({})
    );
    assert_eq!(
        exported_schema["properties"]["MaybeObject"]["anyOf"][0]["minProperties"],
        1
    );
    assert_eq!(
        exported_schema["properties"]["MaybeObject"]["anyOf"][0]["maxProperties"],
        3
    );
    assert_eq!(
        exported_schema["properties"]["ImplicitOpen"]["additionalProperties"],
        serde_json::json!({})
    );
    assert_eq!(
        exported_schema["properties"]["ImplicitOpen"]["dependentRequired"],
        serde_json::json!({"Known":["LegacyPeer"]})
    );
    assert_eq!(
        exported_schema["properties"]["ImplicitOpen"]["propertyNames"],
        serde_json::json!({
            "type":"string",
            "minLength":1,
            "maxLength":24,
            "pattern":"^[A-Za-z]+$",
            "format":"member-name",
        })
    );
    assert_eq!(
        exported_schema["properties"]["TypedOpen"]["additionalProperties"]["type"],
        "integer"
    );
    assert_eq!(
        exported_schema["properties"]["TypedOpen"]["minProperties"],
        2
    );
    assert_eq!(
        exported_schema["properties"]["TypedOpen"]["maxProperties"],
        3
    );
    assert_eq!(
        exported_schema["properties"]["MaybeArray"]["anyOf"][0]["items"]["minProperties"],
        1
    );
    assert_eq!(
        exported_schema["properties"]["MaybeArray"]["anyOf"][0]["items"]["maxProperties"],
        1
    );
    let reimported = mfd::import(&roundtrip_design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(reimported.project.source.dynamic_fields().is_none());
    assert!(
        reimported
            .project
            .source
            .child("MaybeObject")
            .and_then(ir::SchemaNode::dynamic_fields)
            .is_some_and(|dynamic| dynamic.json_any)
    );
    assert!(
        reimported
            .project
            .source
            .child("ImplicitOpen")
            .and_then(ir::SchemaNode::dynamic_fields)
            .is_some_and(|dynamic| dynamic.json_any)
    );
    assert!(matches!(
        reimported
            .project
            .source
            .child("TypedOpen")
            .and_then(ir::SchemaNode::dynamic_fields)
            .map(|dynamic| &dynamic.kind),
        Some(SchemaKind::Scalar {
            ty: ir::ScalarType::Int
        })
    ));
    let reimported_multiple_of = reimported
        .project
        .source
        .child("Amount")
        .and_then(|amount| amount.json_multiple_of.as_ref())
        .ok_or("missing round-tripped amount multipleOf constraint")?;
    assert_eq!(
        reimported_multiple_of
            .any_of()
            .first()
            .and_then(|terms| terms.first())
            .map(|divisor| divisor.to_decimal_lexical())
            .as_deref(),
        Some("0.25")
    );
    assert_eq!(
        reimported
            .project
            .source
            .child("Priority")
            .and_then(|priority| priority.json_allowed_values.as_ref())
            .map(ir::JsonAllowedValues::values),
        Some(priority_values.values())
    );
    assert!(
        reimported
            .project
            .source
            .child("MaybeArray")
            .is_some_and(|array| array.json_unique_items)
    );
    assert_eq!(
        reimported
            .project
            .source
            .property_count_range
            .map(|range| (range.minimum(), range.maximum())),
        Some((6, Some(8)))
    );
    assert_eq!(
        reimported
            .project
            .source
            .child("MaybeObject")
            .and_then(|object| object.property_count_range)
            .map(|range| (range.minimum(), range.maximum())),
        Some((1, Some(3)))
    );
    assert_eq!(
        reimported
            .project
            .source
            .child("MaybeArray")
            .and_then(|array| array.property_count_range)
            .map(|range| (range.minimum(), range.maximum())),
        Some((1, Some(1)))
    );
    assert_eq!(
        reimported
            .project
            .source
            .json_property_dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.requirements("TypedOpen")),
        Some(&["Status".to_string()][..])
    );
    assert_eq!(
        reimported
            .project
            .source
            .child("ImplicitOpen")
            .and_then(|object| object.json_property_dependencies.as_ref())
            .and_then(|dependencies| dependencies.requirements("Known")),
        Some(&["LegacyPeer".to_string()][..])
    );
    let reimported_names = reimported
        .project
        .source
        .child("ImplicitOpen")
        .and_then(|object| object.json_property_names.as_ref())
        .ok_or("missing round-tripped property-name constraints")?;
    assert!(reimported_names.accepts("Known"));
    assert!(!reimported_names.accepts("bad-key"));
    assert_eq!(
        reimported_names
            .formats()
            .and_then(|formats| formats.as_slice().first())
            .map(String::as_str),
        Some("member-name")
    );
    assert!(matches!(
        format_json::from_str(
            r#"{"MaybeObject":{"Code":"A"},"ImplicitOpen":{"Known":"A","LegacyPeer":"B","bad-key":"value"},"MaybeArray":[{"Id":1}],"Amount":12.5,"Status":"ready","Priority":"normal","Tracking":"opaque"}"#,
            &reimported.project.source,
        ),
        Err(format_json::JsonFormatError::InvalidPropertyName {
            ref object,
            ref property,
        }) if object == "ImplicitOpen" && property == "bad-key"
    ));
    Ok(())
}

fn assert_contains_range(
    schema: &SchemaNode,
    expected_minimum: u64,
    expected_maximum: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let constraints = schema
        .json_contains
        .as_ref()
        .ok_or("missing contains constraints")?;
    let [constraint] = constraints.as_slice() else {
        return Err("expected one contains constraint".into());
    };
    assert!(matches!(
        constraint.predicate(),
        JsonContainsPredicate::Schema { .. }
    ));
    assert_eq!(constraint.range().minimum(), expected_minimum);
    assert_eq!(constraint.range().maximum(), expected_maximum);
    Ok(())
}

#[test]
fn imports_executes_and_roundtrips_json_contains_constraints()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("source.schema.json"),
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"ContainsEnvelope",
  "type":"object",
  "properties":{
    "Codes":{
      "type":"array",
      "items":{"type":"string"},
      "contains":{"type":"string","const":"keep"},
      "minContains":2,
      "maxContains":2
    },
    "DefaultCodes":{
      "type":"array",
      "items":{"type":"string"},
      "contains":{"type":"string","const":"default"}
    },
    "Nested":{
      "type":"object",
      "properties":{
        "Codes":{
          "type":"array",
          "items":{"type":"string"},
          "contains":{"type":"string","pattern":"^nested$"},
          "minContains":1
        }
      }
    },
    "Rows":{
      "type":"array",
      "items":{
        "type":"object",
        "properties":{
          "Codes":{
            "type":"array",
            "items":{"type":"string"},
            "contains":{"type":"string","const":"row"},
            "minContains":1
          }
        }
      }
    },
    "Maybe":{
      "type":["array","null"],
      "items":{"type":"string"},
      "contains":false,
      "minContains":0,
      "maxContains":0
    },
    "Amount":{"type":"integer"}
  }
}"#,
    )?;
    std::fs::write(
        directory.0.join("input.json"),
        r#"{
  "Codes":["keep","other","keep"],
  "DefaultCodes":["other","default"],
  "Nested":{"Codes":["other","nested"]},
  "Rows":[{"Codes":["row"]},{"Codes":["other","row"]}],
  "Maybe":null,
  "Amount":7
}"#,
    )?;
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
    assert_contains_range(source.child("Codes").ok_or("missing Codes")?, 2, Some(2))?;
    assert_contains_range(
        source.child("DefaultCodes").ok_or("missing DefaultCodes")?,
        1,
        None,
    )?;
    assert_contains_range(
        source
            .child("Nested")
            .and_then(|nested| nested.child("Codes"))
            .ok_or("missing nested Codes")?,
        1,
        None,
    )?;
    assert_contains_range(
        source
            .child("Rows")
            .and_then(|row| row.child("Codes"))
            .ok_or("missing row Codes")?,
        1,
        None,
    )?;
    assert!(
        source
            .child("Maybe")
            .is_some_and(|maybe| maybe.container_nullable && maybe.json_contains.is_none())
    );

    let valid = std::fs::read_to_string(directory.0.join("input.json"))?;
    assert!(format_json::from_str(&valid, source).is_ok());
    for invalid in [
        valid.replace(r#""keep","other","keep""#, r#""keep","other""#),
        valid.replace(r#""other","default""#, r#""other","missing""#),
        valid.replace(r#""other","nested""#, r#""other","missing""#),
        valid.replace(r#""Codes":["row"]"#, r#""Codes":["other"]"#),
    ] {
        assert!(matches!(
            format_json::from_str(&invalid, source),
            Err(format_json::JsonFormatError::ContainsCountMismatch { .. })
        ));
    }
    let input = format_json::from_str(&valid, source)?;
    assert!(
        engine::run(&imported.project, &input)?
            .field("Amount")
            .is_some()
    );

    let roundtrip_design = directory.0.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip_design)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        directory.0.join("roundtrip-source.schema.json"),
    )?)?;
    assert_eq!(exported["properties"]["Codes"]["contains"]["const"], "keep");
    assert_eq!(exported["properties"]["Codes"]["minContains"], 2);
    assert_eq!(exported["properties"]["Codes"]["maxContains"], 2);
    assert_eq!(
        exported["properties"]["DefaultCodes"]["contains"]["const"],
        "default"
    );
    assert!(
        exported["properties"]["DefaultCodes"]
            .get("minContains")
            .is_none()
    );
    assert!(
        exported["properties"]["DefaultCodes"]
            .get("maxContains")
            .is_none()
    );

    let reimported = mfd::import(&roundtrip_design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_contains_range(
        reimported
            .project
            .source
            .child("Codes")
            .ok_or("missing round-tripped Codes")?,
        2,
        Some(2),
    )?;
    assert_contains_range(
        reimported
            .project
            .source
            .child("DefaultCodes")
            .ok_or("missing round-tripped DefaultCodes")?,
        1,
        None,
    )?;
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
