use codegen_runtime::{ExecutionContext, FunctionError, RuntimeError, RuntimeParameters, Value};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&paths[0])?)?;
    let target = std::fs::read_to_string(&paths[2])?;
    for case in cases {
        let mut parameters = RuntimeParameters::new();
        parameters.insert(
            "LookupText",
            Value::String(case["lookup"].as_str().unwrap().into()),
        )?;
        let wire = case["input"].as_str().unwrap();
        let primary = json_lookup_mapping::parse_x12(wire)?;
        assert_eq!(
            primary,
            json_lookup_mapping::parse_x12_bytes(wire.as_bytes())?
        );
        let before = primary.clone();
        let context = ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
        let result = json_lookup_mapping::execute_with_context(&primary, &context);
        if let Some(message) = case["error"].as_str() {
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
        assert_eq!(primary, before);
    }
    println!("Rust nested-EDI parsed-array lookup host passed");
    Ok(())
}
