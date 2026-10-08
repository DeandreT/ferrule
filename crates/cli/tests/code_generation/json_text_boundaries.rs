use super::*;

fn text_boundary_project() -> Project {
    let arbitrary = |name: &str| {
        string(name)
            .json_any()
            .expect("string encodes arbitrary JSON")
    };
    Project {
        source: SchemaNode::group("Source", vec![string("Text"), arbitrary("Any")]),
        target: SchemaNode::group(
            "Target",
            vec![
                string("Text"),
                arbitrary("Any"),
                arbitrary("Parsed"),
                int("Length"),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Any".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "length".into(),
                        args: vec![1],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Text".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Any".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Parsed".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Length".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        },
    }
}

#[test]
fn generated_json_text_boundaries_match_interpreter_in_rust_and_csharp() -> TestResult<()> {
    let project = text_boundary_project();
    assert!(engine::validate(&project).is_empty());
    let valid_inputs = [
        serde_json::json!({"Text": "😀e\u{301}", "Any": {"valid": "😀", "items": [1, true, null]}})
            .to_string(),
        // These are valid graph strings containing invalid encoded JSON. The
        // arbitrary-JSON writer must retain the original string as its fallback.
        serde_json::json!({"Text": r#""\uD800""#, "Any": "valid"}).to_string(),
        serde_json::json!({"Text": r#"{"value":1e400}"#, "Any": 7}).to_string(),
        serde_json::json!({"Text": r#"{"value":"\uDFFF"}"#, "Any": []}).to_string(),
        r#"{"Text":"first","Text":"last","Any":0}"#.into(),
    ];
    let mut cases = Vec::new();
    for input in valid_inputs {
        let source = format_json::from_str(&input, &project.source)?;
        let result = engine::run(&project, &source)?;
        let expected: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &result)?)?;
        cases.push(serde_json::json!({"input": input, "expected": expected}));
    }
    for input in [
        r#"{"Text":"\uD800","Text":"valid","Any":0}"#,
        r#"{"Text":1e400,"Text":"valid","Any":0}"#,
        r#"{"Text":"ok","Any":{"value":"\uDFFF"}}"#,
        r#"{"Text":"ok","Any":1e400}"#,
        r#"{"Text":"ok","Any":-1e400}"#,
        r#"{"Text":"ok","Any":{"value":1e400,"value":1}}"#,
        r#"{"Text":"ok","Any":{"\uD800":1}}"#,
        r#"{"Text":"ok","Any":0,"Unexpected":true}"#,
    ] {
        assert!(
            format_json::from_str(input, &project.source).is_err(),
            "native boundary accepted invalid input: {input}"
        );
        cases.push(serde_json::json!({"input": input, "reject": true}));
    }
    for bytes in [
        b"{\"Text\":\"\xFF\",\"Any\":0}".as_slice(),
        b"{\"Text\":\"\xED\xA0\x80\",\"Any\":0}".as_slice(),
        b"{\"Text\":\"\xF0\x9F\",\"Any\":0}".as_slice(),
    ] {
        cases.push(serde_json::json!({"bytes": bytes, "reject": true}));
    }

    run_generated_boundary_cases(&project, &cases, "json_text_boundaries")
}

#[test]
fn generated_arbitrary_json_canonicalization_matches_interpreter() -> TestResult<()> {
    let mut project = text_boundary_project();
    let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
        unreachable!("test target is a group");
    };
    children.push(string("Encoded"));
    project.root.bindings.push(Binding {
        target_field: "Encoded".into(),
        node: 2,
    });
    let mut inputs = Vec::new();
    for token in numeric_tokens() {
        inputs.push(format!(
            r#"{{"Text":{},"Any":{token}}}"#,
            serde_json::to_string(&token)?
        ));
        inputs.push(format!(
            r#"{{"Text":{},"Any":{{"first":{token},"nested":[{token}],"first":{token}}}}}"#,
            serde_json::to_string(&format!(r#"{{"nested":{token}}}"#))?
        ));
    }
    inputs.push(r#"{"Text":"ok","Any":{"😀":"😀","control":"\u001f","slash":"/","cjk":"中","separator":"\u2028","del":"\u007f"}}"#.into());
    inputs.push(r#"{"Text":"ok","Any":{"first":1e1,"nested":{"x":1,"x":-0},"first":1e-400,"last":18446744073709551615}}"#.into());
    // A malformed number overwritten outside json_any still fails the complete
    // input parse; schema projection must not hide that error.
    inputs.push(r#"{"Text":1.7976931348623158e308,"Text":"ok","Any":0}"#.into());
    // The same token inside an ordinary string uses the output string fallback.
    inputs.push(serde_json::json!({"Text": "1.7976931348623158e308", "Any": 0}).to_string());
    for depth in [126, 127, 128, 129] {
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        inputs.push(format!(r#"{{"Text":"ok","Any":{nested}}}"#));
        inputs.push(serde_json::json!({"Text": nested, "Any": 0}).to_string());
    }
    let cases = interpreter_cases(&project, &inputs)?;
    run_generated_boundary_cases(&project, &cases, "json_canonicalization")
}

#[test]
fn generated_json_field_projection_preserves_large_raw_descriptors() -> TestResult<()> {
    let mut project = text_boundary_project();
    project.source = SchemaNode::group("Source", vec![string("Text")]);
    project.target = SchemaNode::group("Target", vec![int("Value")]);
    // This is a raw graph string, rather than one of the generated host's
    // trusted boundary schemas. Native projection has no descriptor byte cap.
    let descriptor = format!(
        r#"{{"name":"Embedded","padding":"{}","kind":{{"kind":"scalar","ty":"int"}}}}"#,
        "x".repeat(codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
    );
    assert!(descriptor.len() > codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES);
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec!["Text".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(descriptor),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("[]".into()),
            },
        ),
        (
            4,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![1, 2, 3],
            },
        ),
    ]);
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 4,
        }],
        ..Scope::default()
    };
    let inputs = ["7", "-9", "9007199254740993", "0"]
        .map(|text| serde_json::json!({"Text": text}).to_string());
    let cases = interpreter_cases(&project, &inputs)?;
    run_generated_boundary_cases(&project, &cases, "json_projection_descriptor")
}

#[test]
fn generated_json_numeric_domains_match_interpreter() -> TestResult<()> {
    for ty in [ScalarType::Float, ScalarType::Int] {
        let mut project = text_boundary_project();
        project.source = SchemaNode::group("Source", vec![SchemaNode::scalar("Value", ty)]);
        project.target = SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ty), string("Text")],
        );
        project.graph.nodes = BTreeMap::from([(
            1,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            },
        )]);
        project.root.bindings = vec![
            Binding {
                target_field: "Value".into(),
                node: 1,
            },
            Binding {
                target_field: "Text".into(),
                node: 1,
            },
        ];
        let inputs = numeric_tokens()
            .iter()
            .map(|token| format!(r#"{{"Value":{token}}}"#))
            .collect::<Vec<_>>();
        let cases = interpreter_cases(&project, &inputs)?;
        run_generated_boundary_cases(&project, &cases, "json_numeric_domains")?;
    }
    Ok(())
}

#[test]
fn generated_json_object_text_matches_interpreter() -> TestResult<()> {
    let mut project = text_boundary_project();
    project.source = SchemaNode::group("Source", Vec::new());
    project.target = SchemaNode::group(
        "Target",
        vec![string("Unicode"), string("Numbers"), string("Deep")],
    );
    project.graph.nodes.clear();
    project.root.bindings.clear();
    let mut next = 1;
    for (field, entries) in [
        (
            "Unicode",
            vec![(
                vec![String::from("😀")],
                "string",
                Value::String("😀\u{2028}\u{2029}\u{1f}".into()),
            )],
        ),
        (
            "Numbers",
            vec![
                (vec!["large".into()], "number", Value::Float(1e20)),
                (vec!["small".into()], "number", Value::Float(1e-6)),
                (
                    vec!["tiny".into()],
                    "number",
                    Value::Float(f64::from_bits(1)),
                ),
                (vec!["zero".into()], "number", Value::Float(-0.0)),
                (vec!["whole".into()], "number", Value::Float(1.0)),
                (vec!["integer".into()], "integer", Value::Int(i64::MAX)),
            ],
        ),
        (
            "Deep",
            vec![(vec!["nested".into(); 300], "boolean", Value::Bool(true))],
        ),
    ] {
        let mut args = Vec::new();
        for (path, ty, value) in entries {
            for value in [
                Value::String(serde_json::to_string(&path)?),
                Value::String(ty.into()),
                value,
            ] {
                project.graph.nodes.insert(next, Node::Const { value });
                args.push(next);
                next += 1;
            }
        }
        project.graph.nodes.insert(
            next,
            Node::Call {
                function: "json_serialize_object".into(),
                args,
            },
        );
        project.root.bindings.push(Binding {
            target_field: field.into(),
            node: next,
        });
        next += 1;
    }
    let cases = interpreter_cases(&project, &["{}".into()])?;
    run_generated_boundary_cases(&project, &cases, "json_object_text")
}

#[test]
fn generated_json_output_constraints_match_interpreter() -> TestResult<()> {
    for (schema, inputs) in [
        (
            r#"{"name":"Values","repeating":true,"json_unique_items":true,"kind":{"kind":"scalar","ty":"float"}}"#,
            vec![
                ("0.9999999999998891", "0.9999999999998892"),
                ("0.9999999999998892", "0.9999999999998892"),
            ],
        ),
        (
            r#"{"name":"Values","repeating":true,"json_multiple_of":{"any_of":[[{"coefficient":1,"decimal_exponent":-307}]]},"kind":{"kind":"scalar","ty":"float"}}"#,
            vec![("1e-307", "1e-307"), ("1.0000000000000001e-307", "1e-307")],
        ),
    ] {
        let mut project = text_boundary_project();
        project.source = SchemaNode::group("Source", vec![string("X"), string("Y")]);
        project.target = SchemaNode::group("Target", vec![serde_json::from_str(schema)?]);
        project.graph.nodes.clear();
        project.root.bindings.clear();
        for (source, node) in [("X", 1), ("Y", 3)] {
            project.graph.nodes.insert(
                node,
                Node::SourceField {
                    path: vec![source.into()],
                    frame: None,
                },
            );
            project.graph.nodes.insert(
                node + 1,
                Node::Call {
                    function: "to_number".into(),
                    args: vec![node],
                },
            );
            project.root.bindings.push(Binding {
                target_field: "Values".into(),
                node: node + 1,
            });
        }
        let inputs = inputs
            .into_iter()
            .map(|(x, y)| serde_json::json!({"X": x, "Y": y}).to_string())
            .collect::<Vec<_>>();
        let cases = interpreter_cases(&project, &inputs)?;
        assert!(cases.iter().any(|case| case["reject_output"] == true));
        assert!(cases.iter().any(|case| case["exact_output"] == true));
        run_generated_boundary_cases(&project, &cases, "json_output_constraints")?;
    }
    Ok(())
}

#[test]
fn generated_unstable_schema_metadata_matches_native_outputs() -> TestResult<()> {
    let original = include_str!("../../../codegen/src/tests/fixtures/unstable_float_project.json");
    for (project_json, accepted) in [
        (original.to_owned(), true),
        (
            original.replace(r#""value": 1.0000000000000001e-307"#, r#""value": 1e-307"#),
            true,
        ),
        (
            original.replace(r#""value": 1.0000000000000001e-307"#, r#""value": 1e-308"#),
            false,
        ),
    ] {
        let project: Project = serde_json::from_str(&project_json)?;
        let cases = interpreter_cases(&project, &["{}".into()])?;
        assert_eq!(cases[0]["reject_output"] == true, !accepted);
        assert_eq!(cases[0]["exact_output"] == true, accepted);
        // Both adjacent finite values satisfy the exact physical minimum;
        // the smaller control must fail before and after generated emission.
        // Generate from the physical fixture exactly as the CLI receives it.
        run_generated_boundary_cases_json(
            project_json.as_bytes(),
            &cases,
            "unstable_schema_metadata",
        )?;
    }
    Ok(())
}

#[test]
fn generated_unstable_source_schema_matches_native_json_inputs() -> TestResult<()> {
    let project_json = r#"{
        "source": {"name":"Source","kind":{"kind":"group","children":[
            {"name":"Value","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307}}},"kind":{"kind":"scalar","ty":"float"}}
        ]}},
        "target": {"name":"Target","kind":{"kind":"group","children":[
            {"name":"Value","kind":{"kind":"scalar","ty":"float"}}
        ]}},
        "graph":{"nodes":{"0":{"kind":"source_field","path":["Value"]}}},
        "root":{"bindings":[{"target_field":"Value","node":0}]}
    }"#;
    let project: Project = serde_json::from_str(project_json)?;
    let inputs = [
        r#"{"Value":1e-307}"#.into(),
        r#"{"Value":1.0000000000000001e-307}"#.into(),
        r#"{"Value":1e-308}"#.into(),
    ];
    let cases = interpreter_cases(&project, &inputs)?;
    assert_eq!(cases[0]["exact_output"], true);
    assert_eq!(cases[1]["exact_output"], true);
    assert_eq!(cases[2]["reject"], true);
    run_generated_boundary_cases_json(project_json.as_bytes(), &cases, "unstable_source_metadata")
}

#[test]
fn generated_saved_project_preserves_exact_float_constants() -> TestResult<()> {
    let mut project = text_boundary_project();
    project.source = SchemaNode::group("Source", Vec::new());
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar("Value", ScalarType::Float),
            string("Text"),
        ],
    );
    project.graph.nodes = BTreeMap::from([(
        1,
        Node::Const {
            value: Value::Float(f64::from_bits(0x0031_fa18_2c40_c60e)),
        },
    )]);
    project.root.bindings = vec![
        Binding {
            target_field: "Value".into(),
            node: 1,
        },
        Binding {
            target_field: "Text".into(),
            node: 1,
        },
    ];
    let encoded = mapping::project_file::encode_pretty(&project)?;
    let decoded: Project = serde_json::from_str(&encoded)?;
    let Node::Const {
        value: Value::Float(value),
    } = decoded.graph.nodes[&1]
    else {
        panic!("saved constant lost its Float tag");
    };
    assert_eq!(value.to_bits(), 0x0031_fa18_2c40_c60e);
    assert_eq!(mapping::project_file::encode_pretty(&decoded)?, encoded);
    let cases = interpreter_cases(&project, &["{}".into()])?;
    run_generated_boundary_cases_json(encoded.as_bytes(), &cases, "saved_float_constants")
}

#[test]
fn generation_rejects_oversized_embedded_schema_before_publication() -> TestResult<()> {
    let directory = TempDir::new("embedded_schema_publication")?;
    let project_path = directory.0.join("project.json");
    let mut project: Project = serde_json::from_str(include_str!(
        "../../../codegen/src/tests/fixtures/unstable_float_project.json"
    ))?;
    let mut output = string("Out");
    output.fixed = Some("x".repeat(codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES));
    project.target = SchemaNode::group("Target", vec![output]);
    project.graph.nodes = BTreeMap::from([(
        0,
        Node::Const {
            value: Value::String(String::new()),
        },
    )]);
    std::fs::write(&project_path, serde_json::to_vec(&project)?)?;
    for (name, target) in [
        (
            "rust",
            GenerateTarget::Rust {
                runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
            },
        ),
        ("csharp", GenerateTarget::CSharp),
    ] {
        let destination = directory.0.join("unpublished").join(name);
        let error = generate_project(&project_path, &destination, target)
            .expect_err("oversized schema must reject generation");
        let typed = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<codegen::EmbeddedSchemaError>());
        assert!(
            matches!(typed, Some(codegen::EmbeddedSchemaError::TooLarge { schema, bytes, max })
                if schema == "Target"
                    && *bytes > codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES
                    && *max == codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES),
            "{error:?}",
        );
        assert!(!destination.exists());
        assert!(!directory.0.join("unpublished").exists());
    }
    Ok(())
}

fn numeric_tokens() -> Vec<String> {
    let mut tokens: Vec<String> = [
        "0",
        "-0",
        "-0.0",
        "1.0",
        "1e0",
        "1e1",
        "1e-5",
        "1e-6",
        "1e15",
        "1e16",
        "1e-307",
        "0e-309",
        "-0e-309",
        "1e-400",
        "-1e-400",
        "5e-324",
        "2.4703282292062328e-324",
        "1.00000000000000011102230246251565404236316680908203125",
        "9007199254740993",
        "9007199254740993.0",
        "-9223372036854775808",
        "-9223372036854775809",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
        "18446744073709551616",
        "1.7976931348623157e308",
        "1.7976931348623158e308",
        "-1.7976931348623158e308",
        "1e309",
        "-1e309",
        "0e999999999999999999999999999999999999",
        "-0e999999999999999999999999999999999999",
        "1e-999999999999999999999999999999999999",
        "1e999999999999999999999999999999999999",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for sign in ["", "-"] {
        tokens.push(format!("{sign}0.{}1", "0".repeat(4096)));
        tokens.push(format!("{sign}0.{}0", "0".repeat(4096)));
        tokens.push(format!("{sign}0.0e-2147483647"));
    }
    tokens
}

fn interpreter_cases(project: &Project, inputs: &[String]) -> TestResult<Vec<serde_json::Value>> {
    assert!(engine::validate(project).is_empty());
    inputs
        .iter()
        .map(
            |input| match format_json::from_str(input, &project.source) {
                Ok(source) => {
                    let result = engine::run(project, &source)?;
                    match format_json::to_string(&project.target, &result) {
                        Ok(expected) => Ok(serde_json::json!({
                            "input": input,
                            "expected_json": expected,
                            "exact_output": true,
                        })),
                        Err(_) => Ok(serde_json::json!({"input": input, "reject_output": true})),
                    }
                }
                Err(_) => Ok(serde_json::json!({"input": input, "reject": true})),
            },
        )
        .collect()
}

pub(super) fn run_generated_boundary_cases(
    project: &Project,
    cases: &[serde_json::Value],
    name: &str,
) -> TestResult<()> {
    run_generated_boundary_cases_json(
        mapping::project_file::encode_pretty(project)?.as_bytes(),
        cases,
        name,
    )
}

struct JsonBoundaryDirectory {
    path: PathBuf,
    complete: bool,
}

impl JsonBoundaryDirectory {
    fn new(name: &str) -> io::Result<Self> {
        let directory = TempDir::new(name)?;
        let path = directory.0.clone();
        std::mem::forget(directory);
        Ok(Self {
            path,
            complete: false,
        })
    }
}

impl Drop for JsonBoundaryDirectory {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained JSON boundary test artifacts: {}",
                self.path.display()
            );
        }
    }
}

fn recorded_json_boundary_command(
    command: &mut Command,
    directory: &Path,
    name: &str,
) -> io::Result<Output> {
    std::fs::write(
        directory.join(format!("{name}-command.txt")),
        format!("{command:?}\n"),
    )?;
    let output = match command.isolated_output() {
        Ok(output) => output,
        Err(error) => {
            let _ = std::fs::write(
                directory.join(format!("{name}-spawn-error.txt")),
                format!("{error:?}\n"),
            );
            return Err(error);
        }
    };
    std::fs::write(directory.join(format!("{name}-stdout.txt")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.txt")), &output.stderr)?;
    std::fs::write(
        directory.join(format!("{name}-status.json")),
        serde_json::to_vec(
            &serde_json::json!({"success": output.status.success(), "code": output.status.code()}),
        )?,
    )?;
    Ok(output)
}

fn run_generated_boundary_cases_json(
    project_json: &[u8],
    cases: &[serde_json::Value],
    name: &str,
) -> TestResult<()> {
    let mut directory = JsonBoundaryDirectory::new(name)?;
    let project_path = directory.path.join("project.json");
    std::fs::write(&project_path, project_json)?;
    let fixtures = serde_json::to_vec(cases)?;
    let rust_output = directory.path.join("rust");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    let manifest_path = rust_output.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)?;
    std::fs::write(
        manifest_path,
        manifest.replace(
            "\n[workspace]",
            "\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\n\n[workspace]",
        ),
    )?;
    std::fs::write(rust_output.join("cases.json"), &fixtures)?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/json_text_boundaries_rust.rs.txt"),
    )?;
    let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(path.is_absolute(), "generated host target must be absolute");
            path
        }
        None => directory.path.join("cargo-target"),
    };
    let mut rust_command = Command::new("cargo");
    rust_command
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", target)
        .env("RUSTFLAGS", "-Dwarnings");
    let rust = recorded_json_boundary_command(&mut rust_command, &directory.path, "rust")?;
    assert!(
        rust.status.success(),
        "generated Rust text boundaries failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.path.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    std::fs::write(csharp_output.join("cases.json"), fixtures)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/json_text_boundaries_csharp.cs.txt"),
    )?;
    let mut csharp_command = dotnet_command(&csharp_output);
    csharp_command
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output);
    let csharp = recorded_json_boundary_command(&mut csharp_command, &directory.path, "csharp")?;
    assert!(
        csharp.status.success(),
        "generated C# text boundaries failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    directory.complete = true;
    Ok(())
}
