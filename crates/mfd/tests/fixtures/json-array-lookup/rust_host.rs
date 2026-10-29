use codegen_runtime::{ExecutionContext, FunctionError, RuntimeError, RuntimeParameters, Value};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&paths[0])?)?;
    let source = std::fs::read_to_string(&paths[1])?;
    let target = std::fs::read_to_string(&paths[2])?;
    for case in cases {
        let mut parameters = RuntimeParameters::new();
        match case["parameter_failure"].as_str() {
            Some("missing") => {}
            Some("null") => parameters.insert("LookupText", Value::Null)?,
            Some("int") => parameters.insert("LookupText", Value::Int(7))?,
            _ => parameters.insert(
                "LookupText",
                Value::String(case["lookup"].as_str().unwrap().into()),
            )?,
        }
        let primary = codegen_runtime::parse_json(&source, &case["primary"].to_string())?;
        let context = ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
        let result = json_lookup_mapping::execute_with_context(&primary, &context);
        if let Some(kind) = case["parameter_failure"].as_str() {
            match kind {
                "missing" => assert!(
                    matches!(result, Err(RuntimeError::MissingRuntimeParameter { name, .. }) if name == "LookupText")
                ),
                _ => assert!(
                    matches!(result, Err(RuntimeError::RuntimeParameterType { name, expected: codegen_runtime::ScalarType::String, found, .. }) if name == "LookupText" && found == kind),
                    "{case}"
                ),
            }
        } else if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(RuntimeError::Function(FunctionError::InvalidArgument {
                function: "json_parse_array_length", message: actual
            })) if actual == message),
                "{case}: {result:?}"
            );
        } else {
            let expected = codegen_runtime::parse_json(&target, &case["expected"].to_string())?;
            assert_eq!(result?, expected, "{case}");
        }
    }
    println!("Rust parsed-array lookup host passed");
    Ok(())
}
