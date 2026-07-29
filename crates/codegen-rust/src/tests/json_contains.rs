use super::*;
use codegen::{Expression, ExpressionNode};
use ir::{
    FiniteF64, ItemCountRange, JsonContainsConstraint, JsonContainsConstraints,
    JsonContainsPredicate, JsonPatternConstraints, NumberBound, NumberRange, NumericRange,
};

fn contains(
    predicate: SchemaNode,
    minimum: u64,
    maximum: Option<u64>,
) -> Result<JsonContainsConstraints, &'static str> {
    let range = ItemCountRange::new(minimum, maximum).ok_or("test contains interval is ordered")?;
    JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonContainsPredicate::schema(predicate),
        range,
    )])
    .ok_or("test contains constraint is effective and bounded")
}

fn exact_string(name: &str, value: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .with_fixed(value)
        .ok_or("test fixed value matches the string domain")
}

fn exact_strings(
    name: &str,
    value: &str,
    minimum: u64,
    maximum: Option<u64>,
) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .repeating()
        .with_json_contains(contains(
            exact_string("contains item", value)?,
            minimum,
            maximum,
        )?)
        .ok_or("contains metadata belongs to an array")
}

fn exact_number(name: &str, value: f64) -> Result<SchemaNode, &'static str> {
    let value = FiniteF64::new(value).ok_or("test number is finite")?;
    let range = NumberRange::new(
        Some(NumberBound::inclusive(value)),
        Some(NumberBound::inclusive(value)),
    )
    .ok_or("test numeric interval is valid")?;
    SchemaNode::scalar(name, ScalarType::Float)
        .with_numeric_range(NumericRange::Number(range))
        .ok_or("numeric range matches the float predicate")
}

fn scalar_array_scope(source: &str) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: true,
        iteration: Some(IterationPlan::source(vec![source.into()])),
        construction: TargetConstruction::Scalar {
            expression: 1,
            target_domain: ScalarTargetDomain::Single(ScalarType::Float),
        },
        bindings: Vec::new(),
        children: Vec::new(),
    }
}

fn contains_program() -> Result<Program, &'static str> {
    let mut maybe = exact_strings("Maybe", "keep", 1, None)?;
    maybe.container_nullable = true;
    let target = SchemaNode::scalar("Amounts", ScalarType::Float)
        .repeating()
        .with_json_contains(contains(exact_number("contains item", 2.0)?, 1, Some(1))?)
        .ok_or("contains metadata belongs to an array")?;
    let named_target = SchemaNode::scalar("AuditAmounts", ScalarType::Float)
        .repeating()
        .with_json_contains(contains(exact_number("contains item", 3.0)?, 1, Some(1))?)
        .ok_or("contains metadata belongs to a named target array")?;
    Ok(Program {
        source: SchemaNode::group(
            "Source",
            vec![
                exact_strings("Codes", "keep", 2, Some(2))?,
                SchemaNode::group("Nested", vec![exact_strings("Codes", "keep", 1, None)?]),
                SchemaNode::group("Rows", vec![exact_strings("Codes", "keep", 1, None)?])
                    .repeating(),
                maybe,
                SchemaNode::scalar("Raw", ScalarType::String).repeating(),
            ],
        ),
        extra_sources: vec![NamedSourceProgram {
            name: "Named".into(),
            source: exact_strings("Named", "named", 1, Some(1))?,
            dynamic: None,
        }],
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
        root: scalar_array_scope("Raw"),
        extra_targets: vec![NamedTargetProgram {
            name: "Audit".into(),
            target: named_target,
            root: scalar_array_scope("Raw"),
        }],
    })
}

fn pattern_budget_program() -> Result<Program, &'static str> {
    let patterns = JsonPatternConstraints::new([["^(a?){5000}$"]])
        .map_err(|_| "test predicate pattern remains structurally bounded")?;
    let ordinary = SchemaNode::scalar("Ordinary", ScalarType::String)
        .with_json_patterns(patterns.clone())
        .ok_or("pattern matches a string field")?;
    let predicate = SchemaNode::scalar("contains item", ScalarType::String)
        .with_json_patterns(patterns)
        .ok_or("pattern matches a string predicate")?;
    let values = SchemaNode::scalar("Values", ScalarType::String)
        .repeating()
        .with_json_contains(contains(predicate, 1, None)?)
        .ok_or("contains metadata belongs to an array")?;
    Ok(Program {
        source: SchemaNode::group("Source", vec![ordinary, values]),
        target: SchemaNode::scalar("Values", ScalarType::String).repeating(),
        extra_sources: Vec::new(),
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
            repeating: true,
            iteration: Some(IterationPlan::source(vec!["Values".into()])),
            construction: TargetConstruction::Scalar {
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn generated_json_boundaries_enforce_contains_counts() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &contains_program()?,
        &Options {
            package_name: "generated-contains".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains(r#"\"json_contains\":[{\"predicate\":{\"kind\":\"schema\""#));
    assert!(generated_source.contains(r#"\"minimum\":2,\"maximum\":2"#));

    let output = TempDir::new("rust_json_contains_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;
use generated_contains::{NamedJsonBytesInput, NamedJsonInput};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let valid = r#"{
        "Codes":["keep","other","keep"],
        "Nested":{"Codes":["other","keep"]},
        "Rows":[{"Codes":["keep"]},{"Codes":["other","keep"]}],
        "Maybe":null,
        "Raw":["1","2","3"]
    }"#;
    let named = [NamedJsonInput {
        name: "Named",
        document: r#"["other","named"]"#,
    }];
    assert_eq!(
        generated_contains::execute_json_with_sources(valid, &named)?,
        "[\n  1.0,\n  2.0,\n  3.0\n]\n",
    );
    let outputs = generated_contains::execute_json_outputs_with_sources(valid, &named)?;
    assert_eq!(outputs.primary, "[\n  1.0,\n  2.0,\n  3.0\n]\n");
    assert_eq!(outputs.extras.len(), 1);
    assert_eq!(outputs.extras[0].name, "Audit");
    assert_eq!(
        outputs.extras[0].document,
        "[\n  1.0,\n  2.0,\n  3.0\n]\n"
    );

    for invalid in [
        valid.replace(r#""keep","other","keep""#, r#""keep","other""#),
        valid.replace(r#""other","keep"]},"#, r#""other"]},"#),
        valid.replace(r#""Codes":["keep"]},{"#, r#""Codes":["other"]},{"#),
        valid.replace(r#""Maybe":null"#, r#""Maybe":["other"]"#),
    ] {
        assert!(matches!(
            generated_contains::execute_json_with_sources(&invalid, &named),
            Err(JsonBoundaryError::InvalidInput { message })
                if message.contains("matching"),
        ));
    }
    let invalid_named = [NamedJsonInput {
        name: "Named",
        document: r#"["other"]"#,
    }];
    assert!(matches!(
        generated_contains::execute_json_with_sources(valid, &invalid_named),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Named") && message.contains("matching"),
    ));

    for raw in [r#""Raw":["1","3"]"#, r#""Raw":["2","2"]"#] {
        assert!(matches!(
            generated_contains::execute_json_with_sources(
                &valid.replace(r#""Raw":["1","2","3"]"#, raw),
                &named,
            ),
            Err(JsonBoundaryError::InvalidOutput { message })
                if message.contains("Amounts") && message.contains("matching"),
        ));
    }
    assert!(matches!(
        generated_contains::execute_json_outputs_with_sources(
            &valid.replace(r#""Raw":["1","2","3"]"#, r#""Raw":["1","2"]"#),
            &named,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("AuditAmounts") && message.contains("matching"),
    ));

    let named_bytes = [NamedJsonBytesInput {
        name: "Named",
        document: br#"["other","named"]"#,
    }];
    assert_eq!(
        generated_contains::execute_json_bytes_with_sources(valid.as_bytes(), &named_bytes)?,
        b"[\n  1.0,\n  2.0,\n  3.0\n]\n",
    );
    Ok(())
}
"##;
    fs::write(output.path().join("src/main.rs"), harness)?;
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()?;
    assert!(
        run.status.success(),
        "generated contains mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}

#[test]
fn generated_contains_patterns_preserve_typed_work_limit_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &pattern_budget_program()?,
        &Options {
            package_name: "generated-contains-budget".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let output = TempDir::new("rust_json_contains_budget_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;

fn main() {
    let value = "a".repeat(5_000);
    let document = format!("{{\"Ordinary\":{value:?},\"Values\":[{value:?}]}}");
    assert!(matches!(
        generated_contains_budget::execute_json(&document),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("work limit"),
    ));
}
"##;
    fs::write(output.path().join("src/main.rs"), harness)?;
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()?;
    assert!(
        run.status.success(),
        "generated contains budget mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
