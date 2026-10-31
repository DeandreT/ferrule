use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use format_json::{ContentlessSchemaReason, JsonFormatError, from_str, json_schema, to_string};
use ir::{Instance, SchemaNode, Value};
use serde_json::Value as Json;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
const NATIVE: &str = include_str!("fixtures/lexical_numeric_bounds/native-cases.json");
const RESOURCES: &str = include_str!("fixtures/lexical_numeric_bounds/resource-cases.json");
const PARSER: &str = include_str!("fixtures/lexical_numeric_bounds/parser-refusal-cases.json");

/// Evidence under `FERRULE_LEXICAL_BOUNDS_EVIDENCE_DIR` is retained; otherwise it is removed on drop,
/// including after a panic.
struct Evidence {
    directory: PathBuf,
    file: std::fs::File,
}
impl Drop for Evidence {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_LEXICAL_BOUNDS_EVIDENCE_DIR").is_none() {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_LEXICAL_BOUNDS_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = parent.join(format!(
            "ferrule-lexical-bounds-native-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory)?;
        for (name, text) in [
            ("native-cases.json", NATIVE),
            ("resource-cases.json", RESOURCES),
            ("parser-refusal-cases.json", PARSER),
        ] {
            std::fs::write(directory.join(name), text)?;
        }
        eprintln!("LEXICAL_BOUNDS_ORIGINALS={}", directory.display());
        let file = std::fs::File::create(directory.join("COMPLETE-NATIVE-ORIGINALS.txt"))?;
        Ok(Self { directory, file })
    }
    fn record(&mut self, label: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
        writeln!(self.file, "\n{label}\n{original:#?}")?;
        self.file.flush()?;
        Ok(())
    }
}

fn typed(value: &Json) -> TestResult<Instance> {
    let scalar = match value["tag"].as_str().ok_or("scalar tag")? {
        "Int" => Value::Int(value["value"].as_i64().ok_or("signed integer")?),
        "Float" => {
            let bits = u64::from_str_radix(value["bits"].as_str().ok_or("Float bits")?, 16)?;
            Value::Float(f64::from_bits(bits))
        }
        _ => return Err("unselected scalar tag".into()),
    };
    Ok(Instance::Scalar(scalar))
}

fn error_matches(actual: &JsonFormatError, expected: &Json) -> bool {
    let fields = match actual {
        JsonFormatError::UnsupportedSchemaUnion { name, reason } => {
            expected["variant"] == "UnsupportedSchemaUnion"
                && expected["name"] == name.as_str()
                && expected["reason"] == reason.as_str()
        }
        JsonFormatError::RangeMismatch { name, range, got } => {
            expected["variant"] == "RangeMismatch"
                && expected["name"] == name.as_str()
                && expected["range"] == range.as_str()
                && expected["got"] == got.as_str()
        }
        _ => false,
    };
    fields && expected["display"] == actual.to_string()
}

fn typed_equal(actual: &Instance, expected: &Instance) -> bool {
    match (actual, expected) {
        (Instance::Scalar(Value::Float(actual)), Instance::Scalar(Value::Float(expected))) => {
            actual.to_bits() == expected.to_bits()
        }
        _ => actual == expected,
    }
}

fn full_schema(
    evidence: &mut Evidence,
    row: &Json,
    original: &Result<SchemaNode, JsonFormatError>,
    mismatches: &mut Vec<String>,
) -> TestResult<()> {
    let id = row["id"].as_str().ok_or("case id")?;
    evidence.record(&format!("{id}-IMPORT"), &(&row, original))?;
    if !row["expected_error"].is_null() {
        if !matches!(original, Err(error) if error_matches(error, &row["expected_error"])) {
            mismatches.push(format!("{id}: complete import refusal differs"));
        }
        return Ok(());
    }
    let expected = serde_json::from_value::<SchemaNode>(row["expected_schema"].clone());
    evidence.record(&format!("{id}-EXPECTED"), &expected)?;
    let expected = expected?;
    let exported = original.as_ref().ok().map(json_schema::export);
    let reimported = exported
        .as_ref()
        .and_then(|value| value.as_ref().ok())
        .map(|text| json_schema::import_str(text));
    evidence.record(&format!("{id}-EXPORT-REIMPORT"), &(&exported, &reimported))?;
    if !matches!(original, Ok(actual) if actual == &expected)
        || !matches!(&reimported, Some(Ok(actual)) if actual == &expected)
        || exported
            .as_ref()
            .and_then(|value| value.as_ref().ok())
            .and_then(|text| serde_json::from_str::<Json>(text).ok())
            != Some(row["expected_export"].clone())
    {
        mismatches.push(format!("{id}: complete model/export/reimport differs"));
    }
    if let Some(probes) = row["probes"].as_array() {
        for (index, probe) in probes.iter().enumerate() {
            let input = from_str(probe["input"].as_str().ok_or("probe input")?, &expected);
            evidence.record(&format!("{id}-{index}-INPUT"), &(&probe, &input))?;
            let wanted = typed(if probe["typed_output"].is_null() {
                &probe["expected_value"]
            } else {
                &probe["typed_output"]
            });
            evidence.record(&format!("{id}-{index}-EXPECTED-TYPED"), &wanted)?;
            let wanted = wanted?;
            let output = to_string(&expected, &wanted);
            evidence.record(&format!("{id}-{index}-OUTPUT"), &output)?;
            if probe["input_error"].is_null() {
                if !matches!(&input, Ok(actual) if typed_equal(actual, &wanted))
                    || !matches!(&output, Ok(actual) if probe["serialized_json"] == actual.as_str())
                {
                    mismatches.push(format!(
                        "{id}-{index}: complete value/serialization differs"
                    ));
                }
            } else if !matches!(&input, Err(error) if error_matches(error, &probe["input_error"]))
                || !matches!(&output, Err(error) if error_matches(error, &probe["output_error"]))
            {
                mismatches.push(format!(
                    "{id}-{index}: complete input/output range error differs"
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn lexical_numeric_bounds_complete_native_oracles() -> TestResult<()> {
    let mut evidence = Evidence::new()?;
    let rows: Vec<Json> = serde_json::from_str(NATIVE)?;
    let mut mismatches = Vec::new();
    for row in &rows {
        let original = json_schema::import_str(row["schema_text"].as_str().ok_or("raw schema")?);
        full_schema(&mut evidence, row, &original, &mut mismatches)?;
    }
    for row in serde_json::from_str::<Vec<Json>>(PARSER)? {
        let original = json_schema::import_str(row["raw_schema"].as_str().ok_or("raw schema")?);
        evidence.record("PARSER", &(&row, &original))?;
        if row["expected_error_variant"] == "Json" {
            if !matches!(original, Err(JsonFormatError::Json(ref error)) if row["expected_error_category"].as_str().is_none_or(|category| error.to_string().contains(category)))
            {
                mismatches.push(format!(
                    "{}: ordinary JSON parser refusal differs",
                    row["id"]
                ));
            }
        } else {
            full_schema(&mut evidence, &row, &original, &mut mismatches)?;
        }
    }
    evidence.record("ALL-COMPARISONS", &(&rows.len(), &mismatches))?;
    if !mismatches.is_empty() {
        return Err("complete lexical native oracle differs".into());
    }
    Ok(())
}

#[test]
fn lexical_numeric_bounds_confined_resources_preserve_refusals() -> TestResult<()> {
    let mut evidence = Evidence::new()?;
    let package = evidence.directory.join("package/schema");
    std::fs::create_dir_all(&package)?;
    for (name, text) in [
        (
            "root.json",
            include_str!("fixtures/lexical_numeric_bounds/resources/root.json"),
        ),
        (
            "levels/value.json",
            include_str!("fixtures/lexical_numeric_bounds/resources/levels/value.json"),
        ),
        (
            "local.json",
            include_str!("fixtures/lexical_numeric_bounds/resources/local.json"),
        ),
        (
            "anchor-root.json",
            include_str!("fixtures/lexical_numeric_bounds/resources/anchor-root.json"),
        ),
        (
            "anchor-value.json",
            include_str!("fixtures/lexical_numeric_bounds/resources/anchor-value.json"),
        ),
        (
            "cycle.json",
            include_str!("fixtures/lexical_numeric_bounds/resources/cycle.json"),
        ),
        (
            "outside.json",
            include_str!("fixtures/lexical_numeric_bounds/resources/outside.json"),
        ),
    ] {
        let path = package.join(name);
        std::fs::create_dir_all(path.parent().ok_or("parent")?)?;
        std::fs::write(path, text)?;
    }
    std::fs::write(
        evidence.directory.join("outside.json"),
        "{\"type\":\"integer\",\"minimum\":1.0}",
    )?;
    let mut mismatches = Vec::new();
    for row in serde_json::from_str::<Vec<Json>>(RESOURCES)? {
        if row["constructed_object_depth"].is_number() {
            let text = format!("{}0{}", "{\"default\":".repeat(129), "}".repeat(129));
            let original = json_schema::import_str(&text);
            evidence.record("DEPTH", &(&text, &original))?;
            if !matches!(
                original,
                Err(JsonFormatError::SchemaResourceLimit {
                    kind: "JSON nesting depth",
                    limit: 128
                })
            ) && !matches!(original, Err(JsonFormatError::Json(ref error)) if error.to_string().contains("recursion limit"))
            {
                mismatches.push("existing depth refusal differs".into());
            }
            continue;
        }
        let original = if let Some(root) = row["root"].as_str() {
            json_schema::import_with_root(&package.join(root), &package)
        } else {
            json_schema::import_str(row["schema_text"].as_str().ok_or("inline schema")?)
        };
        evidence.record("RESOURCE", &(&row, &original))?;
        if row["expected_error_variant"] == "SchemaResource" {
            if !matches!(&original, Err(JsonFormatError::SchemaResource { reason, .. }) if row["expected_reason"].is_null() || row["expected_reason"] == reason.as_str())
            {
                mismatches.push(format!("{}: existing resource refusal differs", row["id"]));
            }
        } else if row["expected_error_variant"] == "ContentlessSchema" {
            if !matches!(
                &original,
                Err(JsonFormatError::ContentlessSchema {
                    reason: ContentlessSchemaReason::ReferenceCycle,
                    ..
                })
            ) {
                mismatches.push(format!("{}: contentless root refusal differs", row["id"]));
            }
        } else {
            full_schema(&mut evidence, &row, &original, &mut mismatches)?;
        }
    }
    evidence.record("ALL-RESOURCE-COMPARISONS", &mismatches)?;
    if !mismatches.is_empty() {
        return Err("complete lexical resource oracle differs".into());
    }
    Ok(())
}
