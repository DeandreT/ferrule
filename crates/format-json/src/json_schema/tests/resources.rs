use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaKind};

use super::super::{import, import_with_root};
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
