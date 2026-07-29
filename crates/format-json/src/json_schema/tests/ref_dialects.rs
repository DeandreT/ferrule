use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{NumericRange, ScalarType, SchemaKind};

use super::import_str_result;
use crate::{JsonFormatError, json_schema::import_with_root};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-json-ref-dialects-{}-{}",
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

fn integer_bounds(schema: &ir::SchemaNode) -> Option<(Option<i64>, Option<i64>)> {
    let NumericRange::Integer(range) = schema.numeric_range? else {
        return None;
    };
    Some((range.minimum(), range.maximum()))
}

#[test]
fn draft_four_six_and_seven_ignore_all_ref_siblings() -> Result<(), JsonFormatError> {
    for dialect in [
        "http://json-schema.org/draft-04/schema#",
        "https://json-schema.org/draft-04/schema",
        "http://json-schema.org/draft-06/schema#",
        "https://json-schema.org/draft-06/schema",
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft-07/schema",
    ] {
        let schema = import_str_result(&format!(
            r##"{{
  "$schema":"{dialect}",
  "$ref":"#/definitions/Base",
  "title":"Ignored sibling title",
  "type":"object",
  "minimum":"not a number",
  "definitions":{{
    "Base":{{"title":"Resolved title","type":"integer","minimum":1}}
  }}
}}"##
        ))?;
        assert_eq!(schema.name, "Resolved title", "{dialect}");
        assert_eq!(integer_bounds(&schema), Some((Some(1), None)), "{dialect}");
    }
    Ok(())
}

#[test]
fn modern_and_default_dialects_apply_supported_ref_siblings() -> Result<(), JsonFormatError> {
    for dialect in [
        None,
        Some("https://json-schema.org/draft/2019-09/schema"),
        Some("http://json-schema.org/draft/2019-09/schema#"),
        Some("https://json-schema.org/draft/2020-12/schema"),
        Some("http://json-schema.org/draft/2020-12/schema#"),
    ] {
        let dialect = dialect.map_or(String::new(), |dialect| {
            format!(r#""$schema":"{dialect}","#)
        });
        let schema = import_str_result(&format!(
            r##"{{
  {dialect}
  "$ref":"#/$defs/Base",
  "title":"Sibling title",
  "minimum":5,
  "$defs":{{
    "Base":{{"title":"Resolved title","type":"integer","minimum":1,"maximum":9}}
  }}
}}"##
        ))?;
        assert_eq!(schema.name, "Sibling title");
        assert_eq!(integer_bounds(&schema), Some((Some(5), Some(9))));
    }
    Ok(())
}

#[test]
fn modern_structural_and_malformed_supported_siblings_reject() {
    for sibling in [
        r##""$dynamicRef":"#/$defs/Base""##,
        r##""$recursiveRef":"#/$defs/Base""##,
        r#""type":"string""#,
        r#""required":["value"]"#,
        r#""allOf":[{"type":"integer"}]"#,
    ] {
        let schema = format!(
            r##"{{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/Base",
  {sibling},
  "$defs":{{"Base":{{"type":"integer"}}}}
}}"##
        );
        assert!(matches!(
            import_str_result(&schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { reason, .. })
                if reason.contains("modern `$ref` sibling")
        ));
    }

    assert!(matches!(
        import_str_result(
            r##"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "$ref":"#/$defs/Base",
  "minimum":"not a number",
  "$defs":{"Base":{"type":"integer"}}
}"##
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("minimum")
    ));
}

#[test]
fn cyclic_ref_siblings_follow_the_owning_dialect() {
    let legacy = import_str_result(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "$ref":"#/definitions/Loop",
  "minimum":"ignored",
  "definitions":{
    "Loop":{"$ref":"#/definitions/Loop","type":"integer","minimum":"ignored"}
  }
}"##,
    );
    let Ok(legacy) = legacy else {
        panic!("legacy cyclic ref siblings should be ignored");
    };
    assert_eq!(
        legacy.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    );
    assert!(legacy.numeric_range.is_none());

    let modern = import_str_result(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/Loop",
  "$defs":{
    "Loop":{"$ref":"#/$defs/Loop","minimum":"not a number"}
  }
}"##,
    );
    assert!(
        matches!(
            &modern,
            Err(JsonFormatError::UnsupportedSchemaUnion { reason, .. })
                if reason.contains("unresolved or cyclic")
        ),
        "{modern:?}"
    );
}

#[test]
fn nullable_scalar_refs_apply_or_ignore_siblings_by_dialect() -> Result<(), JsonFormatError> {
    let legacy = import_str_result(
        r##"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "title":"LegacyNullable",
  "oneOf":[
    {"$ref":"#/definitions/Value","type":"boolean","minimum":"ignored"},
    {"type":"null"}
  ],
  "definitions":{"Value":{"type":"integer","minimum":1}}
}"##,
    )?;
    assert!(legacy.nullable);
    assert_eq!(integer_bounds(&legacy), Some((Some(1), None)));

    let modern = import_str_result(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"ModernNullable",
  "oneOf":[
    {"$ref":"#/$defs/Value","maximum":5},
    {"type":"null"}
  ],
  "$defs":{"Value":{"type":"integer","minimum":1}}
}"##,
    )?;
    assert!(modern.nullable);
    assert_eq!(integer_bounds(&modern), Some((Some(1), Some(5))));
    Ok(())
}

#[test]
fn array_union_refs_preserve_modern_counts_and_ignore_legacy_counts() -> Result<(), JsonFormatError>
{
    let modern = import_str_result(
        r##"{
  "title":"ModernArrays",
  "anyOf":[
    {"$ref":"#/$defs/Open","maxItems":3},
    {"$ref":"#/$defs/Bounded"}
  ],
  "$defs":{
    "Open":{"type":"array","minItems":1,"items":{"type":"string"}},
    "Bounded":{"type":"array","minItems":1,"maxItems":3,"items":{"type":"string"}}
  }
}"##,
    )?;
    let Some(modern_counts) = modern.item_count_range else {
        panic!("modern ref sibling count should be retained");
    };
    assert_eq!(modern_counts.minimum(), 1);
    assert_eq!(modern_counts.maximum(), Some(3));

    let legacy = import_str_result(
        r##"{
  "$schema":"http://json-schema.org/draft-06/schema#",
  "title":"LegacyArrays",
  "anyOf":[
    {"$ref":"#/definitions/Open","maxItems":3},
    {"$ref":"#/definitions/Bounded"}
  ],
  "definitions":{
    "Open":{"type":"array","minItems":1,"items":{"type":"string"}},
    "Bounded":{"type":"array","minItems":1,"maxItems":3,"items":{"type":"string"}}
  }
}"##,
    )?;
    let Some(legacy_counts) = legacy.item_count_range else {
        panic!("legacy target count should be retained");
    };
    assert_eq!(legacy_counts.minimum(), 1);
    assert_eq!(legacy_counts.maximum(), None);
    Ok(())
}

#[test]
fn object_alternative_refs_reject_modern_structural_siblings_but_ignore_legacy_ones()
-> Result<(), JsonFormatError> {
    let modern = import_str_result(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"ModernObjects",
  "oneOf":[
    {"$ref":"#/$defs/A","required":["extra"]},
    {"$ref":"#/$defs/B"}
  ],
  "$defs":{
    "A":{"type":"object","additionalProperties":false,"properties":{"a":{"type":"string"}},"required":["a"]},
    "B":{"type":"object","additionalProperties":false,"properties":{"b":{"type":"string"}},"required":["b"]}
  }
}"##,
    );
    assert!(matches!(
        modern,
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("modern `$ref` sibling `required`")
    ));

    let legacy = import_str_result(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"LegacyObjects",
  "oneOf":[
    {"$ref":"#/definitions/A","required":["extra"]},
    {"$ref":"#/definitions/B"}
  ],
  "definitions":{
    "A":{"type":"object","additionalProperties":false,"properties":{"a":{"type":"string"}},"required":["a"]},
    "B":{"type":"object","additionalProperties":false,"properties":{"b":{"type":"string"}},"required":["b"]}
  }
}"##,
    )?;
    assert_eq!(legacy.alternatives().len(), 2);
    Ok(())
}

#[test]
fn external_resources_use_their_own_ref_sibling_dialect() -> Result<(), Box<dyn std::error::Error>>
{
    for (root_dialect, external_dialect, expected_minimum) in [
        (
            "https://json-schema.org/draft/2020-12/schema",
            "http://json-schema.org/draft-04/schema#",
            1,
        ),
        (
            "http://json-schema.org/draft-04/schema#",
            "https://json-schema.org/draft/2020-12/schema",
            5,
        ),
    ] {
        let directory = TempDir::new()?;
        std::fs::write(
            directory.0.join("root.schema.json"),
            format!(
                r#"{{
  "$schema":"{root_dialect}",
  "$ref":"external.schema.json#/$defs/Use"
}}"#
            ),
        )?;
        std::fs::write(
            directory.0.join("external.schema.json"),
            format!(
                r##"{{
  "$schema":"{external_dialect}",
  "$defs":{{
    "Base":{{"type":"integer","minimum":1}},
    "Use":{{"$ref":"#/$defs/Base","minimum":5}}
  }}
}}"##
            ),
        )?;
        let schema = import_with_root(&directory.0.join("root.schema.json"), &directory.0)?;
        assert_eq!(
            integer_bounds(&schema),
            Some((Some(expected_minimum), None))
        );
    }
    Ok(())
}

#[test]
fn modern_pattern_siblings_on_unconstrained_refs_reject_locally_and_externally()
-> Result<(), Box<dyn std::error::Error>> {
    let local = import_str_result(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/Any",
  "pattern":"^A$",
  "$defs":{"Any":{}}
}"##,
    );
    assert!(matches!(
        local,
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("unconstrained schema")
    ));

    let legacy = import_str_result(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "$ref":"#/definitions/Any",
  "pattern":"^A$",
  "definitions":{"Any":{}}
}"##,
    )?;
    assert!(legacy.json_any);
    assert!(crate::from_str(r#""B""#, &legacy).is_ok());
    assert!(crate::from_str("7", &legacy).is_ok());

    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("root.schema.json"),
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"external.schema.json",
  "pattern":"^A$"
}"#,
    )?;
    std::fs::write(directory.0.join("external.schema.json"), "{}")?;
    let external = import_with_root(&directory.0.join("root.schema.json"), &directory.0);
    assert!(matches!(
        external,
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("unconstrained schema")
    ));
    Ok(())
}

#[test]
fn legacy_external_root_can_reference_ignored_sibling_definitions()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("root.schema.json"),
        r#"{"$ref":"external.schema.json"}"#,
    )?;
    std::fs::write(
        directory.0.join("external.schema.json"),
        r##"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "$ref":"#/definitions/Value",
  "minimum":9,
  "definitions":{"Value":{"type":"integer","minimum":2}}
}"##,
    )?;
    let schema = import_with_root(&directory.0.join("root.schema.json"), &directory.0)?;
    assert_eq!(integer_bounds(&schema), Some((Some(2), None)));
    Ok(())
}

#[test]
fn reserved_ref_policy_key_is_rejected_before_import() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("root.schema.json"),
        r#"{"type":"string","__ferrule_ignore_ref_siblings":true}"#,
    )?;
    let Err(error) = import_with_root(&directory.0.join("root.schema.json"), &directory.0) else {
        panic!("reserved internal key must reject");
    };
    assert!(matches!(
        error,
        JsonFormatError::SchemaResource { ref reason, .. }
            if reason.contains("reserved `$ref` policy key")
    ));
    Ok(())
}
