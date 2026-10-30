//! Native, prepared, JSON5 and JSON Lines reads and writes of the shared
//! JSON Schema tuple-array corpus (`docs/design/json-tuple-arrays.md`).
//!
//! Each case is read through its expected model; JSON Schema import of the
//! corpus schemas belongs to the importer.

use std::path::PathBuf;

use format_json::{JsonFormatError, PreparedSchema};
use ir::{Instance, SchemaNode, Value};
use serde_json::{Value as Json, json};

const CORPUS: &[u8] = include_bytes!("fixtures/json_tuple_arrays/cases.json");

fn corpus() -> Json {
    serde_json::from_slice(CORPUS).expect("the corpus is JSON")
}

fn typed(value: &Json) -> Instance {
    match value["kind"].as_str() {
        Some("Repeated") => Instance::Repeated(
            value["items"]
                .as_array()
                .expect("repeated items")
                .iter()
                .map(typed)
                .collect(),
        ),
        Some("Group") => Instance::Group(
            value["fields"]
                .as_array()
                .expect("group fields")
                .iter()
                .map(|field| {
                    (
                        field["name"].as_str().expect("field name").to_owned(),
                        typed(&field["value"]),
                    )
                })
                .collect::<Vec<_>>()
                .into(),
        ),
        Some("Scalar") => {
            let scalar = &value["value"];
            Instance::Scalar(match scalar["kind"].as_str() {
                Some("Int") => Value::Int(scalar["value"].as_i64().expect("int")),
                Some("String") => Value::String(scalar["value"].as_str().expect("text").into()),
                Some("Bool") => Value::Bool(scalar["value"].as_bool().expect("bool")),
                Some("Float") => Value::Float(scalar["value"].as_f64().expect("float")),
                Some("JsonNull") => Value::json_null(),
                Some("Null") => Value::Null,
                other => panic!("closed authored scalar kind, got {other:?}"),
            })
        }
        other => panic!("closed authored instance kind, got {other:?}"),
    }
}

/// The corpus record of an error: the innermost error's kind and node, and
/// the one-based position of the outermost positional error.
fn record(error: &JsonFormatError) -> Json {
    let mut position = None;
    let mut error = error;
    while let JsonFormatError::TuplePosition {
        position: at,
        error: inner,
        ..
    } = error
    {
        position.get_or_insert(*at);
        error = inner;
    }
    let (kind, name) = match error {
        JsonFormatError::Shape { name, .. } => ("Shape", name),
        JsonFormatError::TupleLengthMismatch { name, .. } => ("TupleLengthMismatch", name),
        JsonFormatError::TupleGap {
            name,
            position: gap,
        } => {
            position.get_or_insert(*gap);
            ("TupleGap", name)
        }
        JsonFormatError::UndeclaredTupleField { name, .. } => ("UndeclaredTupleField", name),
        JsonFormatError::TupleRootRows { name } => ("TupleRootRows", name),
        JsonFormatError::RangeMismatch { name, .. } => ("RangeMismatch", name),
        JsonFormatError::StringLengthMismatch { name, .. } => ("StringLengthMismatch", name),
        JsonFormatError::ItemCountMismatch { name, .. } => ("ItemCountMismatch", name),
        JsonFormatError::MissingRequiredProperty { object, .. } => {
            ("MissingRequiredProperty", object)
        }
        JsonFormatError::RecursiveJsonTupleFrame { node, frame } => {
            return json!({"kind": "RecursiveJsonTupleFrame", "name": node, "frame": frame});
        }
        other => return json!({"unexpected": format!("{other:?}")}),
    };
    json!({"kind": kind, "name": name, "position": position})
}

fn check_error<T: std::fmt::Debug>(
    failures: &mut Vec<String>,
    label: &str,
    result: Result<T, JsonFormatError>,
    expected: &Json,
) {
    match result {
        Err(error) if record(&error) == *expected => {}
        Err(error) => failures.push(format!(
            "{label}: expected {expected}, got {} ({error})",
            record(&error)
        )),
        Ok(value) => failures.push(format!("{label}: expected {expected}, got {value:?}")),
    }
}

fn check_output(
    failures: &mut Vec<String>,
    label: &str,
    schema: &SchemaNode,
    prepared: &PreparedSchema,
    instance: &Instance,
    output: &Json,
) {
    for (writer, value) in [
        ("to_value", format_json::to_value(schema, instance)),
        ("prepared to_value", prepared.to_value(instance)),
    ] {
        match value {
            Ok(value) if value == *output => {}
            other => failures.push(format!(
                "{label} {writer}: expected {output}, got {other:?}"
            )),
        }
    }
    let pretty = format!(
        "{}\n",
        serde_json::to_string_pretty(output).expect("pretty")
    );
    match format_json::to_string(schema, instance) {
        Ok(text) if text == pretty => {}
        other => failures.push(format!(
            "{label} to_string: expected {pretty:?}, got {other:?}"
        )),
    }
}

fn check_write_error(
    failures: &mut Vec<String>,
    label: &str,
    schema: &SchemaNode,
    prepared: &PreparedSchema,
    instance: &Instance,
    expected: &Json,
) {
    check_error(
        failures,
        &format!("{label} to_value"),
        format_json::to_value(schema, instance),
        expected,
    );
    check_error(
        failures,
        &format!("{label} prepared to_value"),
        prepared.to_value(instance),
        expected,
    );
    check_error(
        failures,
        &format!("{label} to_string"),
        format_json::to_string(schema, instance),
        expected,
    );
    check_error(
        failures,
        &format!("{label} to_json5_string"),
        format_json::to_json5_string(schema, instance),
        expected,
    );
}

fn run_case(failures: &mut Vec<String>, case: &Json) {
    let id = case["id"].as_str().expect("case id");
    let schema: SchemaNode = match serde_json::from_value(case["expected_model"].clone()) {
        Ok(schema) => schema,
        Err(error) => {
            failures.push(format!("{id}: model: {error}"));
            return;
        }
    };
    if !schema.json_tuple_frames_tree_is_valid() {
        failures.push(format!("{id}: the model's tuple frames are not canonical"));
    }
    let prepared = PreparedSchema::new(schema.clone());
    let empty = Vec::new();
    for (index, row) in case["accepted"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        let label = format!("{id} accepted {index}");
        let wire = row["wire"].as_str().expect("wire");
        let expected = typed(&row["typed"]);
        for (reader, read) in [
            ("from_str", format_json::from_str(wire, &schema)),
            ("prepared from_str", prepared.from_str(wire)),
            ("from_json5_str", format_json::from_json5_str(wire, &schema)),
        ] {
            match read {
                Ok(read) if read == expected => {}
                other => failures.push(format!("{label} {reader}: got {other:?}")),
            }
        }
        let output = row.get("output").and_then(Json::as_str).unwrap_or(wire);
        let output: Json = serde_json::from_str(output).expect("output is JSON");
        check_output(failures, &label, &schema, &prepared, &expected, &output);
        match format_json::to_json5_string(&schema, &expected)
            .and_then(|text| format_json::from_json5_str(&text, &schema))
        {
            Ok(read) if read == expected => {}
            other => failures.push(format!("{label} JSON5 round trip: got {other:?}")),
        }
        if let Some(lines) = row.get("lines").and_then(Json::as_str) {
            match format_json::from_lines(lines, &schema) {
                Ok(read) if read == expected => {}
                other => failures.push(format!("{label} from_lines: got {other:?}")),
            }
            match format_json::to_lines(&schema, &expected) {
                Ok(text) if text == lines => {}
                other => failures.push(format!("{label} to_lines: got {other:?}")),
            }
        }
    }
    for (index, row) in case["rejected"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        let label = format!("{id} rejected {index}");
        let wire = row["wire"].as_str().expect("wire");
        check_error(
            failures,
            &format!("{label} from_str"),
            format_json::from_str(wire, &schema),
            &row["error"],
        );
        check_error(
            failures,
            &format!("{label} prepared from_str"),
            prepared.from_str(wire),
            &row["error"],
        );
        check_error(
            failures,
            &format!("{label} from_json5_str"),
            format_json::from_json5_str(wire, &schema),
            &row["error"],
        );
    }
    for (index, row) in case["write_only"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        let label = format!("{id} write-only {index}");
        let instance = typed(&row["typed"]);
        match (row.get("output"), row.get("error")) {
            (Some(output), None) => {
                let output: Json =
                    serde_json::from_str(output.as_str().expect("output text")).expect("JSON");
                check_output(failures, &label, &schema, &prepared, &instance, &output);
            }
            (None, Some(error)) => {
                check_write_error(failures, &label, &schema, &prepared, &instance, error);
            }
            _ => failures.push(format!("{label}: needs exactly one of output and error")),
        }
    }
    if let Some(error) = case.get("json_lines_error") {
        let row = &case["accepted"][0];
        check_error(
            failures,
            &format!("{id} from_lines"),
            format_json::from_lines(row["wire"].as_str().expect("wire"), &schema),
            error,
        );
        check_error(
            failures,
            &format!("{id} to_lines"),
            format_json::to_lines(&schema, &typed(&row["typed"])),
            error,
        );
    }
}

#[test]
fn tuple_frame_corpus_reads_and_writes_natively() {
    let corpus = corpus();
    let mut failures = Vec::new();
    let mut count = 0;
    for section in ["cases", "codec_cases"] {
        for case in corpus[section].as_array().expect("case list") {
            run_case(&mut failures, case);
            count += 1;
        }
    }
    assert!(count >= 14, "the corpus lost cases: {count}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn recursive_frames_are_refused_rather_than_expanded() {
    let corpus = corpus();
    let mut failures = Vec::new();
    for case in corpus["codec_refusal_cases"]
        .as_array()
        .expect("refusal list")
    {
        let id = case["id"].as_str().expect("case id");
        let schema: SchemaNode =
            serde_json::from_value(case["expected_model"].clone()).expect("refusal model");
        // The frame check does not follow references, so it admits these.
        assert!(schema.json_tuple_frames_tree_is_valid(), "{id}");
        let wire = case["wire"].as_str().expect("wire");
        let expected = &case["error"];
        let prepared = PreparedSchema::new(schema.clone());
        let instance = Instance::Group(Vec::new().into());
        check_error(
            &mut failures,
            &format!("{id} from_str"),
            format_json::from_str(wire, &schema),
            expected,
        );
        check_error(
            &mut failures,
            &format!("{id} prepared from_str"),
            prepared.from_str(wire),
            expected,
        );
        check_error(
            &mut failures,
            &format!("{id} to_value"),
            format_json::to_value(&schema, &instance),
            expected,
        );
        check_error(
            &mut failures,
            &format!("{id} to_string"),
            format_json::to_string(&schema, &instance),
            expected,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-format-json-tuple-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn case(id: &str) -> (SchemaNode, Instance, String) {
    let corpus = corpus();
    let case = corpus["cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["id"] == id))
        .expect("corpus case");
    let schema = serde_json::from_value(case["expected_model"].clone()).expect("model");
    let row = &case["accepted"][0];
    let lines = row["lines"].as_str().unwrap_or_default().to_owned();
    (schema, typed(&row["typed"]), lines)
}

#[test]
fn file_readers_and_writers_share_the_tuple_codec() {
    let dir = TempDir::new("files");
    let (schema, instance, lines) = case("repeating-frame-root");
    let path = dir.0.join("tuples.json");
    format_json::write(&path, &schema, &instance).expect("write");
    assert_eq!(format_json::read(&path, &schema).expect("read"), instance);
    format_json::write_json5(&path, &schema, &instance).expect("write_json5");
    assert_eq!(
        format_json::read_json5(&path, &schema).expect("read_json5"),
        instance
    );
    format_json::write_lines(&path, &schema, &instance).expect("write_lines");
    assert_eq!(std::fs::read_to_string(&path).expect("lines"), lines);
    assert_eq!(
        format_json::read_lines(&path, &schema).expect("read_lines"),
        instance
    );
}

#[test]
fn tuple_items_reach_unique_items_inside_members() {
    // A member object whose own array asserts uniqueItems: the raw prepass
    // and the decoded predicate walk must both descend into the position.
    let codes = SchemaNode::scalar("codes", ir::ScalarType::Int).repeating();
    let mut codes = codes;
    codes.json_unique_items = true;
    let member = SchemaNode::group("item2", vec![codes]);
    let frame = SchemaNode::group(
        "Row",
        vec![SchemaNode::scalar("item1", ir::ScalarType::String), member],
    )
    .with_json_tuple(ir::JsonTupleLayout {
        length: ir::ItemCountRange::new(0, Some(2)),
        tail: ir::JsonTupleTail::Closed,
    })
    .expect("canonical frame");
    let document = r#"["a",{"codes":[1,1]}]"#;
    for result in [
        format_json::from_str(document, &frame),
        format_json::from_json5_str(document, &frame),
        PreparedSchema::new(frame.clone()).from_str(document),
    ] {
        assert!(
            matches!(
                result,
                Err(JsonFormatError::UniqueItemsMismatch { ref name, .. }) if name == "codes"
            ),
            "{result:?}"
        );
    }
    assert!(format_json::from_str(r#"["a",{"codes":[1,2]}]"#, &frame).is_ok());
}

#[test]
fn json_schema_export_still_refuses_tuple_frames() {
    let (schema, _, _) = case("closed-draft-2020-12");
    assert!(matches!(
        format_json::json_schema::export(&schema),
        Err(JsonFormatError::UnsupportedJsonTupleFrameExport { .. })
    ));
}
