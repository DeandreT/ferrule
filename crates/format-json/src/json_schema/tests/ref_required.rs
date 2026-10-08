use std::cell::Cell;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ir::{Instance, ScalarType, SchemaNode, Value};
use serde_json::{Map, Value as JsonValue, json};

use crate::{JsonFormatError, json_schema};

// Each public result and its independent oracle are retained before assertions.
// These small authored fixtures are deliberately kept after success or failure.
struct Evidence {
    root: PathBuf,
    next: Cell<usize>,
}

impl Evidence {
    fn new(group: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ferrule-required-ref100-{group}-{}-{timestamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        eprintln!("REQUIRED_REF100_ORIGINALS={}", root.display());
        Self {
            root,
            next: Cell::new(0),
        }
    }

    fn retain(&self, label: &str, body: &str) {
        let ordinal = self.next.get();
        self.next.set(ordinal + 1);
        let path = self.root.join(format!("{ordinal:04}-{label}.txt"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(body.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }

    fn import(
        &self,
        label: &str,
        text: &str,
        from_file: bool,
    ) -> Result<SchemaNode, JsonFormatError> {
        self.retain(&format!("{label}-input-schema"), text);
        let actual = if from_file {
            let path = self.root.join(format!("{label}.schema.json"));
            std::fs::write(&path, text).unwrap();
            json_schema::import_with_root(&path, &self.root)
        } else {
            json_schema::import_str(text)
        };
        self.retain(&format!("{label}-import-result"), &format!("{actual:#?}\n"));
        actual
    }
}

fn expected_envelope(required: &[&str]) -> SchemaNode {
    let mut note = SchemaNode::scalar("note", ScalarType::String);
    note.nullable = true;
    SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::scalar("id", ScalarType::Int),
            note,
            SchemaNode::scalar("optional", ScalarType::String),
        ],
    )
    .with_required_fields(required.iter().map(|name| (*name).to_string()).collect())
    .unwrap()
}

fn expected_export(required: &[&str], nullable: bool) -> JsonValue {
    let content = json!({
        "type":"object", "required":required,
        "properties":{
            "id":{"type":"integer"},
            "note":{"type":["string","null"]},
            "optional":{"type":"string"}
        },
        "additionalProperties":false
    });
    if nullable {
        json!({"title":"Envelope", "anyOf":[content, {"type":"null"}]})
    } else {
        let mut root = Map::new();
        root.insert("title".into(), json!("Envelope"));
        root.extend(content.as_object().unwrap().clone());
        JsonValue::Object(root)
    }
}

fn reference_schema(dialect: Option<&str>, required: JsonValue, object_type: JsonValue) -> String {
    let mut root = json!({
        "title":"Envelope", "$ref":"#/$defs/Base", "required":required,
        "$defs":{"Base":{
            "title":"Envelope", "type":object_type, "additionalProperties":false,
            "required":["id"],
            "properties":{
                "id":{"type":"integer"},
                "note":{"type":["string","null"]},
                "optional":{"type":"string"}
            }
        }}
    });
    if let Some(dialect) = dialect {
        root["$schema"] = json!(dialect);
    }
    root.to_string()
}

fn assert_roundtrip(evidence: &Evidence, label: &str, schema: &SchemaNode, expected: &SchemaNode) {
    evidence.retain(
        &format!("{label}-expected-schema"),
        &format!("{expected:#?}\n"),
    );
    assert_eq!(schema, expected, "{label}");
    let expected_document = expected_export(
        &expected
            .required_fields()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        expected.container_nullable,
    );
    evidence.retain(
        &format!("{label}-expected-export"),
        &expected_document.to_string(),
    );
    let exported = json_schema::export(schema);
    evidence.retain(
        &format!("{label}-export-result"),
        &format!("{exported:#?}\n"),
    );
    let exported = exported.unwrap();
    assert_eq!(
        serde_json::from_str::<JsonValue>(&exported).unwrap(),
        expected_document
    );
    let reimported = json_schema::import_str(&exported);
    evidence.retain(
        &format!("{label}-reimport-result"),
        &format!("{reimported:#?}\n"),
    );
    assert_eq!(reimported.unwrap(), *expected);
}

fn present(id: Value, note: Value, optional: Value) -> Instance {
    Instance::Group(
        vec![
            ("id".into(), Instance::Scalar(id)),
            ("note".into(), Instance::Scalar(note)),
            ("optional".into(), Instance::Scalar(optional)),
        ]
        .into(),
    )
}

fn assert_missing(result: Result<Instance, JsonFormatError>, property: &str) {
    assert!(matches!(result,
        Err(JsonFormatError::MissingRequiredProperty { object, property: actual })
            if object == "Envelope" && actual == property
    ));
}

fn assert_presence_contract(evidence: &Evidence, label: &str, schema: &SchemaNode) {
    let expected = present(Value::Int(7), Value::json_null(), Value::Null);
    let input = r#"{"id":7,"note":null}"#;
    evidence.retain(&format!("{label}-valid-input"), input);
    evidence.retain(
        &format!("{label}-valid-expected"),
        &format!("{expected:#?}\n"),
    );
    let actual = crate::from_str(input, schema);
    evidence.retain(&format!("{label}-valid-read"), &format!("{actual:#?}\n"));
    assert_eq!(actual.unwrap(), expected);
    let output = crate::to_string(schema, &expected);
    evidence.retain(&format!("{label}-valid-write"), &format!("{output:#?}\n"));
    assert_eq!(
        serde_json::from_str::<JsonValue>(&output.unwrap()).unwrap(),
        json!({"id":7,"note":null})
    );
    for (index, input, property) in [
        (0, r#"{"id":7}"#, "note"),
        (1, r#"{"note":null}"#, "id"),
        (2, "{}", "id"),
    ] {
        evidence.retain(&format!("{label}-missing-{index}-input"), input);
        evidence.retain(
            &format!("{label}-missing-{index}-expected"),
            &format!("MissingRequiredProperty {{ object: Envelope, property: {property} }}\n"),
        );
        let actual = crate::from_str(input, schema);
        evidence.retain(
            &format!("{label}-missing-{index}-read"),
            &format!("{actual:#?}\n"),
        );
        assert_missing(actual, property);
    }
    for (index, instance, property) in [
        (0, present(Value::Int(7), Value::Null, Value::Null), "note"),
        (
            1,
            present(Value::Null, Value::String("present".into()), Value::Null),
            "id",
        ),
    ] {
        evidence.retain(
            &format!("{label}-absent-output-{index}"),
            &format!("input={instance:#?}\nexpected=MissingRequiredProperty Envelope.{property}\n"),
        );
        let actual = crate::to_string(schema, &instance);
        evidence.retain(
            &format!("{label}-absent-output-{index}-write"),
            &format!("{actual:#?}\n"),
        );
        assert!(matches!(actual,
            Err(JsonFormatError::MissingRequiredProperty { object, property: actual })
                if object == "Envelope" && actual == property
        ));
    }
    // Presence does not broaden a non-nullable scalar's input domain.
    let native_input = r#"{"id":null,"note":null}"#;
    evidence.retain(&format!("{label}-nonnullable-null-input"), native_input);
    evidence.retain(
        &format!("{label}-nonnullable-null-expected"),
        "Shape { name: id, expected: integer, got: null }\n",
    );
    let native_actual = crate::from_str(native_input, schema);
    evidence.retain(
        &format!("{label}-nonnullable-null-read"),
        &format!("{native_actual:#?}\n"),
    );
    assert!(matches!(native_actual,
        Err(JsonFormatError::Shape { name, expected: "integer", got: "null" })
            if name == "id"
    ));
}

#[test]
fn required_ref_modern_native_import_export_and_presence_are_exact() {
    let evidence = Evidence::new("modern");
    for (index, dialect) in [
        None,
        Some("https://json-schema.org/draft/2019-09/schema"),
        Some("http://json-schema.org/draft/2019-09/schema#"),
        Some("https://json-schema.org/draft/2020-12/schema"),
        Some("http://json-schema.org/draft/2020-12/schema#"),
    ]
    .into_iter()
    .enumerate()
    {
        let text = reference_schema(dialect, json!(["note", "id"]), json!("object"));
        for from_file in [false, true] {
            let label = format!("dialect-{index}-file-{from_file}");
            let schema = evidence.import(&label, &text, from_file).unwrap();
            assert_roundtrip(
                &evidence,
                &label,
                &schema,
                &expected_envelope(&["id", "note"]),
            );
            assert_presence_contract(&evidence, &label, &schema);
        }
    }
}

#[test]
fn required_ref_ordered_chains_and_explicit_nullable_objects_are_exact() {
    let evidence = Evidence::new("nullable-chain");
    let chain = r##"{
        "title":"Envelope", "$ref":"#/$defs/Inner", "required":["optional","id"],
        "$defs":{
            "Inner":{"$ref":"#/$defs/Base","required":["note","id"]},
            "Base":{"type":"object","additionalProperties":false,"required":["id"],
                "properties":{"id":{"type":"integer"},"note":{"type":["string","null"]},"optional":{"type":"string"}}}
        }
    }"##;
    let schema = evidence.import("ordered-chain", chain, false).unwrap();
    assert_roundtrip(
        &evidence,
        "ordered-chain",
        &schema,
        &expected_envelope(&["id", "note", "optional"]),
    );
    let input = r#"{"id":7,"note":null,"optional":"kept"}"#;
    let expected = present(
        Value::Int(7),
        Value::json_null(),
        Value::String("kept".into()),
    );
    let actual = crate::from_str(input, &schema);
    evidence.retain(
        "ordered-chain-instance",
        &format!("input={input}\nexpected={expected:#?}\nactual={actual:#?}\n"),
    );
    assert_eq!(actual.unwrap(), expected);
    for (index, object_type) in [json!(["object", "null"]), json!(["null", "object"])]
        .into_iter()
        .enumerate()
    {
        let label = format!("nullable-{index}");
        let text = reference_schema(None, json!(["note"]), object_type);
        let schema = evidence.import(&label, &text, false).unwrap();
        let mut expected_schema = expected_envelope(&["id", "note"]);
        expected_schema.container_nullable = true;
        assert_roundtrip(&evidence, &label, &schema, &expected_schema);
        assert_presence_contract(&evidence, &label, &schema);
        let expected_null = Instance::Scalar(Value::json_null());
        let actual_null = crate::from_str("null", &schema);
        evidence.retain(
            &format!("{label}-root-null"),
            &format!("input=null\nexpected={expected_null:#?}\nactual={actual_null:#?}\n"),
        );
        assert_eq!(actual_null.unwrap(), expected_null);
        let output = crate::to_string(&schema, &expected_null);
        evidence.retain(
            &format!("{label}-root-null-write"),
            &format!("expected=null\nactual={output:#?}\n"),
        );
        assert_eq!(output.unwrap(), "null\n");
    }
    let nested = r##"{
        "title":"Outer", "type":"object", "additionalProperties":false,"required":["payload"],
        "properties":{"payload":{"$ref":"#/$defs/Payload","required":["note"]}},
        "$defs":{"Payload":{"type":["object","null"],"additionalProperties":false,"required":["id"],
            "properties":{"id":{"type":"integer"},"note":{"type":["string","null"]}}}}
    }"##;
    let nested_schema = evidence.import("nested-nullable", nested, false).unwrap();
    let mut note = SchemaNode::scalar("note", ScalarType::String);
    note.nullable = true;
    let mut payload = SchemaNode::group(
        "payload",
        vec![SchemaNode::scalar("id", ScalarType::Int), note],
    )
    .with_required_fields(vec!["id".into(), "note".into()])
    .unwrap();
    payload.container_nullable = true;
    let expected_schema = SchemaNode::group("Outer", vec![payload])
        .with_required_fields(vec!["payload".into()])
        .unwrap();
    evidence.retain(
        "nested-full-expected-schema",
        &format!("{expected_schema:#?}\n"),
    );
    assert_eq!(nested_schema, expected_schema);
    let expected_export = json!({"title":"Outer","type":"object","required":["payload"],"properties":{"payload":{"anyOf":[
        {"type":"object","required":["id","note"],"properties":{"id":{"type":"integer"},"note":{"type":["string","null"]}},"additionalProperties":false},
        {"type":"null"}
    ]}},"additionalProperties":false});
    let actual_export = json_schema::export(&nested_schema);
    evidence.retain(
        "nested-full-export",
        &format!("expected={expected_export}\nactual={actual_export:#?}\n"),
    );
    let actual_export = actual_export.unwrap();
    assert_eq!(
        serde_json::from_str::<JsonValue>(&actual_export).unwrap(),
        expected_export
    );
    let actual_reimport = json_schema::import_str(&actual_export);
    evidence.retain("nested-full-reimport", &format!("{actual_reimport:#?}\n"));
    assert_eq!(actual_reimport.unwrap(), expected_schema);
    for (index, input, expected_object, expected_property) in [
        (0, "{}", "Outer", "payload"),
        (1, r#"{"payload":{}}"#, "payload", "id"),
    ] {
        let actual = crate::from_str(input, &nested_schema);
        evidence.retain(&format!("nested-missing-{index}"), &format!("input={input}\nexpected=MissingRequiredProperty {{ object: {expected_object}, property: {expected_property} }}\nactual={actual:#?}\n"));
        assert!(
            matches!(actual, Err(JsonFormatError::MissingRequiredProperty { object,property }) if object == expected_object && property == expected_property)
        );
    }
    let actual = crate::from_str(r#"{"payload":null}"#, &nested_schema);
    let expected =
        Instance::Group(vec![("payload".into(), Instance::Scalar(Value::json_null()))].into());
    evidence.retain(
        "nested-present-null",
        &format!("input={{\"payload\":null}}\nexpected={expected:#?}\nactual={actual:#?}\n"),
    );
    assert_eq!(actual.unwrap(), expected);
    let output = crate::to_string(&nested_schema, &expected);
    evidence.retain(
        "nested-present-null-write",
        &format!("expected={{\"payload\":null}}\nactual={output:#?}\n"),
    );
    assert_eq!(
        serde_json::from_str::<JsonValue>(&output.unwrap()).unwrap(),
        json!({"payload":null})
    );
    let outer_all_of = r##"{"title":"Envelope","allOf":[
        {"$ref":"#/$defs/Base","required":["note"]},
        {"type":"object","additionalProperties":false,"required":["optional"],
            "properties":{"id":{"type":"integer"},"note":{"type":["string","null"]},"optional":{"type":"string"}}}
    ],"$defs":{"Base":{"type":"object","additionalProperties":false,"required":["id"],
        "properties":{"id":{"type":"integer"},"note":{"type":["string","null"]},"optional":{"type":"string"}}}}}"##;
    let schema = evidence
        .import("compatible-outer-all-of", outer_all_of, false)
        .unwrap();
    assert_roundtrip(
        &evidence,
        "compatible-outer-all-of",
        &schema,
        &expected_envelope(&["id", "note", "optional"]),
    );
    let expected = present(
        Value::Int(7),
        Value::json_null(),
        Value::String("kept".into()),
    );
    let input = r#"{"id":7,"note":null,"optional":"kept"}"#;
    let actual = crate::from_str(input, &schema);
    evidence.retain(
        "outer-all-of-present",
        &format!("input={input}\nexpected={expected:#?}\nactual={actual:#?}\n"),
    );
    assert_eq!(actual.unwrap(), expected);
    let output = crate::to_string(&schema, &expected);
    evidence.retain("outer-all-of-present-write",&format!("input={expected:#?}\nexpected={{\"id\":7,\"note\":null,\"optional\":\"kept\"}}\nactual={output:#?}\n"));
    assert_eq!(
        serde_json::from_str::<JsonValue>(&output.unwrap()).unwrap(),
        json!({"id":7,"note":null,"optional":"kept"})
    );
    let actual = crate::from_str(r#"{"id":7,"note":null}"#, &schema);
    evidence.retain("outer-all-of-missing",&format!("input={{\"id\":7,\"note\":null}}\nexpected=MissingRequiredProperty Envelope.optional\nactual={actual:#?}\n"));
    assert_missing(actual, "optional");
}

#[test]
fn required_ref_declared_names_preserve_open_domains_and_underlying_presence() {
    let evidence = Evidence::new("open-domain");
    for (index, additional) in [json!(true), json!({"type":"string"})]
        .into_iter()
        .enumerate()
    {
        let mut document: JsonValue =
            serde_json::from_str(&reference_schema(None, json!(["note"]), json!("object")))
                .unwrap();
        document["$defs"]["Base"]["additionalProperties"] = additional;
        let text = document.to_string();
        let schema = evidence
            .import(&format!("open-{index}"), &text, false)
            .unwrap();
        let dynamic = if index == 0 {
            SchemaNode::scalar("*", ScalarType::String)
                .json_any()
                .unwrap()
        } else {
            SchemaNode::scalar("*", ScalarType::String)
        };
        let expected = expected_envelope(&["id", "note"])
            .with_dynamic_fields(dynamic)
            .unwrap();
        evidence.retain(
            &format!("open-{index}-expected-schema"),
            &format!("{expected:#?}\n"),
        );
        assert_eq!(schema, expected);
        let mut expected_document = expected_export(&["id", "note"], false);
        expected_document["additionalProperties"] = if index == 0 {
            json!({})
        } else {
            json!({"type":"string"})
        };
        let actual_export = json_schema::export(&schema);
        evidence.retain(
            &format!("open-{index}-export"),
            &format!("expected={expected_document}\nactual={actual_export:#?}\n"),
        );
        let actual_export = actual_export.unwrap();
        assert_eq!(
            serde_json::from_str::<JsonValue>(&actual_export).unwrap(),
            expected_document
        );
        let reimported = json_schema::import_str(&actual_export);
        evidence.retain(
            &format!("open-{index}-reimport"),
            &format!("{reimported:#?}\n"),
        );
        assert_eq!(reimported.unwrap(), expected);
        let input = r#"{"id":7,"note":null,"extra":"kept"}"#;
        let value = if index == 0 {
            Value::String("\"kept\"".into())
        } else {
            Value::String("kept".into())
        };
        let expected_instance = Instance::Group(
            vec![
                ("id".into(), Instance::Scalar(Value::Int(7))),
                ("note".into(), Instance::Scalar(Value::json_null())),
                ("extra".into(), Instance::Scalar(value)),
                ("optional".into(), Instance::Scalar(Value::Null)),
            ]
            .into(),
        );
        let actual = crate::from_str(input, &schema);
        evidence.retain(
            &format!("open-{index}-read"),
            &format!("input={input}\nexpected={expected_instance:#?}\nactual={actual:#?}\n"),
        );
        assert_eq!(actual.unwrap(), expected_instance);
        let output = crate::to_string(&schema, &expected_instance);
        evidence.retain(
            &format!("open-{index}-write"),
            &format!("input={expected_instance:#?}\nexpected={input}\nactual={output:#?}\n"),
        );
        assert_eq!(
            serde_json::from_str::<JsonValue>(&output.unwrap()).unwrap(),
            json!({"id":7,"note":null,"extra":"kept"})
        );
    }
    let mut document: JsonValue =
        serde_json::from_str(&reference_schema(None, json!(["note"]), json!("object"))).unwrap();
    document["$defs"]["Base"]["additionalProperties"] = json!(true);
    document["$defs"]["Base"]["required"] = json!(["id", "runtime-required"]);
    let schema = evidence
        .import("base-runtime-presence", &document.to_string(), false)
        .unwrap();
    let expected = expected_envelope(&["id"])
        .with_dynamic_fields(
            SchemaNode::scalar("*", ScalarType::String)
                .json_any()
                .unwrap(),
        )
        .unwrap()
        .with_required_fields(vec!["id".into(), "runtime-required".into(), "note".into()])
        .unwrap();
    evidence.retain("base-runtime-expected-schema", &format!("{expected:#?}\n"));
    assert_eq!(schema, expected);
    let mut expected_document = expected_export(&["id", "runtime-required", "note"], false);
    expected_document["additionalProperties"] = json!({});
    let actual_export = json_schema::export(&schema);
    evidence.retain(
        "base-runtime-export",
        &format!("expected={expected_document}\nactual={actual_export:#?}\n"),
    );
    let actual_export = actual_export.unwrap();
    assert_eq!(
        serde_json::from_str::<JsonValue>(&actual_export).unwrap(),
        expected_document
    );
    let reimported = json_schema::import_str(&actual_export);
    evidence.retain(
        "base-runtime-reimport",
        &format!("expected={expected:#?}\nactual={reimported:#?}\n"),
    );
    assert_eq!(reimported.unwrap(), expected);
    let input = r#"{"id":7,"runtime-required":9,"note":null}"#;
    let expected_instance = Instance::Group(
        vec![
            ("id".into(), Instance::Scalar(Value::Int(7))),
            (
                "runtime-required".into(),
                Instance::Scalar(Value::String("9".into())),
            ),
            ("note".into(), Instance::Scalar(Value::json_null())),
            ("optional".into(), Instance::Scalar(Value::Null)),
        ]
        .into(),
    );
    let actual = crate::from_str(input, &schema);
    evidence.retain(
        "base-runtime-read",
        &format!("input={input}\nexpected={expected_instance:#?}\nactual={actual:#?}\n"),
    );
    assert_eq!(actual.unwrap(), expected_instance);
    let output = crate::to_string(&schema, &expected_instance);
    evidence.retain(
        "base-runtime-write",
        &format!("input={expected_instance:#?}\nexpected={input}\nactual={output:#?}\n"),
    );
    assert_eq!(
        serde_json::from_str::<JsonValue>(&output.unwrap()).unwrap(),
        json!({"id":7,"runtime-required":9,"note":null})
    );
    let actual = crate::from_str(r#"{"id":7,"note":null}"#, &schema);
    evidence.retain("base-runtime-still-required",&format!("input={{\"id\":7,\"note\":null}}\nexpected=MissingRequiredProperty Envelope.runtime-required\nactual={actual:#?}\n"));
    assert_missing(actual, "runtime-required");
}

#[test]
fn required_ref_legacy_ignore_and_external_resource_policies_remain_independent() {
    let evidence = Evidence::new("resource-dialects");
    for (index, dialect) in [
        "http://json-schema.org/draft-04/schema#",
        "https://json-schema.org/draft-04/schema",
        "http://json-schema.org/draft-06/schema#",
        "https://json-schema.org/draft-06/schema",
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft-07/schema",
    ]
    .into_iter()
    .enumerate()
    {
        for (value_index, ignored) in [
            json!("malformed"),
            json!(["missing"]),
            json!(["note", "note"]),
        ]
        .into_iter()
        .enumerate()
        {
            let label = format!("legacy-{index}-{value_index}");
            let text = reference_schema(Some(dialect), ignored, json!("object"));
            let schema = evidence.import(&label, &text, false).unwrap();
            assert_roundtrip(&evidence, &label, &schema, &expected_envelope(&["id"]));
            let input = r#"{"id":7}"#;
            let expected = present(Value::Int(7), Value::Null, Value::Null);
            let actual = crate::from_str(input, &schema);
            evidence.retain(
                &format!("{label}-missing-note-allowed"),
                &format!("input={input}\nexpected={expected:#?}\nactual={actual:#?}\n"),
            );
            assert_eq!(actual.unwrap(), expected);
        }
    }
    for root_modern in [false, true] {
        for target_modern in [false, true] {
            let label = format!("root-{root_modern}-target-{target_modern}");
            let resource_dir = evidence.root.join(&label);
            std::fs::create_dir(&resource_dir).unwrap();
            let dialect = |modern| {
                if modern {
                    "https://json-schema.org/draft/2020-12/schema"
                } else {
                    "http://json-schema.org/draft-07/schema#"
                }
            };
            let root = json!({"$schema":dialect(root_modern),"$ref":"external.schema.json#/$defs/Use","required":["optional"]}).to_string();
            let external = json!({
                "$schema":dialect(target_modern),
                "$defs":{
                    "Use":{"$ref":"#/$defs/Base","required":["note"]},
                    "Base":{"title":"Envelope","type":"object","additionalProperties":false,"required":["id"],
                        "properties":{"id":{"type":"integer"},"note":{"type":["string","null"]},"optional":{"type":"string"}}}
                }
            }).to_string();
            std::fs::write(resource_dir.join("root.schema.json"), &root).unwrap();
            std::fs::write(resource_dir.join("external.schema.json"), &external).unwrap();
            let mut expected_names = vec!["id"];
            if target_modern {
                expected_names.push("note");
            }
            if root_modern {
                expected_names.push("optional");
            }
            let expected = expected_envelope(&expected_names);
            let actual = json_schema::import_with_root(
                &resource_dir.join("root.schema.json"),
                &resource_dir,
            );
            evidence.retain(
                &format!("{label}-public-import"),
                &format!(
                    "root={root}\nexternal={external}\nexpected={expected:#?}\nactual={actual:#?}\n"
                ),
            );
            let schema = actual.unwrap();
            assert_roundtrip(&evidence, &label, &schema, &expected);
        }
    }
    // An ignored type beside a legacy $ref is never terminal shape evidence.
    for object_terminal in [false, true] {
        let label = format!("ignored-type-evidence-{object_terminal}");
        let directory = evidence.root.join(&label);
        std::fs::create_dir(&directory).unwrap();
        let root = r#"{"title":"Envelope","$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"legacy.schema.json#/$defs/Use","required":["note"]}"#;
        let base = if object_terminal {
            json!({"type":"object","additionalProperties":false,"required":["id"],
            "properties":{"id":{"type":"integer"},"note":{"type":["string","null"]},"optional":{"type":"string"}}})
        } else {
            json!({"type":"integer"})
        };
        let external = json!({"$schema":"http://json-schema.org/draft-07/schema#","$defs":{
            "Use":{"$ref":"#/$defs/Base","type":if object_terminal { "integer" } else { "object" },"required":"ignored-malformed"},
            "Base":base
        }}).to_string();
        std::fs::write(directory.join("root.schema.json"), root).unwrap();
        std::fs::write(directory.join("legacy.schema.json"), &external).unwrap();
        let actual = json_schema::import_with_root(&directory.join("root.schema.json"), &directory);
        evidence.retain(
            &format!("{label}-public-import"),
            &format!("root={root}\nexternal={external}\nactual={actual:#?}\n"),
        );
        if object_terminal {
            let expected = expected_envelope(&["id", "note"]);
            assert_roundtrip(&evidence, &label, &actual.unwrap(), &expected);
        } else {
            assert_old_shape_refusal(actual);
        }
    }
}

fn assert_old_shape_refusal(actual: Result<SchemaNode, JsonFormatError>) {
    assert!(matches!(actual,
        Err(JsonFormatError::UnsupportedSchemaUnion { name, reason })
            if name == "Envelope" && reason == "modern `$ref` sibling `required` requires an unsupported intersection and cannot be ignored"
    ));
}

#[test]
fn required_ref_unknown_names_and_excluded_shapes_keep_precise_refusals() {
    let evidence = Evidence::new("refusals");
    for (index, terminal) in [
        json!({"type":"integer"}),
        json!({"type":"array","items":{"type":"object","properties":{"id":{"type":"integer"}}}}),
        json!({"properties":{"id":{"type":"integer"}}}),
        json!({"type":["object","string"],"properties":{"id":{"type":"integer"}}}),
        json!({"type":["object","object"],"properties":{"id":{"type":"integer"}}}),
        json!({"type":"object","properties":false}),
        json!({"type":"object","allOf":[{"type":"object","properties":{"id":{"type":"integer"}}}]}),
        json!({"anyOf":[{"type":"object","properties":{"id":{"type":"integer"}}},{"type":"null"}]}),
        json!({"oneOf":[{"type":"object","properties":{"id":{"type":"integer"}}},{"type":"null"}]}),
        json!({}),
        json!(true),
        json!(false),
    ]
    .into_iter()
    .enumerate()
    {
        let label = format!("unsupported-shape-{index}");
        let text = json!({"title":"Envelope","$ref":"#/$defs/Base","required":["id"],"$defs":{"Base":terminal}}).to_string();
        let actual = evidence.import(&label, &text, false);
        evidence.retain(&format!("{label}-expected"), "UnsupportedSchemaUnion Envelope: modern `$ref` sibling `required` requires an unsupported intersection and cannot be ignored\n");
        assert_old_shape_refusal(actual);
    }
    for (index, additional) in [json!(false), json!(true), json!({"type":"string"})]
        .into_iter()
        .enumerate()
    {
        let text = json!({"title":"Envelope","$ref":"#/$defs/Base","required":["unknown"],"$defs":{"Base":{"type":"object","additionalProperties":additional,"properties":{"id":{"type":"integer"}}}}}).to_string();
        let actual = evidence.import(&format!("unknown-{index}"), &text, false);
        assert!(matches!(actual,
            Err(JsonFormatError::UnsupportedSchemaObject { name, reason })
                if name == "Envelope" && reason == "modern `$ref` sibling `required` name `unknown` must identify a declared property"
        ));
    }
    for (index, keyword, value) in [
        (0, "minimum", json!(0)),
        (1, "minProperties", json!(1)),
        (2, "format", json!("custom")),
        (3, "properties", json!({})),
        (4, "const", json!("x")),
        (5, "futureAssertion", json!(true)),
    ] {
        let mut document: JsonValue =
            serde_json::from_str(&reference_schema(None, json!(["note"]), json!("object")))
                .unwrap();
        document[keyword] = value;
        let actual = evidence.import(&format!("mixed-{index}"), &document.to_string(), false);
        assert!(matches!(actual,
            Err(JsonFormatError::UnsupportedSchemaUnion { name, reason })
                if name == "Envelope" && reason == format!("modern `$ref` sibling `{keyword}` cannot be combined with the required-only object subset")
        ));
    }
    for (index, reference, defs) in [
        (0, "#/$defs/Missing", json!({})),
        (1, "#/$defs/Loop", json!({"Loop":{"$ref":"#/$defs/Loop"}})),
        (
            2,
            "#/$defs/Loop",
            json!({"Loop":{"$ref":"#/$defs/Loop","required":["id"]}}),
        ),
    ] {
        let text =
            json!({"title":"Envelope","$ref":reference,"required":["id"],"$defs":defs}).to_string();
        assert_old_shape_refusal(evidence.import(&format!("unresolved-{index}"), &text, false));
    }
    let composition = r##"{"title":"Envelope","oneOf":[
        {"$ref":"#/$defs/A","required":["a"]},{"$ref":"#/$defs/B"}],
        "$defs":{
            "A":{"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"}}},
            "B":{"type":"object","additionalProperties":false,"required":["b"],"properties":{"b":{"type":"string"}}}
        }}"##;
    assert_old_shape_refusal(evidence.import(
        "original-composition-classifier",
        composition,
        false,
    ));
    let incompatible_outer = r##"{"title":"Envelope","allOf":[
        {"$ref":"#/$defs/Base","required":["note"]},
        {"type":"object","additionalProperties":false,"required":["foreign"],"properties":{"foreign":{"type":"string"}}}
    ],"$defs":{"Base":{"type":"object","additionalProperties":false,"required":["id"],
        "properties":{"id":{"type":"integer"},"note":{"type":"string"}}}}}"##;
    let actual = evidence.import("incompatible-outer-all-of", incompatible_outer, false);
    assert!(
        matches!(actual,Err(JsonFormatError::UnsupportedSchemaUnion { name,reason })
        if name == "Envelope" && reason == "allOf requires a property forbidden by a closed object branch")
    );
}

fn large_object(count: usize, base_required: usize, sibling_required: JsonValue) -> String {
    let mut properties = Map::new();
    for index in 0..count {
        properties.insert(format!("p{index:03}"), json!({"type":"integer"}));
    }
    json!({"title":"Bounded","$ref":"#/$defs/Base","required":sibling_required,
        "$defs":{"Base":{"type":"object","additionalProperties":false,"properties":properties,
            "required":(0..base_required).map(|index| format!("p{index:03}")).collect::<Vec<_>>()}}}).to_string()
}

fn assert_required_limit(actual: Result<SchemaNode, JsonFormatError>) {
    assert!(matches!(
        actual,
        Err(JsonFormatError::SchemaResourceLimit {
            kind: "required reference sibling names",
            limit: 256
        })
    ));
}

#[test]
fn required_ref_malformed_names_and_256_name_bound_apply_only_to_the_new_subset() {
    let evidence = Evidence::new("names-budget");
    for (index, required, reason) in [
        (
            0,
            json!("id"),
            "required must be an array of unique property names",
        ),
        (
            1,
            json!(null),
            "required must be an array of unique property names",
        ),
        (
            2,
            json!([""]),
            "required must contain only non-empty property names",
        ),
        (
            3,
            json!([7]),
            "required must contain only non-empty property names",
        ),
        (
            4,
            json!(["note", "note"]),
            "required property names must be unique",
        ),
    ] {
        let actual = evidence.import(
            &format!("malformed-{index}"),
            &reference_schema(None, required, json!("object")),
            false,
        );
        assert!(
            matches!(actual, Err(JsonFormatError::UnsupportedSchemaObject { name, reason:actual }) if name == "Envelope" && actual == reason)
        );
    }
    let empty = evidence
        .import(
            "empty-sibling",
            &reference_schema(None, json!([]), json!("object")),
            false,
        )
        .unwrap();
    assert_roundtrip(
        &evidence,
        "empty-sibling",
        &empty,
        &expected_envelope(&["id"]),
    );
    let names256 = (0..256)
        .map(|index| format!("p{index:03}"))
        .collect::<Vec<_>>();
    let text = large_object(256, 1, json!(names256));
    let accepted = evidence.import("exact-256", &text, false).unwrap();
    let expected = SchemaNode::group(
        "Bounded",
        (0..256)
            .map(|index| SchemaNode::scalar(format!("p{index:03}"), ScalarType::Int))
            .collect(),
    )
    .with_required_fields(names256.clone())
    .unwrap();
    evidence.retain("exact-256-expected", &format!("{expected:#?}\n"));
    assert_eq!(accepted, expected);
    let reimported = json_schema::import_str(&json_schema::export(&accepted).unwrap());
    evidence.retain("exact-256-reimport", &format!("{reimported:#?}\n"));
    assert_eq!(reimported.unwrap(), expected);
    let names257 = (0..257)
        .map(|index| format!("p{index:03}"))
        .collect::<Vec<_>>();
    assert_required_limit(evidence.import(
        "sibling-257",
        &large_object(257, 1, json!(names257)),
        false,
    ));
    assert_required_limit(evidence.import(
        "merged-257",
        &large_object(257, 256, json!(["p256"])),
        false,
    ));
    let duplicated_union = evidence
        .import(
            "duplicate-union-256",
            &large_object(256, 256, json!(["p255", "p000"])),
            false,
        )
        .unwrap();
    assert_eq!(duplicated_union, expected);
    // Unaffected direct schemas, refs without required, and legacy ignored
    // malformed/over-budget siblings retain their pre-existing larger domains.
    let mut ordinary: JsonValue = serde_json::from_str(&large_object(257, 257, json!([]))).unwrap();
    let direct = ordinary["$defs"]["Base"].clone();
    let actual_direct = evidence
        .import("ordinary-257", &direct.to_string(), false)
        .unwrap();
    let mut expected257 = SchemaNode::group(
        "root",
        (0..257)
            .map(|index| SchemaNode::scalar(format!("p{index:03}"), ScalarType::Int))
            .collect(),
    )
    .with_required_fields(names257.clone())
    .unwrap();
    evidence.retain("ordinary-257-full-expected", &format!("{expected257:#?}\n"));
    assert_eq!(actual_direct, expected257);
    ordinary.as_object_mut().unwrap().remove("required");
    let plain_ref = evidence
        .import("plain-reference-257", &ordinary.to_string(), false)
        .unwrap();
    expected257.name = "Bounded".into();
    evidence.retain(
        "plain-reference-257-full-expected",
        &format!("{expected257:#?}\n"),
    );
    assert_eq!(plain_ref, expected257);
    ordinary["$schema"] = json!("http://json-schema.org/draft-07/schema#");
    ordinary["required"] = json!(
        (0..257)
            .map(|index| format!("ignored-{index}"))
            .collect::<Vec<_>>()
    );
    let legacy = evidence
        .import("legacy-ignore-257", &ordinary.to_string(), false)
        .unwrap();
    expected257.name = "root".into();
    evidence.retain(
        "legacy-ignore-257-full-expected",
        &format!("{expected257:#?}\n"),
    );
    assert_eq!(legacy, expected257);
}

fn reference_chain(count: usize) -> String {
    let mut definitions = Map::new();
    for index in 0..count {
        let value = if index + 1 == count {
            json!({"type":"object","additionalProperties":false,"properties":{"id":{"type":"integer"}}})
        } else {
            json!({"$ref":format!("#/$defs/N{}",index+1)})
        };
        definitions.insert(format!("N{index}"), value);
    }
    json!({"title":"Envelope","$ref":"#/$defs/N0","required":["id"],"$defs":definitions})
        .to_string()
}

#[test]
fn required_ref_depth_and_transactional_constraint_refusals_do_not_weaken_existing_metadata() {
    let evidence = Evidence::new("depth-transaction");
    let accepted = evidence
        .import("reference-depth-64", &reference_chain(64), false)
        .unwrap();
    let expected = SchemaNode::group("Envelope", vec![SchemaNode::scalar("id", ScalarType::Int)])
        .with_required_fields(vec!["id".into()])
        .unwrap();
    evidence.retain("reference-depth-64-expected", &format!("{expected:#?}\n"));
    assert_eq!(accepted, expected);
    let rejected = evidence.import("reference-depth-65", &reference_chain(65), false);
    assert!(matches!(
        rejected,
        Err(JsonFormatError::SchemaResourceLimit {
            kind: "reference depth",
            limit: 64
        })
    ));
    let assertion_chain = r##"{"title":"Envelope","$ref":"#/$defs/Inner","required":["note"],
        "$defs":{"Inner":{"$ref":"#/$defs/Base","maxProperties":1},
            "Base":{"type":"object","additionalProperties":false,"required":["id"],
                "properties":{"id":{"type":"integer"},"note":{"type":"string"}}}}}"##;
    let actual = evidence.import("chain-assertion-preserved", assertion_chain, false);
    assert!(
        matches!(actual,Err(JsonFormatError::UnsupportedSchemaObject { name,reason })
        if name == "Envelope" && reason == "modern `$ref` sibling `required` conflicts with the referenced object's presence constraints")
    );
    let bounded = r##"{"title":"Envelope","$ref":"#/$defs/Base","required":["note"],
        "$defs":{"Base":{"type":"object","additionalProperties":false,"maxProperties":1,"required":["id"],
            "properties":{"id":{"type":"integer"},"note":{"type":"string"}}}}}"##;
    let actual = evidence.import("required-count-conflict", bounded, false);
    assert!(
        matches!(actual, Err(JsonFormatError::UnsupportedSchemaObject { name,reason })
        if name == "Envelope" && reason == "modern `$ref` sibling `required` conflicts with the referenced object's presence constraints")
    );
    let direct = r#"{"title":"Envelope","type":"object","additionalProperties":false,"maxProperties":1,"required":["id"],
        "properties":{"id":{"type":"integer"},"note":{"type":"string"}}}"#;
    let mut node = evidence
        .import("transactional-preimage", direct, false)
        .unwrap();
    let before = node.clone();
    // The importer uses this existing transactional setter. Keep these
    // controls independent of the new helper so old-importer witnesses compile.
    let attempted = vec!["id".to_string(), "note".to_string()];
    let actual = node.set_required_fields(attempted.clone());
    evidence.retain("transactional-count-result", &format!("before={before:#?}\nattempted={attempted:#?}\nexpected=false and unchanged full schema\nactual={actual:#?}\nafter={node:#?}\n"));
    assert!(!actual);
    assert_eq!(node, before);
    let attempted = vec!["id".to_string(), "unknown".to_string()];
    let actual = node.set_required_fields(attempted.clone());
    evidence.retain("transactional-unknown-result", &format!("before={before:#?}\nattempted={attempted:#?}\nexpected=false and unchanged full schema\nactual={actual:#?}\nafter={node:#?}\n"));
    assert!(!actual);
    assert_eq!(node, before);
    // A failed required sibling must restore the caller's active-reference set.
    let value = json!({"$ref":"#/$defs/Base","required":["missing"],"$defs":{"Base":{"type":"object","properties":{},"additionalProperties":false}}});
    let mut active = vec!["#/outer".to_string()];
    let actual = super::super::parse("Envelope", &value, &value, &mut active);
    evidence.retain("active-reference-restoration", &format!("schema={value}\nexpected=UnsupportedSchemaObject Envelope: modern `$ref` sibling `required` name `missing` must identify a declared property\nexpected_active=[#/outer]\nactual={actual:#?}\nafter_active={active:#?}\n"));
    assert!(
        matches!(actual,Err(JsonFormatError::UnsupportedSchemaObject { name,reason })
        if name == "Envelope" && reason == "modern `$ref` sibling `required` name `missing` must identify a declared property")
    );
    assert_eq!(active, ["#/outer"]);
}
