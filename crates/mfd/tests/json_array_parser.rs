use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_array_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json-array-parser/mapping.mfd")
}
fn imported() -> mapping::Project {
    let imported = mfd::import_with_profile(
        &fixture(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    imported.project
}
fn run(project: &mapping::Project, text: &str) -> Result<Instance, engine::EngineError> {
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert("InputText", Value::String(text.into()))
        .unwrap();
    let context =
        engine::ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
    engine::run_with_context(project, &Instance::Group(Vec::new().into()), &context)
}

#[test]
fn array_records_preserve_order_cardinality_nested_fields_and_null_layers() {
    let project = imported();
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/json-array-parser/cases.json")).unwrap();
    for case in cases {
        let input = case["input"].as_str().unwrap();
        let result = run(&project, input);
        if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument {
                function: "json_parse_array_length", message: actual
            })) if actual == message),
                "{case}: {result:?}"
            );
        } else {
            let expected =
                format_json::from_str(&case["expected"].to_string(), &project.target).unwrap();
            assert_eq!(result.unwrap(), expected, "{case}");
        }
    }
}

#[test]
fn optional_parent_absence_requires_valid_descriptors_and_schema_valid_input() {
    let suite: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/json-array-parser/absence-cases.json"
    ))
    .unwrap();
    for case in suite["cases"].as_array().unwrap() {
        let mut schema = suite["schema"].clone();
        match case["required"].as_str() {
            Some("root") => schema["kind"]["required"] = serde_json::json!(["Envelope"]),
            Some("rows") => {
                schema["kind"]["children"][0]["kind"]["required"] = serde_json::json!(["Rows"])
            }
            _ => {}
        }
        let function = case["function"].as_str().unwrap();
        let mut args = vec![
            Value::String(case["input"].as_str().unwrap().into()),
            Value::String(schema.to_string()),
            Value::String(case["array"].to_string()),
        ];
        if function == "json_parse_array_field" {
            args.extend([Value::String(case["path"].to_string()), Value::Int(1)]);
        }
        let result = functions::call(function, &args);
        if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(functions::FunctionError::InvalidArgument { function: actual_function, message: actual }) if actual_function == function && actual == message),
                "{case}: {result:?}"
            );
        } else {
            let expected = match &case["expected"] {
                serde_json::Value::Null => Value::Null,
                serde_json::Value::String(value) => Value::String(value.clone()),
                value => Value::Int(value.as_i64().unwrap()),
            };
            assert_eq!(result.unwrap(), expected, "{case}");
        }
    }
}

#[test]
fn array_records_infer_the_same_driver_from_scalar_connections() {
    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    let text = std::fs::read_to_string(&path).unwrap().replace(
        "<vertex vertexkey=\"21\"><edges><edge vertexkey=\"30\"/></edges></vertex>",
        "",
    );
    std::fs::write(&path, text).unwrap();
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let output = run(&imported.project, r#"{"Rows":[{"Code":"A","Qty":2,"Detail":{"Label":"x"}},{"Code":"B","Qty":3,"Detail":{"Label":"y"}}]}"#).unwrap();
    assert_eq!(
        output
            .field("Records")
            .unwrap()
            .as_repeated()
            .unwrap()
            .len(),
        2
    );
}

fn copy_fixture() -> TempDir {
    let dir = TempDir::new();
    for name in [
        "mapping.mfd",
        "payload.schema.json",
        "target.schema.json",
        "source.schema.json",
    ] {
        std::fs::copy(fixture().parent().unwrap().join(name), dir.0.join(name)).unwrap();
    }
    dir
}

#[test]
fn unsupported_array_profiles_refuse_executable_import() {
    for (file, from, to, reason) in [
        (
            "mapping.mfd",
            "datatype=\"string\"",
            "datatype=\"integer\"",
            "required global String",
        ),
        (
            "mapping.mfd",
            "name=\"InputText\" usageKind=\"input\"",
            "name=\"InputText\" usageKind=\"input\" optional=\"1\"",
            "required global String",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
            "",
            "required global String",
        ),
        (
            "mapping.mfd",
            "schema=\"payload.schema.json\"",
            "",
            "resolved JSON schema",
        ),
        (
            "mapping.mfd",
            "schema=\"payload.schema.json\"",
            "schema=\"missing.schema.json\"",
            "resolved JSON schema",
        ),
        (
            "mapping.mfd",
            "<json schema=\"payload.schema.json\"/>",
            "<json schema=\"payload.schema.json\"/><json schema=\"payload.schema.json\"/>",
            "resolved JSON schema",
        ),
        (
            "mapping.mfd",
            "<entry name=\"number\" inpkey=\"32\"/>",
            "<entry name=\"number\" inpkey=\"32\" casttotargettypemode=\"0\"/>",
            "target node controls",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"23\">",
            "<vertex vertexkey=\"10\">",
            "other connected record feeds",
        ),
        (
            "mapping.mfd",
            "<entry name=\"array\" inpkey=\"30\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\">",
            "<entry name=\"array\" inpkey=\"30\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\" clone=\"1\"/><entry name=\"object\">",
            "cloned occurrences",
        ),
        (
            "payload.schema.json",
            "\"type\": \"array\"",
            "\"type\": [\"array\", \"null\"]",
            "unsupported array",
        ),
        (
            "target.schema.json",
            "\"type\": \"array\"",
            "\"type\": \"object\"",
            "repeating record target",
        ),
        (
            "payload.schema.json",
            "\"type\": \"object\",\n            \"properties\": { \"Label\": { \"type\": \"string\" } },",
            "\"type\": \"array\",\n            \"items\": {\"type\":\"object\",\"properties\":{\"Label\":{\"type\":\"string\"}}},",
            "unsupported array",
        ),
    ] {
        let dir = copy_fixture();
        let path = dir.0.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from));
        std::fs::write(&path, text.replace(from, to)).unwrap();
        let outcome = mfd::import_with_profile(
            &dir.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        );
        assert!(
            outcome.is_err(),
            "unsupported profile was executable: {reason}"
        );
        let best_effort = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
        assert!(
            best_effort
                .warnings
                .iter()
                .any(|warning| warning.contains(reason)),
            "{:?}",
            best_effort.warnings
        );
    }
}

#[test]
fn array_record_graphs_lower_and_emit_both_backends() {
    let dir = TempDir::new();
    emit(&dir.0);
}

#[test]
fn array_helpers_export_as_ferrule_extensions_not_native_core_functions() {
    let project = imported();
    let dir = TempDir::new();
    let path = dir.0.join("exported.mfd");
    let report = mfd::preflight_export(&project, &path).unwrap();
    assert!(!report.is_native_compatible());
    assert!(matches!(
        mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!path.exists());
    mfd::export(&project, &path).unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    for name in ["json_parse_array_length", "json_parse_array_field"] {
        let components: Vec<_> = document
            .descendants()
            .filter(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
            .collect();
        assert!(!components.is_empty(), "missing {name}");
        assert!(
            components
                .iter()
                .all(|node| node.attribute("library") == Some("ferrule"))
        );
    }
}

fn emit(root: &Path) {
    let project = imported();
    let program = codegen::lower(&project).unwrap();
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "json-array-mapping".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .canonicalize()
                    .unwrap()
                    .display()
                    .to_string(),
            ),
        },
    )
    .unwrap();
    let csharp = codegen_csharp::emit(&program).unwrap();
    for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
        for file in artifacts.files() {
            let path = root.join(language).join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &file.contents).unwrap();
        }
    }
    std::fs::write(
        root.join("target-schema.json"),
        serde_json::to_string(&project.target).unwrap(),
    )
    .unwrap();
    let schema_node = project
        .graph
        .nodes
        .values()
        .find_map(|node| match node {
            mapping::Node::Call { function, args } if function == "json_parse_array_length" => {
                Some(args[1])
            }
            _ => None,
        })
        .unwrap();
    let mapping::Node::Const {
        value: Value::String(schema),
    } = &project.graph.nodes[&schema_node]
    else {
        panic!("parser schema descriptor");
    };
    std::fs::write(root.join("parser-schema.json"), schema).unwrap();
}

#[test]
#[ignore = "compiles generated Rust and C# Release hosts; requires Cargo and .NET 10"]
fn generated_array_record_release_hosts_match_complete_values_and_typed_failures() {
    let dir = TempDir::new();
    emit(&dir.0);
    let fixtures = fixture();
    let fixtures = fixtures.parent().unwrap();
    let rust = dir.0.join("rust");
    let manifest = rust.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("[dependencies]", "[dependencies]\nserde_json = \"1\"");
    std::fs::write(
        &manifest,
        format!("{text}\n[[bin]]\nname = \"array-record-host\"\npath = \"host.rs\"\n"),
    )
    .unwrap();
    std::fs::copy(fixtures.join("rust_host.rs"), rust.join("host.rs")).unwrap();
    checked(
        std::process::Command::new("cargo")
            .args(["run", "--release", "--offline", "--manifest-path"])
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env("RUSTFLAGS", "-D warnings")
            .arg(manifest)
            .args(["--bin", "array-record-host", "--"])
            .arg(fixtures.join("cases.json"))
            .arg(dir.0.join("target-schema.json"))
            .arg(dir.0.join("parser-schema.json"))
            .arg(fixtures.join("absence-cases.json")),
    );
    let host = dir.0.join("csharp/host");
    std::fs::create_dir(&host).unwrap();
    std::fs::write(
        dir.0.join("NuGet.Config"),
        r#"<configuration><packageSources><clear/></packageSources></configuration>"#,
    )
    .unwrap();
    std::fs::write(host.join("Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
    std::fs::copy(fixtures.join("csharp_host.cs"), host.join("Program.cs")).unwrap();
    let home = dir.0.join("dotnet-home");
    checked(dotnet(&host, &home).arg("--version"));
    checked(
        dotnet(&host, &home)
            .args([
                "run",
                "--configuration",
                "Release",
                "--disable-build-servers",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "--project",
            ])
            .arg(host.join("Host.csproj"))
            .arg("--")
            .arg(fixtures.join("cases.json"))
            .arg(dir.0.join("target-schema.json"))
            .arg(dir.0.join("parser-schema.json"))
            .arg(fixtures.join("absence-cases.json")),
    );
}

fn dotnet(directory: &Path, home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("dotnet");
    command
        .current_dir(directory)
        .env("DOTNET_CLI_HOME", home)
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1");
    command
}

fn checked(command: &mut std::process::Command) {
    let output = command.output().unwrap();
    println!(
        "{command:?}\nstatus: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "{command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
