use super::*;
use codegen::{Expression, ExpressionNode};
use ir::{FiniteF64, JsonAllowedValue, JsonAllowedValues, ScalarTypeSet};

fn allowed_scalar(
    name: &str,
    ty: ScalarType,
    values: impl IntoIterator<Item = JsonAllowedValue>,
) -> SchemaNode {
    let Ok(values) = JsonAllowedValues::new(values) else {
        panic!("test JSON allowed values are valid");
    };
    let Some(schema) = SchemaNode::scalar(name, ty).with_json_allowed_values(values) else {
        panic!("test JSON allowed values match their scalar domain");
    };
    schema
}

fn allowed_values_program() -> Program {
    let Some(number_types) = ScalarTypeSet::new([ScalarType::Int, ScalarType::Float]) else {
        panic!("test scalar union has two types");
    };
    let Some(one_point_five) = FiniteF64::new(1.5) else {
        panic!("test number is finite");
    };
    let Ok(exact_values) = JsonAllowedValues::new([
        JsonAllowedValue::Int(9_007_199_254_740_993),
        JsonAllowedValue::Float(one_point_five),
    ]) else {
        panic!("test numeric allowed values are valid");
    };
    let Some(exact) =
        SchemaNode::scalar_union("Exact", number_types).with_json_allowed_values(exact_values)
    else {
        panic!("test numeric allowed values match their scalar union");
    };

    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                allowed_scalar(
                    "Kind",
                    ScalarType::String,
                    [
                        JsonAllowedValue::String("A".to_string()),
                        JsonAllowedValue::String("B".to_string()),
                    ],
                ),
                exact,
                allowed_scalar(
                    "Optional",
                    ScalarType::String,
                    [
                        JsonAllowedValue::JsonNull,
                        JsonAllowedValue::String("x".to_string()),
                    ],
                ),
                SchemaNode::scalar("Raw", ScalarType::String),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![allowed_scalar(
                "Amount",
                ScalarType::Float,
                [
                    JsonAllowedValue::Int(9_007_199_254_740_993),
                    JsonAllowedValue::Float(one_point_five),
                ],
            )],
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
fn generated_json_entry_point_enforces_exact_allowed_values() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"));
    let Some(runtime) = runtime else {
        panic!("codegen runtime has a workspace parent");
    };
    let artifacts = emit(
        &allowed_values_program(),
        &Options {
            package_name: "generated-allowed-values".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    );
    let Ok(artifacts) = artifacts else {
        panic!("allowed-values program emits: {artifacts:?}");
    };
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok());
    let Some(generated_source) = generated_source else {
        panic!("generated Rust source is present UTF-8");
    };
    assert!(generated_source.contains(r#"\"json_allowed_values\""#));

    let output = TempDir::new("rust_json_allowed_values_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;

fn main() {
    assert_eq!(
        generated_allowed_values::execute_json(
            r#"{"Kind":"A","Exact":9007199254740993,"Optional":null,"Raw":"1.5"}"#,
        ).as_deref(),
        Ok("{\n  \"Amount\": 1.5\n}\n"),
    );
    assert!(generated_allowed_values::execute_json(
        r#"{"Kind":"B","Exact":1.5,"Raw":"1.5"}"#,
    ).is_ok());
    assert!(matches!(
        generated_allowed_values::execute_json(
            r#"{"Kind":"C","Exact":1.5,"Raw":"1.5"}"#,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("allowed values"),
    ));
    assert!(matches!(
        generated_allowed_values::execute_json(
            r#"{"Kind":"A","Exact":9007199254740992,"Raw":"1.5"}"#,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("allowed values"),
    ));
    assert!(matches!(
        generated_allowed_values::execute_json(
            r#"{"Kind":"A","Exact":1.5,"Raw":"9007199254740993"}"#,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("allowed values"),
    ));
}
"##;
    if let Err(error) = fs::write(output.path().join("src/main.rs"), harness) {
        panic!("generated allowed-values harness is written: {error}");
    }
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output();
    let Ok(run) = run else {
        panic!("generated allowed-values cargo run starts");
    };
    assert!(
        run.status.success(),
        "generated allowed-values mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
