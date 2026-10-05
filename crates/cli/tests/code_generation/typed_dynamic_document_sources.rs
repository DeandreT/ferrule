use super::*;
use serde_json::{Value as Json, json};

#[path = "fixtures/typed_document_driver_native.rs.txt"]
mod native;
#[path = "fixtures/typed_document_driver_snapshot.rs.txt"]
mod typed_snapshot;

const MARKER: &str = "typed-dispatch-original-second-host-failure";
const CASES: [&str; 4] = [
    "ordinary-child-host-control",
    "document-driver-with-loader",
    "document-driver-no-loader",
    "document-driver-second-host-failure",
];
static HOST_RUN_LOCK: Mutex<()> = Mutex::new(());

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        let directory = TempDir::new(&format!("typed_document_source_{language}"))?;
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
                "Retained typed document source regression artifacts: {}",
                self.path.display()
            );
        }
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
fn string_instance(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn expected_inputs() -> (Instance, [Instance; 2]) {
    let primary = group(vec![(
        "Driver",
        Instance::Repeated(vec![
            group(vec![("Path", string_instance("a.xml"))]),
            group(vec![("Path", string_instance("b.xml"))]),
        ]),
    )]);
    let loaded = [
        group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![
                ("File", string_instance("same.xml")),
                ("Value", string_instance("A")),
            ])]),
        )]),
        group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![
                ("File", string_instance("../雪😀.xml")),
                ("Value", string_instance("  B  ")),
            ])]),
        )]),
    ];
    (primary, loaded)
}
fn expected_ordinary() -> Instance {
    group(vec![(
        "Line",
        Instance::Repeated(vec![
            group(vec![
                ("File", string_instance("same.xml")),
                ("Value", string_instance("A")),
            ]),
            group(vec![
                ("File", string_instance("../雪😀.xml")),
                ("Value", string_instance("  B  ")),
            ]),
        ]),
    )])
}
fn expected_documents() -> Instance {
    Instance::DocumentSet(vec![
        ir::DocumentMember::new("same.xml", group(vec![("Value", string_instance("A"))])).unwrap(),
        ir::DocumentMember::new(
            "../雪😀.xml",
            group(vec![("Value", string_instance("  B  "))]),
        )
        .unwrap(),
    ])
}
fn assert_records(records: &[Json], backend: &str) {
    assert_eq!(records.len(), 4, "complete original four-control matrix");
    let (primary, loaded) = expected_inputs();
    let prepared = json!([
        typed_snapshot::snapshot(&loaded[0]),
        typed_snapshot::snapshot(&loaded[1])
    ]);
    let first_callback = json!({"ordinal":1,"source":"catalog","path":"a.xml","outcome":{"status":"returned","value":typed_snapshot::snapshot(&loaded[0])}});
    let second_callback = json!({"ordinal":2,"source":"catalog","path":"b.xml","outcome":{"status":"returned","value":typed_snapshot::snapshot(&loaded[1])}});
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record["version"], 1);
        assert_eq!(record["backend"], backend);
        assert_eq!(record["case"], CASES[index]);
        assert_eq!(record["loader_enabled"], index != 2);
        assert_eq!(record["fail_second"], index == 3);
        assert_eq!(
            record["project"],
            if index == 0 {
                "ordinary-child-dynamic-source-control.project.json"
            } else {
                "dynamic-source-root-document-driver.project.json"
            }
        );
        assert_eq!(record["primary_input"], typed_snapshot::snapshot(&primary));
        assert_eq!(record["prepared_loaded_sources"], prepared);
        let result = &record["result"];
        if index < 2 {
            let expected = if index == 0 {
                expected_ordinary()
            } else {
                expected_documents()
            };
            assert_eq!(
                result,
                &json!({"status":"success","primary":typed_snapshot::snapshot(&expected),"extras":[],"error":null}),
                "complete typed output including order, paths, tags, origins and significant spaces"
            );
            assert_eq!(
                record["callbacks"],
                json!([first_callback, second_callback])
            );
            continue;
        }
        assert_eq!(result["status"], "error");
        assert!(
            result["primary"].is_null() && result["extras"].is_null(),
            "failed mapping returns no partial output set"
        );
        let error = &result["error"];
        assert_eq!(error["source"], "catalog");
        assert!(error["display"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(error["debug"].as_str().is_some_and(|s| !s.is_empty()));
        if index == 2 {
            assert_eq!(error["kind"], "MissingDynamicSourceLoader");
            assert!(error["path"].is_null());
            assert_eq!(error["causes"], json!([]));
            assert_eq!(record["callbacks"], json!([]));
        } else {
            assert_eq!(error["kind"], "DynamicSourceLoad");
            assert_eq!(error["path"], "b.xml");
            assert_eq!(error["host_message"], MARKER);
            assert_eq!(error["original_host_message_matches"], true);
            let callbacks = record["callbacks"]
                .as_array()
                .expect("actual callback sequence");
            assert_eq!(callbacks.len(), 2);
            assert_eq!(callbacks[0], first_callback);
            assert_eq!(callbacks[1]["ordinal"], 2);
            assert_eq!(callbacks[1]["source"], "catalog");
            assert_eq!(callbacks[1]["path"], "b.xml");
            assert_eq!(callbacks[1]["outcome"]["status"], "error");
            assert_eq!(callbacks[1]["outcome"]["message"], MARKER);
            if backend == "csharp" {
                assert_eq!(
                    callbacks[1]["outcome"]["original_host_exception_reference"],
                    true
                );
                assert_eq!(error["original_host_exception_reference"], true);
                let causes = error["causes"]
                    .as_array()
                    .expect("complete original host cause");
                assert_eq!(causes.len(), 1);
                assert_eq!(causes[0]["message"], MARKER);
                assert_eq!(causes[0]["type"], "System.InvalidOperationException");
                assert_eq!(
                    error["original_runtime_payload"]["error"],
                    "DynamicSourceLoad"
                );
                assert_eq!(error["original_runtime_payload"]["SourceField"], "catalog");
                assert_eq!(error["original_runtime_payload"]["Detail"], "b.xml");
            } else {
                assert_eq!(
                    error["causes"],
                    json!([]),
                    "native/Rust original loader String is the typed error payload"
                );
            }
        }
    }
}
fn target(directory: &Path) -> io::Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let path = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(value) if value.is_empty() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "dedicated generated-host target must not be empty",
            ));
        }
        Some(value) => {
            let value = PathBuf::from(value);
            if value.is_absolute() {
                value
            } else {
                cwd.join(value)
            }
        }
        None => directory.join("rust-target"),
    };
    std::fs::create_dir_all(&path)?;
    let path = path.canonicalize()?;
    if std::env::current_exe()?.canonicalize()?.starts_with(&path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "generated-host target must not contain active test ELF",
        ));
    }
    if let Some(outer) = std::env::var_os("CARGO_TARGET_DIR") {
        let outer = PathBuf::from(outer);
        let outer = if outer.is_absolute() {
            outer
        } else {
            cwd.join(outer)
        };
        if outer.exists() && outer.canonicalize()? == path {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated-host target must differ from outer Cargo target",
            ));
        }
    }
    Ok(path)
}
fn exercise(language: &str) -> TestResult<()> {
    // Covers this module's build and complete host lifetime; shared targets also require external process coordination.
    let _host_run = HOST_RUN_LOCK
        .lock()
        .map_err(|_| io::Error::other("typed host lock poisoned"))?;
    let mut directory = RegressionDirectory::new(language)?;
    for (name, contents) in [
        (
            "ordinary-child-dynamic-source-control.project.json",
            include_str!(
                "fixtures/typed_document_driver_sources/ordinary-child-dynamic-source-control.project.json"
            ),
        ),
        (
            "dynamic-source-root-document-driver.project.json",
            include_str!(
                "fixtures/typed_document_driver_sources/dynamic-source-root-document-driver.project.json"
            ),
        ),
        (
            "primary.json",
            include_str!("fixtures/typed_document_driver_sources/primary.json"),
        ),
        (
            "a.json",
            include_str!("fixtures/typed_document_driver_sources/a.json"),
        ),
        (
            "b.json",
            include_str!("fixtures/typed_document_driver_sources/b.json"),
        ),
    ] {
        std::fs::write(directory.path.join(name), contents)?;
    }
    for name in [
        "ordinary-child-dynamic-source-control.project.json",
        "dynamic-source-root-document-driver.project.json",
    ] {
        let project: Project =
            serde_json::from_str(&std::fs::read_to_string(directory.path.join(name))?)?;
        assert!(
            engine::validate(&project).is_empty(),
            "complete original project validity"
        );
        codegen::lower(&project)?;
    }
    let native = native::records(&directory.path)?;
    std::fs::write(
        directory.path.join("complete-native-records.json"),
        serde_json::to_vec_pretty(&native)?,
    )?;
    assert_records(&native, "native");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let rust_target = if language == "rust" {
        Some(target(&directory.path)?)
    } else {
        None
    };
    let mut records = Vec::new();
    for (variant, filename, rows) in [
        (
            "ordinary",
            "ordinary-child-dynamic-source-control.project.json",
            1,
        ),
        (
            "documents",
            "dynamic-source-root-document-driver.project.json",
            3,
        ),
    ] {
        let scope = directory.path.join(variant);
        std::fs::create_dir(&scope)?;
        let generated = scope.join("generated");
        let mut generate = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        generate
            .args(["--diagnostics", "json", "generate", "--project"])
            .arg(directory.path.join(filename))
            .args(["--language", language, "--out"])
            .arg(&generated);
        if language == "rust" {
            generate.arg("--rust-runtime-path").arg(&runtime);
        }
        let output = recorded(&mut generate, &scope, "generate")?;
        assert!(
            output.status.success(),
            "CLI generation: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let unchanged = artifact_files(&generated)?;
        assert_eq!(unchanged.len(), if language == "rust" { 2 } else { 74 });
        let host = scope.join("host");
        std::fs::create_dir_all(host.join("src"))?;
        let output = if language == "rust" {
            std::fs::write(
                host.join("src/main.rs"),
                include_str!("fixtures/typed_document_driver_rust.rs.txt"),
            )?;
            std::fs::write(
                host.join("src/typed_snapshot.rs"),
                include_str!("fixtures/typed_document_driver_snapshot.rs.txt"),
            )?;
            let package = format!("ferrule-typed-document-source-{variant}-regression-host");
            std::fs::write(
                host.join("Cargo.toml"),
                format!(
                    "[package]\nname={package:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nir={{path={:?}}}\nmapping={{path={:?}}}\nformat-json={{path={:?}}}\nserde_json={{version=\"1\",features=[\"preserve_order\"]}}\n",
                    workspace.join("crates/ir"),
                    workspace.join("crates/mapping"),
                    workspace.join("crates/format-json")
                ),
            )?;
            let lock = host.join("Cargo.lock");
            std::fs::copy(workspace.join("Cargo.lock"), &lock)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&lock)?.permissions().mode();
                std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(mode | 0o200))?;
            }
            #[cfg(not(unix))]
            {
                let mut permissions = std::fs::metadata(&lock)?.permissions();
                permissions.set_readonly(false);
                std::fs::set_permissions(&lock, permissions)?;
            }
            let target = rust_target.as_ref().expect("Rust host target");
            let mut build = Command::new("cargo");
            build
                .args([
                    "build",
                    "--offline",
                    "--jobs",
                    "1",
                    "--message-format=json-render-diagnostics",
                ])
                .current_dir(&host)
                .env("CARGO_TARGET_DIR", target)
                .env("CARGO_BUILD_JOBS", "1")
                .env("CARGO_INCREMENTAL", "0")
                .env("RUSTFLAGS", "-D warnings");
            let result = recorded(&mut build, &scope, "build")?;
            assert!(
                result.status.success(),
                "Rust host build:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            let binary = target
                .join(format!("debug/{package}"))
                .with_extension(std::env::consts::EXE_EXTENSION);
            let mut run = Command::new(binary);
            run.arg(&directory.path).arg(variant).current_dir(&host);
            recorded(&mut run, &scope, "host")?
        } else {
            std::fs::write(
                host.join("Program.cs"),
                include_str!("fixtures/typed_document_driver_csharp.cs.txt"),
            )?;
            std::fs::write(
                host.join("Host.csproj"),
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
            )?;
            let artifacts = scope.join("artifacts");
            let mut build = dotnet_command(&scope);
            build
                .args([
                    "build",
                    "--nologo",
                    "--verbosity",
                    "normal",
                    "--configuration",
                    "Release",
                    "--artifacts-path",
                ])
                .arg(&artifacts)
                .args(["-m:1", "/p:UseSharedCompilation=false", "host/Host.csproj"])
                .arg(format!("-bl:{}", scope.join("build.binlog").display()))
                .current_dir(&scope)
                .env("DOTNET_PROCESSOR_COUNT", "2");
            let result = recorded(&mut build, &scope, "build")?;
            assert!(
                result.status.success(),
                "C# host build:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            let mut run = dotnet_command(&scope);
            run.arg(artifacts.join("bin/Host/release/Host.dll"))
                .arg(&directory.path)
                .arg(variant)
                .current_dir(&host);
            recorded(&mut run, &scope, "host")?
        };
        assert!(
            output.status.success(),
            "generated {language} calls:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "actual runtime diagnostics retained before assertions"
        );
        let stream: Vec<Json> = String::from_utf8(output.stdout)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert_eq!(stream.len(), rows);
        records.extend(stream);
        assert_eq!(
            artifact_files(&generated)?,
            unchanged,
            "complete generated files unchanged during caller build/run"
        );
    }
    std::fs::write(
        directory.path.join("complete-generated-records.json"),
        serde_json::to_vec_pretty(&records)?,
    )?;
    assert_records(&records, language);
    for (original, generated) in native.iter().zip(&records) {
        assert_eq!(original["result"]["status"], generated["result"]["status"]);
        if original["result"]["status"] == "success" {
            assert_eq!(
                original["result"], generated["result"],
                "whole native typed output equality"
            );
            assert_eq!(original["callbacks"], generated["callbacks"]);
        } else {
            for field in [
                "kind",
                "source",
                "path",
                "host_message",
                "original_host_message_matches",
            ] {
                assert_eq!(
                    original["result"]["error"][field], generated["result"]["error"][field],
                    "typed original error payload {field}"
                );
            }
        }
    }
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_typed_dynamic_document_source_driver_matches_native_dispatch() -> TestResult<()> {
    exercise("rust")
}
#[test]
fn generated_csharp_typed_dynamic_document_source_driver_matches_native_dispatch() -> TestResult<()>
{
    exercise("csharp")
}
