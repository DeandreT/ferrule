use codegen_runtime::{ExecutionContext, FunctionError, Instance, RuntimeError, RuntimeParameters, Value};
use std::path::Path;

fn run(cases: &[serde_json::Value], schema: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for case in cases {
        let mut parameters = RuntimeParameters::new();
        parameters.insert("InputText", Value::String(case["input"].as_str().unwrap().into()))?;
        let context = ExecutionContext::new(Path::new("synthetic.mfd")).with_parameters(&parameters);
        // Reuse the host execution metadata, not parsed invocation documents.
        for _ in 0..2 {
            codegen_runtime::reset_json_projection_full_parse_count();
            let result = json_projection_mapping::execute_with_context(&Instance::Group(Vec::new().into()), &context);
            assert_eq!(codegen_runtime::json_projection_full_parse_count(), case["parses"].as_u64().unwrap_or(1) as usize, "{case}");
            if case.get("error").is_some() {
                assert!(matches!(result, Err(RuntimeError::Function(FunctionError::InvalidArgument {
                    function: "json_parse_field" | "json_parse_array_length", message: "input does not match the JSON schema"
                }))), "{case}: {result:?}");
            } else {
                let expected = codegen_runtime::parse_json(schema, &case["expected"].to_string())?;
                assert_eq!(result?, expected, "{case}");
            }
        }
        println!("Rust Release rows={} full_parses={}", case["rows"], case["parses"].as_u64().unwrap_or(1));
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&paths[0])?)?;
    let schema = std::fs::read_to_string(&paths[1])?;
    let parser = std::fs::read_to_string(&paths[3])?;
    let source = Instance::Group(Vec::new().into());
    let scope = codegen_runtime::ScopeContext::new(&source);
    let mut arguments = vec![Value::String(r#"{"Title":"first"}"#.into()), Value::String(parser.clone()), Value::String(r#"["Title"]"#.into())];
    codegen_runtime::reset_json_projection_full_parse_count();
    for title in ["first", "other", "first"] {
        arguments[0] = Value::String(format!("{{\"Title\":\"{title}\"}}"));
        assert_eq!(scope.call_json_array_projection(0, "json_parse_field", &arguments)?, Value::String(title.into()));
    }
    assert_eq!(codegen_runtime::json_projection_full_parse_count(), 3);
    assert_eq!(scope.call_json_array_projection(1, "json_parse_field", &arguments)?, Value::String("first".into()));
    assert_eq!(codegen_runtime::json_projection_full_parse_count(), 4, "independent owner must not share equal text");
    let mut changed: serde_json::Value = serde_json::from_str(&parser)?;
    changed["kind"]["children"][0]["kind"]["ty"] = serde_json::json!("int");
    arguments[1] = Value::String(changed.to_string());
    assert!(matches!(scope.call_json_array_projection(0, "json_parse_field", &arguments), Err(RuntimeError::Function(FunctionError::InvalidArgument { function: "json_parse_field", message: "input does not match the JSON schema" }))));
    assert_eq!(codegen_runtime::json_projection_full_parse_count(), 5, "same source with changed schema must revalidate");
    arguments[1] = Value::String(parser.clone());
    let kept = scope.clone().call_json_array_projection(0, "json_parse_field", &arguments)?;
    arguments[0] = Value::String(r#"{"Title":"replacement"}"#.into());
    assert_eq!(scope.call_json_array_projection(0, "json_parse_field", &arguments)?, Value::String("replacement".into()));
    drop(scope);
    assert_eq!(kept, Value::String("first".into()));
    run(&cases, &schema)?;
    std::thread::scope(|scope| {
        let threads: Vec<_> = (0..4).map(|_| scope.spawn(|| run(&cases, &schema))).collect();
        for thread in threads { thread.join().unwrap().unwrap(); }
    });
    println!("Rust Release fields={} complete output/error and invocation parse-count checks passed", paths[2]);
    Ok(())
}
