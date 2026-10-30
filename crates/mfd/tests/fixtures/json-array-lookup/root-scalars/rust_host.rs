use codegen_runtime::{ExecutionContext, FunctionError, RuntimeError, RuntimeParameters, Value};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&paths[0])?)?;
    let source = std::fs::read_to_string(&paths[1])?;
    let target = std::fs::read_to_string(&paths[2])?;
    for case in cases {
        let mut parameters = RuntimeParameters::new();
        if case["parameter_failure"] != "missing" {
            parameters.insert(
                "LookupText",
                Value::String(case["lookup"].as_str().unwrap().into()),
            )?;
        }
        let primary = codegen_runtime::parse_json(&source, &case["primary"].to_string())?;
        let context = ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
        let result = json_lookup_mapping::execute_with_context(&primary, &context);
        if case["parameter_failure"] == "missing" {
            assert!(
                matches!(result, Err(RuntimeError::MissingRuntimeParameter { name, .. }) if name == "LookupText"),
                "{case}"
            );
        } else if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(RuntimeError::Function(FunctionError::InvalidArgument {
                function, message: actual
            })) if actual == message && case["error_functions"].as_array().unwrap().iter().any(|allowed| allowed.as_str() == Some(function))),
                "{case}: {result:?}"
            );
        } else {
            let expected = codegen_runtime::parse_json(&target, &case["expected"].to_string())?;
            assert_eq!(result?, expected, "{case}");
        }
    }
    println!("Rust root-scalar lookup host passed");
    Ok(())
}
