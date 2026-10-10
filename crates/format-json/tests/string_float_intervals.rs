use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use format_json::{JsonFormatError, from_str, json_schema, to_string};
use ir::{
    FiniteF64, Instance, NumberBound, NumberRange, NumericRange, ScalarType, ScalarTypeSet,
    SchemaNode, Value,
};
use serde_json::Value as Json;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
const CASES: &str = include_str!("fixtures/string_float_intervals.json");

struct Evidence {
    directory: PathBuf,
    originals: std::fs::File,
}
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_STRING_FLOAT_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = parent.join(format!(
            "ferrule-string-float-native-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join("cases.json"), CASES)?;
        eprintln!("STRING_FLOAT_ORIGINALS={}", directory.display());
        let originals = std::fs::File::create(directory.join("COMPLETE-NATIVE-ORIGINALS.txt"))?;
        Ok(Self {
            directory,
            originals,
        })
    }
    fn record(&mut self, label: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        writeln!(self.originals, "\n{label}\n{value:#?}")?;
        self.originals.flush()?;
        Ok(())
    }
}
fn typed(value: &Json) -> TestResult<Instance> {
    if let Some(items) = value.as_array() {
        return Ok(Instance::Repeated(
            items.iter().map(typed).collect::<TestResult<_>>()?,
        ));
    }
    Ok(Instance::Scalar(
        match value["kind"].as_str().ok_or("tag required")? {
            "float" => Value::Float(value["value"].as_f64().ok_or("Float required")?),
            "int" => Value::Int(value["value"].as_i64().ok_or("Int required")?),
            "string" => Value::String(value["value"].as_str().ok_or("String required")?.to_owned()),
            "bool" => Value::Bool(value["value"].as_bool().ok_or("Bool required")?),
            "json_null" => Value::json_null(),
            other => return Err(format!("unsupported authored tag {other}").into()),
        },
    ))
}
fn expected_schema(row: &Json) -> TestResult<SchemaNode> {
    let number = |key: &str| -> TestResult<FiniteF64> {
        let bits = u64::from_str_radix(row[key].as_str().ok_or("endpoint bits required")?, 16)?;
        Ok(FiniteF64::new(f64::from_bits(bits)).ok_or("finite endpoint")?)
    };
    let minimum = number("minimum_bits")?;
    let maximum = number("maximum_bits")?;
    let minimum = if row["minimum_exclusive"] == true {
        NumberBound::exclusive(minimum)
    } else {
        NumberBound::inclusive(minimum)
    };
    let maximum = if row["maximum_exclusive"] == true {
        NumberBound::exclusive(maximum)
    } else {
        NumberBound::inclusive(maximum)
    };
    let range = NumberRange::new(Some(minimum), Some(maximum)).ok_or("authored interval")?;
    let types =
        ScalarTypeSet::new([ScalarType::String, ScalarType::Float]).ok_or("exact domain")?;
    let mut node = SchemaNode::scalar_union("Boundary", types)
        .with_numeric_range(NumericRange::Number(range))
        .ok_or("authored interval attaches")?;
    node.nullable = row["nullable"] == true;
    if let Some(formats) = row["formats"].as_array() {
        node.json_formats = ir::JsonFormatAnnotations::new(
            formats
                .iter()
                .map(|format| format.as_str().unwrap().to_owned()),
        )?;
    }
    Ok(node)
}
fn range_matches(error: &JsonFormatError, name: &str, range: &str, got: &str) -> bool {
    matches!(error, JsonFormatError::RangeMismatch { name: actual_name, range: actual_range, got: actual_got } if actual_name == name && actual_range == range && actual_got == got)
}
#[test]
fn string_float_interval_complete_native_and_round_trip_oracles() -> TestResult<()> {
    let mut evidence = Evidence::new()?;
    let corpus: Json = serde_json::from_str(CASES)?;
    let mut mismatches = Vec::new();
    for row in corpus["schema_cases"].as_array().ok_or("schema rows")? {
        let id = row["id"].as_str().ok_or("id")?;
        let original = json_schema::import_str(&row["schema"].to_string());
        evidence.record(&format!("{id}-IMPORT"), &(&row, &original))?;
        let expected = expected_schema(row);
        evidence.record(&format!("{id}-EXPECTED-MODEL"), &expected)?;
        let expected = expected?;
        let exported = original.as_ref().ok().map(json_schema::export);
        let reimported = exported
            .as_ref()
            .and_then(|v| v.as_ref().ok())
            .map(|text| json_schema::import_str(text));
        evidence.record(id, &(&row, &original, &expected, &exported, &reimported))?;
        if !matches!(&original, Ok(value) if value == &expected)
            || !matches!(&reimported, Some(Ok(value)) if value == &expected)
            || exported
                .as_ref()
                .and_then(|v| v.as_ref().ok())
                .and_then(|text| serde_json::from_str::<Json>(text).ok())
                != Some(row["exported"].clone())
        {
            mismatches.push(format!("{id}: complete import/export/reimport differs"));
        }
        if let Some(probes) = row["probes"].as_array() {
            for probe in probes {
                let parsed = from_str(probe["input"].as_str().ok_or("input")?, &expected);
                evidence.record("CROSS-FEATURE-BOUND", &(&probe, &parsed))?;
                if probe["accepted"] == true {
                    let bits = u64::from_str_radix(probe["bits"].as_str().ok_or("bits")?, 16)?;
                    if !matches!(parsed, Ok(Instance::Scalar(Value::Float(value))) if value.to_bits() == bits)
                    {
                        mismatches.push(format!("{id}: cross-feature exact Float differs"));
                    }
                } else if !matches!(parsed, Err(ref error) if range_matches(error, "Boundary", probe["range_text"].as_str().unwrap(), probe["got"].as_str().unwrap()))
                {
                    mismatches.push(format!("{id}: cross-feature range error differs"));
                }
            }
        }
        if row["formats"].is_array() {
            let parsed = from_str("\"AUTHORED\"", &expected);
            evidence.record("NONASSERTING-FORMAT", &parsed)?;
            if !matches!(parsed, Ok(Instance::Scalar(Value::String(ref value))) if value == "AUTHORED")
            {
                mismatches.push(format!("{id}: nonasserting format altered String"));
            }
        }
        if row["nullable"] == true {
            let parsed = from_str("null", &expected);
            let output = to_string(&expected, &Instance::Scalar(Value::json_null()));
            evidence.record(&format!("{id}-NULL"), &(&parsed, &output))?;
            if !matches!(parsed, Ok(Instance::Scalar(Value::JsonNull(_))))
                || !matches!(output, Ok(ref text) if text == "null\n")
            {
                mismatches.push(format!("{id}: explicit JSON Null differs"));
            }
        }
    }
    for row in corpus["schema_refusals"].as_array().ok_or("refusal rows")? {
        let original = json_schema::import_str(&row["schema"].to_string());
        evidence.record("IMPORT-REFUSAL", &(&row, &original))?;
        if !matches!(original, Err(JsonFormatError::UnsupportedSchemaUnion { ref name, ref reason }) if name == "Boundary" && reason == row["error"].as_str().unwrap())
        {
            mismatches.push(format!("{}: complete schema refusal differs", row["id"]));
        }
    }
    for row in corpus["malformed_ir_cases"]
        .as_array()
        .ok_or("malformed rows")?
    {
        let original = serde_json::from_value::<SchemaNode>(row["schema"].clone());
        evidence.record("MALFORMED-IR", &(&row, &original))?;
        if !matches!(original, Err(ref error) if error.classify() == serde_json::error::Category::Data)
        {
            mismatches.push(format!("{}: typed metadata refusal differs", row["id"]));
        }
    }
    let schema = expected_schema(&corpus["schema_cases"][0])?;
    for row in corpus["probes"].as_array().ok_or("probes")? {
        let id = row["id"].as_str().ok_or("id")?;
        let parsed = from_str(row["input"].as_str().ok_or("input")?, &schema);
        let wanted = if matches!(row["kind"].as_str(), Some("float" | "int" | "string")) {
            Some(typed(row)?)
        } else {
            None
        };
        let output = wanted.as_ref().map(|value| to_string(&schema, value));
        evidence.record(id, &(&row, &wanted, &parsed, &output))?;
        if row["accepted"] == true {
            if !matches!(&parsed, Ok(actual) if Some(actual) == wanted.as_ref())
                || !matches!(&output, Some(Ok(text)) if Some(text.as_str()) == row["serialized"].as_str())
            {
                mismatches.push(format!("{id}: complete typed/serialized value differs"));
            }
        } else if row["error"] == "range" {
            let input_got =
                serde_json::from_str::<Json>(row["input"].as_str().unwrap())?.to_string();
            let output_got = row["value"].to_string();
            if !matches!(parsed, Err(ref error) if range_matches(error, "Boundary", corpus["range_text"].as_str().unwrap(), &input_got))
                || !matches!(output, Some(Err(ref error)) if range_matches(error, "Boundary", corpus["range_text"].as_str().unwrap(), &output_got))
            {
                mismatches.push(format!("{id}: complete numeric error differs"));
            }
        } else {
            let expected = if row["error"] == "precision" {
                "number"
            } else {
                "declared scalar union"
            };
            if !matches!(parsed, Err(JsonFormatError::Shape { ref name, expected: actual_expected, got }) if name == "Boundary" && actual_expected == expected && got == row["got"].as_str().unwrap())
            {
                mismatches.push(format!("{id}: complete shape/precision error differs"));
            }
        }
    }
    let integer = Instance::Scalar(Value::Int(2));
    let integer_output = to_string(&schema, &integer);
    let integer_readback = integer_output
        .as_ref()
        .ok()
        .map(|text| from_str(text, &schema));
    let expected_readback = Instance::Scalar(Value::Float(2.0));
    evidence.record(
        "INT-TO-FLOAT-OUTPUT",
        &(
            &integer,
            &integer_output,
            &expected_readback,
            &integer_readback,
        ),
    )?;
    if !matches!(integer_output, Ok(ref text) if text == "2\n")
        || !matches!(integer_readback, Some(Ok(ref actual)) if actual == &expected_readback)
    {
        mismatches.push("exact typed Int output wire and Float readback differ".into());
    }
    for row in corpus["direct_cases"].as_array().ok_or("direct rows")? {
        let imported = json_schema::import_str(&row["schema"].to_string());
        evidence.record("DIRECT-IMPORT", &(&row, &imported))?;
        let expected_schema = serde_json::from_value::<SchemaNode>(row["ir_schema"].clone());
        evidence.record("DIRECT-EXPECTED-MODEL", &expected_schema)?;
        let expected_schema = expected_schema?;
        let wanted = typed(&row["expected"])?;
        let parsed = from_str(row["input"].as_str().ok_or("input")?, &expected_schema);
        let output = to_string(&expected_schema, &wanted);
        let exported = json_schema::export(&expected_schema);
        let reimported = exported
            .as_ref()
            .ok()
            .map(|text| json_schema::import_str(text));
        evidence.record(
            "DIRECT-BOUNDARY",
            &(
                &row,
                &expected_schema,
                &imported,
                &wanted,
                &parsed,
                &output,
                &exported,
                &reimported,
            ),
        )?;
        if !matches!(imported, Ok(ref actual) if actual == &expected_schema)
            || !matches!(reimported, Some(Ok(ref actual)) if actual == &expected_schema)
            || exported
                .as_ref()
                .ok()
                .and_then(|text| serde_json::from_str::<Json>(text).ok())
                != Some(row["exported"].clone())
        {
            mismatches.push(format!(
                "{}: direct complete schema round-trip differs",
                row["id"]
            ));
        }
        if row["accepted"] == true {
            if !matches!(parsed, Ok(ref actual) if actual == &wanted)
                || !matches!(output, Ok(ref text) if Some(text.as_str()) == row["serialized"].as_str())
            {
                mismatches.push(format!(
                    "{}: direct complete value/bytes differs",
                    row["id"]
                ));
            }
        } else if !matches!(parsed, Err(ref error) if range_matches(error, row["error_name"].as_str().unwrap(), row["range_text"].as_str().unwrap(), row["got"].as_str().unwrap()))
            || !matches!(output, Err(ref error) if range_matches(error, row["error_name"].as_str().unwrap(), row["range_text"].as_str().unwrap(), row["got"].as_str().unwrap()))
        {
            mismatches.push(format!("{}: direct complete refusal differs", row["id"]));
        }
    }
    std::fs::write(
        evidence.directory.join("COMPLETE-COMPARISONS.txt"),
        format!("{mismatches:#?}\n"),
    )?;
    if !mismatches.is_empty() {
        return Err(format!("String-or-Float oracle differs: {mismatches:#?}").into());
    }
    Ok(())
}
