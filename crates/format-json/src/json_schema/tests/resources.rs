use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaKind};

use super::super::{files, import, import_with_root};
use crate::JsonFormatError;

fn resource_dir(label: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "ferrule_json_schema_resource_{label}_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn imports_nested_external_refs_and_cross_file_cycles() -> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("nested");
    std::fs::create_dir_all(dir.join("models"))?;
    std::fs::write(
        dir.join("root.json"),
        r##"{
  "$ref": "models/order.json#/$defs/order"
}"##,
    )?;
    std::fs::write(
        dir.join("models/order.json"),
        r##"{
  "$defs": {
    "order": {
      "title": "Order",
      "type": "object",
      "properties": {
        "Number": { "type": "integer" },
        "Customer": { "$ref": "../shared%20types.json#/$defs/customer" }
      }
    }
  }
}"##,
    )?;
    std::fs::write(
        dir.join("shared types.json"),
        r##"{
  "$defs": {
    "customer": {
      "type": "object",
      "properties": {
        "Name": { "type": "string" },
        "Postal": { "$ref": "#/$defs/postal%20code" },
        "LastOrder": { "$ref": "models/order.json#/$defs/order" }
      }
    },
    "postal code": {
      "type": "integer"
    }
  }
}"##,
    )?;

    let schema = import_with_root(&dir.join("root.json"), &dir)?;
    assert_eq!(schema.name, "Order");
    assert!(matches!(
        schema.child("Number").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    let customer = schema
        .child("Customer")
        .ok_or_else(|| std::io::Error::other("missing Customer"))?;
    assert!(matches!(
        customer.child("Name").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
    assert!(matches!(
        customer.child("Postal").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    assert!(matches!(
        customer.child("LastOrder").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn external_dependency_keywords_follow_each_resources_dialect()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("dependency_dialects");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("root.json"),
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"object",
  "properties":{
    "legacy":{"$ref":"legacy.json"},
    "modern":{"$ref":"modern.json"}
  }
}"#,
    )?;
    std::fs::write(
        dir.join("legacy.json"),
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"object",
  "properties":{"trigger":{"type":"string"},"legacy":{"type":"string"}},
  "dependencies":{"trigger":{"required":["legacy"]}}
}"#,
    )?;
    std::fs::write(
        dir.join("modern.json"),
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"object",
  "properties":{
    "trigger":{"type":"string"},
    "ignored":{"type":"string"},
    "modern":{"type":"string"}
  },
  "dependencies":{"trigger":["ignored"]},
  "dependentSchemas":{"trigger":{"required":["modern"]}}
}"#,
    )?;

    let schema = import_with_root(&dir.join("root.json"), &dir)?;
    let legacy = schema
        .child("legacy")
        .and_then(|node| node.json_property_dependencies.as_ref())
        .and_then(|dependencies| dependencies.requirements("trigger"));
    assert_eq!(legacy, Some(["legacy".to_string()].as_slice()));
    let modern = schema
        .child("modern")
        .and_then(|node| node.json_property_dependencies.as_ref())
        .and_then(|dependencies| dependencies.requirements("trigger"));
    assert_eq!(modern, Some(["modern".to_string()].as_slice()));

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn external_dynamic_reference_keywords_follow_each_resources_dialect()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("dynamic_reference_dialects");
    std::fs::create_dir_all(&dir)?;
    let cases = [
        (
            "draft2019-recursive.json",
            r##"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "$recursiveRef":"#"
}"##,
            true,
        ),
        (
            "draft2019-dynamic.json",
            r##"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "type":"object",
  "$dynamicRef":"#ignored"
}"##,
            false,
        ),
        (
            "draft2020-dynamic.json",
            r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$dynamicRef":"#ignored"
}"##,
            true,
        ),
        (
            "draft2020-recursive.json",
            r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"object",
  "$recursiveRef":"#"
}"##,
            false,
        ),
    ];
    for (file, schema, _) in cases {
        std::fs::write(dir.join(file), schema)?;
    }

    for (file, _, should_reject) in cases {
        std::fs::write(dir.join("root.json"), format!(r#"{{"$ref":"{file}"}}"#))?;
        let imported = import_with_root(&dir.join("root.json"), &dir);
        if should_reject {
            assert!(matches!(
                imported,
                Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                    if reason.contains("dynamic reference validation is not supported")
            ));
        } else {
            assert!(matches!(
                imported.map(|schema| schema.kind),
                Ok(SchemaKind::Group { .. })
            ));
        }
    }

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn default_import_confines_refs_to_the_schema_directory() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = resource_dir("confined");
    std::fs::create_dir_all(dir.join("schema"))?;
    std::fs::write(
        dir.join("shared.json"),
        r#"{"type":"object","properties":{"Id":{"type":"integer"}}}"#,
    )?;
    std::fs::write(
        dir.join("schema/root.json"),
        r##"{"$ref":"../shared.json"}"##,
    )?;

    assert!(matches!(
        import(&dir.join("schema/root.json")),
        Err(JsonFormatError::SchemaResource { ref reason, .. })
            if reason.contains("escapes package root")
    ));
    let schema = import_with_root(&dir.join("schema/root.json"), &dir)?;
    assert!(matches!(
        schema.child("Id").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn external_refs_participate_in_flat_nullable_compositions()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("nullable_composition");
    std::fs::create_dir_all(dir.join("models"))?;
    std::fs::write(
        dir.join("root.json"),
        r#"{
  "title":"Message",
  "oneOf":[
    {"$ref":"models/created.json"},
    {"$ref":"null.json"},
    {"$ref":"models/deleted.json"}
  ]
}"#,
    )?;
    std::fs::write(dir.join("null.json"), r#"{"type":"null"}"#)?;
    std::fs::write(
        dir.join("models/created.json"),
        r#"{
  "title":"created",
  "type":"object",
  "additionalProperties":false,
  "required":["created"],
  "properties":{"created":{"type":"integer"}}
}"#,
    )?;
    std::fs::write(
        dir.join("models/deleted.json"),
        r#"{
  "title":"deleted",
  "type":"object",
  "additionalProperties":false,
  "required":["deleted"],
  "properties":{"deleted":{"type":"string"}}
}"#,
    )?;

    let schema = import_with_root(&dir.join("root.json"), &dir)?;
    assert!(schema.container_nullable);
    assert_eq!(schema.alternatives().len(), 2);
    for input in ["null", r#"{"created":7}"#, r#"{"deleted":"duplicate"}"#] {
        assert!(crate::from_str(input, &schema).is_ok(), "{input}");
    }

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn rejects_missing_remote_anchor_and_reserved_bundle_refs() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = resource_dir("invalid");
    std::fs::create_dir_all(&dir)?;
    for (name, reference, reason) in [
        ("missing", "missing.json#/$defs/value", "No such file"),
        (
            "remote",
            "https://example.invalid/schema.json",
            "relative local-file",
        ),
        ("anchor", "other.json#named", "named anchors"),
    ] {
        std::fs::write(
            dir.join("root.json"),
            format!(r#"{{"$ref":"{reference}"}}"#),
        )?;
        let error = import_with_root(&dir.join("root.json"), &dir)
            .expect_err("unsupported resource reference imported");
        assert!(
            matches!(error, JsonFormatError::SchemaResource { reason: ref actual, .. } if actual.contains(reason)),
            "{name}: {error}"
        );
    }

    std::fs::write(dir.join("other.json"), r#"{"type":"string"}"#)?;
    std::fs::write(
        dir.join("root.json"),
        r##"{
  "$ref": "other.json",
  "$defs": { "__ferrule_external_documents": {} }
}"##,
    )?;
    assert!(matches!(
        import_with_root(&dir.join("root.json"), &dir),
        Err(JsonFormatError::SchemaResource { ref reason, .. })
            if reason.contains("reserved")
    ));

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn schema_keyword_property_names_remain_ordinary_properties()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("keyword_property_names");
    std::fs::create_dir_all(&dir)?;
    let names = [
        "$ref",
        "if",
        "then",
        "else",
        "items",
        "properties",
        "dependencies",
        "dependentSchemas",
        "prefixItems",
        "contains",
        "propertyNames",
        "unevaluatedItems",
        "unevaluatedProperties",
        "__ferrule_ignore_ref_siblings",
        "__ferrule_validation_dialect",
    ];
    let properties: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|name| ((*name).to_string(), serde_json::json!({"type": "string"})))
        .collect();
    let source = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
    });
    std::fs::write(dir.join("root.json"), serde_json::to_vec(&source)?)?;

    let loaded = files::load(&dir.join("root.json"), &dir)?;
    let loaded_properties = loaded
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| std::io::Error::other("missing loaded properties"))?;
    assert_eq!(loaded_properties.len(), names.len());
    for name in names {
        let property = loaded_properties
            .get(name)
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| std::io::Error::other(format!("missing property `{name}`")))?;
        assert!(!property.contains_key("__ferrule_validation_dialect"));
        assert!(!property.contains_key("__ferrule_ignore_ref_siblings"));
    }

    let schema = import_with_root(&dir.join("root.json"), &dir)?;
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(std::io::Error::other("root was not an object").into());
    };
    assert_eq!(children.len(), names.len());
    for name in names {
        assert!(schema.child(name).is_some(), "missing property `{name}`");
    }

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn references_inside_opaque_values_are_not_loaded_or_rewritten()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("opaque_refs");
    std::fs::create_dir_all(&dir)?;
    let source = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "string",
        "default": {
            "$ref": "missing-default.json",
            "__ferrule_validation_dialect": "literal"
        },
        "examples": [{
            "nested": {
                "$ref": "missing-example.json",
                "__ferrule_ignore_ref_siblings": true
            }
        }],
        "x-metadata": {
            "if": {
                "$ref": "missing-extension.json"
            }
        }
    });
    std::fs::write(dir.join("root.json"), serde_json::to_vec(&source)?)?;

    let loaded = files::load(&dir.join("root.json"), &dir)?;
    assert_eq!(
        loaded.pointer("/default/$ref"),
        Some(&serde_json::json!("missing-default.json"))
    );
    assert_eq!(
        loaded.pointer("/examples/0/nested/$ref"),
        Some(&serde_json::json!("missing-example.json"))
    );
    assert_eq!(
        loaded.pointer("/x-metadata/if/$ref"),
        Some(&serde_json::json!("missing-extension.json"))
    );
    assert_eq!(
        loaded.pointer("/default/__ferrule_validation_dialect"),
        Some(&serde_json::json!("literal"))
    );

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn references_in_known_schema_positions_are_rewritten() -> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("schema_positions");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("target.json"),
        r#"{"type":"string","minLength":1}"#,
    )?;
    let reference = || serde_json::json!({"$ref": "target.json"});
    let source = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": {"definition": reference()},
        "definitions": {"legacyDefinition": reference()},
        "properties": {"property": reference()},
        "patternProperties": {"^pattern$": reference()},
        "dependentSchemas": {"property": reference()},
        "allOf": [reference()],
        "anyOf": [reference()],
        "oneOf": [reference()],
        "prefixItems": [reference()],
        "items": [reference()],
        "dependencies": {
            "schemaDependency": reference(),
            "propertyDependency": ["$ref", "target.json"]
        },
        "additionalItems": reference(),
        "additionalProperties": reference(),
        "contains": reference(),
        "contentSchema": reference(),
        "propertyNames": reference(),
        "unevaluatedItems": reference(),
        "unevaluatedProperties": reference(),
        "not": reference(),
        "if": reference(),
        "then": reference(),
        "else": reference()
    });
    std::fs::write(dir.join("root.json"), serde_json::to_vec(&source)?)?;

    let loaded = files::load(&dir.join("root.json"), &dir)?;
    let rewritten = "#/$defs/__ferrule_external_documents/1";
    for pointer in [
        "/$defs/definition/$ref",
        "/definitions/legacyDefinition/$ref",
        "/properties/property/$ref",
        "/patternProperties/^pattern$/$ref",
        "/dependentSchemas/property/$ref",
        "/allOf/0/$ref",
        "/anyOf/0/$ref",
        "/oneOf/0/$ref",
        "/prefixItems/0/$ref",
        "/items/0/$ref",
        "/dependencies/schemaDependency/$ref",
        "/additionalItems/$ref",
        "/additionalProperties/$ref",
        "/contains/$ref",
        "/contentSchema/$ref",
        "/propertyNames/$ref",
        "/unevaluatedItems/$ref",
        "/unevaluatedProperties/$ref",
        "/not/$ref",
        "/if/$ref",
        "/then/$ref",
        "/else/$ref",
    ] {
        assert_eq!(
            loaded.pointer(pointer).and_then(serde_json::Value::as_str),
            Some(rewritten),
            "{pointer}"
        );
    }
    assert_eq!(
        loaded.pointer("/dependencies/propertyDependency"),
        Some(&serde_json::json!(["$ref", "target.json"]))
    );

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn opaque_values_still_participate_in_the_json_depth_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = resource_dir("opaque_depth");
    std::fs::create_dir_all(&dir)?;
    let mut nested = serde_json::Value::Null;
    for _ in 0..140 {
        nested = serde_json::json!({"metadata": nested});
    }
    let source = serde_json::json!({
        "type": "string",
        "examples": [nested]
    });
    std::fs::write(dir.join("root.json"), serde_json::to_vec(&source)?)?;

    let result = files::load(&dir.join("root.json"), &dir);
    std::fs::remove_dir_all(dir)?;
    match result {
        Err(JsonFormatError::SchemaResourceLimit {
            kind: "JSON nesting depth",
            ..
        }) => {}
        Err(JsonFormatError::Json(error)) if error.to_string().contains("recursion limit") => {}
        result => panic!("unexpected opaque-depth result: {result:?}"),
    }
    Ok(())
}
