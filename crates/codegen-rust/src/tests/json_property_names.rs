use super::*;
use codegen::{DynamicTargetBinding, Expression, ExpressionNode};
use ir::{
    JsonFormatAnnotations, JsonPatternConstraints, JsonPropertyNameConstraints,
    JsonPropertyNameSet, StringLengthRange,
};

fn arbitrary_json(name: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .ok_or("arbitrary JSON metadata is valid on a string scalar")
}

fn property_names(
    allowed: Option<&[&str]>,
    length: Option<(u64, Option<u64>)>,
    patterns: Option<&[&[&str]]>,
    formats: &[&str],
) -> Result<JsonPropertyNameConstraints, &'static str> {
    let allowed = allowed
        .map(|names| JsonPropertyNameSet::new(names.iter().map(|name| (*name).to_string())))
        .transpose()
        .map_err(|_| "test property-name set is bounded")?;
    let length = match length {
        Some((minimum, maximum)) => Some(
            StringLengthRange::new(minimum, maximum)
                .ok_or("test property-name length is constrained and ordered")?,
        ),
        None => None,
    };
    let patterns = patterns
        .map(|alternatives| {
            JsonPatternConstraints::new(
                alternatives
                    .iter()
                    .map(|terms| terms.iter().copied().map(str::to_string)),
            )
        })
        .transpose()
        .map_err(|_| "test property-name patterns are portable and bounded")?;
    let formats = JsonFormatAnnotations::new(formats.iter().map(|format| (*format).to_string()))
        .map_err(|_| "test property-name formats are bounded")?;
    JsonPropertyNameConstraints::schema(allowed, length, patterns, formats)
        .ok_or("test property-name constraints are not tautological")
}

fn constrained_group(
    name: &str,
    children: Vec<SchemaNode>,
    dynamic: SchemaNode,
    constraints: JsonPropertyNameConstraints,
) -> Result<SchemaNode, &'static str> {
    let mut schema = SchemaNode::group(name, children)
        .with_dynamic_fields(dynamic)
        .ok_or("test object accepts dynamic fields")?;
    schema.json_property_names = Some(constraints);
    schema
        .metadata_is_valid()
        .then_some(schema)
        .ok_or("test object property-name constraints are feasible")
}

fn property_name_program() -> Result<Program, &'static str> {
    let root_names = property_names(
        Some(&[
            "",
            "EmptyOnly",
            "Extra",
            "Key",
            "Maybe",
            "Nested",
            "Rows",
            "Value",
        ]),
        Some((0, Some(16))),
        Some(&[&["^$|^[A-Z][A-Za-z]*$"]]),
        &["ferrule-property-name"],
    )?;
    let nested_names = property_names(None, Some((0, Some(8))), Some(&[&["^$|^[a-z]+$"]]), &[])?;
    let named_names = property_names(
        Some(&["", "extra", "named"]),
        None,
        Some(&[&["^$|^[a-z]+$"]]),
        &["named-property"],
    )?;
    let output_names = property_names(
        None,
        Some((0, Some(8))),
        Some(&[&["^$|^[a-z]+$"]]),
        &["output-property"],
    )?;

    let mut maybe = constrained_group(
        "Maybe",
        vec![SchemaNode::scalar("a", ScalarType::Int)],
        SchemaNode::scalar("*", ScalarType::Int),
        nested_names.clone(),
    )?;
    maybe.container_nullable = true;
    let source = constrained_group(
        "Source",
        vec![
            constrained_group(
                "Nested",
                vec![SchemaNode::scalar("known", ScalarType::Int)],
                SchemaNode::scalar("*", ScalarType::Int),
                nested_names.clone(),
            )?,
            constrained_group(
                "Rows",
                vec![SchemaNode::scalar("a", ScalarType::Int)],
                SchemaNode::scalar("*", ScalarType::Int),
                nested_names,
            )?
            .repeating(),
            maybe,
            constrained_group(
                "EmptyOnly",
                Vec::new(),
                arbitrary_json("*")?,
                JsonPropertyNameConstraints::never(),
            )?,
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
        arbitrary_json("*")?,
        root_names,
    )?;
    let target = constrained_group(
        "Target",
        Vec::new(),
        SchemaNode::scalar("*", ScalarType::String),
        output_names,
    )?;

    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Named".into(),
            source: constrained_group(
                "Named",
                vec![SchemaNode::scalar("named", ScalarType::Int)],
                SchemaNode::scalar("*", ScalarType::Int),
                named_names,
            )?,
            dynamic: None,
        }],
        target,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Key".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::DynamicGroup {
                fixed_fields: Vec::new(),
                bindings: vec![DynamicTargetBinding {
                    key: 1,
                    value: 2,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                }],
                children: Vec::new(),
                merge: false,
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

fn pattern_budget_program() -> Result<Program, &'static str> {
    let expensive = property_names(None, None, Some(&[&["^(a?){8000}$"]]), &[])?;
    Ok(Program {
        source: constrained_group("Source", Vec::new(), arbitrary_json("*")?, expensive)?,
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", Vec::new()),
        expressions: Vec::new(),
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn generated_json_boundaries_enforce_property_names_on_actual_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &property_name_program()?,
        &Options {
            package_name: "generated-property-names".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains(r#"\"json_property_names\":{\"kind\":\"schema\""#));
    assert!(generated_source.contains(r#"\"formats\":[\"ferrule-property-name\"]"#));
    assert!(generated_source.contains(r#"\"json_property_names\":{\"kind\":\"never\"}"#));

    let output = TempDir::new("rust_json_property_names_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use generated_property_names::{NamedJsonBytesInput, NamedJsonInput};
use codegen_runtime::JsonBoundaryError;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let named = [NamedJsonInput {
        name: "Named",
        document: r#"{"named":1,"":2}"#,
    }];
    let valid = r#"{
        "Nested":{"known":1,"":2,"extra":3},
        "Rows":[{"a":1,"other":2}],
        "Maybe":null,
        "EmptyOnly":{},
        "Key":"valid",
        "Value":"mapped",
        "Extra":{"retained":true},
        "":"empty root name"
    }"#;
    assert_eq!(
        generated_property_names::execute_json_with_sources(valid, &named)?,
        "{\n  \"valid\": \"mapped\"\n}\n",
    );
    assert_eq!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(r#""valid""#, r#""""#),
            &named,
        )?,
        "{\n  \"\": \"mapped\"\n}\n",
    );

    assert!(matches!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(r#""Extra""#, r#""bad-key""#),
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
    assert!(matches!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(r#""extra":3"#, r#""bad-key":3"#),
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
    assert!(matches!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(r#""other":2"#, r#""bad-key":2"#),
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
    assert!(matches!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(r#""Maybe":null"#, r#""Maybe":{"a":1,"bad-key":2}"#),
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
    assert!(matches!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(r#""EmptyOnly":{}"#, r#""EmptyOnly":{"x":1}"#),
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains('x') && message.contains("property name"),
    ));

    let invalid_named = [NamedJsonInput {
        name: "Named",
        document: r#"{"named":1,"bad-key":2}"#,
    }];
    assert!(matches!(
        generated_property_names::execute_json_with_sources(valid, &invalid_named),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));

    assert!(matches!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(r#""valid""#, r#""bad-key""#),
            &named,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
    assert_eq!(
        generated_property_names::execute_json_with_sources(
            &valid.replace(
                r#""Key":"valid",
        "Value":"mapped""#,
                r#""Key":"bad-key""#,
            ),
            &named,
        )?,
        "{}\n",
    );

    let named_bytes = [NamedJsonBytesInput {
        name: "Named",
        document: br#"{"named":1,"":2}"#,
    }];
    assert_eq!(
        generated_property_names::execute_json_bytes_with_sources(
            valid.as_bytes(),
            &named_bytes,
        )?,
        b"{\n  \"valid\": \"mapped\"\n}\n",
    );
    assert!(matches!(
        generated_property_names::execute_json_bytes_with_sources(
            valid.replace(r#""Extra""#, r#""bad-key""#).as_bytes(),
            &named_bytes,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
    let invalid_named_bytes = [NamedJsonBytesInput {
        name: "Named",
        document: br#"{"named":1,"bad-key":2}"#,
    }];
    assert!(matches!(
        generated_property_names::execute_json_bytes_with_sources(
            valid.as_bytes(),
            &invalid_named_bytes,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
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
        "generated property-name mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}

#[test]
fn generated_property_name_matching_has_a_bounded_work_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &pattern_budget_program()?,
        &Options {
            package_name: "generated-property-name-budget".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let output = TempDir::new("rust_json_property_name_budget_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;

fn main() {
    let key = "a".repeat(8_000);
    let document = format!("{{{key:?}:1}}");
    assert!(matches!(
        generated_property_name_budget::execute_json(&document),
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
        "generated property-name budget mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
