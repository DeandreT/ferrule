use super::*;
use ir::ScalarType;

const OWNER: JsonProjectionOwner = JsonProjectionOwner { source: 17 };

fn descriptor(fields: usize) -> String {
    serde_json::to_string(&SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Title", ScalarType::String),
            SchemaNode::group(
                "Rows",
                (0..fields)
                    .map(|field| {
                        SchemaNode::scalar(format!("F{field}"), ScalarType::String)
                            .nullable()
                            .unwrap()
                    })
                    .collect(),
            )
            .repeating(),
        ],
    ))
    .unwrap()
}

fn args(input: &str, schema: &str) -> Vec<Value> {
    vec![
        Value::String(input.into()),
        Value::String(schema.into()),
        Value::String(r#"["Rows"]"#.into()),
    ]
}

fn count() -> usize {
    format_json::document_dom_decode_attempt_count()
}
fn reset() {
    format_json::reset_document_dom_decode_attempt_count();
}

#[test]
fn stateless_baseline_and_invocation_work_count_grow_independently() {
    for rows in [1, 7, 41] {
        for fields in [1, 4, 13] {
            let schema = descriptor(fields);
            let records: Vec<_> = (0..rows)
                .map(|row| {
                    (0..fields)
                        .map(|field| {
                            (
                                format!("F{field}"),
                                serde_json::json!(format!("r{row}f{field}")),
                            )
                        })
                        .collect::<serde_json::Map<_, _>>()
                })
                .collect();
            let input = serde_json::json!({"Title":"root", "Rows":records}).to_string();
            let base = args(&input, &schema);
            reset();
            assert_eq!(
                crate::call("json_parse_array_length", &base).unwrap(),
                Value::Int(rows)
            );
            for row in 0..rows {
                for field in 0..fields {
                    let mut args = base.clone();
                    args.extend([
                        Value::String(format!("[\"F{field}\"]")),
                        Value::Int(row + 1),
                    ]);
                    assert_eq!(
                        crate::call("json_parse_array_field", &args).unwrap(),
                        Value::String(format!("r{row}f{field}"))
                    );
                }
            }
            assert_eq!(count(), 1 + rows as usize * fields);
            reset();
            let mut state = JsonArrayProjections::default();
            assert_eq!(
                state.call(OWNER, "json_parse_array_length", &base).unwrap(),
                Value::Int(rows)
            );
            for row in 0..rows {
                for field in 0..fields {
                    let mut args = base.clone();
                    args.extend([
                        Value::String(format!("[\"F{field}\"]")),
                        Value::Int(row + 1),
                    ]);
                    assert_eq!(
                        state.call(OWNER, "json_parse_array_field", &args).unwrap(),
                        Value::String(format!("r{row}f{field}"))
                    );
                }
            }
            let mut root = base.clone();
            root[2] = Value::String(r#"["Title"]"#.into());
            assert_eq!(
                state.call(OWNER, "json_parse_field", &root).unwrap(),
                Value::String("root".into())
            );
            root[2] = Value::String("[]".into());
            assert_eq!(
                state
                    .call(OWNER, "json_parse_object_exists", &root)
                    .unwrap(),
                Value::Bool(true)
            );
            assert_eq!(count(), 1);
            assert_eq!(state.documents.len(), 1);
            println!(
                "rows={rows} fields={fields} stateless={} invocation=1",
                1 + rows as usize * fields
            );
        }
    }
}

#[test]
fn changed_content_schema_and_source_owners_never_alias() {
    reset();
    let mut state = JsonArrayProjections::default();
    let schema = descriptor(1);
    for (index, input) in [
        r#"{"Rows":[{"F0":"first"}]}"#,
        r#"{"Rows":[{"F0":"other"}]}"#,
        r#"{"Rows":[{"F0":"first"}]}"#,
    ]
    .iter()
    .enumerate()
    {
        let mut args = args(input, &schema);
        args.extend([Value::String(r#"["F0"]"#.into()), Value::Int(1)]);
        let expected = if index == 1 { "other" } else { "first" };
        assert_eq!(
            state.call(OWNER, "json_parse_array_field", &args).unwrap(),
            Value::String(expected.into())
        );
    }
    assert_eq!(count(), 3);
    let input = r#"{"Rows":[{"F0":"first"}]}"#;
    state
        .call(
            JsonProjectionOwner { source: 18 },
            "json_parse_array_length",
            &args(input, &schema),
        )
        .unwrap();
    assert_eq!(count(), 4, "equal text must not merge independent owners");
    let mut changed: serde_json::Value = serde_json::from_str(&schema).unwrap();
    changed["kind"]["children"][1]["kind"]["children"][0]["kind"]["ty"] = serde_json::json!("int");
    let changed = serde_json::to_string(&changed).unwrap();
    let result = state.call(OWNER, "json_parse_array_length", &args(input, &changed));
    assert_eq!(
        result,
        Err(FunctionError::InvalidArgument {
            function: "json_parse_array_length",
            message: "input does not match the JSON schema"
        })
    );
    assert_eq!(count(), 5);
    assert_eq!(state.documents.len(), 2);
    let mut fresh = JsonArrayProjections::default();
    fresh
        .call(OWNER, "json_parse_array_length", &args(input, &schema))
        .unwrap();
    assert_eq!(count(), 6, "a new invocation must validate independently");
}

#[test]
fn returned_scalars_own_their_data_after_replacement_and_state_drop() {
    let schema = descriptor(1);
    let mut state = JsonArrayProjections::default();
    let mut args = args(r#"{"Rows":[{"F0":"kept"}]}"#, &schema);
    args.extend([Value::String(r#"["F0"]"#.into()), Value::Int(1)]);
    let kept = state.call(OWNER, "json_parse_array_field", &args).unwrap();
    args[0] = Value::String(r#"{"Rows":[{"F0":"replacement"}]}"#.into());
    assert_eq!(
        state.call(OWNER, "json_parse_array_field", &args).unwrap(),
        Value::String("replacement".into())
    );
    drop(state);
    assert_eq!(kept, Value::String("kept".into()));
}

#[test]
fn optional_parent_absence_never_bypasses_descriptor_or_complete_validation() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group(
                "Envelope",
                vec![
                    SchemaNode::group("Rows", vec![SchemaNode::scalar("F0", ScalarType::String)])
                        .repeating(),
                ],
            ),
            SchemaNode::scalar("Tail", ScalarType::Int),
        ],
    );
    let schema = serde_json::to_string(&schema).unwrap();
    let mut state = JsonArrayProjections::default();
    let mut base = args("{}", &schema);
    base[2] = Value::String(r#"["Envelope","Rows"]"#.into());
    reset();
    assert_eq!(
        state.call(OWNER, "json_parse_array_length", &base).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        state.call(OWNER, "json_parse_array_length", &base).unwrap(),
        Value::Int(0)
    );
    assert_eq!(count(), 1);
    let mut field = base.clone();
    field.extend([Value::String(r#"["F0"]"#.into()), Value::Int(1)]);
    assert_eq!(
        state.call(OWNER, "json_parse_array_field", &field),
        Err(FunctionError::InvalidArgument {
            function: "json_parse_array_field",
            message: "array index is out of range"
        })
    );
    base[2] = Value::String(r#"["Envelope","Missing"]"#.into());
    assert_eq!(
        state.call(OWNER, "json_parse_array_length", &base),
        Err(FunctionError::InvalidArgument {
            function: "json_parse_array_length",
            message: "array path does not resolve to an array"
        })
    );
    base[2] = Value::String(r#"["Envelope","Rows"]"#.into());
    base[0] = Value::String(r#"{"Tail":"bad"}"#.into());
    assert_eq!(
        state.call(OWNER, "json_parse_array_length", &base),
        Err(FunctionError::InvalidArgument {
            function: "json_parse_array_length",
            message: "input does not match the JSON schema"
        })
    );
}

#[test]
fn cached_document_keeps_operation_specific_failures_and_null_layers() {
    let schema = descriptor(1);
    let mut state = JsonArrayProjections::default();
    for input in [
        r#"{"Rows":[{}, {"F0":null}]}"#,
        r#"{"Rows":[{"F0":"old","F0":"last"}]}"#,
        "{}",
    ] {
        let base = args(input, &schema);
        let _ = state.call(OWNER, "json_parse_array_length", &base).unwrap();
        for (function, path, index) in [
            ("json_parse_array_field", r#"["F0"]"#, 1),
            ("json_parse_array_field", r#"["F0"]"#, 2),
            ("json_parse_array_field", r#"["Missing"]"#, 1),
            ("json_parse_field", r#"["Missing"]"#, 1),
            ("json_parse_object_exists", r#"["Missing"]"#, 1),
        ] {
            let mut args = base.clone();
            if function == "json_parse_array_field" {
                args.extend([Value::String(path.into()), Value::Int(index)]);
            } else {
                args[2] = Value::String(path.into());
            }
            assert_eq!(
                state.call(OWNER, function, &args),
                crate::call(function, &args)
            );
        }
    }
    for input in [
        r#"{"Rows":[{"F0":"ok"},{"F0":7}]}"#,
        r#"{"Rows":[{"F0":"ok"}],"late":1e400}"#,
    ] {
        for function in [
            "json_parse_array_length",
            "json_parse_field",
            "json_parse_object_exists",
        ] {
            let mut args = args(input, &schema);
            if function != "json_parse_array_length" {
                args[2] = Value::String("[]".into());
            }
            assert_eq!(
                state.call(OWNER, function, &args),
                Err(FunctionError::InvalidArgument {
                    function,
                    message: "input does not match the JSON schema"
                })
            );
        }
    }
}
