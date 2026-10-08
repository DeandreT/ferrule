use super::*;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

// No Drop cleanup: full originals survive both success and every failed attempt.
pub(super) struct RetainedDirectory(pub PathBuf);

impl RetainedDirectory {
    pub(super) fn new(label: &str) -> TestResult<Self> {
        let parent = match std::env::var_os("FERRULE_CODEGEN_JSON5_EVIDENCE_DIR") {
            Some(path) => {
                let path = PathBuf::from(path);
                if !path.is_absolute() {
                    return Err("JSON5 evidence parent must be absolute".into());
                }
                path
            }
            None => std::env::temp_dir(),
        };
        std::fs::create_dir_all(&parent)?;
        let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        for attempt in 0..100_u32 {
            let path = parent.join(format!(
                "ferrule_json5_{label}_{}_{time}_{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err("could not create a fresh owned JSON5 evidence directory".into())
    }

    pub(super) fn record(&self, label: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        std::fs::write(
            self.0.join(format!("{label}.debug.txt")),
            format!("{value:#?}\n"),
        )?;
        Ok(())
    }

    pub(super) fn command(&self, label: &str, command: &mut Command) -> TestResult<Output> {
        self.record(&format!("{label}-COMMAND"), command)?;
        let original = command.isolated_output();
        self.record(&format!("{label}-ORIGINAL-RESULT"), &original)?;
        let result = original?;
        std::fs::write(self.0.join(format!("{label}-stdout.bin")), &result.stdout)?;
        std::fs::write(self.0.join(format!("{label}-stderr.bin")), &result.stderr)?;
        Ok(result)
    }
}

pub(super) fn write_project(
    root: &RetainedDirectory,
    fixture: &str,
    project: &Project,
) -> TestResult<PathBuf> {
    let path = root.0.join(format!("{fixture}-PROJECT.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(project)?)?;
    root.record(&format!("{fixture}-PROJECT-ORIGINAL"), project)?;
    Ok(path)
}

pub(super) fn generate(
    root: &RetainedDirectory,
    project: &Path,
    output: &Path,
    target: GenerateTarget,
    label: &str,
) -> TestResult<()> {
    let original = generate_project_with_json5_adapters(project, output, target);
    root.record(&format!("{label}-GENERATION-ORIGINAL"), &original)?;
    original?;
    Ok(())
}

pub(super) fn copy_tree(source: &Path, target: &Path) -> TestResult<()> {
    std::fs::create_dir(target)?;
    for (relative, bytes) in artifact_files(source)? {
        let path = target.join(relative);
        std::fs::create_dir_all(path.parent().ok_or("artifact has no parent")?)?;
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Identity {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

fn identity(path: &Path) -> TestResult<Identity> {
    // This cohort keeps payloads small; hashing never reads physical-boundary data.
    let bytes = std::fs::read(path)?;
    Ok(Identity {
        path: std::fs::canonicalize(path)?,
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn collect(root: &Path, paths: &mut Vec<PathBuf>) -> TestResult<()> {
    let mut entries = std::fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    for path in entries {
        let name = path.file_name().ok_or("unnamed input")?;
        if matches!(
            name.to_str(),
            Some("target" | "bin" | "obj" | ".git" | ".tmp" | ".nuget-packages" | ".dotnet-home")
        ) {
            continue;
        }
        if path.is_dir() {
            collect(&path, paths)?;
        } else {
            paths.push(path);
        }
    }
    Ok(())
}

pub(super) fn source_guard(root: &RetainedDirectory) -> TestResult<Vec<Identity>> {
    let mut paths = Vec::new();
    collect(&root.0, &mut paths)?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for directory in [
        "codegen-runtime",
        "codegen-schema",
        "functions",
        "format-csv",
        "format-json",
        "format-xml",
        "ir",
        "json-pattern",
    ] {
        collect(&workspace.join("crates").join(directory), &mut paths)?;
    }
    for file in ["Cargo.toml", "Cargo.lock"] {
        paths.push(workspace.join(file));
    }
    paths.push(PathBuf::from(env!("CARGO_BIN_EXE_ferrule")));
    paths.sort();
    paths.dedup();
    let identities = paths
        .iter()
        .map(|path| identity(path))
        .collect::<TestResult<Vec<_>>>()?;
    root.record("COMPLETE-INPUT-SOURCE-GUARD-BEFORE", &identities)?;
    Ok(identities)
}

pub(super) fn check_guard(
    root: &RetainedDirectory,
    before: &[Identity],
    label: &str,
) -> TestResult<()> {
    let actual = before
        .iter()
        .map(|entry| identity(&entry.path))
        .collect::<Vec<_>>();
    root.record(label, &actual)?;
    let same = actual
        .iter()
        .zip(before)
        .all(|(actual, wanted)| matches!(actual, Ok(actual) if actual == wanted));
    if !same {
        return Err("JSON5 complete input/library guard changed; originals retained".into());
    }
    Ok(())
}

pub(super) fn library_guard(
    root: &RetainedDirectory,
    rust_target: &Path,
) -> TestResult<Vec<Identity>> {
    let mut paths = Vec::new();
    for fixture in projects::FIXTURES {
        paths.push(
            root.0
                .join("csharp")
                .join(fixture)
                .join("bin/Debug/net10.0/Ferrule.Generated.dll"),
        );
    }
    paths.push(root.0.join("csharp/Host/bin/Debug/net10.0/Host.dll"));
    paths.push(rust_target.join("debug").join(format!(
        "ferrule-json5-small-host{}",
        std::env::consts::EXE_SUFFIX
    )));
    let libraries = paths
        .iter()
        .map(|path| identity(path))
        .collect::<TestResult<Vec<_>>>()?;
    root.record("COMPLETE-LIBRARY-GUARD-BEFORE", &libraries)?;
    Ok(libraries)
}

pub(super) fn rust_target() -> TestResult<PathBuf> {
    let path = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .ok_or("set FERRULE_CODEGEN_HOST_TARGET_DIR to the coordinated generated-host target")?;
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("generated-host target must be absolute".into());
    }
    Ok(path)
}

pub(super) fn write_small_gate(
    root: &RetainedDirectory,
    sources: &[Identity],
    libraries: &[Identity],
) -> TestResult<()> {
    fn identities(values: &[Identity]) -> serde_json::Value {
        serde_json::Value::Array(
            values
                .iter()
                .map(|value| {
                    serde_json::json!({
                        "path":value.path, "bytes":value.bytes, "sha256":value.sha256,
                    })
                })
                .collect(),
        )
    }
    let mut evidence = Vec::new();
    for file in [
        "CASES.json",
        "COMPLETE_CALL_COUNT_RUST.json",
        "COMPLETE_CALL_COUNT_CSHARP.json",
        "RUST-HOST-COMMAND.debug.txt",
        "CSHARP-HOST-COMMAND.debug.txt",
        "RUST-HOST-ORIGINAL-RESULT.debug.txt",
        "CSHARP-HOST-ORIGINAL-RESULT.debug.txt",
        "RUST-HOST-stdout.bin",
        "RUST-HOST-stderr.bin",
        "CSHARP-HOST-stdout.bin",
        "CSHARP-HOST-stderr.bin",
        "COMPLETE-INPUT-SOURCE-GUARD-AFTER.debug.txt",
        "COMPLETE-LIBRARY-GUARD-AFTER.debug.txt",
    ] {
        evidence.push(identity(&root.0.join(file))?);
    }
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.0.join("CASES.json"))?)?;
    let mut call_results = Vec::new();
    for case in cases["cases"]
        .as_array()
        .ok_or("small cases must be an array")?
    {
        let fixture = case["fixture"].as_str().ok_or("fixture required")?;
        let id = case["id"].as_str().ok_or("case id required")?;
        for route in 0..4 {
            for file in [
                format!("RUST-{fixture}-{id}-route{route}-ORIGINAL.txt"),
                format!("RUST-{fixture}-{id}-route{route}-PUBLIC-ORIGINAL.txt"),
                format!("CSHARP-{fixture}-{id}-route{route}-ORIGINAL.json"),
            ] {
                call_results.push(identity(&root.0.join(file))?);
            }
        }
    }
    if call_results.len() != 276 {
        return Err("complete original per-call result inventory is required".into());
    }
    let receipt = serde_json::json!({
        "schema_version":1, "issue":144,
        "qualification":"ALL184_SMALL_PUBLIC_CALLS_PASS", "root":std::fs::canonicalize(&root.0)?,
        "rust":92, "csharp":92, "total":184, "failures":0, "physical_calls":0,
        "complete_source_after_exact":true, "complete_library_after_exact":true,
        "sources":identities(sources), "libraries":identities(libraries), "evidence":identities(&evidence),
        "call_results":identities(&call_results),
    });
    std::fs::write(
        root.0.join("ALL184_SMALL_PUBLIC_CALLS_COMPLETE.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(())
}

pub(super) const HOST_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
</Project>
"#;
