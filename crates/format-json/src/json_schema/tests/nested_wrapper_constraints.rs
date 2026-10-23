use std::{
    error::Error,
    fmt::{Debug, Display},
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use ir::{
    Instance, ItemCountRange, JsonPatternConstraints, ScalarType, SchemaNode, StringLengthRange,
    Value,
};

use super::import_str_result;
use crate::{JsonFormatError, from_str, to_string};

#[derive(Debug)]
struct Negative {
    id: &'static str,
    input: &'static str,
    typed: Instance,
    error: JsonFormatError,
    display: &'static str,
}

#[derive(Debug)]
struct Case {
    id: &'static str,
    schema: &'static str,
    model: SchemaNode,
    exported: serde_json::Value,
    input: &'static str,
    typed: Instance,
    wire: &'static str,
    negatives: Vec<Negative>,
}

fn rows(values: &[&[&str]]) -> Instance {
    Instance::Repeated(
        values
            .iter()
            .map(|row| {
                Instance::Group(
                    vec![(
                        "item".to_owned(),
                        Instance::Repeated(
                            row.iter()
                                .map(|value| Instance::Scalar(Value::String((*value).to_owned())))
                                .collect(),
                        ),
                    )]
                    .into(),
                )
            })
            .collect(),
    )
}

fn wrapper(name: &str, item: SchemaNode) -> Result<SchemaNode, Box<dyn Error>> {
    SchemaNode::group(name, vec![item])
        .repeating()
        .with_json_array_wrapper()
        .ok_or_else(|| "the independently authored wrapper model is invalid".into())
}

fn cases() -> Result<Vec<Case>, Box<dyn Error>> {
    let pattern_item = SchemaNode::scalar("item", ScalarType::String)
        .with_json_patterns(JsonPatternConstraints::new([vec!["^x$"]])?)
        .ok_or("the independently authored pattern model is invalid")?
        .repeating();
    let count_item = SchemaNode::scalar("item", ScalarType::String)
        .repeating()
        .with_item_count_range(ItemCountRange::new(0, Some(2)).ok_or("invalid inner count")?)
        .ok_or("the independently authored inner-count model is invalid")?;
    let count_model = wrapper("x", count_item)?
        .with_item_count_range(ItemCountRange::new(1, None).ok_or("invalid outer count")?)
        .ok_or("the independently authored outer-count model is invalid")?;
    let length_item = SchemaNode::scalar("item", ScalarType::String)
        .with_string_length_range(StringLengthRange::new(2, None).ok_or("invalid string length")?)
        .ok_or("the independently authored string-length model is invalid")?
        .repeating();
    let outer_unique = wrapper(
        "Rows",
        SchemaNode::scalar("item", ScalarType::String).repeating(),
    )?
    .with_json_unique_items()
    .ok_or("the independently authored outer-unique model is invalid")?;
    let inner_unique_item = SchemaNode::scalar("item", ScalarType::String)
        .repeating()
        .with_json_unique_items()
        .ok_or("the independently authored inner-unique model is invalid")?;

    Ok(vec![
        Case {
            id: "inner-pattern",
            schema: r#"{
              "type":"array",
              "items":{"type":"array","items":{"type":"string","pattern":"^x$"}}
            }"#,
            model: wrapper("root", pattern_item)?,
            exported: serde_json::json!({
                "title": "root", "type": "array",
                "items": {"type": "array", "items": {"type": "string", "pattern": "^x$"}}
            }),
            input: r#"[["x"],[]]"#,
            typed: rows(&[&["x"], &[]]),
            wire: "[\n  [\n    \"x\"\n  ],\n  []\n]\n",
            negatives: vec![Negative {
                id: "nonmatching-inner-string",
                input: r#"[["y"]]"#,
                typed: rows(&[&["y"]]),
                error: JsonFormatError::PatternMismatch {
                    name: "item".into(),
                },
                display: "`item` does not match its JSON Schema pattern constraints",
            }],
        },
        Case {
            id: "outer-minimum-inner-maximum",
            schema: r#"{
  "title":"x",
  "type":"array",
  "minItems":1,
  "items":{"type":"array","maxItems":2,"items":{"type":"string"}}
}"#,
            model: count_model,
            exported: serde_json::json!({
                "title": "x", "type": "array", "minItems": 1,
                "items": {"type": "array", "maxItems": 2, "items": {"type": "string"}}
            }),
            input: r#"[[],["a","b"]]"#,
            typed: rows(&[&[], &["a", "b"]]),
            wire: "[\n  [],\n  [\n    \"a\",\n    \"b\"\n  ]\n]\n",
            negatives: vec![
                Negative {
                    id: "outer-minimum",
                    input: "[]",
                    typed: rows(&[]),
                    error: JsonFormatError::ItemCountMismatch {
                        name: "x".into(),
                        range: "at least 1".into(),
                        got: 0,
                    },
                    display: "`x` requires at least 1 array items, got 0",
                },
                Negative {
                    id: "inner-maximum",
                    input: r#"[["a","b","c"]]"#,
                    typed: rows(&[&["a", "b", "c"]]),
                    error: JsonFormatError::ItemCountMismatch {
                        name: "item".into(),
                        range: "at most 2".into(),
                        got: 3,
                    },
                    display: "`item` requires at most 2 array items, got 3",
                },
            ],
        },
        Case {
            id: "inner-string-length",
            schema: r#"{
          "type":"array",
          "items":{"type":"array","items":{"type":"string","minLength":2}}
        }"#,
            model: wrapper("root", length_item)?,
            exported: serde_json::json!({
                "title": "root", "type": "array",
                "items": {"type": "array", "items": {"type": "string", "minLength": 2}}
            }),
            input: r#"[["ab"],["cd"]]"#,
            typed: rows(&[&["ab"], &["cd"]]),
            wire: "[\n  [\n    \"ab\"\n  ],\n  [\n    \"cd\"\n  ]\n]\n",
            negatives: vec![Negative {
                id: "short-inner-string",
                input: r#"[["a"]]"#,
                typed: rows(&[&["a"]]),
                error: JsonFormatError::StringLengthMismatch {
                    name: "item".into(),
                    range: "at least 2 Unicode scalar values".into(),
                    got: 1,
                },
                display: "`item` requires string length at least 2 Unicode scalar values, got 1 Unicode scalar values",
            }],
        },
        Case {
            id: "outer-unique",
            schema: r#"{
  "title":"Rows",
  "type":"array",
  "uniqueItems":true,
  "items":{"type":"array","items":{"type":"string"}}
}"#,
            model: outer_unique,
            exported: serde_json::json!({
                "title": "Rows", "type": "array", "uniqueItems": true,
                "items": {"type": "array", "items": {"type": "string"}}
            }),
            input: r#"[["x","x"]]"#,
            typed: rows(&[&["x", "x"]]),
            wire: "[\n  [\n    \"x\",\n    \"x\"\n  ]\n]\n",
            negatives: vec![Negative {
                id: "duplicate-outer-arrays",
                input: r#"[["x"],["x"]]"#,
                typed: rows(&[&["x"], &["x"]]),
                error: JsonFormatError::UniqueItemsMismatch {
                    name: "Rows".into(),
                    first_index: 1,
                    duplicate_index: 2,
                },
                display: "`Rows` requires unique array items, but indexes 1 and 2 are equal",
            }],
        },
        Case {
            id: "inner-unique",
            schema: r#"{
  "title":"Rows",
  "type":"array",
  "items":{"type":"array","uniqueItems":true,"items":{"type":"string"}}
}"#,
            model: wrapper("Rows", inner_unique_item)?,
            exported: serde_json::json!({
                "title": "Rows", "type": "array",
                "items": {"type": "array", "uniqueItems": true, "items": {"type": "string"}}
            }),
            input: r#"[["x"],["x"]]"#,
            typed: rows(&[&["x"], &["x"]]),
            wire: "[\n  [\n    \"x\"\n  ],\n  [\n    \"x\"\n  ]\n]\n",
            negatives: vec![Negative {
                id: "duplicate-inner-strings",
                input: r#"[["x","x"]]"#,
                typed: rows(&[&["x", "x"]]),
                error: JsonFormatError::UniqueItemsMismatch {
                    name: "item".into(),
                    first_index: 1,
                    duplicate_index: 2,
                },
                display: "`item` requires unique array items, but indexes 1 and 2 are equal",
            }],
        },
    ])
}

fn retain<T: Debug, E: Debug + Display>(
    directory: &Path,
    name: &str,
    value: &Result<T, E>,
) -> std::io::Result<()> {
    let display = value.as_ref().err().map(ToString::to_string);
    fs::write(
        directory.join(format!("{name}.txt")),
        format!("RESULT={value:#?}\nERROR_DISPLAY={display:#?}\n"),
    )
}

fn compare(failures: &mut Vec<String>, id: &str, label: &str, equal: bool) {
    if !equal {
        failures.push(format!("{id}: {label}"));
    }
}

fn error_equals<T>(actual: &Result<T, JsonFormatError>, expected: &Negative) -> bool {
    match actual {
        Err(error) => {
            format!("{error:?}") == format!("{:?}", expected.error)
                && error.to_string() == expected.display
        }
        Ok(_) => false,
    }
}

/// Removes a per-test directory on drop, including after a panic.
struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn nested_wrapper_constraints_preserve_full_models_values_exports_and_errors()
-> Result<(), Box<dyn Error>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let evidence = std::env::temp_dir().join(format!(
        "ferrule_nested_wrapper_constraints_{}_{}_{}",
        std::process::id(),
        suffix,
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // create_dir refuses a preexisting evidence namespace.
    fs::create_dir(&evidence)?;
    let _cleanup = TempDir(evidence.clone());
    eprintln!("nested wrapper constraint evidence: {}", evidence.display());
    let setup = cases();
    retain(&evidence, "SETUP", &setup)?;
    let cases = setup?;
    let mut failures = Vec::new();

    for case in cases {
        let directory = evidence.join(case.id);
        fs::create_dir(&directory)?;
        fs::write(directory.join("EXPECTED.txt"), format!("{case:#?}\n"))?;
        fs::write(directory.join("schema.json"), case.schema)?;
        fs::write(directory.join("input.json"), case.input)?;
        fs::write(directory.join("expected-wire.json"), case.wire)?;
        fs::write(
            directory.join("expected-export.json"),
            serde_json::to_string_pretty(&case.exported)?,
        )?;

        let imported = import_str_result(case.schema);
        retain(&directory, "IMPORT", &imported)?;
        // The authored model is also exercised if import fails; that fallback is explicit.
        let schema = imported.as_ref().unwrap_or(&case.model);
        fs::write(
            directory.join("BOUNDARY-SCHEMA.txt"),
            format!("{schema:#?}\n"),
        )?;
        let exported = crate::json_schema::export(schema);
        retain(&directory, "EXPORT", &exported)?;
        let exported_value = exported
            .as_ref()
            .ok()
            .map(|text| serde_json::from_str::<serde_json::Value>(text));
        fs::write(
            directory.join("EXPORT-JSON.txt"),
            format!("{exported_value:#?}\n"),
        )?;
        let reimported = exported.as_ref().ok().map(|text| import_str_result(text));
        fs::write(directory.join("REIMPORT.txt"), format!("{reimported:#?}\n"))?;

        let read = from_str(case.input, schema);
        retain(&directory, "READ", &read)?;
        let write = to_string(schema, &case.typed);
        retain(&directory, "WRITE", &write)?;
        if let Ok(text) = &write {
            fs::write(directory.join("actual-wire.json"), text.as_bytes())?;
        }
        let readback = write.as_ref().ok().map(|text| from_str(text, schema));
        fs::write(
            directory.join("WRITE-READBACK.txt"),
            format!("{readback:#?}\n"),
        )?;
        let rewritten = read.as_ref().ok().map(|value| to_string(schema, value));
        fs::write(
            directory.join("READ-WRITE.txt"),
            format!("{rewritten:#?}\n"),
        )?;

        // Retain every positive raw outcome before comparing any expected value.
        compare(
            &mut failures,
            case.id,
            "full imported model",
            matches!(&imported, Ok(value) if value == &case.model),
        );
        compare(
            &mut failures,
            case.id,
            "complete exported schema",
            matches!(&exported_value, Some(Ok(value)) if value == &case.exported),
        );
        compare(
            &mut failures,
            case.id,
            "full reimported model",
            matches!(&reimported, Some(Ok(value)) if value == &case.model),
        );
        compare(
            &mut failures,
            case.id,
            "complete typed read",
            matches!(&read, Ok(value) if value == &case.typed),
        );
        compare(
            &mut failures,
            case.id,
            "literal pretty wire",
            matches!(&write, Ok(value) if value == case.wire),
        );
        compare(
            &mut failures,
            case.id,
            "complete typed wire readback",
            matches!(&readback, Some(Ok(value)) if value == &case.typed),
        );
        compare(
            &mut failures,
            case.id,
            "read value literal rewrite",
            matches!(&rewritten, Some(Ok(value)) if value == case.wire),
        );

        for negative in &case.negatives {
            let negative_directory = directory.join(negative.id);
            fs::create_dir(&negative_directory)?;
            fs::write(
                negative_directory.join("EXPECTED.txt"),
                format!("{negative:#?}\n"),
            )?;
            fs::write(negative_directory.join("input.json"), negative.input)?;
            let read_error = from_str(negative.input, schema);
            retain(&negative_directory, "READ", &read_error)?;
            let write_error = to_string(schema, &negative.typed);
            retain(&negative_directory, "WRITE", &write_error)?;
            compare(
                &mut failures,
                case.id,
                &format!("{} complete read error", negative.id),
                error_equals(&read_error, negative),
            );
            compare(
                &mut failures,
                case.id,
                &format!("{} complete write error", negative.id),
                error_equals(&write_error, negative),
            );
            compare(
                &mut failures,
                case.id,
                &format!("{} independently selected error display", negative.id),
                negative.error.to_string() == negative.display,
            );
        }
    }

    fs::write(
        evidence.join("FINAL.txt"),
        format!("FAILURES={failures:#?}\n"),
    )?;
    assert!(
        failures.is_empty(),
        "complete nested wrapper constraints differ: {failures:#?}; evidence={}",
        evidence.display()
    );
    Ok(())
}
