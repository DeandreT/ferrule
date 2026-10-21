use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

const CASES: &[u8] =
    include_bytes!("../../../format-json/tests/fixtures/nested_array_wrappers/cases.json");
const PROJECT: &str =
    include_str!("../../../mfd/tests/fixtures/nested_array_wrappers/project.json");

fn record(root: &Path, name: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
    std::fs::write(
        root.join(format!("{name}.original.txt")),
        format!("{original:#?}\n"),
    )?;
    Ok(())
}

fn command(root: &Path, name: &str, command: &mut Command) -> TestResult<Output> {
    record(root, &format!("{name}-command"), command)?;
    let actual = command.isolated_output();
    record(root, &format!("{name}-result"), &actual)?;
    let output = actual?;
    std::fs::write(root.join(format!("{name}.stdout.bin")), &output.stdout)?;
    std::fs::write(root.join(format!("{name}.stderr.bin")), &output.stderr)?;
    Ok(output)
}

#[test]
fn nested_array_wrappers_compiled_complete_oracles() -> TestResult<()> {
    let parent = std::env::var_os("FERRULE_NESTED_ARRAY_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if !parent.is_absolute() {
        return Err("nested-array evidence parent must be absolute".into());
    }
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = parent.join(format!(
        "ferrule-nested-array-compiled-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root)?;
    eprintln!("NESTED_ARRAY_ORIGINALS={}", root.display());
    std::fs::write(root.join("cases.json"), CASES)?;
    std::fs::write(root.join("project.json"), PROJECT)?;
    let project = serde_json::from_str::<Project>(PROJECT);
    record(&root, "authored-project", &project)?;
    let project = project?;
    let validation = engine::validate(&project);
    let lowered = codegen::lower(&project);
    record(&root, "complete-admission", &(&validation, &lowered))?;
    if !validation.is_empty() || lowered.is_err() {
        return Err("authored nested-array project admission differs".into());
    }
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("cargo-target"));
    if !target.is_absolute() {
        return Err("generated host target must be absolute".into());
    }
    let dotnet_temp = std::env::var_os("FERRULE_NESTED_ARRAY_DOTNET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("fnj-{}-{stamp}", std::process::id()))
        });
    if !dotnet_temp.is_absolute() {
        return Err(".NET temporary directory must be absolute".into());
    }
    std::fs::create_dir_all(&dotnet_temp)?;
    record(&root, "dotnet-temporary-directory", &dotnet_temp)?;
    let mut all_results = Vec::new();
    for language in ["rust", "csharp"] {
        let directory = root.join(language);
        let selected = if language == "rust" {
            GenerateTarget::Rust {
                runtime_path: runtime.clone(),
            }
        } else {
            GenerateTarget::CSharp
        };
        let generated = generate_project(&root.join("project.json"), &directory, selected);
        record(&root, &format!("{language}-generation"), &generated)?;
        generated?;
        let published = artifact_files(&directory)?;
        record(&root, &format!("{language}-published"), &published)?;
        if language == "rust"
            && published
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>()
                != ["Cargo.toml", "src/lib.rs"]
        {
            return Err("ordinary nested-array Rust publication differs".into());
        }
        if language == "csharp" {
            let names = published
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>();
            if names != expected_csharp_artifact_paths(false) {
                return Err("ordinary nested-array C# publication differs".into());
            }
            let runtime_root =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/csharp/Ferrule.Runtime");
            let body_readback = published
                .iter()
                .filter_map(|(path, body)| {
                    path.strip_prefix("Runtime/")
                        .map(|relative| (path, body, std::fs::read(runtime_root.join(relative))))
                })
                .collect::<Vec<_>>();
            record(
                &root,
                "csharp-complete-runtime-body-readback",
                &body_readback,
            )?;
            let expected_runtime_count = expected_csharp_artifact_paths(false)
                .iter()
                .filter(|path| path.starts_with("Runtime/"))
                .count();
            if body_readback.len() != expected_runtime_count
                || body_readback
                    .iter()
                    .any(|(_, wanted, actual)| !matches!(actual, Ok(actual) if actual == *wanted))
            {
                return Err("nested-array emitted runtime source body differs".into());
            }
        }
        std::fs::write(directory.join("cases.json"), CASES)?;
        if language == "rust" {
            if directory.join("Cargo.lock").exists() {
                return Err("fresh generated Rust host lock already exists".into());
            }
            let manifest_path = directory.join("Cargo.toml");
            let manifest = std::fs::read_to_string(&manifest_path)?;
            std::fs::write(manifest_path, manifest.replace("\n[workspace]", "\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\n\n[workspace]"))?;
            std::fs::write(
                directory.join("src/main.rs"),
                include_str!("nested_array_wrappers/Host.rs.txt"),
            )?;
        } else {
            std::fs::create_dir(directory.join("Harness"))?;
            std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
            std::fs::write(
                directory.join("Harness/Program.cs"),
                include_str!("nested_array_wrappers/Host.cs.txt"),
            )?;
            std::fs::write(
                directory.join("NuGet.Config"),
                "<configuration><packageSources><clear /></packageSources></configuration>\n",
            )?;
        }
        let bound = artifact_files(&directory)?;
        record(&root, &format!("{language}-bound-artifacts"), &bound)?;
        let result = (|| -> TestResult<bool> {
            if language == "rust" {
                let mut run = Command::new("cargo");
                run.args(["run", "--quiet", "--offline", "--jobs", "1", "--"])
                    .arg(&directory)
                    .current_dir(&directory)
                    .env("CARGO_BUILD_BUILD_DIR", &target)
                    .env("CARGO_TARGET_DIR", &target)
                    .env("CARGO_INCREMENTAL", "0")
                    .env("RUSTFLAGS", "-Dwarnings");
                return Ok(command(&root, "rust-run", &mut run)?.status.success());
            }
            let mut build = dotnet_command(&directory);
            build
                .args([
                    "msbuild",
                    "Harness/Host.csproj",
                    "-t:Restore;Build",
                    "-nologo",
                    "-m:1",
                    "-nr:false",
                    "-p:UseSharedCompilation=false",
                    "-p:BuildInParallel=false",
                    "-p:NuGetAudit=false",
                    "-p:EnableTargetingPackDownload=false",
                    "-p:EnableRuntimePackDownload=false",
                ])
                .current_dir(&directory)
                .env("TMPDIR", &dotnet_temp)
                .env("MSBuildEnableWorkloadResolver", "false")
                .env("DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE", "true")
                .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
                .env("MSBUILDDISABLENODEREUSE", "1");
            if !command(&root, "csharp-build", &mut build)?.status.success() {
                return Ok(false);
            }
            let assembly = directory.join("Harness/bin/Debug/net10.0/Host.dll");
            if !assembly.is_file() {
                return Err("nested-array host assembly omitted".into());
            }
            let mut run = dotnet_command(&directory);
            run.arg(assembly)
                .arg(&directory)
                .current_dir(&directory)
                .env("TMPDIR", &dotnet_temp);
            Ok(command(&root, "csharp-run", &mut run)?.status.success())
        })();
        record(&root, &format!("{language}-complete-host-result"), &result)?;
        let readback = bound
            .iter()
            .map(|(relative, _)| std::fs::read(directory.join(relative)))
            .collect::<Vec<_>>();
        record(
            &root,
            &format!("{language}-complete-source-readback"),
            &readback,
        )?;
        let source_equal = readback
            .iter()
            .zip(&bound)
            .all(|(actual, (_, wanted))| matches!(actual, Ok(actual) if actual == wanted));
        all_results.push((language, result, source_equal));
    }
    record(&root, "all-complete-results", &all_results)?;
    if all_results
        .iter()
        .any(|(_, result, stable)| !*stable || !matches!(result, Ok(true)))
    {
        return Err("complete nested-array compiled oracle cohort differs".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
