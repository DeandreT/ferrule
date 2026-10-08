use super::*;

use mapping::{FunctionId, FunctionParameter, FunctionParameterId, UserFunction};

#[path = "value_maps/null_presence.rs"]
mod null_presence;

struct ValueMapDirectory {
    path: PathBuf,
    complete: bool,
}

impl ValueMapDirectory {
    fn new() -> io::Result<Self> {
        let directory = TempDir::new("value_maps")?;
        let path = directory.0.clone();
        std::mem::forget(directory);
        Ok(Self {
            path,
            complete: false,
        })
    }
}

impl Drop for ValueMapDirectory {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!("Retained ValueMap test artifacts: {}", self.path.display());
        }
    }
}

fn recorded_value_map_command(
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
        serde_json::to_vec(&serde_json::json!({
            "success": output.status.success(),
            "code": output.status.code(),
        }))?,
    )?;
    Ok(output)
}

fn value_map_project() -> Project {
    let fields = [
        "Duplicate",
        "Default",
        "NoDefault",
        "FloatString",
        "Int",
        "Float",
        "Bool",
        "Failed",
        "Null",
        "XmlNil",
        "TypedJsonNull",
        "UntypedJsonNull",
        "TypedXmlNil",
        "UntypedXmlNil",
        "FunctionConstantJsonNull",
        "FunctionParameterJsonNull",
    ];
    let mut nodes = BTreeMap::new();
    let mut bindings = Vec::new();
    let marker_table = vec![
        (Value::Null, Value::String("null-row".into())),
        (Value::json_null(), Value::String("json-null-row".into())),
        (Value::xml_nil(), Value::String("xml-nil-row".into())),
    ];
    let cases = [
        (
            Value::String("same".into()),
            None,
            vec![
                (Value::String("same".into()), Value::String("first".into())),
                (Value::String("same".into()), Value::String("second".into())),
            ],
            Some(Value::String("unused".into())),
        ),
        (
            Value::String("missing".into()),
            None,
            vec![(Value::String("known".into()), Value::String("known".into()))],
            Some(Value::String("fallback".into())),
        ),
        (Value::String("missing".into()), None, Vec::new(), None),
        (
            Value::Float(1e20),
            Some(ScalarType::String),
            vec![(
                Value::String("100000000000000000000".into()),
                Value::String("float-string".into()),
            )],
            None,
        ),
        (
            Value::String(" 1 ".into()),
            Some(ScalarType::Int),
            vec![(Value::Int(1), Value::String("int".into()))],
            None,
        ),
        (
            Value::String(" 1 ".into()),
            Some(ScalarType::Float),
            vec![(Value::Float(1.0), Value::String("float".into()))],
            None,
        ),
        (
            Value::String(" 1 ".into()),
            Some(ScalarType::Bool),
            vec![(Value::Bool(true), Value::String("bool".into()))],
            None,
        ),
        (
            Value::String("1.0".into()),
            Some(ScalarType::Int),
            vec![(
                Value::String("1.0".into()),
                Value::String("retained".into()),
            )],
            None,
        ),
        (
            Value::Null,
            Some(ScalarType::Bool),
            vec![(Value::Null, Value::String("null".into()))],
            None,
        ),
        (
            Value::xml_nil(),
            Some(ScalarType::Float),
            vec![(Value::xml_nil(), Value::String("xml-nil".into()))],
            None,
        ),
        (
            Value::json_null(),
            Some(ScalarType::String),
            marker_table.clone(),
            Some(Value::String("marker-default".into())),
        ),
        (
            Value::json_null(),
            None,
            marker_table.clone(),
            Some(Value::String("marker-default".into())),
        ),
        (
            Value::xml_nil(),
            Some(ScalarType::String),
            marker_table.clone(),
            Some(Value::String("marker-default".into())),
        ),
        (
            Value::xml_nil(),
            None,
            marker_table,
            Some(Value::String("marker-default".into())),
        ),
    ];
    for (index, (input, input_type, table, default)) in cases.into_iter().enumerate() {
        let input_id = index as u32 * 2 + 1;
        let map_id = input_id + 1;
        nodes.insert(input_id, Node::Const { value: input });
        nodes.insert(
            map_id,
            Node::ValueMap {
                input: input_id,
                input_type,
                table,
                default,
            },
        );
        bindings.push(Binding {
            target_field: fields[index].into(),
            node: map_id,
        });
    }
    let parameter = FunctionParameterId::new(1);
    let function = |name: &str, input, parameters| UserFunction {
        library: "value_map_tests".into(),
        name: name.into(),
        description: None,
        parameters,
        output_name: "result".into(),
        output_type: ScalarType::String,
        body: Graph {
            nodes: BTreeMap::from([
                (1, input),
                (
                    2,
                    Node::ValueMap {
                        input: 1,
                        input_type: Some(ScalarType::String),
                        table: vec![
                            (Value::Null, Value::String("null-row".into())),
                            (Value::json_null(), Value::String("json-null-row".into())),
                            (Value::xml_nil(), Value::String("xml-nil-row".into())),
                        ],
                        default: Some(Value::String("marker-default".into())),
                    },
                ),
            ]),
        },
        output: 2,
    };
    let constant_function = FunctionId::new(1);
    let parameter_function = FunctionId::new(2);
    nodes.insert(
        29,
        Node::UserFunctionCall {
            function: constant_function,
            args: Vec::new(),
        },
    );
    nodes.insert(
        30,
        Node::UserFunctionCall {
            function: parameter_function,
            args: vec![21],
        },
    );
    bindings.extend([
        Binding {
            target_field: "FunctionConstantJsonNull".into(),
            node: 29,
        },
        Binding {
            target_field: "FunctionParameterJsonNull".into(),
            node: 30,
        },
    ]);
    null_presence::append_controls(&mut nodes, &mut bindings);
    let user_functions = BTreeMap::from([
        (
            constant_function,
            function(
                "constant_json_null",
                Node::Const {
                    value: Value::json_null(),
                },
                Vec::new(),
            ),
        ),
        (
            parameter_function,
            function(
                "parameter_json_null",
                Node::FunctionParameter { parameter },
                vec![FunctionParameter {
                    id: parameter,
                    name: "value".into(),
                    ty: ScalarType::String,
                }],
            ),
        ),
    ]);
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            fields
                .into_iter()
                .chain(null_presence::FIELDS)
                .map(string)
                .collect(),
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions,
        graph: Graph { nodes },
        root: Scope {
            bindings,
            ..Scope::default()
        },
    }
}

fn expected_output() -> Instance {
    let mut fields: Vec<(String, Instance)> = vec![
        (
            "Duplicate".into(),
            Instance::Scalar(Value::String("first".into())),
        ),
        (
            "Default".into(),
            Instance::Scalar(Value::String("fallback".into())),
        ),
        ("NoDefault".into(), Instance::Scalar(Value::Null)),
        (
            "FloatString".into(),
            Instance::Scalar(Value::String("float-string".into())),
        ),
        ("Int".into(), Instance::Scalar(Value::String("int".into()))),
        (
            "Float".into(),
            Instance::Scalar(Value::String("float".into())),
        ),
        (
            "Bool".into(),
            Instance::Scalar(Value::String("bool".into())),
        ),
        (
            "Failed".into(),
            Instance::Scalar(Value::String("retained".into())),
        ),
        (
            "Null".into(),
            Instance::Scalar(Value::String("null".into())),
        ),
        (
            "XmlNil".into(),
            Instance::Scalar(Value::String("xml-nil".into())),
        ),
        (
            "TypedJsonNull".into(),
            Instance::Scalar(Value::String("null-row".into())),
        ),
        (
            "UntypedJsonNull".into(),
            Instance::Scalar(Value::String("json-null-row".into())),
        ),
        (
            "TypedXmlNil".into(),
            Instance::Scalar(Value::String("xml-nil-row".into())),
        ),
        (
            "UntypedXmlNil".into(),
            Instance::Scalar(Value::String("xml-nil-row".into())),
        ),
        (
            "FunctionConstantJsonNull".into(),
            Instance::Scalar(Value::String("json-null-row".into())),
        ),
        (
            "FunctionParameterJsonNull".into(),
            Instance::Scalar(Value::String("json-null-row".into())),
        ),
    ];
    fields.extend(null_presence::expected());
    Instance::Group(fields.into())
}

#[test]
fn value_maps_match_engine_and_generated_backends() -> TestResult<()> {
    let mut directory = ValueMapDirectory::new()?;
    let project = value_map_project();
    let source = Instance::Group((Vec::new()).into());
    let project_path = directory.path.join("value-maps.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let native = engine::run(&project, &source);
    std::fs::write(
        directory.path.join("native-outcome.txt"),
        format!("{native:?}\n"),
    )?;
    let native = native?;
    std::fs::write(
        directory.path.join("native-output.json"),
        serde_json::to_vec_pretty(&native)?,
    )?;
    let expected = expected_output();
    std::fs::write(
        directory.path.join("expected-output.json"),
        serde_json::to_vec_pretty(&expected)?,
    )?;
    assert_eq!(native, expected);

    let rust_output = directory.path.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/value_maps_rust_harness.rs.txt"),
    )?;
    let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(target) => {
            let target = PathBuf::from(target);
            if target.is_absolute() {
                target
            } else {
                std::env::current_dir()?.join(target)
            }
        }
        None => directory.path.join("cargo-target"),
    };
    let mut rust_command = Command::new("cargo");
    rust_command
        .args(["run", "--quiet", "--jobs", "1"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0");
    let rust = recorded_value_map_command(&mut rust_command, &directory.path, "rust")?;
    assert!(
        rust.status.success(),
        "generated Rust value maps failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.path.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
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
        include_str!("fixtures/value_maps_csharp_harness.cs.txt"),
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
    let csharp = recorded_value_map_command(&mut csharp_command, &directory.path, "csharp")?;
    assert!(
        csharp.status.success(),
        "generated C# value maps failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    directory.complete = true;
    Ok(())
}

fn isolated_precision_cases() -> [(i64, &'static str); 10] {
    [
        (9_007_199_254_740_991, "converted"),
        (9_007_199_254_740_992, "converted"),
        (9_007_199_254_740_993, "original"),
        (9_007_199_254_740_994, "converted"),
        (-9_007_199_254_740_991, "converted"),
        (-9_007_199_254_740_992, "converted"),
        (-9_007_199_254_740_993, "original"),
        (i64::MIN, "converted"),
        (i64::MIN + 1, "original"),
        (i64::MAX, "original"),
    ]
}

fn isolated_precision_project() -> Project {
    let parameter = FunctionParameterId::new(1);
    let precision = FunctionId::new(1);
    let nested = FunctionId::new(2);
    let mut table = Vec::new();
    for integer in [
        9_007_199_254_740_991,
        9_007_199_254_740_992,
        9_007_199_254_740_993,
        9_007_199_254_740_994,
        -9_007_199_254_740_991,
        -9_007_199_254_740_992,
        -9_007_199_254_740_993,
        i64::MIN,
        i64::MIN + 1,
        i64::MAX,
    ] {
        table.push((Value::Int(integer), Value::String("original".into())));
        table.push((Value::Int(integer), Value::String("later duplicate".into())));
    }
    for rounded in [
        9_007_199_254_740_991.0,
        9_007_199_254_740_992.0,
        9_007_199_254_740_994.0,
        -9_007_199_254_740_991.0,
        -9_007_199_254_740_992.0,
        -9_223_372_036_854_775_808.0,
        9_223_372_036_854_775_808.0,
    ] {
        table.push((Value::Float(rounded), Value::String("converted".into())));
    }
    let definition = |name: &str, body, parameters, output| UserFunction {
        library: "isolated_value_map_tests".into(),
        name: name.into(),
        description: None,
        parameters,
        output_name: "result".into(),
        output_type: ScalarType::String,
        body: Graph { nodes: body },
        output,
    };
    let parameters = vec![FunctionParameter {
        id: parameter,
        name: "value".into(),
        ty: ScalarType::Int,
    }];
    let mut user_functions = BTreeMap::from([
        (
            precision,
            definition(
                "precision",
                BTreeMap::from([
                    (1, Node::FunctionParameter { parameter }),
                    (
                        2,
                        Node::ValueMap {
                            input: 1,
                            input_type: Some(ScalarType::Float),
                            table: table.clone(),
                            default: Some(Value::String("miss".into())),
                        },
                    ),
                ]),
                parameters.clone(),
                2,
            ),
        ),
        (
            nested,
            definition(
                "nested",
                BTreeMap::from([
                    (1, Node::FunctionParameter { parameter }),
                    (
                        2,
                        Node::UserFunctionCall {
                            function: precision,
                            args: vec![1],
                        },
                    ),
                ]),
                parameters,
                2,
            ),
        ),
    ]);
    user_functions.insert(
        FunctionId::new(50),
        definition(
            "failing_input",
            BTreeMap::from([
                (
                    1,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![1, 2],
                    },
                ),
                (
                    4,
                    Node::ValueMap {
                        input: 3,
                        input_type: Some(ScalarType::Float),
                        table: Vec::new(),
                        default: Some(Value::String("must not hide input failure".into())),
                    },
                ),
            ]),
            Vec::new(),
            4,
        ),
    );
    let mut nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec!["Input".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec!["Selected".into()],
                frame: None,
            },
        ),
        (
            3,
            Node::ValueMap {
                input: 1,
                input_type: Some(ScalarType::Float),
                table,
                default: Some(Value::String("miss".into())),
            },
        ),
        (
            4,
            Node::UserFunctionCall {
                function: precision,
                args: vec![1],
            },
        ),
        (
            5,
            Node::UserFunctionCall {
                function: nested,
                args: vec![1],
            },
        ),
        (
            6,
            Node::SourceField {
                path: vec!["FunctionSelected".into()],
                frame: None,
            },
        ),
        (
            7,
            Node::UserFunctionCall {
                function: FunctionId::new(50),
                args: Vec::new(),
            },
        ),
        (
            8,
            Node::If {
                condition: 6,
                then: 7,
                else_: 94,
            },
        ),
        (
            90,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            91,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            92,
            Node::Call {
                function: "divide".into(),
                args: vec![90, 91],
            },
        ),
        (
            93,
            Node::ValueMap {
                input: 92,
                input_type: Some(ScalarType::Float),
                table: Vec::new(),
                default: Some(Value::String("must not hide input failure".into())),
            },
        ),
        (
            94,
            Node::Const {
                value: Value::String("safe".into()),
            },
        ),
        (
            95,
            Node::If {
                condition: 2,
                then: 93,
                else_: 94,
            },
        ),
    ]);
    let mut bindings = vec![
        Binding {
            target_field: "Main".into(),
            node: 3,
        },
        Binding {
            target_field: "Function".into(),
            node: 4,
        },
        Binding {
            target_field: "Nested".into(),
            node: 5,
        },
        Binding {
            target_field: "Lazy".into(),
            node: 95,
        },
        Binding {
            target_field: "LazyFunction".into(),
            node: 8,
        },
    ];
    let mut fields = vec![
        string("Main"),
        string("Function"),
        string("Nested"),
        string("Lazy"),
        string("LazyFunction"),
    ];
    let controls = [
        (
            "Duplicate",
            Value::Int(2),
            Some(ScalarType::Float),
            vec![
                (Value::Float(2.0), Value::String("first".into())),
                (Value::Float(2.0), Value::String("second".into())),
            ],
            None,
        ),
        (
            "NoDefault",
            Value::Int(1),
            Some(ScalarType::Float),
            Vec::new(),
            None,
        ),
        (
            "NullDefault",
            Value::Int(1),
            Some(ScalarType::Float),
            Vec::new(),
            Some(Value::Null),
        ),
        (
            "EmptyDefault",
            Value::Int(1),
            Some(ScalarType::Float),
            Vec::new(),
            Some(Value::String(String::new())),
        ),
        (
            "Null",
            Value::Null,
            Some(ScalarType::Float),
            vec![(Value::Null, Value::String("null".into()))],
            None,
        ),
        (
            "XmlNil",
            Value::xml_nil(),
            Some(ScalarType::Float),
            vec![(Value::xml_nil(), Value::String("nil".into()))],
            None,
        ),
        (
            "JsonNull",
            Value::json_null(),
            Some(ScalarType::Float),
            vec![
                (Value::Null, Value::String("wrong marker".into())),
                (Value::json_null(), Value::String("json-null".into())),
            ],
            None,
        ),
        (
            "NoConversion",
            Value::Int(1),
            None,
            vec![(Value::Float(1.0), Value::String("wrong tag".into()))],
            Some(Value::String("tagged".into())),
        ),
        (
            "FailedText",
            Value::String("not-a-number".into()),
            Some(ScalarType::Float),
            vec![(
                Value::String("not-a-number".into()),
                Value::String("retained".into()),
            )],
            None,
        ),
        (
            "SignedZero",
            Value::Float(-0.0),
            Some(ScalarType::Float),
            vec![
                (Value::Float(0.0), Value::String("first zero".into())),
                (Value::Float(-0.0), Value::String("second zero".into())),
            ],
            None,
        ),
    ];
    for (index, (name, value, input_type, table, default)) in controls.into_iter().enumerate() {
        let function = FunctionId::new(index as u64 + 3);
        user_functions.insert(
            function,
            definition(
                name,
                BTreeMap::from([
                    (1, Node::Const { value }),
                    (
                        2,
                        Node::ValueMap {
                            input: 1,
                            input_type,
                            table,
                            default,
                        },
                    ),
                ]),
                Vec::new(),
                2,
            ),
        );
        let node = index as u32 + 10;
        nodes.insert(
            node,
            Node::UserFunctionCall {
                function,
                args: Vec::new(),
            },
        );
        bindings.push(Binding {
            target_field: name.into(),
            node,
        });
        fields.push(string(name));
    }
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Input", ScalarType::Int),
                SchemaNode::scalar("Selected", ScalarType::Bool),
                SchemaNode::scalar("FunctionSelected", ScalarType::Bool),
            ],
        ),
        target: SchemaNode::group("Target", fields),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions,
        graph: Graph { nodes },
        root: Scope {
            bindings,
            ..Scope::default()
        },
    }
}

fn isolated_precision_source(integer: i64, selected: bool, function_selected: bool) -> Instance {
    Instance::Group(
        vec![
            ("Input".into(), Instance::Scalar(Value::Int(integer))),
            ("Selected".into(), Instance::Scalar(Value::Bool(selected))),
            (
                "FunctionSelected".into(),
                Instance::Scalar(Value::Bool(function_selected)),
            ),
        ]
        .into(),
    )
}

fn isolated_precision_expected(marker: &str) -> Instance {
    Instance::Group(
        vec![
            (
                "Main".into(),
                Instance::Scalar(Value::String("converted".into())),
            ),
            (
                "Function".into(),
                Instance::Scalar(Value::String(marker.into())),
            ),
            (
                "Nested".into(),
                Instance::Scalar(Value::String(marker.into())),
            ),
            (
                "Lazy".into(),
                Instance::Scalar(Value::String("safe".into())),
            ),
            (
                "LazyFunction".into(),
                Instance::Scalar(Value::String("safe".into())),
            ),
            (
                "Duplicate".into(),
                Instance::Scalar(Value::String("first".into())),
            ),
            ("NoDefault".into(), Instance::Scalar(Value::Null)),
            ("NullDefault".into(), Instance::Scalar(Value::Null)),
            (
                "EmptyDefault".into(),
                Instance::Scalar(Value::String(String::new())),
            ),
            (
                "Null".into(),
                Instance::Scalar(Value::String("null".into())),
            ),
            (
                "XmlNil".into(),
                Instance::Scalar(Value::String("nil".into())),
            ),
            (
                "JsonNull".into(),
                Instance::Scalar(Value::String("json-null".into())),
            ),
            (
                "NoConversion".into(),
                Instance::Scalar(Value::String("tagged".into())),
            ),
            (
                "FailedText".into(),
                Instance::Scalar(Value::String("retained".into())),
            ),
            (
                "SignedZero".into(),
                Instance::Scalar(Value::String("first zero".into())),
            ),
        ]
        .into(),
    )
}

#[test]
fn isolated_value_map_precision_matches_engine_and_public_generated_hosts() -> TestResult<()> {
    let mut directory = ValueMapDirectory::new()?;
    let project = isolated_precision_project();
    let project_path = directory.path.join("isolated-value-maps.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let issues = engine::validate(&project);
    std::fs::write(
        directory.path.join("native-validation.txt"),
        format!("{issues:#?}\n"),
    )?;
    assert!(
        issues.is_empty(),
        "precision fixture must be valid: {issues:?}"
    );
    for (index, (integer, marker)) in isolated_precision_cases().into_iter().enumerate() {
        let source = isolated_precision_source(integer, false, false);
        let expected = isolated_precision_expected(marker);
        std::fs::write(
            directory.path.join(format!("case-{index}-source.json")),
            serde_json::to_vec_pretty(&source)?,
        )?;
        std::fs::write(
            directory
                .path
                .join(format!("case-{index}-literal-expected.json")),
            serde_json::to_vec_pretty(&expected)?,
        )?;
        let native = engine::run(&project, &source);
        std::fs::write(
            directory
                .path
                .join(format!("case-{index}-native-outcome.txt")),
            format!("{native:?}\n"),
        )?;
        assert_eq!(native?, expected, "integer {integer}");
    }
    for (name, selected, function_selected) in [("main", true, false), ("function", false, true)] {
        let source = isolated_precision_source(9_007_199_254_740_993, selected, function_selected);
        std::fs::write(
            directory.path.join(format!("{name}-selected-source.json")),
            serde_json::to_vec_pretty(&source)?,
        )?;
        let result = engine::run(&project, &source);
        std::fs::write(
            directory
                .path
                .join(format!("{name}-selected-native-outcome.txt")),
            format!("{result:?}\n"),
        )?;
        if function_selected {
            assert!(
                matches!(result, Err(engine::EngineError::UserFunctionBuiltin {
                function, node: 3, source: codegen_runtime::FunctionError::DivideByZero,
            }) if function == FunctionId::new(50))
            );
        } else {
            assert!(matches!(
                result,
                Err(engine::EngineError::Function(
                    codegen_runtime::FunctionError::DivideByZero
                ))
            ));
        }
    }

    let rust_output = directory.path.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    let generated = generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    );
    std::fs::write(
        directory.path.join("rust-generation-outcome.txt"),
        format!("{generated:#?}\n"),
    )?;
    generated?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/value_maps_isolated_rust_harness.rs.txt"),
    )?;
    let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(path) => PathBuf::from(path),
        None => directory.path.join("cargo-target"),
    };
    let mut rust_command = Command::new("cargo");
    rust_command
        .args(["run", "--quiet", "--jobs", "1"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0");
    let rust = recorded_value_map_command(&mut rust_command, &directory.path, "rust-isolated")?;
    assert!(
        rust.status.success(),
        "generated Rust isolated value maps failed: {rust:?}"
    );

    let csharp_output = directory.path.join("csharp");
    let generated = generate_project(&project_path, &csharp_output, GenerateTarget::CSharp);
    std::fs::write(
        directory.path.join("csharp-generation-outcome.txt"),
        format!("{generated:#?}\n"),
    )?;
    generated?;
    let runtime_source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runtime/csharp/Ferrule.Runtime/FerruleValueMaps.cs");
    let expected_runtime = std::fs::read(runtime_source)?;
    std::fs::write(
        directory.path.join("expected-csharp-value-maps-runtime.cs"),
        &expected_runtime,
    )?;
    let emitted_runtime = std::fs::read(csharp_output.join("Runtime/FerruleValueMaps.cs"))?;
    assert_eq!(
        emitted_runtime, expected_runtime,
        "fresh emitted runtime must match canonical source"
    );
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/value_maps_isolated_csharp_harness.cs.txt"),
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
    let csharp =
        recorded_value_map_command(&mut csharp_command, &directory.path, "csharp-isolated")?;
    assert!(
        csharp.status.success(),
        "generated C# isolated value maps failed: {csharp:?}"
    );
    directory.complete = true;
    Ok(())
}
