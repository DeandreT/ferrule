use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use format_json::{JsonFormatError, from_str, json_schema, to_string};
use ir::{
    FiniteF64, Instance, NumberBound, NumberRange, NumericRange, ScalarType, SchemaNode, Value,
};
use serde_json::Value as Json;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
const CASES: &str = include_str!("fixtures/signed_integer_float_bounds.json");

fn evidence() -> TestResult<PathBuf> {
    let parent = std::env::var_os("FERRULE_JSON_BOUNDS_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if !parent.is_absolute() {
        return Err("evidence parent must be absolute".into());
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = parent.join(format!(
        "ferrule-json-bounds-native-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path)?;
    std::fs::write(path.join("cases.json"), CASES)?;
    eprintln!("JSON_BOUNDS_ORIGINALS={}", path.display());
    Ok(path)
}

fn float(bits: &str) -> TestResult<f64> {
    Ok(f64::from_bits(u64::from_str_radix(bits, 16)?))
}

fn expected_bound(value: &Json) -> TestResult<Option<NumberBound>> {
    if value.is_null() {
        return Ok(None);
    }
    let number = FiniteF64::new(float(value["bits"].as_str().ok_or("bound bits required")?)?)
        .ok_or("finite authored endpoint required")?;
    Ok(Some(if value["exclusive"] == true {
        NumberBound::exclusive(number)
    } else {
        NumberBound::inclusive(number)
    }))
}

fn expected_schema(row: &Json) -> TestResult<SchemaNode> {
    let range = NumberRange::new(
        expected_bound(&row["minimum"])?,
        expected_bound(&row["maximum"])?,
    )
    .ok_or("authored range must contain a Float")?;
    Ok(SchemaNode::scalar("Boundary", ScalarType::Float)
        .with_numeric_range(NumericRange::Number(range))
        .ok_or("authored Float range must attach")?)
}

fn range_error_matches(error: &JsonFormatError, row: &Json, got: &str) -> bool {
    matches!(error, JsonFormatError::RangeMismatch { name, range, got: actual }
        if name == "Boundary" && range == row["range_text"].as_str().unwrap() && actual == got)
}

#[test]
fn signed_integer_float_bounds_preserve_exact_domain_and_round_trip() -> TestResult<()> {
    let directory = evidence()?;
    let corpus: Json = serde_json::from_str(CASES)?;
    let rows = corpus["boundary_cases"]
        .as_array()
        .ok_or("schema cases required")?;
    let mut originals = String::new();
    let mut mismatches = Vec::new();
    let mut probes = 0;
    for row in rows {
        let id = row["id"].as_str().ok_or("case identity required")?;
        let schema_text = row["schema"].as_str().ok_or("schema required")?;
        let imported = json_schema::import_str(schema_text);
        originals.push_str(&format!(
            "\n{id}\nINPUT={schema_text}\nIMPORT={imported:#?}\nEXPECTED={row:#?}\n"
        ));
        if let Some(reason) = row["error"].as_str() {
            if !matches!(&imported, Err(JsonFormatError::UnsupportedSchemaUnion { name, reason: actual }) if name == "Boundary" && actual == reason)
            {
                mismatches.push(format!("{id}: complete import refusal differs"));
            }
            continue;
        }
        let wanted = expected_schema(row)?;
        let schema = match imported {
            Ok(schema) => schema,
            Err(_) => {
                mismatches.push(format!("{id}: supported authored endpoint refused"));
                continue;
            }
        };
        if schema != wanted {
            mismatches.push(format!("{id}: complete schema differs"));
        }
        let exported = json_schema::export(&schema);
        let reimported = exported
            .as_ref()
            .ok()
            .map(|text| json_schema::import_str(text));
        originals.push_str(&format!("EXPORT={exported:#?}\nREIMPORT={reimported:#?}\n"));
        if exported
            .as_ref()
            .ok()
            .and_then(|text| serde_json::from_str::<Json>(text).ok())
            != Some(row["exported"].clone())
            || !matches!(reimported, Some(Ok(ref actual)) if actual == &wanted)
        {
            mismatches.push(format!("{id}: normalized export/reimport differs"));
        }
        for probe in row["probes"].as_array().ok_or("probe cases required")? {
            probes += 1;
            let input = probe["input"].as_str().ok_or("probe input required")?;
            let number = float(probe["bits"].as_str().ok_or("probe bits required")?)?;
            let instance = Instance::Scalar(Value::Float(number));
            let parsed = from_str(input, &schema);
            let serialized = to_string(&schema, &instance);
            originals.push_str(&format!("PROBE={probe:#?}\nTYPED={instance:#?}\nPARSED={parsed:#?}\nSERIALIZED={serialized:#?}\n"));
            if probe["accepted"] == true {
                let parsed_bits = matches!(&parsed, Ok(Instance::Scalar(Value::Float(value))) if value.to_bits() == number.to_bits());
                let serialized_bits = serialized
                    .as_ref()
                    .ok()
                    .and_then(|text| serde_json::from_str::<Json>(text).ok())
                    .and_then(|value| value.as_f64())
                    .is_some_and(|value| value.to_bits() == number.to_bits());
                if !parsed_bits || !serialized_bits {
                    mismatches.push(format!("{id}: complete accepted Float value differs"));
                }
            } else {
                // Standard JSON numeric spelling is separate from the authored
                // exact value/inequality oracle; every public error field is checked.
                let input_got = serde_json::from_str::<Json>(input)?.to_string();
                let output_got = serde_json::Number::from_f64(number)
                    .ok_or("finite probe")?
                    .to_string();
                if !matches!(&parsed, Err(error) if range_error_matches(error, row, &input_got))
                    || !matches!(&serialized, Err(error) if range_error_matches(error, row, &output_got))
                {
                    mismatches.push(format!("{id}: complete range refusal differs"));
                }
            }
        }
    }
    std::fs::write(directory.join("COMPLETE-NATIVE-OUTCOMES.txt"), originals)?;
    std::fs::write(
        directory.join("COMPLETE-COMPARISONS.txt"),
        format!("schemas={} probes={probes}\n{mismatches:#?}\n", rows.len()),
    )?;
    if rows.len() != 100 || probes != 264 || !mismatches.is_empty() {
        return Err(format!("authored numeric boundary cohort differs: {mismatches:#?}").into());
    }
    Ok(())
}
