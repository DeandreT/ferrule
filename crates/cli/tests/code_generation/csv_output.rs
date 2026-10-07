use super::*;

use cli::generate_project_with_csv_output;
use format_csv::{CsvBoundedError, CsvFormatError, CsvWriteOptions};
use mapping::{FormatOptions, SequenceWindow};

const MAXIMUM: usize = 64 * 1024 * 1024;
const TEXT_COLUMN: &str = "Text'\n😀";

struct CsvDirectory {
    path: PathBuf,
    complete: bool,
}

impl CsvDirectory {
    fn new(name: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_csv_{name}_{}_{}_{}",
            std::process::id(),
            nonce,
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            complete: false,
        })
    }
}

impl Drop for CsvDirectory {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!("Retained CSV test artifacts: {}", self.path.display());
        }
    }
}

fn recorded_csv_command(command: &mut Command, directory: &Path, name: &str) -> io::Result<Output> {
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
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt as _;
        output.status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    std::fs::write(
        directory.join(format!("{name}-status.json")),
        serde_json::to_vec(&serde_json::json!({
            "success": output.status.success(), "code": output.status.code(), "signal": signal,
        }))?,
    )?;
    Ok(output)
}

fn record_native_bytes(
    directory: &Path,
    name: &str,
    result: &Result<Vec<u8>, CsvBoundedError>,
) -> TestResult<()> {
    match result {
        Ok(bytes) => std::fs::write(directory.join(format!("{name}.csv")), bytes)?,
        Err(error) => std::fs::write(
            directory.join(format!("{name}-error.txt")),
            format!("{error:?}\n{error}\n"),
        )?,
    }
    Ok(())
}

fn csv_project(options: FormatOptions) -> Project {
    let columns = [
        (TEXT_COLUMN, ScalarType::String, 1),
        ("Integer", ScalarType::Int, 2),
        ("Number", ScalarType::Float, 3),
        ("Boolean", ScalarType::Bool, 4),
        ("Absent", ScalarType::String, 7),
        ("HostLabel", ScalarType::String, 10),
        ("HostCount", ScalarType::Int, 12),
        ("Guard", ScalarType::Int, 17),
    ];
    let mut nodes = BTreeMap::new();
    for (id, name) in [
        (1, "Text"),
        (2, "Integer"),
        (3, "Number"),
        (4, "Boolean"),
        (5, "Keep"),
        (6, "Rank"),
    ] {
        nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.into()],
                frame: None,
            },
        );
    }
    for (id, value) in [
        (7, Value::json_null()),
        (8, Value::Int(2)),
        (9, Value::String("fallback".into())),
        (11, Value::Int(7)),
        (13, Value::Bool(false)),
        (16, Value::Int(42)),
    ] {
        nodes.insert(id, Node::Const { value });
    }
    for (id, name, ty, default) in [
        (10, "Label", ScalarType::String, 9),
        (12, "Count", ScalarType::Int, 11),
        (14, "Require", ScalarType::Bool, 13),
    ] {
        nodes.insert(
            id,
            Node::RuntimeParameterDefault {
                name: name.into(),
                ty,
                default,
                preview: Some("unused-preview".into()),
            },
        );
    }
    nodes.insert(
        15,
        Node::RuntimeParameter {
            name: "Required".into(),
            ty: ScalarType::Int,
            preview: Some("99".into()),
        },
    );
    nodes.insert(
        17,
        Node::If {
            condition: 14,
            then: 15,
            else_: 16,
        },
    );
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                string("Text"),
                int("Integer"),
                SchemaNode::scalar("Number", ScalarType::Float),
                bool_("Boolean"),
                bool_("Keep"),
                int("Rank"),
            ],
        )
        .repeating(),
        target: SchemaNode::group(
            "Row",
            columns
                .iter()
                .map(|(name, ty, _)| SchemaNode::scalar(*name, *ty))
                .collect(),
        ),
        source_path: None,
        target_path: Some("output.csv".into()),
        source_options: Default::default(),
        target_options: options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            filter: Some(5),
            sort_by: Some(6),
            sort_descending: true,
            windows: vec![SequenceWindow::First { count: 8 }],
            // The serializer must retain schema order despite reverse mapping order.
            bindings: columns
                .iter()
                .rev()
                .map(|(name, _, node)| Binding {
                    target_field: (*name).into(),
                    node: *node,
                })
                .collect(),
            ..Scope::default()
        },
    }
}

fn row(text: &str, integer: i64, number: f64, boolean: bool, keep: bool, rank: i64) -> Instance {
    row_value(
        Value::String(text.into()),
        integer,
        number,
        boolean,
        keep,
        rank,
    )
}

fn row_value(
    text: Value,
    integer: i64,
    number: f64,
    boolean: bool,
    keep: bool,
    rank: i64,
) -> Instance {
    Instance::Group(
        (vec![
            ("Text".into(), Instance::Scalar(text)),
            ("Integer".into(), Instance::Scalar(Value::Int(integer))),
            ("Number".into(), Instance::Scalar(Value::Float(number))),
            ("Boolean".into(), Instance::Scalar(Value::Bool(boolean))),
            ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
            ("Rank".into(), Instance::Scalar(Value::Int(rank))),
        ])
        .into(),
    )
}

fn source() -> Instance {
    Instance::Repeated(vec![
        row("tail", 9, 1.25, true, true, 1),
        row("filtered", 9, 1.25, true, false, 4),
        row("A;'B\"\n😀", -7, 0.0000001, true, true, 3),
        row("", 0, -0.0, false, true, 2),
    ])
}

fn native_bytes(project: &Project, primary: &Instance) -> Result<Vec<u8>, CsvBoundedError> {
    let Instance::Repeated(rows) = primary else {
        panic!("mapping must produce repeated rows")
    };
    format_csv::to_bytes_with_options_bounded(
        &project.target,
        rows,
        &CsvWriteOptions::from(&project.target_options),
        MAXIMUM,
    )
}

fn native_run(project: &Project, source: &Instance) -> TestResult<Vec<u8>> {
    Ok(native_bytes(project, &engine::run(project, source)?)?)
}

fn write_oracle(directory: &Path, name: &str, bytes: &[u8]) -> TestResult<()> {
    std::fs::write(directory.join(name), bytes)?;
    Ok(())
}

fn context(parameters: &engine::RuntimeParameters) -> engine::ExecutionContext<'_> {
    engine::ExecutionContext::new(Path::new("csv-project.json")).with_parameters(parameters)
}

fn assert_engine_host_failures(
    directory: &Path,
    project: &Project,
    source: &Instance,
) -> TestResult<()> {
    let mut required = engine::RuntimeParameters::new();
    required.insert("Require", Value::Bool(true))?;
    let required_result = engine::run_with_context(project, source, &context(&required));
    std::fs::write(
        directory.join("native-required-outcome.txt"),
        format!("{required_result:?}\n"),
    )?;
    assert_eq!(
        required_result,
        Err(engine::EngineError::MissingRuntimeParameter {
            node: 15,
            name: "Required".into()
        })
    );
    let mut wrong = engine::RuntimeParameters::new();
    wrong.insert("Count", Value::Bool(true))?;
    let wrong_result = engine::run_with_context(project, source, &context(&wrong));
    std::fs::write(
        directory.join("native-wrong-outcome.txt"),
        format!("{wrong_result:?}\n"),
    )?;
    assert_eq!(
        wrong_result,
        Err(engine::EngineError::RuntimeParameterType {
            node: 12,
            name: "Count".into(),
            expected: ScalarType::Int,
            found: "bool",
        })
    );
    Ok(())
}

fn write_mapping(directory: &Path, project: &Project) -> TestResult<PathBuf> {
    let path = directory.join("project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(project)?)?;
    Ok(path)
}

fn run_csharp(output: &Path, fixture: &str, mode: &str) -> TestResult<()> {
    let harness = output.join("Harness");
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
    std::fs::write(harness.join("Program.cs"), fixture)?;
    let mut command = dotnet_command(output);
    command
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--",
            mode,
        ])
        .current_dir(output);
    let result = recorded_csv_command(&mut command, output, "csharp-host")?;
    assert!(
        result.status.success(),
        "generated C# CSV {mode} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("generated CSV checks passed"));
    Ok(())
}

fn small_matrix(
    language: &str,
    target: GenerateTarget,
    run_host: fn(&Path, &str) -> TestResult<()>,
) -> TestResult<()> {
    let cases = [
        ("default", FormatOptions::default()),
        (
            "custom",
            FormatOptions {
                delimiter: Some(';'),
                csv_quote: Some('\''),
                csv_utf8_bom: true,
                ..Default::default()
            },
        ),
        (
            "disabled",
            FormatOptions {
                delimiter: Some('\t'),
                csv_quote_disabled: true,
                has_header_row: Some(false),
                ..Default::default()
            },
        ),
    ];
    for (mode, options) in cases {
        let mut directory = CsvDirectory::new(&format!("{language}_csv_{mode}"))?;
        let project = csv_project(options);
        let path = write_mapping(&directory.path, &project)?;
        assert!(engine::validate(&project).is_empty());
        let source = if mode == "disabled" {
            Instance::Repeated(vec![row("safe's\"quoted", -7, 1.25, true, true, 1)])
        } else {
            source()
        };
        let primary = engine::run(&project, &source)?;
        let expected_result = native_bytes(&project, &primary);
        record_native_bytes(&directory.path, "native-default", &expected_result)?;
        let expected = expected_result?;
        let header = "Text'\n😀";
        let literal = match mode {
            "default" => format!("\"{header}\",Integer,Number,Boolean,Absent,HostLabel,HostCount,Guard\n\"A;'B\"\"\n😀\",-7,0.0000001,true,,fallback,7,42\n,0,-0,false,,fallback,7,42\n"),
            "custom" => "\u{feff}'Text''\n😀';Integer;Number;Boolean;Absent;HostLabel;HostCount;Guard\n'A;''B\"\n😀';-7;0.0000001;true;;fallback;7;42\n;0;-0;false;;fallback;7;42\n".into(),
            "disabled" => "safe's\"quoted\t-7\t1.25\ttrue\t\tfallback\t7\t42\n".into(),
            _ => unreachable!(),
        };
        assert_eq!(expected, literal.as_bytes());
        assert_engine_host_failures(&directory.path, &project, &source)?;
        let output = directory.path.join("generated");
        generate_project_with_csv_output(&path, &output, target.clone())?;
        let cli_output = directory.path.join("from-cli");
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .arg("generate")
            .arg("--project")
            .arg(&path)
            .args(["--language", language])
            .arg("--out")
            .arg(&cli_output)
            .arg("--csv-output");
        add_runtime_path(&mut command, &target);
        let command = recorded_csv_command(&mut command, &directory.path, "cli-csv")?;
        assert!(
            command.status.success(),
            "CSV CLI failed: {}",
            String::from_utf8_lossy(&command.stderr)
        );
        assert_eq!(artifact_files(&output)?, artifact_files(&cli_output)?);
        let legacy = directory.path.join("legacy");
        generate_project(&path, &legacy, target.clone())?;
        assert!(!legacy.join("GeneratedMapping.Csv.cs").exists());
        assert!(!legacy.join("Runtime/FerruleCsv.cs").exists());
        if language == "rust" {
            assert!(
                !std::fs::read_to_string(legacy.join("src/lib.rs"))?
                    .contains("pub fn execute_csv(")
            );
        }
        let legacy_cli = directory.path.join("legacy-cli");
        let mut no_flag = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        no_flag
            .arg("generate")
            .arg("--project")
            .arg(&path)
            .args(["--language", language])
            .arg("--out")
            .arg(&legacy_cli);
        add_runtime_path(&mut no_flag, &target);
        let no_flag = recorded_csv_command(&mut no_flag, &directory.path, "cli-ordinary")?;
        assert!(no_flag.status.success(), "ordinary CLI generation failed");
        assert_eq!(artifact_files(&legacy)?, artifact_files(&legacy_cli)?);
        let before = artifact_files(&output)?;
        let mut duplicate = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        duplicate
            .arg("generate")
            .arg("--project")
            .arg(&path)
            .args(["--language", language])
            .arg("--out")
            .arg(&output)
            .arg("--csv-output");
        add_runtime_path(&mut duplicate, &target);
        let duplicate = recorded_csv_command(&mut duplicate, &directory.path, "cli-existing")?;
        assert!(!duplicate.status.success());
        assert_eq!(artifact_files(&output)?, before);
        assert!(std::fs::read_dir(&directory.path)?.all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("ferrule-stage")
        }));
        write_oracle(&output, "expected-default.csv", &expected)?;
        let mut supplied = engine::RuntimeParameters::new();
        supplied.insert("Label", Value::String("host".into()))?;
        supplied.insert("Count", Value::String(" 9 ".into()))?;
        let primary = engine::run_with_context(&project, &source, &context(&supplied))?;
        let result = native_bytes(&project, &primary);
        record_native_bytes(&output, "expected-context", &result)?;
        let _ = result?;
        let mut null = engine::RuntimeParameters::new();
        null.insert("Count", Value::Null)?;
        let primary = engine::run_with_context(&project, &source, &context(&null))?;
        let result = native_bytes(&project, &primary);
        record_native_bytes(&output, "expected-null", &result)?;
        let _ = result?;
        let empty = Instance::Repeated(Vec::new());
        let empty_bytes = native_run(&project, &empty)?;
        let empty_literal = match mode {
            "default" => {
                format!("\"{header}\",Integer,Number,Boolean,Absent,HostLabel,HostCount,Guard\n")
            }
            "custom" => {
                "\u{feff}'Text''\n😀';Integer;Number;Boolean;Absent;HostLabel;HostCount;Guard\n"
                    .into()
            }
            "disabled" => String::new(),
            _ => unreachable!(),
        };
        write_oracle(&output, "expected-empty.csv", &empty_bytes)?;
        assert_eq!(empty_bytes, empty_literal.as_bytes());
        let mut required = engine::RuntimeParameters::new();
        required.insert("Require", Value::Bool(true))?;
        let empty_required = native_bytes(
            &project,
            &engine::run_with_context(&project, &empty, &context(&required))?,
        );
        record_native_bytes(&output, "native-empty-required", &empty_required)?;
        assert_eq!(empty_required?, empty_bytes);
        let filtered = Instance::Repeated(vec![row("not evaluated", -7, 1.25, true, false, 1)]);
        let filtered_required = native_bytes(
            &project,
            &engine::run_with_context(&project, &filtered, &context(&required))?,
        );
        record_native_bytes(&output, "native-filtered-required", &filtered_required)?;
        assert_eq!(filtered_required?, empty_bytes);
        if mode == "disabled" {
            let bad = Instance::Repeated(vec![row("bad\tvalue", -7, 1.25, true, true, 1)]);
            let bad_result = native_bytes(&project, &engine::run(&project, &bad)?);
            record_native_bytes(&output, "native-no-quote", &bad_result)?;
            assert!(matches!(bad_result,
                Err(CsvBoundedError::Format(CsvFormatError::UnquotedFieldBoundary { row: 0, field })) if field == TEXT_COLUMN));
        }
        for (name, number) in [
            ("negative-zero", -0.0),
            ("small", 1e-7),
            ("large", 1e20),
            ("epsilon", f64::from_bits(1)),
            ("min-normal", f64::MIN_POSITIVE),
            ("max", f64::MAX),
        ] {
            let numeric = Instance::Repeated(vec![row_value(
                Value::Float(number),
                -7,
                number,
                true,
                true,
                1,
            )]);
            let primary = engine::run(&project, &numeric)?;
            let result = native_bytes(&project, &primary);
            record_native_bytes(&output, &format!("expected-numeric-{name}"), &result)?;
            let _ = result?;
        }
        let nil = Instance::Repeated(vec![row_value(Value::xml_nil(), -7, 1.25, true, true, 1)]);
        let primary = engine::run(&project, &nil)?;
        let result = native_bytes(&project, &primary);
        record_native_bytes(&output, "native-xml-nil", &result)?;
        assert!(
            matches!(result, Err(CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 0, field, expected: ScalarType::String, got: "xml nil",
        })) if field == TEXT_COLUMN)
        );
        run_host(&output, mode)?;
        directory.complete = true;
    }
    Ok(())
}

fn large_matrix(
    language: &str,
    target: GenerateTarget,
    run_host: fn(&Path, &str) -> TestResult<()>,
) -> TestResult<()> {
    let mut directory = CsvDirectory::new(&format!("{language}_csv_large"))?;
    let project = csv_project(FormatOptions {
        csv_utf8_bom: true,
        ..Default::default()
    });
    let stub = Instance::Repeated(vec![row("\"", -7, 1.25, true, true, 1)]);
    let stub_bytes = native_run(&project, &stub)?;
    write_oracle(&directory.path, "native-large-stub.csv", &stub_bytes)?;
    let overhead = stub_bytes.len();
    let extra = MAXIMUM
        .checked_sub(overhead)
        .expect("small header and row fit");
    let path = write_mapping(&directory.path, &project)?;
    let output = directory.path.join("generated");
    generate_project_with_csv_output(&path, &output, target.clone())?;
    std::fs::write(output.join("large-extra.txt"), extra.to_string())?;
    {
        let text = format!("\"{}", "x".repeat(extra));
        let source = Instance::Repeated(vec![row(&text, -7, 1.25, true, true, 1)]);
        let bytes = native_run(&project, &source)?;
        write_oracle(&output, "expected-maximum.csv", &bytes)?;
        assert_eq!(bytes.len(), MAXIMUM);
        assert!(bytes.starts_with(&[0xef, 0xbb, 0xbf]));
    }
    {
        let text = format!("\"{}", "x".repeat(extra + 1));
        let source = Instance::Repeated(vec![row(&text, -7, 1.25, true, true, 2)]);
        let primary = engine::run(&project, &source)?;
        let result = native_bytes(&project, &primary);
        record_native_bytes(&output, "native-plus-one", &result)?;
        assert!(matches!(
            result,
            Err(CsvBoundedError::OutputTooLarge { maximum: MAXIMUM })
        ));
        let errors = Instance::Repeated(vec![
            row(&text, -7, 1.25, true, true, 2),
            row("late-invalid", -7, f64::INFINITY, true, true, 1),
        ]);
        let primary = engine::run(&project, &errors)?;
        let result = native_bytes(&project, &primary);
        record_native_bytes(&output, "native-late-invalid", &result)?;
        assert!(matches!(result,
            Err(CsvBoundedError::Format(CsvFormatError::ValueType {
                row: 1, field, expected: ScalarType::Float, got: "non-finite float",
            })) if field == "Number"));
        // Mapping errors must precede both this over-budget row and invalid CSV value.
        let mut parameters = engine::RuntimeParameters::new();
        parameters.insert("Require", Value::Bool(true))?;
        let result = engine::run_with_context(&project, &errors, &context(&parameters));
        std::fs::write(
            output.join("native-required-before-budget-and-row.txt"),
            format!("{result:?}\n"),
        )?;
        assert_eq!(
            result,
            Err(engine::EngineError::MissingRuntimeParameter {
                node: 15,
                name: "Required".into()
            })
        );
    }
    run_host(&output, "large")?;
    directory.complete = true;
    Ok(())
}

fn add_runtime_path(command: &mut Command, target: &GenerateTarget) {
    if let GenerateTarget::Rust { runtime_path } = target {
        command.arg("--rust-runtime-path").arg(runtime_path);
    }
}

fn rust_target() -> GenerateTarget {
    GenerateTarget::Rust {
        runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
    }
}

fn run_csharp_fixture(output: &Path, mode: &str) -> TestResult<()> {
    let fixture = if mode == "large" {
        include_str!("fixtures/csv_output_large_csharp.cs.txt")
    } else {
        include_str!("fixtures/csv_output_csharp.cs.txt")
    };
    run_csharp(output, fixture, mode)
}

fn run_rust_fixture(output: &Path, mode: &str) -> TestResult<()> {
    std::fs::write(
        output.join("src/main.rs"),
        include_str!("fixtures/csv_output_rust_harness.rs.txt"),
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
        None => output
            .parent()
            .expect("generated directory has a parent")
            .join("cargo-target"),
    };
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet", "--jobs", "1", "--", mode])
        .current_dir(output)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_INCREMENTAL", "0");
    let result = recorded_csv_command(&mut command, output, "rust-host")?;
    assert!(
        result.status.success(),
        "generated Rust CSV {mode} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("generated CSV checks passed"));
    Ok(())
}

#[test]
fn generated_csharp_csv_small_matrix_and_cli_flag_match_native() -> TestResult<()> {
    small_matrix("csharp", GenerateTarget::CSharp, run_csharp_fixture)
}

#[test]
fn generated_csharp_csv_real_maximum_plus_one_and_failure_precedence() -> TestResult<()> {
    large_matrix("csharp", GenerateTarget::CSharp, run_csharp_fixture)
}

#[test]
fn generated_rust_csv_small_matrix_and_cli_flag_match_native() -> TestResult<()> {
    small_matrix("rust", rust_target(), run_rust_fixture)
}

#[test]
fn generated_rust_csv_real_maximum_plus_one_and_failure_precedence() -> TestResult<()> {
    large_matrix("rust", rust_target(), run_rust_fixture)
}

// Static named CSV uses the ordinary typed mapping API, with no file loading.
fn named_csv_project(options: FormatOptions) -> Project {
    let mut project = csv_project(options);
    project.extra_sources = vec![
        mapping::NamedSource {
            name: "Settings".into(),
            path: "settings.json".into(),
            schema: SchemaNode::group(
                "Settings",
                vec![SchemaNode::scalar("Label", ScalarType::String)],
            ),
            options: FormatOptions::default(),
            dynamic_path: None,
        },
        mapping::NamedSource {
            name: "Catalog".into(),
            path: "catalog.json".into(),
            schema: SchemaNode::group(
                "Catalog",
                vec![SchemaNode::group("Rows", Vec::new()).repeating()],
            ),
            options: FormatOptions::default(),
            dynamic_path: None,
        },
    ];
    project.graph.nodes.insert(
        9,
        Node::SourceField {
            path: vec!["Settings".into(), "Label".into()],
            frame: None,
        },
    );
    project.graph.nodes.insert(
        11,
        Node::Aggregate {
            function: mapping::AggregateOp::Count,
            collection: vec!["Catalog".into(), "Rows".into()],
            value: Vec::new(),
            expression: None,
            arg: None,
        },
    );
    project.graph.nodes.insert(
        18,
        Node::RuntimeParameterDefault {
            name: "Fail".into(),
            ty: ScalarType::Bool,
            default: 13,
            preview: None,
        },
    );
    project.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: Vec::new(),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 18 },
        message: Some(10),
    });
    project
}

fn named_csv_inputs(label: Value, count: usize) -> Vec<(String, Instance)> {
    vec![
        (
            "Settings".into(),
            Instance::Group(vec![("Label".into(), Instance::Scalar(label))].into()),
        ),
        (
            "Catalog".into(),
            Instance::Group(
                vec![(
                    "Rows".into(),
                    Instance::Repeated(
                        (0..count)
                            .map(|_| Instance::Group(Vec::new().into()))
                            .collect(),
                    ),
                )]
                .into(),
            ),
        ),
    ]
}

fn retain_named_native(
    directory: &Path,
    tag: &str,
    source: &Instance,
    inputs: &[(String, Instance)],
    parameters: &engine::RuntimeParameters,
) -> TestResult<()> {
    use std::io::Write as _;
    let mut file = std::fs::File::create(directory.join(format!("{tag}-native-inputs.txt")))?;
    writeln!(
        file,
        "Source: {source:#?}\nInputs: {inputs:#?}\nParameters: {parameters:#?}"
    )?;
    Ok(())
}

fn named_native_csv(
    directory: &Path,
    tag: &str,
    project: &Project,
    source: &Instance,
    inputs: &[(String, Instance)],
    parameters: &engine::RuntimeParameters,
) -> TestResult<Vec<u8>> {
    retain_named_native(directory, tag, source, inputs, parameters)?;
    let primary = engine::run_with_sources_and_context(
        project,
        source,
        inputs.to_vec(),
        &context(parameters),
    );
    std::fs::write(
        directory.join(format!("{tag}-native-mapping.txt")),
        format!("{primary:#?}\n"),
    )?;
    let bytes = native_bytes(project, &primary?);
    record_native_bytes(directory, tag, &bytes)?;
    Ok(bytes?)
}

fn named_csv_native_errors(
    directory: &Path,
    project: &Project,
    source: &Instance,
    inputs: &[(String, Instance)],
) -> TestResult<()> {
    for (tag, parameter, expected) in [
        (
            "required",
            "Require",
            engine::EngineError::MissingRuntimeParameter {
                node: 15,
                name: "Required".into(),
            },
        ),
        (
            "failure-rule",
            "Fail",
            engine::EngineError::MappingFailure {
                rule: 1,
                message: Some(match &inputs[0].1 {
                    Instance::Group(fields) => match fields
                        .iter()
                        .find(|(name, _)| name == "Label")
                        .map(|(_, value)| value)
                        .unwrap()
                    {
                        Instance::Scalar(Value::String(label)) => label.clone(),
                        _ => unreachable!(),
                    },
                    _ => unreachable!(),
                }),
            },
        ),
    ] {
        let mut parameters = engine::RuntimeParameters::new();
        parameters.insert(parameter, Value::Bool(true))?;
        retain_named_native(directory, tag, source, inputs, &parameters)?;
        let result = engine::run_with_sources_and_context(
            project,
            source,
            inputs.to_vec(),
            &context(&parameters),
        );
        std::fs::write(
            directory.join(format!("{tag}-native-mapping.txt")),
            format!("{result:#?}\n"),
        )?;
        assert_eq!(result, Err(expected));
    }
    let mut wrong = engine::RuntimeParameters::new();
    wrong.insert("Count", Value::Bool(true))?;
    retain_named_native(directory, "wrong-count", source, inputs, &wrong)?;
    let result =
        engine::run_with_sources_and_context(project, source, inputs.to_vec(), &context(&wrong));
    std::fs::write(
        directory.join("wrong-count-native-mapping.txt"),
        format!("{result:#?}\n"),
    )?;
    assert_eq!(
        result,
        Err(engine::EngineError::RuntimeParameterType {
            node: 12,
            name: "Count".into(),
            expected: ScalarType::Int,
            found: "bool",
        })
    );
    Ok(())
}

fn named_csv_small_matrix(
    language: &str,
    target: GenerateTarget,
    run_host: fn(&Path, &str) -> TestResult<()>,
) -> TestResult<()> {
    for mode in ["custom", "disabled"] {
        let mut directory = CsvDirectory::new(&format!("{language}_csv_named_{mode}"))?;
        let options = if mode == "custom" {
            FormatOptions {
                delimiter: Some(';'),
                csv_quote: Some('\''),
                csv_utf8_bom: true,
                ..Default::default()
            }
        } else {
            FormatOptions {
                delimiter: Some('\t'),
                csv_quote_disabled: true,
                has_header_row: Some(false),
                ..Default::default()
            }
        };
        let project = named_csv_project(options);
        let source = if mode == "custom" {
            source()
        } else {
            Instance::Repeated(vec![row("safe's\"quoted", -7, 1.25, true, true, 1)])
        };
        let label = if mode == "custom" {
            "名;'\n🙂"
        } else {
            "named"
        };
        let inputs = named_csv_inputs(Value::String(label.into()), 3);
        let path = write_mapping(&directory.path, &project)?;
        let output = directory.path.join("generated");
        generate_project_with_csv_output(&path, &output, target.clone())?;
        let ordinary = directory.path.join("ordinary");
        generate_project(&path, &ordinary, target.clone())?;
        let ordinary_before = artifact_files(&ordinary)?;
        assert!(
            ordinary_before
                .iter()
                .all(|(name, _)| !name.contains("Csv"))
        );
        let defaults = engine::RuntimeParameters::new();
        let expected = named_native_csv(
            &output,
            "expected-default",
            &project,
            &source,
            &inputs,
            &defaults,
        )?;
        let literal = if mode == "custom" {
            "\u{feff}'Text''\n😀';Integer;Number;Boolean;Absent;HostLabel;HostCount;Guard\n'A;''B\"\n😀';-7;0.0000001;true;;'名;''\n🙂';3;42\n;0;-0;false;;'名;''\n🙂';3;42\n"
        } else {
            "safe's\"quoted\t-7\t1.25\ttrue\t\tnamed\t3\t42\n"
        };
        assert_eq!(expected, literal.as_bytes());
        let mut reversed = inputs.clone();
        reversed.reverse();
        assert_eq!(
            named_native_csv(
                &output,
                "expected-reversed",
                &project,
                &source,
                &reversed,
                &defaults
            )?,
            expected
        );
        let mut supplied = engine::RuntimeParameters::new();
        supplied.insert("Label", Value::String("host".into()))?;
        supplied.insert("Count", Value::String(" 9 ".into()))?;
        named_native_csv(
            &output,
            "expected-context",
            &project,
            &source,
            &inputs,
            &supplied,
        )?;
        let mut null = engine::RuntimeParameters::new();
        null.insert("Count", Value::Null)?;
        named_native_csv(&output, "expected-null", &project, &source, &inputs, &null)?;
        let named_null = named_csv_inputs(Value::Null, 3);
        named_native_csv(
            &output,
            "expected-named-null",
            &project,
            &source,
            &named_null,
            &defaults,
        )?;
        let empty_catalog = named_csv_inputs(Value::String(label.into()), 0);
        named_native_csv(
            &output,
            "expected-zero-count",
            &project,
            &source,
            &empty_catalog,
            &defaults,
        )?;
        let mut required = engine::RuntimeParameters::new();
        required.insert("Require", Value::Bool(true))?;
        let empty = Instance::Repeated(Vec::new());
        let empty_bytes = named_native_csv(
            &output,
            "expected-empty",
            &project,
            &empty,
            &inputs,
            &required,
        )?;
        assert_eq!(
            empty_bytes,
            if mode == "custom" {
                "\u{feff}'Text''\n😀';Integer;Number;Boolean;Absent;HostLabel;HostCount;Guard\n"
                    .as_bytes()
            } else {
                b""
            }
        );
        let filtered = Instance::Repeated(vec![row("not evaluated", -7, 1.25, true, false, 1)]);
        assert_eq!(
            named_native_csv(
                &output,
                "expected-filtered",
                &project,
                &filtered,
                &inputs,
                &required
            )?,
            empty_bytes
        );
        named_csv_native_errors(&output, &project, &source, &inputs)?;
        let bad = Instance::Repeated(vec![row("bad\tvalue", -7, 1.25, true, true, 2)]);
        if mode == "disabled" {
            retain_named_native(&output, "noquote", &bad, &inputs, &defaults)?;
            let primary = engine::run_with_sources(&project, &bad, inputs.clone());
            std::fs::write(
                output.join("noquote-native-mapping.txt"),
                format!("{primary:#?}\n"),
            )?;
            let result = native_bytes(&project, &primary?);
            record_native_bytes(&output, "noquote", &result)?;
            assert!(
                matches!(result, Err(CsvBoundedError::Format(CsvFormatError::UnquotedFieldBoundary { row: 0, field })) if field == TEXT_COLUMN)
            );
        }
        let late = Instance::Repeated(vec![
            row("bad\tvalue", -7, 1.25, true, true, 2),
            row("late", -7, f64::INFINITY, true, true, 1),
        ]);
        retain_named_native(&output, "late-invalid", &late, &inputs, &defaults)?;
        let primary = engine::run_with_sources(&project, &late, inputs.clone());
        std::fs::write(
            output.join("late-invalid-native-mapping.txt"),
            format!("{primary:#?}\n"),
        )?;
        let result = native_bytes(&project, &primary?);
        record_native_bytes(&output, "late-invalid", &result)?;
        assert!(
            matches!(result, Err(CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 1, field, expected: ScalarType::Float, got: "non-finite float",
        })) if field == "Number")
        );
        let nil = Instance::Repeated(vec![row_value(Value::xml_nil(), -7, 1.25, true, true, 1)]);
        retain_named_native(&output, "nil", &nil, &inputs, &defaults)?;
        let primary = engine::run_with_sources(&project, &nil, inputs.clone());
        std::fs::write(
            output.join("nil-native-mapping.txt"),
            format!("{primary:#?}\n"),
        )?;
        let result = native_bytes(&project, &primary?);
        record_native_bytes(&output, "nil", &result)?;
        assert!(
            matches!(result, Err(CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 0, field, expected: ScalarType::String, got: "xml nil",
        })) if field == TEXT_COLUMN)
        );
        run_host(&output, mode)?;
        assert_eq!(artifact_files(&ordinary)?, ordinary_before);
        directory.complete = true;
    }
    Ok(())
}

fn named_csv_large_matrix(
    language: &str,
    target: GenerateTarget,
    run_host: fn(&Path, &str) -> TestResult<()>,
) -> TestResult<()> {
    let mut directory = CsvDirectory::new(&format!("{language}_csv_named_large"))?;
    let project = named_csv_project(FormatOptions {
        csv_utf8_bom: true,
        ..Default::default()
    });
    let source = Instance::Repeated(vec![row("safe", -7, 1.25, true, true, 2)]);
    let stub = named_csv_inputs(Value::String("\"".into()), 3);
    let path = write_mapping(&directory.path, &project)?;
    let output = directory.path.join("generated");
    generate_project_with_csv_output(&path, &output, target)?;
    let defaults = engine::RuntimeParameters::new();
    let stub_bytes = named_native_csv(
        &output,
        "native-large-stub",
        &project,
        &source,
        &stub,
        &defaults,
    )?;
    let extra = MAXIMUM
        .checked_sub(stub_bytes.len())
        .expect("small complete CSV fits");
    std::fs::write(output.join("large-extra.txt"), extra.to_string())?;
    {
        let inputs = named_csv_inputs(Value::String(format!("\"{}", "x".repeat(extra))), 3);
        let bytes = named_native_csv(
            &output,
            "expected-maximum",
            &project,
            &source,
            &inputs,
            &defaults,
        )?;
        assert_eq!(bytes.len(), MAXIMUM);
        assert!(bytes.starts_with(&[0xef, 0xbb, 0xbf]));
    }
    {
        let inputs = named_csv_inputs(Value::String(format!("\"{}", "x".repeat(extra + 1))), 3);
        retain_named_native(&output, "plus-one", &source, &inputs, &defaults)?;
        let primary = engine::run_with_sources(&project, &source, inputs.clone());
        // Stream the complete original output tree rather than duplicating its formatted Debug.
        use std::io::Write as _;
        writeln!(
            std::fs::File::create(output.join("plus-one-native-mapping.txt"))?,
            "{primary:#?}"
        )?;
        let result = native_bytes(&project, &primary?);
        record_native_bytes(&output, "plus-one", &result)?;
        assert!(matches!(
            result,
            Err(CsvBoundedError::OutputTooLarge { maximum: MAXIMUM })
        ));
        let late = Instance::Repeated(vec![
            row("safe", -7, 1.25, true, true, 2),
            row("late", -7, f64::INFINITY, true, true, 1),
        ]);
        retain_named_native(&output, "large-late-invalid", &late, &inputs, &defaults)?;
        let primary = engine::run_with_sources(&project, &late, inputs.clone());
        writeln!(
            std::fs::File::create(output.join("large-late-invalid-native-mapping.txt"))?,
            "{primary:#?}"
        )?;
        let result = native_bytes(&project, &primary?);
        record_native_bytes(&output, "large-late-invalid", &result)?;
        assert!(
            matches!(result, Err(CsvBoundedError::Format(CsvFormatError::ValueType {
            row: 1, field, expected: ScalarType::Float, got: "non-finite float",
        })) if field == "Number")
        );
        let mut required = engine::RuntimeParameters::new();
        required.insert("Require", Value::Bool(true))?;
        let result = engine::run_with_sources_and_context(
            &project,
            &late,
            inputs.clone(),
            &context(&required),
        );
        writeln!(
            std::fs::File::create(output.join("large-required-native-mapping.txt"))?,
            "{result:#?}"
        )?;
        assert_eq!(
            result,
            Err(engine::EngineError::MissingRuntimeParameter {
                node: 15,
                name: "Required".into()
            })
        );
    }
    run_host(&output, "large")?;
    directory.complete = true;
    Ok(())
}

fn run_named_csharp_fixture(output: &Path, mode: &str) -> TestResult<()> {
    run_csharp(
        output,
        include_str!("fixtures/csv_output_named_csharp.cs.txt"),
        mode,
    )
}

fn run_named_rust_fixture(output: &Path, mode: &str) -> TestResult<()> {
    std::fs::write(
        output.join("src/main.rs"),
        include_str!("fixtures/csv_output_named_rust_harness.rs.txt"),
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
        None => output
            .parent()
            .expect("generated directory has a parent")
            .join("cargo-target"),
    };
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet", "--jobs", "1", "--", mode])
        .current_dir(output)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_INCREMENTAL", "0");
    let result = recorded_csv_command(&mut command, output, "rust-named-host")?;
    assert!(
        result.status.success(),
        "generated named Rust CSV {mode} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("generated CSV checks passed"));
    Ok(())
}

#[test]
fn generated_rust_csv_static_named_inputs_and_name_error_order_match_ordinary() -> TestResult<()> {
    named_csv_small_matrix("rust", rust_target(), run_named_rust_fixture)
}
#[test]
fn generated_csharp_csv_static_named_inputs_and_name_error_order_match_ordinary() -> TestResult<()>
{
    named_csv_small_matrix("csharp", GenerateTarget::CSharp, run_named_csharp_fixture)
}
#[test]
fn generated_rust_csv_static_named_real_limit_and_error_precedence() -> TestResult<()> {
    named_csv_large_matrix("rust", rust_target(), run_named_rust_fixture)
}
#[test]
fn generated_csharp_csv_static_named_real_limit_and_error_precedence() -> TestResult<()> {
    named_csv_large_matrix("csharp", GenerateTarget::CSharp, run_named_csharp_fixture)
}

#[test]
fn generated_csv_dynamic_named_refusal_is_typed_before_any_destination_publication()
-> TestResult<()> {
    let mut directory = CsvDirectory::new("csv_named_dynamic_refusal")?;
    let mut project = named_csv_project(FormatOptions::default());
    project.graph.nodes.insert(
        19,
        Node::Const {
            value: Value::String("dynamic.json".into()),
        },
    );
    for name in ["FirstDynamic", "SecondDynamic"] {
        project.extra_sources.push(mapping::NamedSource {
            name: name.into(),
            path: "unused.json".into(),
            schema: SchemaNode::group(name, Vec::new()),
            options: FormatOptions::default(),
            dynamic_path: Some(mapping::DynamicSourcePath {
                node: 19,
                iteration: Vec::new(),
            }),
        });
    }
    let path = write_mapping(&directory.path, &project)?;
    for (language, target) in [("rust", rust_target()), ("csharp", GenerateTarget::CSharp)] {
        let output = directory.path.join(language);
        let result = generate_project_with_csv_output(&path, &output, target);
        std::fs::write(
            directory
                .path
                .join(format!("{language}-generation-original.txt")),
            format!("{result:#?}\n"),
        )?;
        let error = result.expect_err("valid dynamic source must refuse the CSV adapter");
        let admission = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<codegen::CsvOutputError>());
        assert_eq!(
            admission,
            Some(&codegen::CsvOutputError::DynamicInputs {
                name: "FirstDynamic".into()
            })
        );
        assert!(!output.exists());
    }
    assert!(std::fs::read_dir(&directory.path)?.all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("ferrule-stage")
    }));
    directory.complete = true;
    Ok(())
}
