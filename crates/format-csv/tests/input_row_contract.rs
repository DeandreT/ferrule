//! Independent row oracles for the native flat CSV input contract.
use format_csv::{CsvFormatError, CsvReadOptions, from_str_with_options};
use ir::{Instance, ScalarType, SchemaNode, Value};
use serde_json::{Value as Json, json};
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

const ORACLES: &str = include_str!("fixtures/input_row_contract.json");

fn scalar(value: &Json) -> Result<Value, Box<dyn Error>> {
    Ok(match value["kind"].as_str().ok_or("authored scalar tag")? {
        "null" => Value::Null,
        "string" => Value::String(value["value"].as_str().ok_or("string literal")?.into()),
        "int" => Value::Int(value["value"].as_i64().ok_or("integer literal")?),
        "bool" => Value::Bool(value["value"].as_bool().ok_or("boolean literal")?),
        "float" => Value::Float(f64::from_bits(u64::from_str_radix(
            value["bits"].as_str().ok_or("IEEE bits literal")?,
            16,
        )?)),
        _ => return Err("unsupported authored scalar tag".into()),
    })
}

fn tagged(value: &Instance) -> Json {
    match value {
        Instance::Group(fields) => json!({
            "kind":"group", "fields": fields.iter().map(|(name,value)|
                json!({"name":name,"value":tagged(value)})).collect::<Vec<_>>()
        }),
        Instance::Scalar(value) => match value {
            Value::Null => json!({"kind":"null"}),
            Value::String(value) => json!({"kind":"string","value":value}),
            Value::Int(value) => json!({"kind":"int","value":value}),
            Value::Float(value) => {
                json!({"kind":"float","bits":format!("{:016x}",value.to_bits())})
            }
            Value::Bool(value) => json!({"kind":"bool","value":value}),
            other => json!({"unqualified_scalar":format!("{other:#?}")}),
        },
        other => json!({"unqualified_instance":format!("{other:#?}")}),
    }
}

fn error_fields(error: &CsvFormatError) -> Json {
    match error {
        CsvFormatError::ColumnCount { row, expected, got } => json!({
            "category":"ColumnCount","row":row,"expected":expected,"got":got,"display":error.to_string()
        }),
        CsvFormatError::Parse {
            row,
            field,
            expected,
            value,
        } => json!({
            "category":"Parse","row":row,"field":field,"expected":format!("{expected:?}"),
            "value":value,"display":error.to_string()
        }),
        other => json!({"unqualified_error":format!("{other:#?}"),"display":error.to_string()}),
    }
}

fn character(value: &Json) -> Result<Option<char>, Box<dyn Error>> {
    if value.is_null() {
        return Ok(None);
    }
    let text = value.as_str().ok_or("character literal")?;
    let mut characters = text.chars();
    let first = characters.next().ok_or("empty character literal")?;
    if characters.next().is_some() {
        return Err("multiple character literal".into());
    }
    Ok(Some(first))
}

#[test]
fn complete_native_rows_match_independent_tags_bits_dialects_and_errors()
-> Result<(), Box<dyn Error>> {
    let document: Json = serde_json::from_str(ORACLES)?;
    let fields = document["schema"]["fields"]
        .as_array()
        .ok_or("authored fields")?;
    let schema = SchemaNode::group(
        "Row",
        fields
            .iter()
            .map(|field| {
                let ty = match field["type"].as_str() {
                    Some("String") => ScalarType::String,
                    Some("Int") => ScalarType::Int,
                    Some("Float") => ScalarType::Float,
                    Some("Bool") => ScalarType::Bool,
                    _ => panic!("unqualified authored field type"),
                };
                SchemaNode::scalar(field["name"].as_str().unwrap(), ty)
            })
            .collect(),
    );
    let cases = document["cases"].as_array().ok_or("authored cases")?;
    let root = std::env::temp_dir().join(format!(
        "ferrule-csv-input275-native-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir_all(&root)?;
    std::fs::write(root.join("ORACLES-ORIGINAL.json"), ORACLES)?;
    std::fs::write(root.join("SCHEMA-ORIGINAL.txt"), format!("{schema:#?}\n"))?;
    eprintln!("CSV_INPUT275_NATIVE_ORIGINALS={}", root.display());
    let mut comparisons = Vec::new();
    for case in cases {
        let id = case["id"].as_str().ok_or("authored case id")?;
        let options = &case["options"];
        let options = CsvReadOptions {
            delimiter: character(&options["delimiter"])?,
            quote: character(&options["quote"])?,
            quote_disabled: options["quote_disabled"].as_bool().ok_or("quote option")?,
            has_headers: options["has_headers"].as_bool().ok_or("header option")?,
            preserve_empty_strings: options["preserve_empty_strings"]
                .as_bool()
                .ok_or("presence option")?,
            repair_dependency: None,
        };
        let input = case["input"].as_str().ok_or("authored input")?;
        let actual = from_str_with_options(input, &schema, &options);
        std::fs::write(
            root.join(format!("{id}-ACTUAL-ORIGINAL.txt")),
            format!("{actual:#?}\n"),
        )?;
        let expected = &case["expected"];
        let matched = if let Some(rows) = expected["rows"].as_array() {
            let expected_rows = rows
                .iter()
                .map(|row| {
                    let cells = row.as_array().ok_or("authored row")?;
                    if cells.len() != fields.len() {
                        return Err("authored row width".into());
                    }
                    Ok(Instance::Group(
                        fields
                            .iter()
                            .zip(cells)
                            .map(|(field, cell)| {
                                Ok((
                                    field["name"].as_str().unwrap().into(),
                                    Instance::Scalar(scalar(cell)?),
                                ))
                            })
                            .collect::<Result<Vec<_>, Box<dyn Error>>>()?
                            .into(),
                    ))
                })
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
            let actual_tagged = actual
                .as_ref()
                .ok()
                .map(|rows| rows.iter().map(tagged).collect::<Vec<_>>());
            let expected_tagged = expected_rows.iter().map(tagged).collect::<Vec<_>>();
            std::fs::write(
                root.join(format!("{id}-COMPLETE-VALUES.json")),
                serde_json::to_vec_pretty(&json!({
                    "expected":expected_tagged,"actual":actual_tagged,"expected_original":format!("{expected_rows:#?}")
                }))?,
            )?;
            actual.as_ref().ok() == Some(&expected_rows)
                && actual_tagged.as_ref() == Some(&expected_tagged)
        } else {
            let actual_fields = actual.as_ref().err().map(error_fields);
            std::fs::write(
                root.join(format!("{id}-COMPLETE-ERROR.json")),
                serde_json::to_vec_pretty(&json!({
                    "expected":expected["error"],"actual":actual_fields
                }))?,
            )?;
            actual_fields.as_ref() == Some(&expected["error"])
        };
        comparisons.push(json!({"id":id,"matched":matched}));
    }
    std::fs::write(
        root.join("ALL-COMPLETE-COMPARISONS.json"),
        serde_json::to_vec_pretty(&comparisons)?,
    )?;
    if cases.len() != 30 || comparisons.iter().any(|case| case["matched"] != true) {
        return Err("complete native CSV row contract differs from independent literals".into());
    }
    Ok(())
}
