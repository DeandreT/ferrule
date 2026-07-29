use super::*;
use codegen::{Expression, ExpressionNode, ScalarTargetDomain};
use ir::ScalarTypeSet;

fn scalar_union(name: &str, types: [ScalarType; 2]) -> (SchemaNode, ScalarTypeSet) {
    let Some(types) = ScalarTypeSet::new(types) else {
        panic!("test union must contain distinct scalar types");
    };
    (SchemaNode::scalar_union(name, types), types)
}

fn union_program() -> Program {
    let (value_source, value_types) = scalar_union("Value", [ScalarType::String, ScalarType::Int]);
    let (value_target, _) = scalar_union("Value", [ScalarType::String, ScalarType::Int]);
    let (number_target, number_types) =
        scalar_union("Number", [ScalarType::Float, ScalarType::Bool]);
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                value_source,
                SchemaNode::scalar("ExactInteger", ScalarType::Int),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", vec![value_target, number_target]),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["ExactInteger".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    expression: 1,
                    target_domain: ScalarTargetDomain::Union(value_types),
                    repeating: false,
                },
                Binding {
                    target_field: "Number".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Union(number_types),
                    repeating: false,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn scalar_root_construction_renders_union_adaptation() {
    let (target, types) = scalar_union("Target", [ScalarType::Float, ScalarType::Bool]);
    let program = Program {
        source: SchemaNode::scalar("Source", ScalarType::Int),
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
                target_domain: ScalarTargetDomain::Union(types),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };
    let artifacts = emit(
        &program,
        &Options {
            package_name: "generated-scalar-union".into(),
            runtime_dependency: RuntimeDependency::Version("0.1.0".into()),
        },
    );
    let Ok(artifacts) = artifacts else {
        panic!("scalar root union emits: {artifacts:?}");
    };
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok());
    let Some(source) = source else {
        panic!("generated scalar root source is present UTF-8");
    };
    assert!(source.contains(
        "let output = scalar(adapt_union_target_value(expression_1(context)?, &[ScalarType::Float, ScalarType::Bool]));"
    ));
}

#[test]
fn generated_union_targets_preserve_tags_and_adapt_exact_numbers() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"));
    let Some(runtime) = runtime else {
        panic!("codegen runtime has a workspace parent");
    };
    let artifacts = emit(
        &union_program(),
        &Options {
            package_name: "generated-union".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    );
    let Ok(artifacts) = artifacts else {
        panic!("scalar union program emits: {artifacts:?}");
    };
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok());
    let Some(generated_source) = generated_source else {
        panic!("generated Rust source is present UTF-8");
    };
    assert!(generated_source.contains(
        "adapt_union_target_value(expression_1(context)?, &[ScalarType::String, ScalarType::Int])"
    ));
    assert!(generated_source.contains(
        "adapt_union_target_value(expression_2(context)?, &[ScalarType::Float, ScalarType::Bool])"
    ));
    assert!(generated_source.contains(r#"\"kind\":\"scalar_union\""#));

    let output = TempDir::new("rust_scalar_union_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::{field, group, integer, scalar, string, Instance, JsonBoundaryError, Value};

fn main() {
    let exact = (1_i64 << 53) + 2;
    let source = group([
        field("Value", scalar(integer(7))),
        field("ExactInteger", scalar(integer(exact))),
    ]);
    let output = match generated_union::execute(&source) {
        Ok(output) => output,
        Err(error) => panic!("generated direct execution failed: {error}"),
    };
    let Instance::Group(fields) = output else {
        panic!("generated output is a group");
    };
    assert_eq!(fields[0].1.as_scalar(), Some(&Value::Int(7)));
    assert_eq!(fields[1].1.as_scalar(), Some(&Value::Float(exact as f64)));

    let json = match generated_union::execute_json(
        r#"{"Value":"external","ExactInteger":9007199254740994}"#,
    ) {
        Ok(output) => output,
        Err(error) => panic!("generated JSON execution failed: {error}"),
    };
    assert_eq!(
        json,
        "{\n  \"Value\": \"external\",\n  \"Number\": 9007199254740994.0\n}\n",
    );
    assert!(matches!(
        generated_union::execute_json(
            r#"{"Value":"external","ExactInteger":9007199254740993}"#,
        ),
        Err(JsonBoundaryError::InvalidOutput { .. }),
    ));

    let string_source = group([
        field("Value", scalar(string("internal"))),
        field("ExactInteger", scalar(integer(2))),
    ]);
    let output = match generated_union::execute(&string_source) {
        Ok(output) => output,
        Err(error) => panic!("generated string execution failed: {error}"),
    };
    let Instance::Group(fields) = output else {
        panic!("generated string output is a group");
    };
    assert_eq!(
        fields[0].1.as_scalar(),
        Some(&Value::String("internal".into())),
    );
}
"##;
    if let Err(error) = fs::write(output.path().join("src/main.rs"), harness) {
        panic!("generated harness is written: {error}");
    }
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output();
    let Ok(run) = run else {
        panic!("generated scalar union cargo run starts");
    };
    assert!(
        run.status.success(),
        "generated scalar union mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
