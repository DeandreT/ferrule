use super::*;

#[test]
fn memory_import_matches_file_dialects_local_refs_and_float_decoding() {
    for dialect in [
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft/2020-12/schema",
        "",
    ] {
        let text = format!(
            r##"{{"$schema":"{dialect}","$ref":"#/definitions/Measurement","definitions":{{"Measurement":{{"title":"Measurement","type":"number","minimum":1e-307}}}},"maximum":2}}"##
        );
        let from_file = import_str_result(&text).unwrap();
        let from_memory = super::super::import_str(&text).unwrap();
        assert_eq!(
            serde_json::to_string(&from_memory).unwrap(),
            serde_json::to_string(&from_file).unwrap()
        );
        assert_eq!(from_memory.name, "Measurement");
    }
}

#[test]
fn memory_import_rejects_external_refs_without_loading_them() {
    for reference in [
        "sibling.schema.json",
        "../missing.schema.json#/properties/value",
        "https://example.test/schema.json",
        "file:///missing/schema.json",
    ] {
        let text = serde_json::json!({"$ref":reference}).to_string();
        assert!(matches!(
            super::super::import_str(&text),
            Err(JsonFormatError::SchemaResource { reason, .. })
                if reason == "in-memory schemas cannot load external resources"
        ));
    }
}

#[test]
fn memory_import_enforces_bytes_before_parsing_and_rejects_reserved_policy_keys() {
    let oversized = " ".repeat(64 * 1024 * 1024 + 1);
    assert!(matches!(
        super::super::import_str(&oversized),
        Err(JsonFormatError::SchemaResourceLimit { kind: "total bytes", limit })
            if limit == 64 * 1024 * 1024
    ));
    for reserved in [
        "__ferrule_ignore_ref_siblings",
        "__ferrule_validation_dialect",
    ] {
        let text = serde_json::json!({"type":"string", reserved: true}).to_string();
        assert!(matches!(
            super::super::import_str(&text),
            Err(JsonFormatError::SchemaResource { .. })
        ));
    }
}
