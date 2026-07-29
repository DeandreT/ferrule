use super::*;

fn arbitrary_json(name: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .ok_or("arbitrary JSON metadata is valid on a string scalar")
}

fn open_group(
    name: &str,
    children: Vec<SchemaNode>,
    dynamic: SchemaNode,
) -> Result<SchemaNode, &'static str> {
    SchemaNode::group(name, children)
        .with_dynamic_fields(dynamic)
        .ok_or("dynamic fields are valid on an ordinary group")
}

fn object_openness_program() -> Result<Program, &'static str> {
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Closed",
                vec![
                    SchemaNode::scalar("Id", ScalarType::Int),
                    SchemaNode::group(
                        "Inner",
                        vec![SchemaNode::scalar("Label", ScalarType::String)],
                    ),
                ],
            ),
            open_group(
                "Typed",
                vec![SchemaNode::scalar("Known", ScalarType::String)],
                SchemaNode::scalar("*", ScalarType::Int),
            )?,
            open_group(
                "Any",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
                arbitrary_json("*")?,
            )?,
        ],
    );
    let mut target = source.clone();
    target.name = "Target".into();
    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "NamedOpen".into(),
            source: open_group(
                "NamedOpen",
                Vec::new(),
                SchemaNode::scalar("*", ScalarType::Int),
            )?,
            dynamic: None,
        }],
        target,
        expressions: Vec::new(),
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::CopyCurrentSource,
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn generated_json_boundaries_enforce_and_preserve_object_openness()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &object_openness_program()?,
        &Options {
            package_name: "generated-object-openness".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains(r#"\"dynamic\":{\"name\":\"*\""#));
    assert!(generated_source.contains(r#"\"json_any\":true"#));

    let output = TempDir::new("rust_json_object_openness_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;
use generated_object_openness::NamedJsonInput;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"{
        "Closed":{"Id":7,"Inner":{"Label":"kept"}},
        "Typed":{"Known":"fixed","alpha":1,"beta":2},
        "Any":{
            "Name":"document",
            "object":{"z":1,"a":[true,null]},
            "array":[3,"value",false],
            "nil":null
        }
    }"#;
    let named = [NamedJsonInput {
        name: "NamedOpen",
        document: r#"{"runtimeKey":9}"#,
    }];
    let output = generated_object_openness::execute_json_with_sources(source, &named)?;
    assert_eq!(
        output,
        concat!(
            "{\n",
            "  \"Closed\": {\n",
            "    \"Id\": 7,\n",
            "    \"Inner\": {\n",
            "      \"Label\": \"kept\"\n",
            "    }\n",
            "  },\n",
            "  \"Typed\": {\n",
            "    \"Known\": \"fixed\",\n",
            "    \"alpha\": 1,\n",
            "    \"beta\": 2\n",
            "  },\n",
            "  \"Any\": {\n",
            "    \"Name\": \"document\",\n",
            "    \"object\": {\n",
            "      \"z\": 1,\n",
            "      \"a\": [\n",
            "        true,\n",
            "        null\n",
            "      ]\n",
            "    },\n",
            "    \"array\": [\n",
            "      3,\n",
            "      \"value\",\n",
            "      false\n",
            "    ],\n",
            "    \"nil\": null\n",
            "  }\n",
            "}\n",
        ),
    );

    assert!(matches!(
        generated_object_openness::execute_json_with_sources(
            r#"{
                "Closed":{"Id":7,"Inner":{"Label":"kept"}},
                "Typed":{},"Any":{},"undeclared":true
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("undeclared")
                && message.contains("Source"),
    ));
    assert!(matches!(
        generated_object_openness::execute_json_with_sources(
            r#"{
                "Closed":{"Id":7,"Inner":{"Label":"kept","extra":false}},
                "Typed":{},"Any":{}
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("extra")
                && message.contains("Inner"),
    ));
    assert!(matches!(
        generated_object_openness::execute_json_with_sources(
            r#"{
                "Closed":{"Id":7,"Inner":{"Label":"kept"}},
                "Typed":{"wrong":"not-an-integer"},"Any":{}
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("wrong")
                || message.contains("integer"),
    ));
    let invalid_named = [NamedJsonInput {
        name: "NamedOpen",
        document: r#"{"runtimeKey":"not-an-integer"}"#,
    }];
    assert!(matches!(
        generated_object_openness::execute_json_with_sources(source, &invalid_named),
        Err(JsonBoundaryError::InvalidInput { .. }),
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
        "generated object-openness mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
