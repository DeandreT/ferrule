//! Serial GNU-bounded process custody and source/corpus guards; no RSS guarantee.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

pub(super) struct Originals {
    pub(super) path: PathBuf,
}
impl Originals {
    pub(super) fn new(language: &str, large: bool) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_combined_xml_output_{language}_{large}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }
}
impl Drop for Originals {
    fn drop(&mut self) {
        eprintln!(
            "Retained combined XML output originals: {}",
            self.path.display()
        );
    }
}
pub(super) fn json_file(path: &Path, value: &Json) -> io::Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)
}
pub(super) fn identity(path: &Path) -> io::Result<Json> {
    use std::os::unix::fs::MetadataExt;
    let before = std::fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err(io::Error::other("ordinary original required"));
    }
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut block = [0u8; 65536];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        digest.update(&block[..n]);
        size += n as u64;
    }
    let after = std::fs::symlink_metadata(path)?;
    let fields = |m: &std::fs::Metadata| {
        (
            m.len(),
            m.mode(),
            m.ino(),
            m.dev(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    if fields(&before) != fields(&after) || size != after.len() {
        return Err(io::Error::other("original changed during hash"));
    }
    Ok(
        json!({"path":path,"size":size,"sha256":format!("{:x}",digest.finalize()),
        "mode":after.mode(),"inode":after.ino(),"device":after.dev(),
        "mtime":[after.mtime(),after.mtime_nsec()],"ctime":[after.ctime(),after.ctime_nsec()]}),
    )
}
pub(super) fn source_identities() -> TestResult<Json> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let paths = [
        "crates/cli/tests/code_generation/xml_dynamic_outputs.rs",
        "crates/cli/tests/code_generation/xml_dynamic_outputs/combined_output_bytes.rs",
        "crates/cli/tests/code_generation/xml_dynamic_outputs/combined_output_bytes_support.rs",
        "crates/cli/tests/code_generation/fixtures/xml_combined_output_bytes_rust.rs.txt",
        "crates/cli/tests/code_generation/fixtures/xml_combined_output_bytes_csharp.cs.txt",
        "crates/codegen/src/model.rs",
        "crates/codegen/src/lower.rs",
        "crates/codegen/src/validate/xml.rs",
        "crates/codegen-rust/src/xml_api.rs",
        "crates/codegen-csharp/src/mapping/xml_api.rs",
        "crates/codegen-runtime/src/xml_boundary.rs",
        "crates/codegen-runtime/src/xml_boundary/document_set.rs",
        "crates/codegen-runtime/src/xml_boundary/output_set.rs",
        "runtime/csharp/Ferrule.Runtime/FerruleXml.cs",
        "runtime/csharp/Ferrule.Runtime/FerruleXml.DocumentSet.cs",
        "runtime/csharp/Ferrule.Runtime/FerruleXml.OutputSet.cs",
        "runtime/csharp/Ferrule.Runtime/FerruleFunctions.Strings.cs",
        "crates/functions/src/builtins/mod.rs",
        "crates/codegen-runtime/src/context/resolve.rs",
        "crates/format-xml/src/instance/structured.rs",
        "runtime/csharp/Ferrule.Runtime/FerruleXml.Input.Structured.cs",
        "crates/cli/Cargo.toml",
        "Cargo.toml",
        "Cargo.lock",
    ];
    let mut values = paths
        .into_iter()
        .map(|path| identity(&root.join(path)))
        .collect::<Result<Vec<_>, _>>()?;
    values.push(identity(Path::new(env!("CARGO_BIN_EXE_ferrule")))?);
    values.push(identity(&std::env::current_exe()?)?);
    values.push(identity(Path::new("/usr/bin/timeout"))?);
    values.push(identity(&gnu_time()?)?);
    Ok(json!(values))
}
pub(super) fn library_identities(root: &Path) -> TestResult<Json> {
    let files = artifact_files(root)?;
    Ok(json!(
        files
            .into_iter()
            .map(|(path, bytes)| json!({"path":path,"size":bytes.len(),
        "sha256":format!("{:x}",Sha256::digest(&bytes))}))
            .collect::<Vec<_>>()
    ))
}
pub(super) fn host_identities(path: &Path, language: &str) -> TestResult<Json> {
    if language == "rust" {
        return Ok(json!([identity(path)?]));
    }
    let root = path
        .parent()
        .ok_or("actual C# host closure parent missing")?;
    let mut paths = std::fs::read_dir(root)?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let values = paths
        .into_iter()
        .map(|path| identity(&path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!(values))
}
pub(super) fn corpus_identities(directory: &Path) -> TestResult<Json> {
    let mut values = Vec::new();
    for name in [
        "project.json",
        "source-schema.txt",
        "target-schema.txt",
        "small.xml",
        "exact.xml",
        "plus-one.xml",
    ] {
        let path = directory.join(name);
        if path.exists() {
            values.push(identity(&path)?);
        }
    }
    Ok(json!(values))
}
pub(super) fn caught<T>(directory: &Path, run: impl FnOnce() -> TestResult<T>) -> TestResult<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => value,
        Err(payload) => {
            let (kind, message) = if let Some(value) = payload.downcast_ref::<String>() {
                ("String", value.as_str())
            } else if let Some(value) = payload.downcast_ref::<&str>() {
                ("str", *value)
            } else {
                (
                    "opaque",
                    "non-text payload; original panic hook remains active",
                )
            };
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            json_file(
                &directory.join(format!(
                    "original-panic-{}.json",
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )),
                &json!({"kind":kind,"message":message}),
            )?;
            Err(io::Error::other(format!("retained original assertion panic: {message}")).into())
        }
    }
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
    measured: bool,
) -> TestResult<Output> {
    let seconds = deadline
        .saturating_duration_since(Instant::now())
        .as_secs()
        .min(seconds);
    if seconds == 0 {
        return Err("overall combined XML output deadline exhausted".into());
    }
    let mut bounded = Command::new("/usr/bin/timeout");
    bounded
        .args(["--signal=TERM", "--kill-after=5s"])
        .arg(format!("{seconds}s"));
    if measured {
        bounded
            .arg(gnu_time()?)
            .args(["--verbose", "--output"])
            .arg(directory.join(format!("{name}-usage.txt")));
    }
    bounded.arg(command.get_program()).args(command.get_args());
    if let Some(cwd) = command.get_current_dir() {
        bounded.current_dir(cwd);
    }
    for (name, value) in command.get_envs() {
        if let Some(value) = value {
            bounded.env(name, value);
        } else {
            bounded.env_remove(name);
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
                .map_err(|_| io::Error::other("dotnet lock poisoned"))?,
        )
    } else {
        None
    };
    let began = Instant::now();
    let original = bounded.output();
    let elapsed = began.elapsed();
    std::fs::write(
        directory.join(format!("{name}-original-spawn-result.txt")),
        format!("{original:#?}\n"),
    )?;
    let output = original?;
    std::fs::write(directory.join(format!("{name}-stdout.bin")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.bin")), &output.stderr)?;
    json_file(
        &directory.join(format!("{name}-status.json")),
        &json!({"code":output.status.code(),
        "success":output.status.success(),"bound_seconds":seconds,"measured":measured,"controller_wall_ns":elapsed.as_nanos().to_string(),
        "timeout_is_not_qualification":true,"raw_usage_units_platform_specific":true}),
    )?;
    Ok(output)
}
pub(super) fn require_success(output: &Output) {
    assert!(
        output.status.success(),
        "original command failed {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
pub(super) fn headroom(directory: &Path, deadline: Instant, label: &str) -> TestResult<()> {
    let memory = std::fs::read("/proc/meminfo")?;
    std::fs::write(directory.join(format!("{label}-meminfo.bin")), &memory)?;
    let memory = std::str::from_utf8(&memory)?
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))
        .ok_or("MemAvailable absent")?
        .split_whitespace()
        .next()
        .ok_or("MemAvailable value absent")?
        .parse::<u64>()?;
    let mut command = Command::new("df");
    command.args(["-Pk"]).arg(directory);
    let output = recorded(
        &mut command,
        directory,
        &format!("{label}-disk"),
        deadline,
        15,
        false,
    )?;
    require_success(&output);
    let disk = std::str::from_utf8(&output.stdout)?
        .lines()
        .nth(1)
        .ok_or("df row absent")?
        .split_whitespace()
        .nth(3)
        .ok_or("df free column absent")?
        .parse::<u64>()?;
    json_file(
        &directory.join(format!("{label}-headroom.json")),
        &json!({"MemAvailable_kib":memory,
        "filesystem_available_kib":disk,"policy_only_not_product_RSS_limit":true}),
    )?;
    assert!(
        memory >= 8 * 1024 * 1024,
        "large controls need at least 8GiB observed MemAvailable"
    );
    assert!(
        disk >= 20 * 1024 * 1024,
        "large controls reserve 20GiB filesystem headroom"
    );
    Ok(())
}
pub(super) fn build(
    directory: &Path,
    language: &str,
    _generated: &Path,
    deadline: Instant,
) -> TestResult<PathBuf> {
    let host = directory.join("host");
    std::fs::create_dir(&host)?;
    if language == "rust" {
        std::fs::create_dir(host.join("src"))?;
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("../fixtures/xml_combined_output_bytes_rust.rs.txt"),
        )?;
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../codegen-runtime")
            .canonicalize()?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname=\"ferrule-combined-xml-output-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path=\"../generated\"}}\ncodegen-runtime={{path={runtime:?}}}\nserde_json=\"1\"\nsha2=\"0.10\"\n"
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
        let output = recorded(&mut command, directory, "build", deadline, 600, false)?;
        require_success(&output);
        Ok(target
            .join("debug/ferrule-combined-xml-output-host")
            .with_extension(std::env::consts::EXE_EXTENSION))
    } else {
        std::fs::write(
            host.join("Program.cs"),
            include_str!("../fixtures/xml_combined_output_bytes_csharp.cs.txt"),
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
        let output = recorded(&mut command, directory, "build", deadline, 600, false)?;
        require_success(&output);
        Ok(artifacts.join("bin/Host/release/Host.dll"))
    }
}
pub(super) fn host_command(path: &Path, language: &str, directory: &Path) -> Command {
    let mut command = if language == "rust" {
        Command::new(path)
    } else {
        let mut value = dotnet_command(directory);
        value.arg(path);
        value
    };
    command
        .current_dir(directory)
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1")
        .env("DOTNET_PROCESSOR_COUNT", "2");
    command
}
