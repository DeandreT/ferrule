use super::*;
use codegen::{Expression, ExpressionNode};
use ir::{JsonMultipleOf, JsonMultipleOfConstraints};

fn multiple_of_scalar(name: &str, ty: ScalarType, divisor: &str) -> SchemaNode {
    let Some(divisor) = JsonMultipleOf::from_decimal_lexical(divisor) else {
        panic!("test multipleOf divisor is valid");
    };
    let Ok(constraints) = JsonMultipleOfConstraints::new([[divisor]]) else {
        panic!("test multipleOf constraints are valid");
    };
    let schema = SchemaNode::scalar(name, ty).with_json_multiple_of(constraints);
    let Some(schema) = schema else {
        panic!("test multipleOf constraints match the numeric schema");
    };
    schema
}

fn multiple_of_program() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                multiple_of_scalar("Quantity", ScalarType::Int, "3"),
                multiple_of_scalar("Fraction", ScalarType::Float, "0.1"),
                multiple_of_scalar("Minimum", ScalarType::Int, "9223372036854775808"),
                multiple_of_scalar("Subnormal", ScalarType::Float, "3e-324"),
                SchemaNode::scalar("Raw", ScalarType::String),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![multiple_of_scalar("Amount", ScalarType::Float, "0.25")],
        ),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Raw".into()],
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Amount".into(),
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::Float),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_json_entry_point_enforces_exact_input_and_normalized_output_multiple_of() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"));
    let Some(runtime) = runtime else {
        panic!("codegen runtime has a workspace parent");
    };
    let artifacts = emit(
        &multiple_of_program(),
        &Options {
            package_name: "generated-multiple-of".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    );
    let Ok(artifacts) = artifacts else {
        panic!("multipleOf program emits: {artifacts:?}");
    };
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok());
    let Some(generated_source) = generated_source else {
        panic!("generated Rust source is present UTF-8");
    };
    assert!(generated_source.contains(r#"\"json_multiple_of\""#));

    let output = TempDir::new("rust_json_multiple_of_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;

fn main() {
    assert_eq!(
        generated_multiple_of::execute_json(
            r#"{"Quantity":6,"Fraction":0.3,"Raw":"1.50"}"#,
        ).as_deref(),
        Ok("{\n  \"Amount\": 1.5\n}\n"),
    );
    assert!(matches!(
        generated_multiple_of::execute_json(
            r#"{"Quantity":7,"Fraction":0.3,"Raw":"1.50"}"#,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("exact multiple"),
    ));
    assert!(matches!(
        generated_multiple_of::execute_json(
            r#"{"Quantity":6,"Fraction":0.30000000000000004,"Raw":"1.50"}"#,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("exact multiple"),
    ));
    assert!(matches!(
        generated_multiple_of::execute_json(
            r#"{"Quantity":6,"Fraction":0.3,"Raw":"1.3"}"#,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("exact multiple"),
    ));
    assert!(generated_multiple_of::execute_json(
        r#"{"Quantity":6,"Fraction":0.3,"Minimum":-9223372036854775808,"Subnormal":3e-323,"Raw":"1.50"}"#,
    ).is_ok());
    assert!(matches!(
        generated_multiple_of::execute_json(
            r#"{"Quantity":6,"Fraction":0.3,"Minimum":9223372036854775807,"Subnormal":3e-323,"Raw":"1.50"}"#,
        ),
        Err(JsonBoundaryError::InvalidInput { .. }),
    ));
    assert!(matches!(
        generated_multiple_of::execute_json(
            r#"{"Quantity":6,"Fraction":0.3,"Minimum":-9223372036854775808,"Subnormal":5e-324,"Raw":"1.50"}"#,
        ),
        Err(JsonBoundaryError::InvalidInput { .. }),
    ));
}
"##;
    if let Err(error) = fs::write(output.path().join("src/main.rs"), harness) {
        panic!("generated multipleOf harness is written: {error}");
    }
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output();
    let Ok(run) = run else {
        panic!("generated multipleOf cargo run starts");
    };
    assert!(
        run.status.success(),
        "generated multipleOf mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
