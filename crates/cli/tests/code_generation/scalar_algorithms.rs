use super::*;

#[derive(Clone, Copy)]
enum ErrorMode {
    None,
    Type,
    Invalid,
    Arity,
    NumberInvalid,
    DelayType,
    DelayInvalid,
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

fn call(graph: &mut GraphBuilder, function: &str, values: Vec<Value>) -> u32 {
    graph.call_values(function, values)
}

fn scalar_algorithm_project() -> Project {
    let mut graph = GraphBuilder::new();
    let mut target = Vec::new();
    let mut scopes = Vec::new();

    let unicode = call(
        &mut graph,
        "substring",
        vec![Value::String("A🙂éZ".into()), Value::Int(2), Value::Int(2)],
    );
    let rounded_start = call(
        &mut graph,
        "substring",
        vec![Value::String("abcdef".into()), Value::Float(2.5)],
    );
    let rounded_length = call(
        &mut graph,
        "substring",
        vec![
            Value::String("abcdef".into()),
            Value::Int(2),
            Value::Float(2.5),
        ],
    );
    let zero_start = call(
        &mut graph,
        "substring",
        vec![Value::String("abc".into()), Value::Int(0), Value::Int(2)],
    );
    let maximum = call(
        &mut graph,
        "substring",
        vec![
            Value::String("abc".into()),
            Value::Int(i64::MAX),
            Value::Int(i64::MAX),
        ],
    );
    let nan = graph.source("NaN");
    let nan_substring_input = graph.literal(Value::String("abc".into()));
    let nan_substring = graph.call("substring", vec![nan_substring_input, nan]);
    let infinity = graph.source("Infinity");
    let infinity_substring_input = graph.literal(Value::String("abc".into()));
    let infinity_substring = graph.call("substring", vec![infinity_substring_input, infinity]);
    add_group(
        &mut target,
        &mut scopes,
        "Substring",
        vec![
            ("Unicode", ScalarType::String, unicode),
            ("RoundedStart", ScalarType::String, rounded_start),
            ("RoundedLength", ScalarType::String, rounded_length),
            ("ZeroStart", ScalarType::String, zero_start),
            ("Maximum", ScalarType::String, maximum),
            ("NaN", ScalarType::String, nan_substring),
            ("Infinity", ScalarType::String, infinity_substring),
        ],
    );

    let like = |graph: &mut GraphBuilder, value: &str, pattern: &str| {
        call(
            graph,
            "sql_like",
            vec![Value::String(value.into()), Value::String(pattern.into())],
        )
    };
    let ascii_fold = like(&mut graph, "baker", "B%");
    let unicode_wildcards = like(&mut graph, "A🙂é", "a__");
    let non_ascii_exact = like(&mut graph, "É", "é");
    let empty_percent = like(&mut graph, "", "%");
    let repeated_percent = like(&mut graph, "abc", "a%%c");
    add_group(
        &mut target,
        &mut scopes,
        "Like",
        vec![
            ("AsciiFold", ScalarType::Bool, ascii_fold),
            ("UnicodeWildcards", ScalarType::Bool, unicode_wildcards),
            ("NonAsciiExact", ScalarType::Bool, non_ascii_exact),
            ("EmptyPercent", ScalarType::Bool, empty_percent),
            ("RepeatedPercent", ScalarType::Bool, repeated_percent),
        ],
    );

    let float_length = call(
        &mut graph,
        "pad_string_left",
        vec![Value::Int(7), Value::Float(3.9), Value::String("0".into())],
    );
    let emoji_padding = call(
        &mut graph,
        "pad_string_left",
        vec![
            Value::String("é".into()),
            Value::Int(3),
            Value::String("🙂".into()),
        ],
    );
    let emoji_value = call(
        &mut graph,
        "pad_string_right",
        vec![
            Value::String("🙂".into()),
            Value::Int(3),
            Value::String("é".into()),
        ],
    );
    let negative = call(
        &mut graph,
        "pad_string_right",
        vec![
            Value::String("abc".into()),
            Value::Int(-2),
            Value::String("x".into()),
        ],
    );
    let exact = call(
        &mut graph,
        "pad_string_left",
        vec![
            Value::String("abc".into()),
            Value::Int(3),
            Value::String("x".into()),
        ],
    );
    let null_value = call(
        &mut graph,
        "pad_string_right",
        vec![Value::Null, Value::Int(2), Value::String("*".into())],
    );
    add_group(
        &mut target,
        &mut scopes,
        "Pad",
        vec![
            ("FloatLength", ScalarType::String, float_length),
            ("EmojiPadding", ScalarType::String, emoji_padding),
            ("EmojiValue", ScalarType::String, emoji_value),
            ("Negative", ScalarType::String, negative),
            ("Exact", ScalarType::String, exact),
            ("NullValue", ScalarType::String, null_value),
        ],
    );

    let hyphenated = call(
        &mut graph,
        "isbn10_to_isbn13",
        vec![Value::String("0-7645-4964-2".into())],
    );
    let lower_x = call(
        &mut graph,
        "isbn10_to_isbn13",
        vec![Value::String("0 8044 2957 x".into())],
    );
    add_group(
        &mut target,
        &mut scopes,
        "Isbn",
        vec![
            ("Hyphenated", ScalarType::String, hyphenated),
            ("LowerXAndSpaces", ScalarType::String, lower_x),
        ],
    );

    let integer = call(&mut graph, "round", vec![Value::Int(7)]);
    let positive_half = call(&mut graph, "round", vec![Value::Float(2.5)]);
    let negative_half = call(&mut graph, "round", vec![Value::Float(-2.5)]);
    let precision = call(
        &mut graph,
        "round",
        vec![Value::Float(1.23456), Value::Int(2)],
    );
    let rounded_digits = call(
        &mut graph,
        "round",
        vec![Value::Float(1.23456), Value::Float(1.5)],
    );
    let negative_digits = call(
        &mut graph,
        "round",
        vec![Value::Float(149.0), Value::Int(-1)],
    );
    let round_infinity = graph.call("round", vec![infinity]);
    add_group(
        &mut target,
        &mut scopes,
        "Round",
        vec![
            ("Integer", ScalarType::Int, integer),
            ("PositiveHalf", ScalarType::Float, positive_half),
            ("NegativeHalf", ScalarType::Float, negative_half),
            ("Precision", ScalarType::Float, precision),
            ("RoundedDigits", ScalarType::Float, rounded_digits),
            ("NegativeDigits", ScalarType::Float, negative_digits),
            ("Infinity", ScalarType::Float, round_infinity),
        ],
    );

    let date_time = call(
        &mut graph,
        "date_from_datetime",
        vec![Value::String(" 2024-03-01T23:30:00-05:00 ".into())],
    );
    let date_only = call(
        &mut graph,
        "date_from_datetime",
        vec![Value::String("\t2024-03-01Z \n".into())],
    );
    let multiple_t = call(
        &mut graph,
        "date_from_datetime",
        vec![Value::String("alphaTbetaTgamma".into())],
    );
    add_group(
        &mut target,
        &mut scopes,
        "Date",
        vec![
            ("DateTime", ScalarType::String, date_time),
            ("DateOnly", ScalarType::String, date_only),
            ("MultipleT", ScalarType::String, multiple_t),
        ],
    );

    let trimmed = call(
        &mut graph,
        "trim",
        vec![Value::String("\u{2003}\u{202f}value\u{a0}\u{3000}".into())],
    );
    let numeric_int = call(&mut graph, "is_numeric", vec![Value::Int(i64::MIN)]);
    let numeric_decimal = call(
        &mut graph,
        "is_numeric",
        vec![Value::String(" -1.25e2 ".into())],
    );
    let numeric_overflow = call(
        &mut graph,
        "is_numeric",
        vec![Value::String("9223372036854775808".into())],
    );
    let numeric_infinity = call(
        &mut graph,
        "is_numeric",
        vec![Value::String("1e9999".into())],
    );
    let numeric_null = call(&mut graph, "is_numeric", vec![Value::Null]);
    let numeric_xml_nil = call(&mut graph, "is_numeric", vec![Value::xml_nil()]);
    let numeric_bool = call(&mut graph, "is_numeric", vec![Value::Bool(true)]);
    let number_int = call(
        &mut graph,
        "to_number",
        vec![Value::String(i64::MAX.to_string())],
    );
    let number_min = call(
        &mut graph,
        "to_number",
        vec![Value::String(i64::MIN.to_string())],
    );
    let number_float = call(&mut graph, "to_number", vec![Value::String("12.5".into())]);
    let number_exponent = call(&mut graph, "to_number", vec![Value::String("1e3".into())]);
    let number_negative_zero = call(&mut graph, "to_number", vec![Value::Float(-0.0)]);
    let number_null = call(&mut graph, "to_number", vec![Value::Null]);
    let delay_text = call(
        &mut graph,
        "delay_passthrough",
        vec![Value::String("ready".into()), Value::Float(0.25)],
    );
    let delay_nil = call(
        &mut graph,
        "delay_passthrough",
        vec![Value::xml_nil(), Value::Int(0)],
    );
    add_group(
        &mut target,
        &mut scopes,
        "Conversions",
        vec![
            ("Trimmed", ScalarType::String, trimmed),
            ("NumericInt", ScalarType::Bool, numeric_int),
            ("NumericDecimal", ScalarType::Bool, numeric_decimal),
            ("NumericOverflow", ScalarType::Bool, numeric_overflow),
            ("NumericInfinity", ScalarType::Bool, numeric_infinity),
            ("NumericNull", ScalarType::Bool, numeric_null),
            ("NumericXmlNil", ScalarType::Bool, numeric_xml_nil),
            ("NumericBool", ScalarType::Bool, numeric_bool),
            ("NumberInt", ScalarType::Int, number_int),
            ("NumberMin", ScalarType::Int, number_min),
            ("NumberFloat", ScalarType::Float, number_float),
            ("NumberExponent", ScalarType::Float, number_exponent),
            (
                "NumberNegativeZero",
                ScalarType::Float,
                number_negative_zero,
            ),
            ("NumberNull", ScalarType::Float, number_null),
            ("DelayText", ScalarType::String, delay_text),
            ("DelayNil", ScalarType::String, delay_nil),
        ],
    );

    let fail_type = graph.source("FailType");
    let bad_type_value = graph.literal(Value::Int(9));
    let bad_type_start = graph.literal(Value::Int(1));
    let bad_type = graph.call("substring", vec![bad_type_value, bad_type_start]);
    let safe_type = graph.literal(Value::String("safe-type".into()));
    let type_probe = graph.if_(fail_type, bad_type, safe_type);
    let fail_invalid = graph.source("FailInvalid");
    let bad_isbn_value = graph.literal(Value::String("0764549643".into()));
    let bad_invalid = graph.call("isbn10_to_isbn13", vec![bad_isbn_value]);
    let safe_invalid = graph.literal(Value::String("safe-invalid".into()));
    let invalid_probe = graph.if_(fail_invalid, bad_invalid, safe_invalid);
    let fail_arity = graph.source("FailArity");
    let bad_arity = graph.call("date_from_datetime", Vec::new());
    let safe_arity = graph.literal(Value::String("safe-arity".into()));
    let arity_probe = graph.if_(fail_arity, bad_arity, safe_arity);
    let fail_number_invalid = graph.source("FailNumberInvalid");
    let bad_number_input = graph.literal(Value::Bool(true));
    let bad_number = graph.call("to_number", vec![bad_number_input]);
    let safe_number = graph.literal(Value::String("safe-number".into()));
    let number_probe = graph.if_(fail_number_invalid, bad_number, safe_number);
    let fail_delay_type = graph.source("FailDelayType");
    let delay_type_value = graph.literal(Value::String("response".into()));
    let delay_type_duration = graph.literal(Value::Bool(false));
    let bad_delay_type = graph.call(
        "delay_passthrough",
        vec![delay_type_value, delay_type_duration],
    );
    let safe_delay_type = graph.literal(Value::String("safe-delay-type".into()));
    let delay_type_probe = graph.if_(fail_delay_type, bad_delay_type, safe_delay_type);
    let fail_delay_invalid = graph.source("FailDelayInvalid");
    let delay_invalid_value = graph.literal(Value::String("response".into()));
    let delay_invalid_duration = graph.literal(Value::Int(-1));
    let bad_delay_invalid = graph.call(
        "delay_passthrough",
        vec![delay_invalid_value, delay_invalid_duration],
    );
    let safe_delay_invalid = graph.literal(Value::String("safe-delay-invalid".into()));
    let delay_invalid_probe = graph.if_(fail_delay_invalid, bad_delay_invalid, safe_delay_invalid);
    add_group(
        &mut target,
        &mut scopes,
        "Errors",
        vec![
            ("Type", ScalarType::String, type_probe),
            ("Invalid", ScalarType::String, invalid_probe),
            ("Arity", ScalarType::String, arity_probe),
            ("NumberInvalid", ScalarType::String, number_probe),
            ("DelayType", ScalarType::String, delay_type_probe),
            ("DelayInvalid", ScalarType::String, delay_invalid_probe),
        ],
    );

    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                bool_("FailType"),
                bool_("FailInvalid"),
                bool_("FailArity"),
                bool_("FailNumberInvalid"),
                bool_("FailDelayType"),
                bool_("FailDelayInvalid"),
                SchemaNode::scalar("NaN", ScalarType::Float),
                SchemaNode::scalar("Infinity", ScalarType::Float),
            ],
        ),
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
            "FailInvalid".into(),
            Instance::Scalar(Value::Bool(matches!(mode, ErrorMode::Invalid))),
        ),
        (
            "FailArity".into(),
            Instance::Scalar(Value::Bool(matches!(mode, ErrorMode::Arity))),
        ),
        (
            "FailNumberInvalid".into(),
            Instance::Scalar(Value::Bool(matches!(mode, ErrorMode::NumberInvalid))),
        ),
        (
            "FailDelayType".into(),
            Instance::Scalar(Value::Bool(matches!(mode, ErrorMode::DelayType))),
        ),
        (
            "FailDelayInvalid".into(),
            Instance::Scalar(Value::Bool(matches!(mode, ErrorMode::DelayInvalid))),
        ),
        ("NaN".into(), Instance::Scalar(Value::Float(f64::NAN))),
        (
            "Infinity".into(),
            Instance::Scalar(Value::Float(f64::INFINITY)),
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
            "Substring".into(),
            values(vec![
                ("Unicode", Value::String("🙂é".into())),
                ("RoundedStart", Value::String("cdef".into())),
                ("RoundedLength", Value::String("bcd".into())),
                ("ZeroStart", Value::String("a".into())),
                ("Maximum", Value::String(String::new())),
                ("NaN", Value::String("abc".into())),
                ("Infinity", Value::String(String::new())),
            ]),
        ),
        (
            "Like".into(),
            values(vec![
                ("AsciiFold", Value::Bool(true)),
                ("UnicodeWildcards", Value::Bool(true)),
                ("NonAsciiExact", Value::Bool(false)),
                ("EmptyPercent", Value::Bool(true)),
                ("RepeatedPercent", Value::Bool(true)),
            ]),
        ),
        (
            "Pad".into(),
            values(vec![
                ("FloatLength", Value::String("007".into())),
                ("EmojiPadding", Value::String("🙂🙂é".into())),
                ("EmojiValue", Value::String("🙂éé".into())),
                ("Negative", Value::String("abc".into())),
                ("Exact", Value::String("abc".into())),
                ("NullValue", Value::String("**".into())),
            ]),
        ),
        (
            "Isbn".into(),
            values(vec![
                ("Hyphenated", Value::String("9780764549649".into())),
                ("LowerXAndSpaces", Value::String("9780804429573".into())),
            ]),
        ),
        (
            "Round".into(),
            values(vec![
                ("Integer", Value::Int(7)),
                ("PositiveHalf", Value::Float(3.0)),
                ("NegativeHalf", Value::Float(-3.0)),
                ("Precision", Value::Float(1.23)),
                ("RoundedDigits", Value::Float(1.23)),
                ("NegativeDigits", Value::Float(150.0)),
                ("Infinity", Value::Float(f64::INFINITY)),
            ]),
        ),
        (
            "Date".into(),
            values(vec![
                ("DateTime", Value::String("2024-03-01".into())),
                ("DateOnly", Value::String("2024-03-01Z".into())),
                ("MultipleT", Value::String("alpha".into())),
            ]),
        ),
        (
            "Conversions".into(),
            values(vec![
                ("Trimmed", Value::String("value".into())),
                ("NumericInt", Value::Bool(true)),
                ("NumericDecimal", Value::Bool(true)),
                ("NumericOverflow", Value::Bool(true)),
                ("NumericInfinity", Value::Bool(false)),
                ("NumericNull", Value::Bool(false)),
                ("NumericXmlNil", Value::Bool(false)),
                ("NumericBool", Value::Bool(false)),
                ("NumberInt", Value::Int(i64::MAX)),
                ("NumberMin", Value::Int(i64::MIN)),
                ("NumberFloat", Value::Float(12.5)),
                ("NumberExponent", Value::Float(1000.0)),
                ("NumberNegativeZero", Value::Float(-0.0)),
                ("NumberNull", Value::Null),
                ("DelayText", Value::String("ready".into())),
                ("DelayNil", Value::xml_nil()),
            ]),
        ),
        (
            "Errors".into(),
            values(vec![
                ("Type", Value::String("safe-type".into())),
                ("Invalid", Value::String("safe-invalid".into())),
                ("Arity", Value::String("safe-arity".into())),
                ("NumberInvalid", Value::String("safe-number".into())),
                ("DelayType", Value::String("safe-delay-type".into())),
                ("DelayInvalid", Value::String("safe-delay-invalid".into())),
            ]),
        ),
    ])
}

#[test]
fn scalar_algorithms_match_engine_and_generated_backends() -> TestResult<()> {
    let project = scalar_algorithm_project();
    assert!(engine::validate(&project).is_empty());
    assert_eq!(engine::run(&project, &source(ErrorMode::None))?, expected());
    for (mode, expected) in [
        (ErrorMode::Type, "`substring` cannot accept a int argument"),
        (
            ErrorMode::Invalid,
            "`isbn10_to_isbn13` ISBN-10 check digit is invalid",
        ),
        (
            ErrorMode::Arity,
            "`date_from_datetime` expected 1 argument(s), got 0",
        ),
        (
            ErrorMode::NumberInvalid,
            "`to_number` requires a finite numeric value",
        ),
        (
            ErrorMode::DelayType,
            "`delay_passthrough` cannot accept a bool argument",
        ),
        (
            ErrorMode::DelayInvalid,
            "`delay_passthrough` requires a finite nonnegative duration",
        ),
    ] {
        let error = engine::run(&project, &source(mode))
            .expect_err("selected invalid function call must fail");
        assert!(matches!(error, engine::EngineError::Function(_)));
        assert_eq!(error.to_string(), expected);
    }

    let directory = TempDir::new("scalar_algorithms")?;
    let project_path = directory.0.join("scalar-algorithms.json");
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
        include_str!("fixtures/scalar_algorithms_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust scalar algorithms failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/scalar_algorithms_csharp_harness.cs.txt"),
    )?;
    let csharp = dotnet_command(&csharp_output)
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
        "generated C# scalar algorithms failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
