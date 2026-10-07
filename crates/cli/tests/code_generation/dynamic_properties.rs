use super::*;
use mapping::DynamicBinding;

struct RetainedDirectory {
    path: PathBuf,
    complete: bool,
}

impl RetainedDirectory {
    fn new(role: &str) -> TestResult<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_dynamic_properties_{role}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            complete: false,
        })
    }
}

impl Drop for RetainedDirectory {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "dynamic-property originals retained at {}",
                self.path.display()
            );
        }
    }
}

fn nullable_string(name: &str) -> SchemaNode {
    string(name)
        .nullable()
        .expect("string accepts explicit JSON null")
}

fn frontier_project() -> Project {
    let key = |name: &str| {
        SchemaNode::scalar_union(
            name,
            ir::ScalarTypeSet::new([ScalarType::String, ScalarType::Int, ScalarType::Bool])
                .unwrap(),
        )
        .nullable()
        .unwrap()
    };
    let open = |schema: SchemaNode| schema.with_dynamic_fields(nullable_string("*")).unwrap();
    let names = ir::JsonPropertyNameConstraints::schema_with_exclusions(
        None,
        Some(ir::JsonPropertyNameSet::new(["forbidden".into()]).unwrap()),
        None,
        None,
        None,
        ir::JsonFormatAnnotations::default(),
    )
    .unwrap();
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: None,
    };
    let constant = |value| Node::Const { value };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                key("FirstKey"),
                key("SecondKey"),
                nullable_string("FirstValue"),
                nullable_string("SecondValue"),
                bool_("FirstFail"),
                bool_("SecondFail"),
            ],
        ),
        target: open(SchemaNode::group(
            "Output",
            vec![
                string("Bound"),
                string("Reserved"),
                open(SchemaNode::group("Nested", vec![string("Bound")])),
            ],
        ))
        .with_json_property_names(names)
        .unwrap(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (1, constant(Value::Int(1))),
                (2, constant(Value::Int(0))),
                (3, constant(Value::Bool(false))),
                (
                    90,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![1, 2],
                    },
                ),
                (21, field("FirstKey")),
                (41, field("FirstValue")),
                (5, field("SecondKey")),
                (42, field("SecondValue")),
                (87, field("FirstFail")),
                (86, field("SecondFail")),
                (
                    23,
                    Node::If {
                        condition: 3,
                        then: 90,
                        else_: 21,
                    },
                ),
                (
                    22,
                    Node::If {
                        condition: 87,
                        then: 90,
                        else_: 41,
                    },
                ),
                (
                    6,
                    Node::If {
                        condition: 86,
                        then: 90,
                        else_: 42,
                    },
                ),
                (30, constant(Value::String("fixed".into()))),
                (31, constant(Value::String("child-fixed".into()))),
                (32, constant(Value::String("child-default".into()))),
                (17, constant(Value::String("child".into()))),
                (
                    18,
                    Node::RuntimeParameterDefault {
                        name: "Child".into(),
                        ty: ScalarType::String,
                        default: 32,
                        preview: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Bound".into(),
                node: 30,
            }],
            dynamic_bindings: vec![
                DynamicBinding { key: 23, value: 22 },
                DynamicBinding { key: 5, value: 6 },
            ],
            children: vec![Scope {
                target_field: "Nested".into(),
                bindings: vec![Binding {
                    target_field: "Bound".into(),
                    node: 31,
                }],
                dynamic_bindings: vec![DynamicBinding { key: 17, value: 18 }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}
fn source(
    first: Value,
    first_value: Value,
    second: Value,
    second_value: Value,
    first_fail: bool,
    second_fail: bool,
) -> Instance {
    Instance::Group(
        vec![
            ("FirstKey".into(), scalar(first)),
            ("SecondKey".into(), scalar(second)),
            ("FirstValue".into(), scalar(first_value)),
            ("SecondValue".into(), scalar(second_value)),
            ("FirstFail".into(), scalar(Value::Bool(first_fail))),
            ("SecondFail".into(), scalar(Value::Bool(second_fail))),
        ]
        .into(),
    )
}
fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn mapped(
    first: &str,
    first_value: Value,
    second: &str,
    second_value: Value,
    child: &str,
) -> Instance {
    Instance::Group(
        vec![
            ("Bound".into(), scalar(text("fixed"))),
            (first.into(), scalar(first_value)),
            (second.into(), scalar(second_value)),
            (
                "Nested".into(),
                Instance::Group(
                    vec![
                        ("Bound".into(), scalar(text("child-fixed"))),
                        ("child".into(), scalar(text(child))),
                    ]
                    .into(),
                ),
            ),
        ]
        .into(),
    )
}

fn wire(instance: &Instance) -> serde_json::Value {
    use serde_json::json;
    match instance {
        Instance::Scalar(value) => match value {
            Value::Null => json!({"kind":"null"}),
            Value::JsonNull(_) => json!({"kind":"json-null"}),
            Value::XmlNil(_) => json!({"kind":"xml-nil"}),
            Value::Bool(value) => json!({"kind":"bool", "value":value}),
            Value::Int(value) => json!({"kind":"int", "value":value}),
            Value::String(value) => json!({"kind":"string", "value":value}),
            Value::Float(_) => panic!("frontier fixture has no float"),
        },
        Instance::Group(fields) => {
            json!({"kind":"group", "fields": fields.iter().map(|(name,value)|
            json!({"name":name,"value":wire(value)})).collect::<Vec<_>>() })
        }
        _ => panic!("frontier fixture has no repeated or document instances"),
    }
}

fn expected_json(instance: &Instance) -> serde_json::Value {
    match instance {
        Instance::Scalar(Value::String(value)) => value.clone().into(),
        Instance::Scalar(Value::JsonNull(_)) => serde_json::Value::Null,
        Instance::Group(fields) => serde_json::Value::Object(
            fields
                .iter()
                .filter(|(_, value)| !matches!(value, Instance::Scalar(Value::Null)))
                .map(|(name, value)| (name.clone(), expected_json(value)))
                .collect(),
        ),
        _ => panic!("only independently authored output domains appear here"),
    }
}
fn assert_json_order(actual: &serde_json::Value, wanted: &serde_json::Value) {
    match (actual, wanted) {
        (serde_json::Value::Object(actual), serde_json::Value::Object(wanted)) => {
            assert_eq!(
                actual.keys().collect::<Vec<_>>(),
                wanted.keys().collect::<Vec<_>>()
            );
            for (name, value) in wanted {
                assert_json_order(&actual[name], value);
            }
        }
        _ => assert_eq!(actual, wanted),
    }
}
fn runtime_error(error: &engine::EngineError) -> serde_json::Value {
    use serde_json::json;
    match error {
        engine::EngineError::DynamicPropertyName { node, found } => {
            json!({"kind":"name","node":node,"found":found})
        }
        engine::EngineError::DuplicateDynamicProperty(name) => {
            json!({"kind":"duplicate","name":name})
        }
        engine::EngineError::Function(codegen_runtime::FunctionError::DivideByZero) => {
            json!({"kind":"divide"})
        }
        _ => panic!("unexpected native error: {error:?}"),
    }
}

struct Case {
    name: &'static str,
    source: Instance,
    error: Option<serde_json::Value>,
    output: Option<(String, Value, String, Value)>,
    input: Option<Vec<u8>>,
    json: bool,
    text: bool,
    output_refusal: bool,
}
fn cases() -> Vec<Case> {
    use serde_json::json;
    let mut cases = Vec::new();
    for (name, first, fv, second, sv, refusal) in [
        (
            "ordered_unicode_empty",
            "名🙂",
            text("one"),
            "",
            text("two"),
            false,
        ),
        ("ordinal_case", "A", text("one"), "a", text("two"), false),
        (
            "absent_and_explicit_null",
            "A",
            Value::Null,
            "B",
            Value::json_null(),
            false,
        ),
        (
            "output_name_refusal",
            "forbidden",
            text("one"),
            "safe",
            text("two"),
            true,
        ),
    ] {
        cases.push(Case {
            name,
            source: source(
                text(first),
                fv.clone(),
                text(second),
                sv.clone(),
                false,
                false,
            ),
            error: None,
            output: Some((first.into(), fv, second.into(), sv)),
            input: None,
            json: true,
            text: true,
            output_refusal: refusal,
        });
    }
    for (name, key, found, json) in [
        ("name_before_value_int", Value::Int(7), "int", true),
        ("name_before_value_bool", Value::Bool(true), "bool", true),
        ("name_before_value_absent", Value::Null, "null", true),
        (
            "name_before_value_json_null",
            Value::json_null(),
            "json null",
            true,
        ),
        (
            "name_before_value_xml_nil",
            Value::xml_nil(),
            "xml nil",
            false,
        ),
    ] {
        cases.push(Case {
            name,
            source: source(key, text("one"), text("B"), text("two"), true, false),
            error: Some(json!({"kind":"name","node":23,"found":found})),
            output: None,
            input: None,
            json,
            text: true,
            output_refusal: false,
        });
    }
    for (name, first, fv, second, first_fail, second_fail, error) in [
        (
            "valid_name_value_failure",
            "A",
            text("one"),
            "B",
            true,
            false,
            json!({"kind":"divide"}),
        ),
        (
            "null_reserved_value_before_duplicate",
            "same",
            Value::Null,
            "same",
            false,
            true,
            json!({"kind":"divide"}),
        ),
        (
            "null_reserved_duplicate",
            "same",
            Value::Null,
            "same",
            false,
            false,
            json!({"kind":"duplicate","name":"same"}),
        ),
        (
            "unbound_fixed_value_before_collision",
            "A",
            text("one"),
            "Reserved",
            false,
            true,
            json!({"kind":"divide"}),
        ),
        (
            "unbound_fixed_collision",
            "A",
            text("one"),
            "Reserved",
            false,
            false,
            json!({"kind":"duplicate","name":"Reserved"}),
        ),
        (
            "future_child_value_before_collision",
            "A",
            text("one"),
            "Nested",
            false,
            true,
            json!({"kind":"divide"}),
        ),
        (
            "future_child_collision",
            "A",
            text("one"),
            "Nested",
            false,
            false,
            json!({"kind":"duplicate","name":"Nested"}),
        ),
        (
            "mapping_before_output_constraint",
            "forbidden",
            text("one"),
            "safe",
            false,
            true,
            json!({"kind":"divide"}),
        ),
    ] {
        cases.push(Case {
            name,
            source: source(
                text(first),
                fv,
                text(second),
                text("two"),
                first_fail,
                second_fail,
            ),
            error: Some(error),
            output: None,
            input: None,
            json: true,
            text: true,
            output_refusal: false,
        });
    }
    for (name, input, text_api) in [
        ("malformed_before_mapping", vec![b'{'], true),
        ("utf8_before_mapping", vec![0xff], false),
    ] {
        cases.push(Case {
            name,
            source: source(text("A"), text("one"), text("B"), text("two"), true, false),
            error: Some(json!({"kind":"divide"})),
            output: None,
            input: Some(input),
            json: true,
            text: text_api,
            output_refusal: false,
        });
    }
    cases
}

fn prepare(directory: &Path, project: &Project) -> TestResult<()> {
    use serde_json::json;
    std::fs::write(
        directory.join("project.json"),
        serde_json::to_vec_pretty(project)?,
    )?;
    let validation = engine::validate(project);
    std::fs::write(
        directory.join("native-validation.txt"),
        format!("{validation:#?}"),
    )?;
    assert!(validation.is_empty());
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("Child", text("context-child"))?;
    let context = engine::ExecutionContext::new(Path::new("dynamic-properties.json"))
        .with_parameters(&parameters);
    std::fs::write(
        directory.join("native-context.json"),
        br#"{"mapping":"dynamic-properties.json","Child":"context-child"}"#,
    )?;
    let mut fixtures = Vec::new();
    for case in cases() {
        let owned = directory.join(case.name);
        std::fs::create_dir(&owned)?;
        std::fs::write(
            owned.join("typed-input.json"),
            serde_json::to_vec_pretty(&wire(&case.source))?,
        )?;
        std::fs::write(
            owned.join("authored-case.json"),
            serde_json::to_vec_pretty(&json!({
                "name":case.name,"source":wire(&case.source),"runtime_error":case.error,
                "output":case.output,"raw_input":case.input,"json":case.json,"text":case.text,
                "output_refusal":case.output_refusal
            }))?,
        )?;
        let mut expected = Vec::new();
        for (label, child, execution) in [
            ("default", "child-default", None),
            ("context", "context-child", Some(&context)),
        ] {
            let actual = match execution {
                Some(context) => engine::run_with_context(project, &case.source, context),
                None => engine::run(project, &case.source),
            };
            // Complete native result/error exists before any semantic assertion.
            std::fs::write(
                owned.join(format!("native-{label}.txt")),
                format!("{actual:#?}"),
            )?;
            match (&actual, &case.error, &case.output) {
                (Err(error), Some(wanted), None) => {
                    assert_eq!(&runtime_error(error), wanted, "{} {label}", case.name)
                }
                (Ok(result), None, Some((first, fv, second, sv))) => assert_eq!(
                    result,
                    &mapped(first, fv.clone(), second, sv.clone(), child),
                    "{} {label}",
                    case.name
                ),
                _ => panic!(
                    "{} {label}: unexpected native outcome {actual:?}",
                    case.name
                ),
            }
            let typed = case.output.as_ref().map(|(first, fv, second, sv)| {
                mapped(first, fv.clone(), second, sv.clone(), child)
            });
            if let Ok(result) = &actual {
                let encoded = format_json::to_string(&project.target, result);
                std::fs::write(
                    owned.join(format!("native-json-{label}.txt")),
                    format!("{encoded:#?}"),
                )?;
                if case.output_refusal {
                    assert!(
                        matches!(encoded, Err(format_json::JsonFormatError::InvalidPropertyName { ref object, ref property }) if object == "Output" && property == "forbidden")
                    );
                } else {
                    let bytes = encoded?.into_bytes();
                    std::fs::write(owned.join(format!("native-json-{label}.json")), &bytes)?;
                    assert_json_order(
                        &serde_json::from_slice::<serde_json::Value>(&bytes)?,
                        &expected_json(typed.as_ref().unwrap()),
                    );
                }
            }
            expected.push(json!({"mapped":typed.as_ref().map(wire), "runtime_error":case.error,
                "output_json":typed.as_ref().map(expected_json),
                "boundary_error":if case.input.is_some() { Some("input") } else if case.output_refusal { Some("output") } else { None }}));
        }
        if case.json {
            let input = match &case.input {
                Some(raw) => raw.clone(),
                None => format_json::to_string(&project.source, &case.source)?.into_bytes(),
            };
            std::fs::write(owned.join("input.json"), &input)?;
            let utf8 = std::str::from_utf8(&input);
            std::fs::write(owned.join("native-input-utf8.txt"), format!("{utf8:#?}"))?;
            match utf8 {
                Ok(input) => {
                    let parsed = format_json::from_str(input, &project.source);
                    std::fs::write(owned.join("native-input-parse.txt"), format!("{parsed:#?}"))?;
                    if case.input.is_some() {
                        assert!(parsed.is_err());
                    } else {
                        assert_eq!(parsed?, case.source);
                    }
                }
                Err(_) => assert!(case.input.is_some() && !case.text),
            }
        }
        fixtures.push(json!({"name":case.name,"source":wire(&case.source),"json":case.json,"text":case.text,"expected":expected}));
    }
    std::fs::write(
        directory.join("cases.json"),
        serde_json::to_vec_pretty(&fixtures)?,
    )?;
    Ok(())
}

fn retain_command(directory: &Path, label: &str, command: &mut Command) -> TestResult<Output> {
    std::fs::write(
        directory.join(format!("{label}-invocation.txt")),
        format!("{command:#?}"),
    )?;
    let actual = command.isolated_output();
    match &actual {
        Ok(output) => {
            std::fs::write(
                directory.join(format!("{label}-stdout.bin")),
                &output.stdout,
            )?;
            std::fs::write(
                directory.join(format!("{label}-stderr.bin")),
                &output.stderr,
            )?;
            std::fs::write(
                directory.join(format!("{label}-status.txt")),
                format!("{:?}", output.status),
            )?;
        }
        Err(error) => {
            std::fs::write(
                directory.join(format!("{label}-launch-error.txt")),
                format!("{error:#?}"),
            )?;
        }
    }
    Ok(actual?)
}

fn run_frontier(target: GenerateTarget) -> TestResult<()> {
    let role = match &target {
        GenerateTarget::Rust { .. } => "rust",
        GenerateTarget::CSharp => "csharp",
    };
    let mut directory = RetainedDirectory::new(role)?;
    prepare(&directory.path, &frontier_project())?;
    let generated = directory.path.join("generated");
    let generation = generate_project(&directory.path.join("project.json"), &generated, target);
    std::fs::write(
        directory.path.join("generation.txt"),
        format!("{generation:#?}"),
    )?;
    generation?;
    let mut command = if role == "rust" {
        std::fs::write(
            generated.join("src/main.rs"),
            include_str!("fixtures/dynamic_properties_rust_harness.rs.txt"),
        )?;
        let manifest = generated.join("Cargo.toml");
        let body = std::fs::read_to_string(&manifest)?;
        assert_eq!(body.matches("\n[workspace]\n").count(), 1);
        let body = body.replacen("\n[workspace]\n",
            "\nserde_json = { version = \"1.0\", features = [\"preserve_order\"] }\n\n[workspace]\n", 1);
        std::fs::write(manifest, body)?;
        let mut command = Command::new("cargo");
        command
            .args(["run", "--quiet", "--"])
            .arg(&directory.path)
            .current_dir(&generated)
            .env("CARGO_BUILD_JOBS", "1")
            .env("CARGO_INCREMENTAL", "0");
        if let Some(shared) = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
            command.env("CARGO_TARGET_DIR", shared);
        }
        command
    } else {
        let host = directory.path.join("host");
        std::fs::create_dir(&host)?;
        std::fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/dynamic_properties_csharp_harness.cs.txt"),
        )?;
        std::fs::write(
            host.join("Harness.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
        )?;
        let mut command = dotnet_command(&directory.path);
        command
            .args(["run", "--project"])
            .arg(host.join("Harness.csproj"))
            .arg("--")
            .arg(&directory.path);
        command
    };
    let actual = retain_command(&directory.path, role, &mut command)?;
    assert!(
        actual.status.success(),
        "{role} dynamic-property host failed; originals: {}\n{}\n{}",
        directory.path.display(),
        String::from_utf8_lossy(&actual.stdout),
        String::from_utf8_lossy(&actual.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(
        directory.path.join(format!("{role}-complete.json")),
    )?)?;
    assert_eq!(receipt["cases"], 19);
    assert_eq!(receipt["calls"], 108);
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_open_json_property_dual_failures_preserve_public_api_order() -> TestResult<()> {
    run_frontier(GenerateTarget::Rust {
        runtime_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../codegen-runtime")
            .canonicalize()?,
    })
}

#[test]
fn generated_csharp_open_json_property_dual_failures_preserve_public_api_order() -> TestResult<()> {
    run_frontier(GenerateTarget::CSharp)
}
