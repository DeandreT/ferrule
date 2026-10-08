//! Evidence retention and bounded serial child-process support for dynamic XML request-count controls.
use super::super::super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::Instant;

pub(super) struct Originals {
    pub(super) path: PathBuf,
    pub(super) complete: bool,
}
impl Originals {
    pub(super) fn new() -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xml_request_count_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?; // Never remove or reuse an existing path.
        Ok(Self {
            path,
            complete: false,
        })
    }
}
impl Drop for Originals {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained dynamic XML public-host originals: {}",
                self.path.display()
            );
        }
    }
}
pub(super) fn json_file(path: &Path, value: &Json) -> io::Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)
}
pub(super) fn identity(path: &Path) -> io::Result<Json> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::other("expected ordinary artifact"));
    }
    let mut input = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok(
        json!({"path":path,"size":bytes,"metadata_size":metadata.len(),"sha256":format!("{:x}",hash.finalize()),
        "modified":metadata.modified()?.duration_since(std::time::UNIX_EPOCH).map_err(io::Error::other)?.as_nanos().to_string()}),
    )
}
pub(super) fn source_identities() -> TestResult<Json> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let paths = [
        "crates/cli/tests/code_generation/xml_dynamic_inputs.rs",
        "crates/cli/tests/code_generation/xml_dynamic_inputs/request_count.rs",
        "crates/cli/tests/code_generation/xml_dynamic_inputs/request_count/support.rs",
        "crates/cli/tests/code_generation/fixtures/xml_dynamic_request_count_rust.rs.txt",
        "crates/cli/tests/code_generation/fixtures/xml_dynamic_request_count_csharp.cs.txt",
        "crates/codegen/src/lower.rs",
        "crates/codegen/src/model.rs",
        "crates/codegen/src/validate/xml.rs",
        "crates/codegen-rust/src/xml_api/dynamic_inputs.rs",
        "crates/codegen-rust/src/lib.rs",
        "crates/codegen-csharp/src/mapping/xml_api/dynamic_inputs.rs",
        "crates/codegen-csharp/src/mapping.rs",
        "crates/codegen-runtime/src/xml_boundary/input_set.rs",
        "crates/codegen-runtime/src/xml_boundary/dynamic_inputs.rs",
        "crates/codegen-runtime/src/dynamic_source.rs",
        "runtime/csharp/Ferrule.Runtime/FerruleXml.InputSet.cs",
        "runtime/csharp/Ferrule.Runtime/FerruleXml.DynamicSources.cs",
        "runtime/csharp/Ferrule.Runtime/FerruleDynamicSources.cs",
    ];
    let mut records = paths
        .into_iter()
        .map(|path| identity(&workspace.join(path)))
        .collect::<Result<Vec<_>, _>>()?;
    records.push(identity(Path::new(env!("CARGO_BIN_EXE_ferrule")))?);
    records.push(identity(&std::env::current_exe()?)?);
    Ok(json!(records))
}
pub(super) fn gnu_time() -> TestResult<PathBuf> {
    let path = std::env::var_os("FERRULE_CODEGEN_GNU_TIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin/time"));
    if !path.is_absolute() || !path.is_file() {
        return Err(io::Error::other("an absolute GNU time executable is required").into());
    }
    Ok(path)
}
pub(super) fn recorded(
    command: &mut Command,
    directory: &Path,
    name: &str,
    deadline: Instant,
    seconds: u64,
    measure: bool,
) -> TestResult<Output> {
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_secs()
        .min(seconds);
    if remaining == 0 {
        return Err("overall public-host deadline exhausted".into());
    }
    let mut bounded = Command::new("timeout");
    bounded
        .args(["--signal=TERM", "--kill-after=5s"])
        .arg(format!("{remaining}s"));
    if measure {
        bounded
            .arg(gnu_time()?)
            .args(["--verbose", "--output"])
            .arg(directory.join(format!("{name}-memory.txt")));
    }
    bounded.arg(command.get_program()).args(command.get_args());
    if let Some(cwd) = command.get_current_dir() {
        bounded.current_dir(cwd);
    }
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            bounded.env(key, value);
        } else {
            bounded.env_remove(key);
        }
    }
    bounded.env("LC_ALL", "C");
    std::fs::write(
        directory.join(format!("{name}-command.txt")),
        format!("{bounded:?}\n"),
    )?;
    let _dotnet = if command.get_program() == OsStr::new("dotnet") {
        Some(
            DOTNET_RUN_LOCK
                .lock()
                .map_err(|_| io::Error::other("dotnet command lock poisoned"))?,
        )
    } else {
        None
    };
    let result = bounded.output();
    std::fs::write(
        directory.join(format!("{name}-spawn-result.txt")),
        format!("{result:#?}\n"),
    )?;
    let output = result?;
    std::fs::write(directory.join(format!("{name}-stdout.bin")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.bin")), &output.stderr)?;
    json_file(
        &directory.join(format!("{name}-status.json")),
        &json!({"code":output.status.code(),"success":output.status.success(),
        "bounded_seconds":remaining,"timeout_is_not_qualification":true,"measured":measure}),
    )?;
    Ok(output)
}
// Preserve ordinary assertion failures, then let callers complete immutable afterguards.
pub(super) fn caught<T>(
    directory: &Path,
    name: &str,
    run: impl FnOnce() -> TestResult<T>,
) -> TestResult<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(result) => result,
        Err(payload) => {
            let (kind, message) = if let Some(message) = payload.downcast_ref::<String>() {
                ("String", message.as_str())
            } else if let Some(message) = payload.downcast_ref::<&str>() {
                ("str", *message)
            } else {
                (
                    "opaque",
                    "non-text panic payload; original panic hook remains active",
                )
            };
            json_file(
                &directory.join(format!("{name}-original-panic-payload.json")),
                &json!({"kind":kind,"message":message}),
            )?;
            Err(io::Error::other(format!("retained {name} assertion panic: {message}")).into())
        }
    }
}
pub(super) fn require_success(output: &Output) {
    assert!(
        output.status.success(),
        "retained command failed; code {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
pub(super) fn headroom(directory: &Path, deadline: Instant, stage: &str) -> TestResult<()> {
    let memory = std::fs::read("/proc/meminfo")?;
    std::fs::write(directory.join(format!("{stage}-meminfo.bin")), &memory)?;
    let available = std::str::from_utf8(&memory)?
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))
        .ok_or("MemAvailable unavailable")?
        .split_whitespace()
        .next()
        .ok_or("memory value")?
        .parse::<u64>()?;
    let mut command = Command::new("df");
    command.args(["-Pk"]).arg(directory);
    let output = recorded(
        &mut command,
        directory,
        &format!("{stage}-disk"),
        deadline,
        15,
        false,
    )?;
    require_success(&output);
    let disk = std::str::from_utf8(&output.stdout)?
        .lines()
        .nth(1)
        .ok_or("disk row unavailable")?
        .split_whitespace()
        .nth(3)
        .ok_or("disk available column")?
        .parse::<u64>()?;
    json_file(
        &directory.join(format!("{stage}-headroom.json")),
        &json!({"MemAvailable_kib":available,"filesystem_available_kib":disk,
        "policy_only_not_product_RSS_limit":true}),
    )?;
    assert!(
        available >= 1024 * 1024,
        "request-count opt-in needs at least 1 GiB observed memory headroom"
    );
    assert!(
        disk >= 1024 * 1024,
        "request-count opt-in needs at least 1 GiB observed filesystem headroom"
    );
    Ok(())
}
pub(super) fn exclusive_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
pub(super) fn prepare(
    directory: &Path,
    language: &str,
    project: &Project,
    deadline: Instant,
) -> TestResult<(PathBuf, PathBuf)> {
    let validation = engine::validate(project);
    let lowered = codegen::lower(project);
    std::fs::write(
        directory.join("complete-original-validation-lowering.txt"),
        format!("{validation:#?}\n{lowered:#?}\n"),
    )?;
    assert!(validation.is_empty());
    let program = lowered?;
    assert_eq!(
        program
            .extra_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog"]
    );
    assert!(program.extra_sources[0].dynamic.is_none());
    let dynamic = program.extra_sources[1]
        .dynamic
        .as_ref()
        .ok_or("real dynamic declaration")?;
    assert_eq!(dynamic.path, 0);
    assert_eq!(dynamic.driver.path(), ["Driver"]);
    let policy = program
        .xml_boundary
        .as_ref()
        .ok_or("Structured adapter required")?;
    assert_eq!(
        policy.input.profile(),
        Some(codegen::XmlInputProfile::Structured)
    );
    assert_eq!(
        policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog"]
    );
    let path = directory.join("project.json");
    let encoded = mapping::project_file::encode_pretty(project);
    std::fs::write(
        directory.join("original-project-codec-result.txt"),
        format!("{encoded:#?}\n"),
    )?;
    exclusive_file(&path, encoded?.as_bytes())?;
    let generated = directory.join("generated");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args(["--diagnostics", "json", "generate", "--project"])
        .arg(&path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        command.arg("--rust-runtime-path").arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .canonicalize()?,
        );
    }
    let output = recorded(&mut command, directory, "generate", deadline, 120, false)?;
    require_success(&output);
    let source = std::fs::read_to_string(generated.join(if language == "rust" {
        "src/lib.rs"
    } else {
        "GeneratedMapping.cs"
    }))?;
    let names = if language == "rust" {
        "pub const EXTRA_SOURCE_NAMES: &[&str] = &[\"rates\"];"
    } else {
        "private static readonly string[] ExtraXmlInputNames = new string[] { \"rates\" };"
    };
    let policy_literal = if language == "rust" {
        "XmlDynamicSourcePolicy { declaration_index: 1, source: \"catalog\", schema: EXTRA_XML_INPUT_SCHEMA_1 }"
    } else {
        "new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy(1, \"catalog\", ExtraXmlInputSchema_1)"
    };
    let mut schemas = Vec::new();
    for (index, input) in project.extra_sources.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &input.schema,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        let literal = serde_json::to_string(&descriptor)?;
        let declaration = if language == "rust" {
            format!("const EXTRA_XML_INPUT_SCHEMA_{index}: &str = {literal};")
        } else {
            format!("private const string ExtraXmlInputSchema_{index} = {literal};")
        };
        schemas.push(json!({"index":index,"name":input.name,"descriptor":descriptor,"exact_occurrences":source.matches(&declaration).count()}));
    }
    json_file(
        &directory.join("complete-actual-emitted-declaration-policy-census.json"),
        &json!({"static_names":names,"static_names_count":source.matches(names).count(),"dynamic_policy":policy_literal,"dynamic_policy_count":source.matches(policy_literal).count(),"schemas":&schemas}),
    )?;
    assert_eq!(source.matches(names).count(), 1);
    assert_eq!(
        source.matches(policy_literal).count(),
        4,
        "four underlying text/bytes/context loader policy constructions"
    );
    for schema in schemas {
        assert_eq!(schema["exact_occurrences"], 1);
    }
    Ok((path, generated))
}
pub(super) fn build(
    directory: &Path,
    language: &str,
    generated: &Path,
    deadline: Instant,
) -> TestResult<Command> {
    let host = directory.join("host");
    std::fs::create_dir(&host)?;
    if language == "rust" {
        std::fs::create_dir(host.join("src"))?;
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("../../fixtures/xml_dynamic_request_count_rust.rs.txt"),
        )?;
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../codegen-runtime")
            .canonicalize()?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname=\"ferrule-xml-request-count-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nserde_json=\"1\"\n"
            ),
        )?;
        let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| directory.join("rust-target"));
        let target = if target.is_absolute() {
            target
        } else {
            std::env::current_dir()?.join(target)
        };
        let mut command = Command::new("cargo");
        command
            .args(["build", "--offline", "--quiet", "--jobs", "1"])
            .current_dir(&host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-D warnings");
        let output = recorded(&mut command, directory, "build", deadline, 300, false)?;
        require_success(&output);
        Ok(Command::new(
            target
                .join("debug/ferrule-xml-request-count-host")
                .with_extension(std::env::consts::EXE_EXTENSION),
        ))
    } else {
        std::fs::write(
            host.join("Program.cs"),
            include_str!("../../fixtures/xml_dynamic_request_count_csharp.cs.txt"),
        )?;
        std::fs::write(
            directory.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>",
        )?;
        std::fs::write(
            host.join("Host.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
        )?;
        let artifacts = directory.join("artifacts");
        let mut command = dotnet_command(directory);
        command
            .args([
                "build",
                "--disable-build-servers",
                "--nologo",
                "--verbosity",
                "quiet",
                "--configuration",
                "Release",
                "--artifacts-path",
            ])
            .arg(&artifacts)
            .args([
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:AutomaticallyUseReferenceAssemblyPackages=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                "-p:NuGetAudit=false",
                "host/Host.csproj",
            ])
            .current_dir(directory)
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let output = recorded(&mut command, directory, "build", deadline, 300, false)?;
        require_success(&output);
        let mut command = dotnet_command(directory);
        command.arg(artifacts.join("bin/Host/release/Host.dll"));
        command
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("DOTNET_PROCESSOR_COUNT", "2");
        Ok(command)
    }
}
