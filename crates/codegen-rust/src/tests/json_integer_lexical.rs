use super::*;
use codegen::{Expression, ExpressionNode, ScalarTargetDomain};
use ir::{IntegerRange, JsonMultipleOf, JsonMultipleOfConstraints, NumericRange};

fn program() -> Program {
    let range = IntegerRange::new(Some(6), Some(8)).unwrap();
    let divisor = JsonMultipleOf::from_decimal_lexical("3").unwrap();
    let multiples = JsonMultipleOfConstraints::new([[divisor]]).unwrap();
    let target = SchemaNode::scalar("Target", ScalarType::Int)
        .with_numeric_range(NumericRange::Integer(range))
        .unwrap()
        .with_json_multiple_of(multiples)
        .unwrap();
    Program {
        xml_boundary: None,
        source: SchemaNode::scalar("Source", ScalarType::String),
        extra_sources: Vec::new(),
        target,
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Scalar {
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::Int),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_integer_json_output_normalizes_exact_decimal_strings() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let artifacts = emit(
        &program(),
        &Options {
            package_name: "generated-integer-lexical".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    let output = TempDir::new("rust_integer_lexical_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::{scalar, string, JsonBoundaryError, Value};

fn main() {
    let typed = generated_integer_lexical::execute(&scalar(string("6.000"))).unwrap();
    assert_eq!(typed.as_scalar(), Some(&Value::String("6.000".into())));
    assert_eq!(generated_integer_lexical::execute_json(r#""6.000""#).as_deref(), Ok("6\n"));
    assert_eq!(
        generated_integer_lexical::execute_json_bytes(br#""6.000""#).as_deref(),
        Ok(b"6\n".as_slice()),
    );
    for source in [r#""6.001""#, r#""7.000""#, r#""9.000""#, r#""9223372036854775808.0""#] {
        assert!(matches!(
            generated_integer_lexical::execute_json(source),
            Err(JsonBoundaryError::InvalidOutput { .. }),
        ));
    }
    assert!(matches!(
        generated_integer_lexical::execute_json("6"),
        Err(JsonBoundaryError::InvalidInput { .. }),
    ));
}
"##;
    fs::write(output.path().join("src/main.rs"), harness).unwrap();
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated integer lexical mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
