use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ir::{Instance, SchemaNode, Value};
use serde_json::{Value as Json, json};

use crate::JsonFormatError;

const CORPUS: &str = include_str!("fixtures/scalar_type_siblings.json");

fn error(error: &JsonFormatError) -> Json {
    match error {
        JsonFormatError::UnsupportedSchemaUnion { name, reason } => {
            json!({"name":name,"reason":reason})
        }
        JsonFormatError::Shape {
            name,
            expected,
            got,
        } => {
            json!({"category":"shape","name":name,"expected":expected,"got":got})
        }
        JsonFormatError::AllowedValueMismatch { name, got } => {
            json!({"category":"allowed","name":name,"got":got})
        }
        JsonFormatError::StringLengthMismatch { name, range, got } => {
            json!({"category":"string_length","name":name,"range":range,"got":got})
        }
        JsonFormatError::RangeMismatch { name, range, got } => {
            json!({"category":"range","name":name,"range":range,"got":got})
        }
        other => json!({"unexpected_complete_error":format!("{other:#?}")}),
    }
}

fn expected_value(expected: &Json) -> Result<Instance, Box<dyn std::error::Error>> {
    Ok(Instance::Scalar(match expected["kind"].as_str() {
        Some("string") => Value::String(expected["value"].as_str().ok_or("String payload")?.into()),
        Some("bool") => Value::Bool(expected["value"].as_bool().ok_or("Bool payload")?),
        Some("int") => Value::Int(expected["value"].as_i64().ok_or("Int payload")?),
        Some("float") => Value::Float(expected["value"].as_f64().ok_or("Float payload")?),
        Some("json_null") => Value::json_null(),
        _ => return Err("independent scalar kind required".into()),
    }))
}

#[test]
fn redundant_scalar_type_siblings_complete_native_contract()
-> Result<(), Box<dyn std::error::Error>> {
    let parent = std::env::var_os("FERRULE_JSON_SCALAR_TYPE_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if !parent.is_absolute() {
        return Err("evidence parent must be absolute".into());
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let retained = parent.join(format!(
        "ferrule-json-scalar-types284-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&retained)?;
    std::fs::write(retained.join("cases.json"), CORPUS)?;
    eprintln!("SCALAR_TYPES284_ORIGINALS={}", retained.display());
    let corpus: Json = serde_json::from_str(CORPUS)?;
    let fixtures = corpus["fixtures"].as_array().ok_or("fixtures")?;
    let refusals = corpus["refusals"].as_array().ok_or("refusals")?;
    let mut failures = Vec::new();
    let mut calls = 0;
    for fixture in fixtures {
        let id = fixture["id"].as_str().ok_or("fixture identity")?;
        let input = serde_json::to_string(&fixture["schema"])?;
        let imported = super::super::import_str(&input);
        std::fs::write(
            retained.join(format!("{id}-IMPORT-ORIGINAL.txt")),
            format!("{imported:#?}\n"),
        )?;
        let schema = match imported {
            Ok(schema) => schema,
            Err(error) => {
                failures.push(format!("{id}: unexpected complete import {error:?}"));
                continue;
            }
        };
        let expected: SchemaNode = serde_json::from_value(fixture["expected_model"].clone())?;
        let exported = super::super::export(&schema);
        std::fs::write(
            retained.join(format!("{id}-EXPORT-ORIGINAL.txt")),
            format!("{exported:#?}\n"),
        )?;
        let reimported = exported
            .as_ref()
            .ok()
            .map(|text| super::super::import_str(text));
        std::fs::write(
            retained.join(format!("{id}-REIMPORT-ORIGINAL.txt")),
            format!("{reimported:#?}\n"),
        )?;
        if schema != expected
            || !reimported
                .as_ref()
                .is_some_and(|result| result.as_ref().is_ok_and(|node| node == &expected))
        {
            failures.push(format!(
                "{id}: complete schema or canonical round trip differs"
            ));
        }
        for (index, case) in fixture["cases"]
            .as_array()
            .ok_or("value cases")?
            .iter()
            .enumerate()
        {
            calls += 1;
            let input = case["input"].as_str().ok_or("literal document")?;
            let actual = crate::from_str(input, &schema);
            let written = actual
                .as_ref()
                .ok()
                .map(|value| crate::to_string(&schema, value));
            let roundtrip = reimported
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(|schema| crate::from_str(input, schema));
            std::fs::write(
                retained.join(format!("{id}-{index}-COMPLETE-ORIGINAL.txt")),
                format!(
                    "input={input:?}\nactual={actual:#?}\nwritten={written:#?}\nroundtrip={roundtrip:#?}\nexpected={case:#?}\n"
                ),
            )?;
            let correct = if case.get("expected").is_some() {
                let expected = expected_value(&case["expected"])?;
                actual.as_ref().is_ok_and(|value| value == &expected)
                    && written.as_ref().is_some_and(|result| {
                        result
                            .as_ref()
                            .is_ok_and(|text| Some(text.as_str()) == case["output"].as_str())
                    })
                    && roundtrip
                        .as_ref()
                        .is_some_and(|result| result.as_ref().is_ok_and(|value| value == &expected))
            } else {
                actual
                    .as_ref()
                    .err()
                    .is_some_and(|actual| error(actual) == case["error"])
                    && roundtrip.as_ref().is_some_and(|result| {
                        result
                            .as_ref()
                            .err()
                            .is_some_and(|actual| error(actual) == case["error"])
                    })
            };
            if !correct {
                failures.push(format!("{id}-{index}: complete value/output/error differs"));
            }
        }
    }
    for refusal in refusals {
        let id = refusal["id"].as_str().ok_or("refusal identity")?;
        let actual = super::super::import_str(&serde_json::to_string(&refusal["schema"])?);
        std::fs::write(
            retained.join(format!("{id}-REFUSAL-ORIGINAL.txt")),
            format!("{actual:#?}\nexpected={refusal:#?}\n"),
        )?;
        if !actual
            .as_ref()
            .err()
            .is_some_and(|actual| error(actual) == refusal["error"])
        {
            failures.push(format!("{id}: complete import refusal differs"));
        }
    }
    std::fs::write(
        retained.join("COMPLETE-COMPARISONS.txt"),
        format!("{calls}\n{failures:#?}\n"),
    )?;
    if fixtures.len() != 18 || refusals.len() != 19 || calls != 57 || !failures.is_empty() {
        return Err(format!("complete scalar type-sibling contract differs: {failures:?}").into());
    }
    Ok(())
}
