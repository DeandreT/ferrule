use codegen_schema::{CodecError, V3_PREFIX, decode, encode};
use ir::{ScalarType, SchemaNode};
use serde_json::Value;

const CASES: &str =
    include_str!("../../format-json/tests/fixtures/nested_array_wrappers/cases.json");

#[test]
fn nested_array_wrapper_descriptors_preserve_every_level_and_reject_unknown_versions() {
    let evidence = std::env::temp_dir().join(format!(
        "ferrule-nested-array-descriptors-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&evidence).unwrap();
    std::fs::write(evidence.join("cases.json"), CASES).unwrap();
    let corpus: Value = serde_json::from_str(CASES).unwrap();
    for row in corpus["cases"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let model = serde_json::from_value::<SchemaNode>(row["expected_model"].clone());
        std::fs::write(
            evidence.join(format!("{id}-model.txt")),
            format!("{model:#?}"),
        )
        .unwrap();
        let model = model.unwrap();
        let encoded = encode(&model, 1024 * 1024);
        std::fs::write(
            evidence.join(format!("{id}-encoded.txt")),
            format!("{encoded:#?}"),
        )
        .unwrap();
        let encoded = encoded.unwrap();
        let decoded = decode(&encoded, 1024 * 1024);
        std::fs::write(
            evidence.join(format!("{id}-decoded.txt")),
            format!("{decoded:#?}"),
        )
        .unwrap();
        assert_eq!(decoded.unwrap(), model, "{}", row["id"]);
        if model.contains_json_array_wrapper() {
            assert!(encoded.starts_with(V3_PREFIX));
            assert!(encoded.contains("\"json_array_wrapper\":true"));
            let limited = decode(&encoded, encoded.len() - 1);
            std::fs::write(
                evidence.join(format!("{id}-byte-limit.txt")),
                format!("{limited:#?}"),
            )
            .unwrap();
            assert_eq!(
                limited,
                Err(CodecError::TooLarge {
                    bytes: encoded.len(),
                    max: encoded.len() - 1,
                })
            );
        }
    }
    let scalar = SchemaNode::scalar("item", ScalarType::Int);
    assert!(
        !encode(&scalar, 1024 * 1024)
            .unwrap()
            .contains("json_array_wrapper")
    );
    assert_eq!(
        encode(&scalar, 1024 * 1024).unwrap(),
        serde_json::to_string(&scalar).unwrap()
    );
    let mut malformed = scalar;
    malformed.json_array_wrapper = true;
    let encoded = serde_json::to_string(&malformed).unwrap();
    let malformed_result = decode(&encoded, 1024 * 1024);
    std::fs::write(
        evidence.join("malformed.txt"),
        format!("{malformed_result:#?}"),
    )
    .unwrap();
    assert!(
        matches!(malformed_result, Err(CodecError::Deserialization(ref message)) if message.starts_with("invalid JSON array-wrapper metadata"))
    );
    let malformed_v3 = decode(&format!("{V3_PREFIX}{encoded}"), 1024 * 1024);
    std::fs::write(
        evidence.join("malformed-v3.txt"),
        format!("{malformed_v3:#?}"),
    )
    .unwrap();
    assert!(
        matches!(malformed_v3, Err(CodecError::Deserialization(ref message)) if message.starts_with("invalid JSON array-wrapper metadata"))
    );
    let future = decode("FERRULE-EMBEDDED-SCHEMA/5\n{}", 1024 * 1024);
    std::fs::write(evidence.join("future.txt"), format!("{future:#?}")).unwrap();
    assert!(matches!(future, Err(CodecError::UnsupportedVersion)));
}
