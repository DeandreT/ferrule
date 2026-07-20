use super::*;

#[derive(Clone, Copy)]
enum ErrorMode {
    None,
    Type,
    Arity,
}

struct GraphBuilder {
    next: u32,
    nodes: BTreeMap<u32, Node>,
}

impl GraphBuilder {
    fn new() -> Self {
        Self {
            next: 1,
            nodes: BTreeMap::new(),
        }
    }

    fn insert(&mut self, node: Node) -> u32 {
        let id = self.next;
        self.next += 1;
        self.nodes.insert(id, node);
        id
    }

    fn literal(&mut self, value: Value) -> u32 {
        self.insert(Node::Const { value })
    }

    fn source(&mut self, field: &str) -> u32 {
        self.insert(Node::SourceField {
            path: vec![field.into()],
            frame: None,
        })
    }

    fn call(&mut self, function: &str, args: Vec<u32>) -> u32 {
        self.insert(Node::Call {
            function: function.into(),
            args,
        })
    }

    fn call_values(&mut self, function: &str, values: Vec<Value>) -> u32 {
        let args = values
            .into_iter()
            .map(|value| self.literal(value))
            .collect();
        self.call(function, args)
    }

    fn if_(&mut self, condition: u32, then: u32, else_: u32) -> u32 {
        self.insert(Node::If {
            condition,
            then,
            else_,
        })
    }
}

fn add_group(
    target: &mut Vec<SchemaNode>,
    scopes: &mut Vec<Scope>,
    name: &str,
    fields: Vec<(&str, ScalarType, u32)>,
) {
    target.push(SchemaNode::group(
        name,
        fields
            .iter()
            .map(|(name, ty, _)| SchemaNode::scalar(*name, *ty))
            .collect(),
    ));
    scopes.push(Scope {
        target_field: name.into(),
        bindings: fields
            .into_iter()
            .map(|(target_field, _, node)| Binding {
                target_field: target_field.into(),
                node,
            })
            .collect(),
        ..Scope::default()
    });
}

fn scalar_function_project() -> Project {
    let mut graph = GraphBuilder::new();
    let mut target = Vec::new();
    let mut scopes = Vec::new();

    let concat_empty = graph.call("concat", Vec::new());
    let concat_mixed = graph.call_values(
        "concat",
        vec![
            Value::Null,
            Value::xml_nil(),
            Value::Bool(false),
            Value::Int(-7),
            Value::Float(1.25),
            Value::String("|done".into()),
        ],
    );
    add_group(
        &mut target,
        &mut scopes,
        "Concat",
        vec![
            ("Empty", ScalarType::String, concat_empty),
            ("Mixed", ScalarType::String, concat_mixed),
        ],
    );

    let upper = graph.call_values("upper", vec![Value::String("alpha Beta é".into())]);
    let lower = graph.call_values("lower", vec![Value::String("MIXED É".into())]);
    add_group(
        &mut target,
        &mut scopes,
        "Case",
        vec![
            ("Upper", ScalarType::String, upper),
            ("Lower", ScalarType::String, lower),
        ],
    );

    let grouped = graph.call_values(
        "format_number",
        vec![Value::Float(12345.678), Value::String("#,##0.00".into())],
    );
    let negative = graph.call_values(
        "format_number",
        vec![Value::Float(-12.5), Value::String("0.0;[0.0]".into())],
    );
    let percent = graph.call_values(
        "format_number",
        vec![Value::Float(0.126), Value::String("0.0%".into())],
    );
    let custom = graph.call_values(
        "format_number",
        vec![
            Value::Float(1234.5),
            Value::String("#.##0,00".into()),
            Value::String(",".into()),
            Value::String(".".into()),
        ],
    );
    let exact_integer = graph.call_values(
        "format_number",
        vec![Value::Int(i64::MIN), Value::String("#,##0".into())],
    );
    add_group(
        &mut target,
        &mut scopes,
        "FormatNumber",
        vec![
            ("Grouped", ScalarType::String, grouped),
            ("Negative", ScalarType::String, negative),
            ("Percent", ScalarType::String, percent),
            ("Custom", ScalarType::String, custom),
            ("ExactInteger", ScalarType::String, exact_integer),
        ],
    );

    let normalized = graph.call_values(
        "normalize_space",
        vec![Value::String(
            " \talpha\r\n beta\u{000b}gamma\u{00a0} delta ".into(),
        )],
    );
    let left = graph.call_values(
        "left_trim",
        vec![Value::String(" \t\r\n\u{000b}\u{00a0}left \t".into())],
    );
    let right = graph.call_values(
        "right_trim",
        vec![Value::String(" \t right\u{00a0}\u{000b}\r\n ".into())],
    );
    add_group(
        &mut target,
        &mut scopes,
        "Whitespace",
        vec![
            ("Normalized", ScalarType::String, normalized),
            ("Left", ScalarType::String, left),
            ("Right", ScalarType::String, right),
        ],
    );

    let length_unicode = graph.call_values("length", vec![Value::String("Aé🙂e\u{0301}".into())]);
    let length_null = graph.call_values("length", vec![Value::Null]);
    let length_nil = graph.call_values("length", vec![Value::xml_nil()]);
    add_group(
        &mut target,
        &mut scopes,
        "Length",
        vec![
            ("Unicode", ScalarType::Int, length_unicode),
            ("Null", ScalarType::Int, length_null),
            ("Nil", ScalarType::Int, length_nil),
        ],
    );

    let split = |graph: &mut GraphBuilder, function: &str, value: &str, separator: &str| {
        graph.call_values(
            function,
            vec![Value::String(value.into()), Value::String(separator.into())],
        )
    };
    let before_empty = split(&mut graph, "substring_before", "alpha", "");
    let after_empty = split(&mut graph, "substring_after", "alpha", "");
    let before_miss = split(&mut graph, "substring_before", "alpha", "|");
    let after_miss = split(&mut graph, "substring_after", "alpha", "|");
    let before_unicode = split(&mut graph, "substring_before", "head🙂tail🙂end", "🙂");
    let after_unicode = split(&mut graph, "substring_after", "head🙂tail🙂end", "🙂");
    add_group(
        &mut target,
        &mut scopes,
        "Split",
        vec![
            ("BeforeEmpty", ScalarType::String, before_empty),
            ("AfterEmpty", ScalarType::String, after_empty),
            ("BeforeMiss", ScalarType::String, before_miss),
            ("AfterMiss", ScalarType::String, after_miss),
            ("BeforeUnicode", ScalarType::String, before_unicode),
            ("AfterUnicode", ScalarType::String, after_unicode),
        ],
    );

    let string_null = graph.call_values("string", vec![Value::Null]);
    let string_nil = graph.call_values("string", vec![Value::xml_nil()]);
    let string_bool = graph.call_values("string", vec![Value::Bool(false)]);
    let string_int = graph.call_values("string", vec![Value::Int(-42)]);
    add_group(
        &mut target,
        &mut scopes,
        "String",
        vec![
            ("Null", ScalarType::String, string_null),
            ("Nil", ScalarType::String, string_nil),
            ("Bool", ScalarType::String, string_bool),
            ("Int", ScalarType::String, string_int),
        ],
    );

    let missing_null = graph.call_values(
        "substitute_missing",
        vec![Value::Null, Value::String("fallback-null".into())],
    );
    let missing_nil = graph.call_values(
        "substitute_missing",
        vec![Value::xml_nil(), Value::String("fallback-nil".into())],
    );
    let missing_empty = graph.call_values(
        "substitute_missing",
        vec![
            Value::String(String::new()),
            Value::String("fallback-empty".into()),
        ],
    );
    let missing_int = graph.call_values("substitute_missing", vec![Value::Int(0), Value::Int(9)]);
    add_group(
        &mut target,
        &mut scopes,
        "Missing",
        vec![
            ("Null", ScalarType::String, missing_null),
            ("Nil", ScalarType::String, missing_nil),
            ("Empty", ScalarType::String, missing_empty),
            ("Int", ScalarType::Int, missing_int),
        ],
    );

    let nil_nil = graph.call_values("is_xml_nil", vec![Value::xml_nil()]);
    let nil_null = graph.call_values("is_xml_nil", vec![Value::Null]);
    let nil_empty = graph.call_values("is_xml_nil", vec![Value::String(String::new())]);
    add_group(
        &mut target,
        &mut scopes,
        "Nil",
        vec![
            ("Nil", ScalarType::Bool, nil_nil),
            ("Null", ScalarType::Bool, nil_null),
            ("Empty", ScalarType::Bool, nil_empty),
        ],
    );

    let path = |graph: &mut GraphBuilder, function: &str, values: &[&str]| {
        graph.call_values(
            function,
            values
                .iter()
                .map(|value| Value::String((*value).into()))
                .collect(),
        )
    };
    let mixed_folder = path(&mut graph, "get_folder", &[r"one/two\file.xml"]);
    let mixed_name = path(&mut graph, "remove_folder", &[r"one/two\file.xml"]);
    let url_folder = path(
        &mut graph,
        "get_folder",
        &["https://example.test/a/file.xml"],
    );
    let drive_folder = path(&mut graph, "get_folder", &[r"C:\work\data\file.xml"]);
    let mixed_base = path(
        &mut graph,
        "resolve_filepath",
        &[r"C:/work\data", "reports/out.xml"],
    );
    let mixed_path = path(
        &mut graph,
        "resolve_filepath",
        &["/var/data", r"reports\out.xml"],
    );
    let windows = path(
        &mut graph,
        "resolve_filepath",
        &[r"C:\work", "reports/out.xml"],
    );
    let url = path(
        &mut graph,
        "resolve_filepath",
        &["/ignored/base", "https://example.test/config.xml"],
    );
    let drive = path(
        &mut graph,
        "resolve_filepath",
        &[r"C:\ignored", r"D:\data\config.xml"],
    );
    let parent = path(
        &mut graph,
        "resolve_filepath",
        &[r"C:\work\data\", r"..\out.xml"],
    );
    add_group(
        &mut target,
        &mut scopes,
        "Paths",
        vec![
            ("MixedFolder", ScalarType::String, mixed_folder),
            ("MixedName", ScalarType::String, mixed_name),
            ("UrlFolder", ScalarType::String, url_folder),
            ("DriveFolder", ScalarType::String, drive_folder),
            ("MixedBase", ScalarType::String, mixed_base),
            ("MixedPath", ScalarType::String, mixed_path),
            ("Windows", ScalarType::String, windows),
            ("Url", ScalarType::String, url),
            ("Drive", ScalarType::String, drive),
            ("Parent", ScalarType::String, parent),
        ],
    );

    let year = graph.call_values(
        "year_from_datetime",
        vec![Value::String("-0001-12-31T24:00:00.0Z".into())],
    );
    let month = graph.call_values(
        "month_from_datetime",
        vec![Value::String("2000-02-29T24:00:00".into())],
    );
    let day = graph.call_values(
        "day_from_datetime",
        vec![Value::String("1999-12-31T24:00:00".into())],
    );
    let hour = graph.call_values(
        "hours_from_datetime",
        vec![Value::String("1999-12-31T24:00:00.000-05:00".into())],
    );
    let minute = graph.call_values(
        "minutes_from_datetime",
        vec![Value::String("-0004-02-29T23:59:59.5+14:00".into())],
    );
    let time = graph.call_values(
        "time_from_datetime",
        vec![Value::String("2001-12-17T09:30:02.5+05:00".into())],
    );
    let composed = graph.call_values(
        "datetime_from_date_and_time",
        vec![
            Value::String("2024-02-29+05:30".into()),
            Value::String("09:08:07.125+05:30".into()),
        ],
    );
    let parts = graph.call_values(
        "datetime_from_parts",
        vec![
            Value::String("2024".into()),
            Value::Int(2),
            Value::Float(29.0),
            Value::Int(9),
            Value::Int(8),
            Value::Int(7),
            Value::Float(125.5),
            Value::Int(330),
        ],
    );
    let coerced = graph.call_values(
        "coerce_datetime",
        vec![Value::String("2031-08-17+05:45".into())],
    );
    let null = graph.call_values("month_from_datetime", vec![Value::Null]);
    add_group(
        &mut target,
        &mut scopes,
        "Temporal",
        vec![
            ("Year", ScalarType::Int, year),
            ("Month", ScalarType::Int, month),
            ("Day", ScalarType::Int, day),
            ("Hour", ScalarType::Int, hour),
            ("Minute", ScalarType::Int, minute),
            ("Time", ScalarType::String, time),
            ("Composed", ScalarType::String, composed),
            ("Parts", ScalarType::String, parts),
            ("Coerced", ScalarType::String, coerced),
            ("Null", ScalarType::String, null),
        ],
    );

    let fail_type = graph.source("FailType");
    let fail_arity = graph.source("FailArity");
    let bad_type = graph.call_values("normalize_space", vec![Value::Int(9)]);
    let bad_arity = graph.call_values("resolve_filepath", vec![Value::String("base".into())]);
    let safe_type = graph.literal(Value::String("safe-type".into()));
    let safe_arity = graph.literal(Value::String("safe-arity".into()));
    let type_probe = graph.if_(fail_type, bad_type, safe_type);
    let arity_probe = graph.if_(fail_arity, bad_arity, safe_arity);
    add_group(
        &mut target,
        &mut scopes,
        "Errors",
        vec![
            ("Type", ScalarType::String, type_probe),
            ("Arity", ScalarType::String, arity_probe),
        ],
    );

    Project {
        source: SchemaNode::group("Source", vec![bool_("FailType"), bool_("FailArity")]),
        target: SchemaNode::group("Target", target),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph { nodes: graph.nodes },
        root: Scope {
            children: scopes,
            ..Scope::default()
        },
    }
}

fn source(mode: ErrorMode) -> Instance {
    Instance::Group(vec![
        (
            "FailType".into(),
            Instance::Scalar(Value::Bool(matches!(mode, ErrorMode::Type))),
        ),
        (
            "FailArity".into(),
            Instance::Scalar(Value::Bool(matches!(mode, ErrorMode::Arity))),
        ),
    ])
}

fn values(fields: Vec<(&str, Value)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), Instance::Scalar(value)))
            .collect(),
    )
}

fn expected() -> Instance {
    Instance::Group(vec![
        (
            "Concat".into(),
            values(vec![
                ("Empty", Value::String(String::new())),
                ("Mixed", Value::String("false-71.25|done".into())),
            ]),
        ),
        (
            "Case".into(),
            values(vec![
                ("Upper", Value::String("ALPHA BETA É".into())),
                ("Lower", Value::String("mixed é".into())),
            ]),
        ),
        (
            "FormatNumber".into(),
            values(vec![
                ("Grouped", Value::String("12,345.68".into())),
                ("Negative", Value::String("[12.5]".into())),
                ("Percent", Value::String("12.6%".into())),
                ("Custom", Value::String("1.234,50".into())),
                (
                    "ExactInteger",
                    Value::String("-9,223,372,036,854,775,808".into()),
                ),
            ]),
        ),
        (
            "Whitespace".into(),
            values(vec![
                (
                    "Normalized",
                    Value::String("alpha beta\u{000b}gamma\u{00a0} delta".into()),
                ),
                ("Left", Value::String("\u{000b}\u{00a0}left \t".into())),
                ("Right", Value::String(" \t right\u{00a0}\u{000b}".into())),
            ]),
        ),
        (
            "Length".into(),
            values(vec![
                ("Unicode", Value::Int(5)),
                ("Null", Value::Int(0)),
                ("Nil", Value::Int(0)),
            ]),
        ),
        (
            "Split".into(),
            values(vec![
                ("BeforeEmpty", Value::String(String::new())),
                ("AfterEmpty", Value::String("alpha".into())),
                ("BeforeMiss", Value::String(String::new())),
                ("AfterMiss", Value::String(String::new())),
                ("BeforeUnicode", Value::String("head".into())),
                ("AfterUnicode", Value::String("tail🙂end".into())),
            ]),
        ),
        (
            "String".into(),
            values(vec![
                ("Null", Value::String(String::new())),
                ("Nil", Value::String(String::new())),
                ("Bool", Value::String("false".into())),
                ("Int", Value::String("-42".into())),
            ]),
        ),
        (
            "Missing".into(),
            values(vec![
                ("Null", Value::String("fallback-null".into())),
                ("Nil", Value::String("fallback-nil".into())),
                ("Empty", Value::String(String::new())),
                ("Int", Value::Int(0)),
            ]),
        ),
        (
            "Nil".into(),
            values(vec![
                ("Nil", Value::Bool(true)),
                ("Null", Value::Bool(false)),
                ("Empty", Value::Bool(false)),
            ]),
        ),
        (
            "Paths".into(),
            values(vec![
                ("MixedFolder", Value::String("one/two\\".into())),
                ("MixedName", Value::String("file.xml".into())),
                ("UrlFolder", Value::String("https://example.test/a/".into())),
                ("DriveFolder", Value::String("C:\\work\\data\\".into())),
                (
                    "MixedBase",
                    Value::String("C:/work\\data/reports/out.xml".into()),
                ),
                (
                    "MixedPath",
                    Value::String("/var/data\\reports\\out.xml".into()),
                ),
                ("Windows", Value::String("C:\\work\\reports/out.xml".into())),
                (
                    "Url",
                    Value::String("https://example.test/config.xml".into()),
                ),
                ("Drive", Value::String("D:\\data\\config.xml".into())),
                ("Parent", Value::String("C:\\work\\out.xml".into())),
            ]),
        ),
        (
            "Temporal".into(),
            values(vec![
                ("Year", Value::Int(1)),
                ("Month", Value::Int(3)),
                ("Day", Value::Int(1)),
                ("Hour", Value::Int(0)),
                ("Minute", Value::Int(59)),
                ("Time", Value::String("09:30:02.5+05:00".into())),
                (
                    "Composed",
                    Value::String("2024-02-29T09:08:07.125+05:30".into()),
                ),
                (
                    "Parts",
                    Value::String("2024-02-29T09:08:07.1255+05:30".into()),
                ),
                ("Coerced", Value::String("2031-08-17T00:00:00+05:45".into())),
                ("Null", Value::Null),
            ]),
        ),
        (
            "Errors".into(),
            values(vec![
                ("Type", Value::String("safe-type".into())),
                ("Arity", Value::String("safe-arity".into())),
            ]),
        ),
    ])
}

#[test]
fn scalar_function_batch_matches_engine_and_generated_backends() -> TestResult<()> {
    let project = scalar_function_project();
    assert!(engine::validate(&project).is_empty());
    assert_eq!(engine::run(&project, &source(ErrorMode::None))?, expected());
    for (mode, expected) in [
        (
            ErrorMode::Type,
            "`normalize_space` cannot accept a int argument",
        ),
        (
            ErrorMode::Arity,
            "`resolve_filepath` expected 2 argument(s), got 1",
        ),
    ] {
        let error = engine::run(&project, &source(mode))
            .expect_err("selected invalid function call must fail");
        assert!(matches!(error, engine::EngineError::Function(_)));
        assert_eq!(error.to_string(), expected);
    }

    let directory = TempDir::new("scalar_functions")?;
    let project_path = directory.0.join("scalar-functions.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let rust_output = directory.0.join("rust");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/scalar_functions_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust scalar functions failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
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
        include_str!("fixtures/scalar_functions_csharp_harness.cs.txt"),
    )?;
    let csharp = Command::new("dotnet")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output)
        .output()?;
    assert!(
        csharp.status.success(),
        "generated C# scalar functions failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
