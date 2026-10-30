//! JSON Schema import, canonical form, export and re-import of the shared
//! tuple-array corpus (`docs/design/json-tuple-arrays.md`), and the tuple
//! shapes the importer keeps refusing.

use format_json::JsonFormatError;
use format_json::json_schema::{export, import_str};
use ir::{ItemCountRange, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode};
use serde_json::Value as Json;

const CORPUS: &[u8] = include_bytes!("fixtures/json_tuple_arrays/cases.json");

fn corpus() -> Json {
    serde_json::from_slice(CORPUS).expect("the corpus is JSON")
}

fn text(schema: &Json) -> String {
    serde_json::to_string(schema).expect("a corpus schema is JSON")
}

fn model(case: &Json) -> SchemaNode {
    serde_json::from_value(case["expected_model"].clone()).expect("the expected model")
}

fn rendered(schema: &SchemaNode) -> String {
    serde_json::to_string(schema).expect("a schema node serializes")
}

/// A case's schema and every schema the corpus records as importing to the
/// same model.
fn schemas(case: &Json) -> impl Iterator<Item = &Json> {
    std::iter::once(&case["schema"]).chain(
        case.get("equivalent_schemas")
            .and_then(Json::as_array)
            .into_iter()
            .flatten(),
    )
}

/// Cases whose import another lane enables; until then the importer refuses
/// them.
fn refused_until(case: &Json) -> Option<&str> {
    case.get("import_refused_until").and_then(Json::as_str)
}

fn refusal_reason(result: Result<SchemaNode, JsonFormatError>) -> Result<String, String> {
    match result {
        Err(
            JsonFormatError::UnsupportedSchemaUnion { reason, .. }
            | JsonFormatError::UnsupportedSchemaObject { reason, .. },
        ) => Ok(reason),
        Err(other) => Err(format!("unexpected error {other:?}")),
        Ok(schema) => Err(format!("imported {}", rendered(&schema))),
    }
}

#[test]
fn corpus_schemas_import_to_their_canonical_models() {
    let corpus = corpus();
    let mut failures = Vec::new();
    let mut imported = 0;
    for case in corpus["cases"].as_array().expect("case list") {
        let id = case["id"].as_str().expect("case id");
        let expected = model(case);
        for (index, schema) in schemas(case).enumerate() {
            let result = import_str(&text(schema));
            if let Some(issue) = refused_until(case) {
                match refusal_reason(result) {
                    Ok(reason) if reason.contains("unevaluatedItems") => {}
                    other => failures.push(format!(
                        "{id} schema {index}: refused until {issue}, got {other:?}"
                    )),
                }
                continue;
            }
            match result {
                Ok(schema) if schema == expected => imported += 1,
                Ok(schema) => failures.push(format!(
                    "{id} schema {index}: imported {}",
                    rendered(&schema)
                )),
                Err(error) => failures.push(format!("{id} schema {index}: {error}")),
            }
        }
    }
    assert!(
        imported >= 20,
        "the corpus lost imported schemas: {imported}"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn corpus_models_export_canonically_and_reimport_identically() {
    let corpus = corpus();
    let mut failures = Vec::new();
    for case in corpus["cases"].as_array().expect("case list") {
        if refused_until(case).is_some() {
            continue;
        }
        let id = case["id"].as_str().expect("case id");
        let expected = model(case);
        let text = match export(&expected) {
            Ok(text) => text,
            Err(error) => {
                failures.push(format!("{id}: export: {error}"));
                continue;
            }
        };
        let exported: Json = serde_json::from_str(&text).expect("export writes JSON");
        if exported != case["export"] {
            failures.push(format!("{id}: exported {exported}"));
        }
        if text.contains("additionalItems") || text.contains("$schema") {
            failures.push(format!("{id}: export uses a dialect-specific spelling"));
        }
        match import_str(&text) {
            Ok(reimported) if reimported == expected => {}
            Ok(reimported) => failures.push(format!("{id}: re-imported {}", rendered(&reimported))),
            Err(error) => failures.push(format!("{id}: re-import: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn unsupported_tuple_shapes_are_refused() {
    let corpus = corpus();
    let mut failures = Vec::new();
    let cases = corpus["schema_refusal_cases"]
        .as_array()
        .expect("refusal list");
    assert!(
        cases.len() >= 20,
        "the corpus lost refusals: {}",
        cases.len()
    );
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let message = case["message"].as_str().expect("refusal message");
        match refusal_reason(import_str(&text(&case["schema"]))) {
            Ok(reason) if reason.contains(message) => {}
            other => failures.push(format!("{id}: expected `{message}`, got {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn export_refuses_a_non_canonical_frame() {
    // A closed frame whose maximum is not its member count.
    let mut frame = SchemaNode::group(
        "Pair",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    );
    frame.json_tuple = Some(JsonTupleLayout {
        length: ItemCountRange::new(0, Some(3)),
        tail: JsonTupleTail::Closed,
    });
    assert!(matches!(
        export(&frame),
        Err(JsonFormatError::InvalidJsonTupleFrameMetadata {
            reason: ir::JsonTupleFrameError::Length,
            ..
        })
    ));
}
