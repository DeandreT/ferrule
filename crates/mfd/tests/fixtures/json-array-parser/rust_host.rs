use codegen_runtime::{
    ExecutionContext, FunctionError, Instance, RuntimeError, RuntimeParameters, Value,
};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&paths[0])?)?;
    let schema = std::fs::read_to_string(&paths[1])?;
    for case in cases {
        let mut parameters = RuntimeParameters::new();
        parameters.insert(
            "InputText",
            Value::String(case["input"].as_str().unwrap().into()),
        )?;
        let context = ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
        let result =
            json_array_mapping::execute_with_context(&Instance::Group(Vec::new().into()), &context);
        if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(RuntimeError::Function(FunctionError::InvalidArgument {
                function: "json_parse_array_length", message: actual
            })) if actual == message),
                "{case}: {result:?}"
            );
        } else {
            let actual = result?;
            let expected = codegen_runtime::parse_json(&schema, &case["expected"].to_string())?;
            assert_eq!(actual, expected, "{case}");
        }
    }
    let schema = std::fs::read_to_string(&paths[2])?;
    for index in [i64::MIN, -1, 0, 2, i64::MAX] {
        let result = codegen_runtime::call(
            "json_parse_array_field",
            &[
                Value::String(r#"{"Rows":[{"Qty":2}]}"#.into()),
                Value::String(schema.clone()),
                Value::String(r#"["Rows"]"#.into()),
                Value::String(r#"["Qty"]"#.into()),
                Value::Int(index),
            ],
        );
        assert!(
            matches!(
                result,
                Err(RuntimeError::Function(FunctionError::InvalidArgument {
                    function: "json_parse_array_field",
                    message: "array index is out of range"
                }))
            ),
            "{result:?}"
        );
    }
    for input in [Value::Null, Value::json_null(), Value::Int(7)] {
        let result = codegen_runtime::call(
            "json_parse_array_length",
            &[
                input.clone(),
                Value::String(schema.clone()),
                Value::String(r#"["Rows"]"#.into()),
            ],
        );
        assert!(
            matches!(result, Err(RuntimeError::Function(FunctionError::TypeMismatch {
            function: "json_parse_array_length", got
        })) if got == input.type_name()),
            "{result:?}"
        );
    }
    let suite: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&paths[3])?)?;
    for case in suite["cases"].as_array().unwrap() {
        let mut schema = suite["schema"].clone();
        match case["required"].as_str() {
            Some("root") => schema["kind"]["required"] = serde_json::json!(["Envelope"]),
            Some("rows") => {
                schema["kind"]["children"][0]["kind"]["required"] = serde_json::json!(["Rows"])
            }
            _ => {}
        }
        let function = case["function"].as_str().unwrap();
        let mut args = vec![
            Value::String(case["input"].as_str().unwrap().into()),
            Value::String(schema.to_string()),
            Value::String(case["array"].to_string()),
        ];
        if function == "json_parse_array_field" {
            args.extend([Value::String(case["path"].to_string()), Value::Int(1)]);
        }
        let result = codegen_runtime::call(function, &args);
        if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(RuntimeError::Function(FunctionError::InvalidArgument { function: actual_function, message: actual })) if actual_function == function && actual == message),
                "{case}: {result:?}"
            );
        } else {
            let expected = match &case["expected"] {
                serde_json::Value::Null => Value::Null,
                serde_json::Value::String(value) => Value::String(value.clone()),
                value => Value::Int(value.as_i64().unwrap()),
            };
            assert_eq!(result?, expected, "{case}");
        }
    }
    println!("Rust array-record host passed");
    Ok(())
}
