use super::*;
use serde_json::{Value as Json, json};
use std::cell::RefCell;
use std::sync::Arc;

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        let directory = TempDir::new(&format!("xml_dynamic_inputs_{language}"))?;
        let path = directory.0.clone();
        std::mem::forget(directory);
        Ok(Self {
            path,
            complete: false,
        })
    }
}
impl Drop for RegressionDirectory {
    fn drop(&mut self) {
        if self.complete {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained dynamic XML input test artifacts: {}",
                self.path.display()
            );
        }
    }
}
struct NativeLoader<'a> {
    sources: Vec<(&'a str, &'a SchemaNode)>,
    directory: &'a Path,
    calls: RefCell<Vec<(String, String)>>,
}
impl engine::DynamicSourceLoader for NativeLoader<'_> {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        let (_, schema) = self
            .sources
            .iter()
            .find(|(name, _)| *name == source)
            .expect("one declared dynamic source");
        assert!(matches!(path, "a.xml" | "b.xml"));
        self.calls.borrow_mut().push((source.into(), path.into()));
        let document = std::fs::read_to_string(self.directory.join(path))
            .map_err(|error| error.to_string())?;
        let instance =
            format_xml::from_str(&document, schema).map_err(|error| error.to_string())?;
        if self.sources.len() == 2 {
            for row in repeated_field(&instance, "Row") {
                assert!(
                    matches!(
                        (source, group_field(row, "Amount")),
                        ("alpha", Instance::Scalar(Value::Float(_)))
                            | ("beta", Instance::Scalar(Value::Int(_)))
                    ),
                    "source-specific parsed numeric tag under the same logical path"
                );
            }
        }
        Ok(Arc::new(instance))
    }
}
fn recorded(command: &mut Command, directory: &Path, name: &str) -> io::Result<Output> {
    std::fs::write(
        directory.join(format!("{name}-command.txt")),
        format!("{command:?}\n"),
    )?;
    let output = command.isolated_output()?;
    std::fs::write(directory.join(format!("{name}-stdout.txt")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.txt")), &output.stderr)?;
    std::fs::write(
        directory.join(format!("{name}-status.json")),
        serde_json::to_vec(
            &json!({"success":output.status.success(),"code":output.status.code()}),
        )?,
    )?;
    Ok(output)
}
fn group_field<'a>(instance: &'a Instance, name: &str) -> &'a Instance {
    let Instance::Group(fields) = instance else {
        panic!("typed group")
    };
    &fields
        .iter()
        .find(|(field, _)| field == name)
        .expect("complete named field")
        .1
}
fn repeated_field<'a>(instance: &'a Instance, name: &str) -> &'a [Instance] {
    let Instance::Repeated(rows) = group_field(instance, name) else {
        panic!("complete repeated field")
    };
    rows
}
fn exercise(language: &str, multiple: bool) -> TestResult<()> {
    let mode = if multiple { "multiple" } else { "single" };
    let mut directory = RegressionDirectory::new(&format!("{language}_{mode}"))?;
    let project: Project = serde_json::from_str(if multiple {
        include_str!(
            "../../../codegen/src/tests/fixtures/multiple_dynamic_named_xml_input_mixed.json"
        )
    } else {
        include_str!("../../../codegen/src/tests/fixtures/dynamic_named_xml_input_mixed.json")
    })?;
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    let boundary = lowered
        .xml_boundary
        .as_ref()
        .expect("ordinary Structured XML admission");
    assert_eq!(
        boundary.input.profile(),
        Some(codegen::XmlInputProfile::Structured)
    );
    assert_eq!(boundary.extra_inputs.len(), if multiple { 4 } else { 3 });
    assert_eq!(
        project.extra_sources[1].name,
        if multiple { "alpha" } else { "catalog" }
    );
    assert!(lowered.extra_sources[1].dynamic.is_some());
    if multiple {
        assert_eq!(project.extra_sources[3].name, "beta");
        assert!(lowered.extra_sources[3].dynamic.is_some());
    }
    for (name, document) in [
        (
            "primary.xml",
            include_str!("fixtures/xml_dynamic_inputs/primary.xml"),
        ),
        (
            "rates.xml",
            include_str!("fixtures/xml_dynamic_inputs/rates.xml"),
        ),
        (
            "labels.xml",
            include_str!("fixtures/xml_dynamic_inputs/labels.xml"),
        ),
        (
            "a.xml",
            if multiple {
                include_str!("fixtures/xml_dynamic_inputs/multiple-a.xml")
            } else {
                include_str!("fixtures/xml_dynamic_inputs/a.xml")
            },
        ),
        (
            "b.xml",
            if multiple {
                include_str!("fixtures/xml_dynamic_inputs/multiple-b.xml")
            } else {
                include_str!("fixtures/xml_dynamic_inputs/b.xml")
            },
        ),
    ] {
        std::fs::write(directory.path.join(name), document)?;
    }
    let path = directory.path.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let generated = directory.path.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut generate = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    generate
        .args(["--diagnostics", "json", "generate", "--project"])
        .arg(&path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        generate.arg("--rust-runtime-path").arg(&runtime);
    }
    let output = recorded(&mut generate, &directory.path, "generate")?;
    assert!(
        output.status.success(),
        "public CLI generation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let unchanged = artifact_files(&generated)?;
    let host = directory.path.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    let package = if multiple {
        "ferrule-xml-multiple-dynamic-input-regression-host"
    } else {
        "ferrule-xml-dynamic-input-regression-host"
    };
    let output = if language == "rust" {
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("fixtures/xml_dynamic_inputs_rust.rs.txt"),
        )?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname={package:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nserde_json=\"1\"\n"
            ),
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
            None => directory.path.join("rust-target"),
        };
        let mut build = Command::new("cargo");
        build
            .args(["build", "--quiet", "--jobs", "1"])
            .current_dir(&host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-D warnings");
        let result = recorded(&mut build, &directory.path, "build")?;
        assert!(
            result.status.success(),
            "Rust host build: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let binary = target
            .join(format!("debug/{package}"))
            .with_extension(std::env::consts::EXE_EXTENSION);
        let mut run = Command::new(binary);
        run.arg(&directory.path).arg(mode).current_dir(&host);
        recorded(&mut run, &directory.path, "host")?
    } else {
        std::fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/xml_dynamic_inputs_csharp.cs.txt"),
        )?;
        std::fs::write(
            host.join("Host.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
        )?;
        let artifacts = directory.path.join("artifacts");
        let mut build = dotnet_command(&directory.path);
        build
            .args([
                "build",
                "--nologo",
                "--verbosity",
                "quiet",
                "--configuration",
                "Release",
                "--artifacts-path",
            ])
            .arg(&artifacts)
            .args(["-m:1", "/p:UseSharedCompilation=false", "host/Host.csproj"])
            .current_dir(&directory.path)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let result = recorded(&mut build, &directory.path, "build")?;
        assert!(
            result.status.success(),
            "C# host build: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let mut run = dotnet_command(&directory.path);
        run.arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(&directory.path)
            .arg(mode)
            .current_dir(&directory.path)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        recorded(&mut run, &directory.path, "host")?
    };
    assert_eq!(
        artifact_files(&generated)?,
        unchanged,
        "generated library remains unchanged"
    );
    assert!(
        output.status.success(),
        "actual public loader host: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let rows = std::str::from_utf8(&output.stdout)?
        .lines()
        .map(serde_json::from_str::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        rows.len(),
        9,
        "all eight additive APIs and one original product refusal"
    );
    let primary = format_xml::from_str(
        &std::fs::read_to_string(directory.path.join("primary.xml"))?,
        &project.source,
    )?;
    let inputs = [0, 2]
        .into_iter()
        .map(|index| {
            let source = &project.extra_sources[index];
            let document =
                std::fs::read_to_string(directory.path.join(format!("{}.xml", source.name)))?;
            Ok((
                source.name.clone(),
                format_xml::from_str(&document, &source.schema)?,
            ))
        })
        .collect::<TestResult<Vec<_>>>()?;
    let loader = NativeLoader {
        sources: project
            .extra_sources
            .iter()
            .filter(|source| source.dynamic_path.is_some())
            .map(|source| (source.name.as_str(), &source.schema))
            .collect(),
        directory: &directory.path,
        calls: RefCell::default(),
    };
    let execution = engine::ExecutionContext::new(&path).with_dynamic_source_loader(&loader);
    let native =
        engine::run_outputs_with_sources_and_context(&project, &primary, inputs, &execution)?;
    let line_names: &[&str] = if multiple {
        &["AlphaLine", "BetaLine"]
    } else {
        &["Line"]
    };
    for name in line_names {
        let lines = repeated_field(&native.primary, name);
        assert_eq!(
            lines.len(),
            5,
            "two first-document rows, one second-document row, repeated first document"
        );
        let driver_ids = lines
            .iter()
            .map(|line| group_field(line, "DriverId").clone())
            .collect::<Vec<_>>();
        assert_eq!(
            driver_ids,
            [10, 10, 20, 30, 30].map(|id| Instance::Scalar(Value::Int(id)))
        );
        if multiple {
            let amounts = lines
                .iter()
                .map(|line| group_field(line, "Amount").clone())
                .collect::<Vec<_>>();
            let expected = if *name == "AlphaLine" {
                [9007199254740992.0, 9.0, 8.0, 9007199254740992.0, 9.0]
                    .map(|value| Instance::Scalar(Value::Float(value)))
            } else {
                [9007199254740993, 9, 8, 9007199254740993, 9]
                    .map(|value| Instance::Scalar(Value::Int(value)))
            };
            assert_eq!(
                amounts, expected,
                "same bytes and path use distinct Float/Int schemas; precision makes dispatch observable"
            );
        }
    }
    let calls = if multiple {
        json!([
            ["alpha", "a.xml"],
            ["alpha", "b.xml"],
            ["alpha", "a.xml"],
            ["beta", "a.xml"],
            ["beta", "b.xml"],
            ["beta", "a.xml"]
        ])
    } else {
        json!([
            ["catalog", "a.xml"],
            ["catalog", "b.xml"],
            ["catalog", "a.xml"]
        ])
    };
    assert_eq!(
        json!(loader.calls.borrow().clone()),
        calls,
        "independent engine driver order, repeated path and Null skip"
    );
    for (api, row) in rows[..8].iter().enumerate() {
        assert_eq!(row["api"], api);
        assert_eq!(row["kind"], "ok");
        assert_eq!(row["calls"], calls);
        assert_eq!(
            format_xml::from_str(
                row["primary"].as_str().expect("complete XML"),
                &project.target
            )?,
            native.primary
        );
        let extras = row["extras"].as_array().expect("ordered complete outputs");
        if matches!(api, 2 | 3 | 6 | 7) {
            assert!(extras.is_empty());
        } else {
            assert_eq!(extras.len(), native.extras.len());
            for ((actual, expected), target) in extras
                .iter()
                .zip(&native.extras)
                .zip(&project.extra_targets)
            {
                assert_eq!(actual["name"], expected.name);
                assert_eq!(
                    format_xml::from_str(
                        actual["document"].as_str().expect("named XML"),
                        &target.schema
                    )?,
                    expected.instance
                );
            }
        }
    }
    let refusal = if multiple {
        json!({"api":8,"kind":"Utf8","input_index":3,"source":"beta","path":"b.xml",
        "ordinal":5,"callback_invoked":true,"boundary_identity":true,"typed_cause":true,"no_output":true,
        "calls":[["alpha","a.xml"],["alpha","b.xml"],["alpha","a.xml"],["beta","a.xml"],["beta","b.xml"]]})
    } else {
        json!({"api":8,"kind":"Utf8","input_index":1,"source":"catalog","path":"b.xml",
        "ordinal":2,"callback_invoked":true,"boundary_identity":true,"typed_cause":true,"no_output":true,
        "calls":[["catalog","a.xml"],["catalog","b.xml"]]})
    };
    assert_eq!(rows[8], refusal);
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_dynamic_xml_loader_preserves_outputs_and_original_product_refusal()
-> TestResult<()> {
    exercise("rust", false)
}
#[test]
fn generated_csharp_dynamic_xml_loader_preserves_outputs_and_original_product_refusal()
-> TestResult<()> {
    exercise("csharp", false)
}

#[test]
fn generated_rust_multiple_dynamic_xml_loaders_preserve_source_schemas_and_original_product_refusal()
-> TestResult<()> {
    exercise("rust", true)
}
#[test]
fn generated_csharp_multiple_dynamic_xml_loaders_preserve_source_schemas_and_original_product_refusal()
-> TestResult<()> {
    exercise("csharp", true)
}
