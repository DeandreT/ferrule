use format_json::{JsonFormatError, json_schema};
use ir::{Instance, SchemaKind, SchemaNode, Value, XmlTypeOrigin};
use serde_json::{Value as Json, json};
use std::{fs, path::PathBuf};

fn evidence() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferrule-nested-array-native-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("cases.json"),
        include_bytes!("fixtures/nested_array_wrappers/cases.json"),
    )
    .unwrap();
    root
}

fn record(root: &std::path::Path, name: &str, value: impl std::fmt::Debug) {
    fs::write(
        root.join(format!("{name}.original.txt")),
        format!("{value:#?}\n"),
    )
    .unwrap();
}

fn typed(value: &Json) -> Instance {
    match value["kind"].as_str().unwrap() {
        "Repeated" => Instance::Repeated(
            value["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(typed)
                .collect(),
        ),
        "Group" => Instance::Group(
            value["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|field| {
                    (
                        field["name"].as_str().unwrap().to_owned(),
                        typed(&field["value"]),
                    )
                })
                .collect::<Vec<_>>()
                .into(),
        ),
        "Scalar" => Instance::Scalar(match value["value"]["kind"].as_str().unwrap() {
            "Int" => Value::Int(value["value"]["value"].as_i64().unwrap()),
            "String" => Value::String(value["value"]["value"].as_str().unwrap().to_owned()),
            "Bool" => Value::Bool(value["value"]["value"].as_bool().unwrap()),
            "Float" => Value::Float(value["value"]["value"].as_f64().unwrap()),
            "JsonNull" => Value::json_null(),
            "XmlNil" => Value::xml_nil(),
            "Null" => Value::Null,
            _ => panic!("closed authored scalar kind"),
        }),
        _ => panic!("closed authored instance kind"),
    }
}

fn full(value: &Instance) -> Json {
    match value {
        Instance::Scalar(value) => json!({"kind":"Scalar", "value":match value {
            Value::Int(value) => json!({"kind":"Int","value":value}),
            Value::String(value) => json!({"kind":"String","value":value}),
            Value::Bool(value) => json!({"kind":"Bool","value":value}),
            Value::Float(value) => json!({"kind":"Float","bits":format!("{:016x}", value.to_bits())}),
            Value::Null => json!({"kind":"Null"}),
            Value::JsonNull(_) => json!({"kind":"JsonNull"}),
            Value::XmlNil(_) => json!({"kind":"XmlNil"}),
            Value::Decimal(value) => json!({"kind":"Decimal","value":value.to_string()}),
        }}),
        Instance::Repeated(items) => {
            json!({"kind":"Repeated","items":items.iter().map(full).collect::<Vec<_>>()})
        }
        Instance::Group(fields) => {
            json!({"kind":"Group","origin":match fields.xml_type_origin() {XmlTypeOrigin::Unknown => Json::Null, origin => json!({"unexpected":format!("{origin:?}")})},"fields":fields.iter().map(|(name,value)|json!({"name":name,"value":full(value)})).collect::<Vec<_>>()})
        }
        other => json!({"unexpected":format!("{other:#?}")}),
    }
}

fn error(error: &JsonFormatError) -> Json {
    let mut value = match error {
        JsonFormatError::Shape {
            name,
            expected,
            got,
        } => json!({"variant":"Shape","name":name,"expected":expected,"got":got}),
        JsonFormatError::ItemCountMismatch { name, range, got } => {
            json!({"variant":"ItemCountMismatch","name":name,"range":range,"got":got})
        }
        JsonFormatError::ContainsCountMismatch { name, range, got } => {
            json!({"variant":"ContainsCountMismatch","name":name,"range":range,"got":got})
        }
        JsonFormatError::UniqueItemsMismatch {
            name,
            first_index,
            duplicate_index,
        } => {
            json!({"variant":"UniqueItemsMismatch","name":name,"first_index":first_index,"duplicate_index":duplicate_index})
        }
        JsonFormatError::UnsupportedSchemaUnion { name, reason } => {
            json!({"variant":"UnsupportedSchemaUnion","name":name,"reason":reason})
        }
        other => json!({"unexpected":format!("{other:#?}")}),
    };
    value["display"] = error.to_string().into();
    value
}

#[test]
fn nested_array_wrappers_complete_native_oracles() {
    let root = evidence();
    let fixture: Json =
        serde_json::from_slice(include_bytes!("fixtures/nested_array_wrappers/cases.json"))
            .unwrap();
    let mut failures = Vec::new();
    for row in fixture["cases"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let imported = json_schema::import_str(&row["schema"].to_string());
        record(&root, &format!("{id}-import"), &imported);
        let expected = serde_json::from_value::<SchemaNode>(row["expected_model"].clone());
        record(&root, &format!("{id}-expected-model"), &expected);
        let (Ok(schema), Ok(expected)) = (imported, expected) else {
            failures.push(format!("{id}: setup"));
            continue;
        };
        if schema != expected {
            failures.push(format!("{id}: full schema"));
        }
        let exported = json_schema::export(&schema);
        record(&root, &format!("{id}-export"), &exported);
        let reimported = exported
            .as_ref()
            .ok()
            .map(|text| json_schema::import_str(text));
        record(&root, &format!("{id}-reimport"), &reimported);
        if !matches!(reimported, Some(Ok(ref model)) if model == &expected) {
            failures.push(format!("{id}: full roundtrip"));
        }
        let actual = format_json::from_str(row["wire"].as_str().unwrap(), &schema);
        record(&root, &format!("{id}-input"), &actual);
        match actual {
            Ok(value) if row.get("expected_typed").is_some() => {
                let wanted = typed(&row["expected_typed"]);
                let output = format_json::to_string(&schema, &value);
                let normalized = format_json::to_value(&schema, &value);
                record(&root, &format!("{id}-typed"), full(&value));
                record(&root, &format!("{id}-output"), &output);
                record(&root, &format!("{id}-normalized"), &normalized);
                if full(&value) != full(&wanted) {
                    failures.push(format!("{id}: typed tags/origin/order"));
                }
                if !matches!(output.as_deref(), Ok(text) if text == row["expected_output"].as_str().unwrap())
                {
                    failures.push(format!("{id}: complete output"));
                }
                if !normalized.is_ok_and(|value| {
                    value
                        == serde_json::from_str::<Json>(row["expected_output"].as_str().unwrap())
                            .unwrap()
                }) {
                    failures.push(format!("{id}: normalized output"));
                }
            }
            Err(actual) if row.get("expected_native_error").is_some() => {
                if error(&actual) != row["expected_native_error"] {
                    failures.push(format!("{id}: full input error"));
                }
            }
            _ => failures.push(format!("{id}: outcome kind")),
        }
    }
    for row in fixture["typed_output_negative_cases"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let schema = serde_json::from_value::<SchemaNode>(row["expected_model"].clone());
        record(&root, &format!("{id}-typed-model"), &schema);
        let Ok(schema) = schema else {
            failures.push(format!("{id}: typed output setup"));
            continue;
        };
        let value = typed(&row["typed"]);
        record(&root, &format!("{id}-typed-input"), &value);
        let actual = format_json::to_string(&schema, &value);
        record(&root, &format!("{id}-typed-output"), &actual);
        if !actual.is_err_and(|actual| error(&actual) == row["expected_native_error"]) {
            failures.push(format!("{id}: full typed output error"));
        }
    }
    for row in fixture["schema_refusal_cases"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let imported = json_schema::import_str(&row["schema"].to_string());
        record(&root, &format!("{id}-schema-refusal"), &imported);
        if !imported.is_err_and(|actual| error(&actual) == row["expected_native_error"]) {
            failures.push(format!("{id}: complete schema refusal"));
        }
    }
    fs::write(
        root.join("failures.json"),
        serde_json::to_vec_pretty(&failures).unwrap(),
    )
    .unwrap();
    assert!(
        failures.is_empty(),
        "{failures:?}; evidence {}",
        root.display()
    );
}

#[test]
fn nested_array_wrapper_malformed_ir_refuses_and_false_marker_is_omitted() {
    let root = evidence();
    let fixture: Json =
        serde_json::from_slice(include_bytes!("fixtures/nested_array_wrappers/cases.json"))
            .unwrap();
    let base: SchemaNode =
        serde_json::from_value(fixture["cases"][0]["expected_model"].clone()).unwrap();
    let mut failures = Vec::new();
    for row in fixture["malformed_ir"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let mut schema = base.clone();
        match id {
            "scalar-marker" => {
                schema.kind = SchemaKind::Scalar {
                    ty: ir::ScalarType::Int,
                }
            }
            "nonrepeated-wrapper" => schema.repeating = false,
            "scalar-nullable" => schema.nullable = true,
            "scalar-range" => {
                schema.numeric_range = Some(ir::NumericRange::Integer(
                    ir::IntegerRange::new(Some(0), Some(1)).unwrap(),
                ))
            }
            "object-property-count" => {
                schema.property_count_range = ir::PropertyCountRange::new(1, None)
            }
            "xml-attribute" => schema.attribute = true,
            "json-any-text" => schema.json_any = true,
            "recursion-anchor" => schema.recursive_ref = Some("Loop".into()),
            _ => {
                let SchemaKind::Group {
                    children,
                    required,
                    dynamic,
                    ..
                } = &mut schema.kind
                else {
                    unreachable!()
                };
                match id {
                    "zero-child" => children.clear(),
                    "two-children" => {
                        children.push(SchemaNode::scalar("extra", ir::ScalarType::Int).repeating())
                    }
                    "wrong-child-name" => children[0].name = "row".into(),
                    "nonrepeated-child" => children[0].repeating = false,
                    "object-required" => required.push("item".into()),
                    "dynamic-child" => {
                        *dynamic = Some(Box::new(SchemaNode::scalar("*", ir::ScalarType::String)))
                    }
                    _ => panic!("closed malformed category"),
                }
            }
        }
        let encoded = serde_json::to_string(&schema).unwrap();
        let decoded = serde_json::from_str::<SchemaNode>(&encoded);
        let input = format_json::from_str("[[7,9]]", &schema);
        let output =
            format_json::to_string(&schema, &typed(&fixture["cases"][0]["expected_typed"]));
        let export = json_schema::export(&schema);
        record(
            &root,
            &format!("{id}-full"),
            (&schema, &encoded, &decoded, &input, &output, &export),
        );
        if schema.metadata_is_valid()
            || !decoded.is_err_and(|error| {
                error
                    .to_string()
                    .starts_with("invalid JSON array-wrapper metadata")
            })
            || !matches!(input, Err(JsonFormatError::InvalidArrayWrapperMetadata{ref name}) if name == "Batches")
            || !matches!(output, Err(JsonFormatError::InvalidArrayWrapperMetadata{ref name}) if name == "Batches")
            || !matches!(export, Err(JsonFormatError::InvalidArrayWrapperMetadata{ref name}) if name == "Batches")
        {
            failures.push(id.to_owned());
        }
    }
    let ordinary = SchemaNode::group(
        "Document",
        vec![SchemaNode::scalar("item", ir::ScalarType::String)],
    );
    let ordinary_json = serde_json::to_string(&ordinary).unwrap();
    record(&root, "ordinary-item-model", (&ordinary, &ordinary_json));
    if ordinary_json.contains("json_array_wrapper") {
        failures.push("false marker omission".into());
    }
    fs::write(
        root.join("failures.json"),
        serde_json::to_vec_pretty(&failures).unwrap(),
    )
    .unwrap();
    assert!(
        failures.is_empty(),
        "{failures:?}; evidence {}",
        root.display()
    );
}
