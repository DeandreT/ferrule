use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_NESTED_CLONES_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        std::fs::create_dir_all(&parent)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule_nested_clones239_compiled_{}_{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        eprintln!("NESTED_CLONES239_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn record(&self, name: &str, value: impl std::fmt::Debug) -> io::Result<()> {
        std::fs::write(self.0.join(name), format!("{value:#?}\n"))
    }
    fn command(&self, name: &str, command: &mut Command) -> TestResult<Output> {
        self.record(&format!("{name}-command.original.txt"), &command)?;
        let observed = command.isolated_output();
        self.record(&format!("{name}-outcome.original.txt"), &observed)?;
        let output = observed?;
        std::fs::write(
            self.0.join(format!("{name}-stdout.original.bin")),
            &output.stdout,
        )?;
        std::fs::write(
            self.0.join(format!("{name}-stderr.original.bin")),
            &output.stderr,
        )?;
        Ok(output)
    }
}

#[test]
fn declared_nested_clone_owners_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| evidence.0.join("host-target"));
    if !target.is_absolute() {
        return Err("shared host target must be absolute".into());
    }
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/nested_clone_owners");
    let cases = std::fs::read(fixtures.join("cases.json"))?;
    let mut outcomes = Vec::new();
    for design in [
        "equal",
        "unequal",
        "parent_condition",
        "child_condition",
        "reversed_keys_and_edges",
        "three_level",
    ] {
        let package = evidence.0.join(format!("{design}-package"));
        std::fs::create_dir(&package)?;
        for file in [
            "source.xsd",
            "target.xsd",
            "source-deep.xsd",
            "target-deep.xsd",
        ] {
            std::fs::copy(fixtures.join(file), package.join(file))?;
        }
        let mapping = package.join("mapping.mfd");
        std::fs::copy(fixtures.join(format!("{design}.mfd")), &mapping)?;
        std::fs::write(package.join("expected-before-run.json"), &cases)?;
        let imported = mfd::import(&mapping);
        let snapshot = match &imported {
            Ok(value) => serde_json::json!({"project": value.project, "warnings": value.warnings}),
            Err(error) => serde_json::json!({"error": error.to_string()}),
        };
        std::fs::write(
            package.join("import.original.json"),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
        let imported = imported?;
        let validation = engine::validate(&imported.project);
        evidence.record(&format!("{design}-validation.original.txt"), &validation)?;
        if !imported.warnings.is_empty() || !validation.is_empty() {
            return Err("authored project must import and validate before generation".into());
        }
        let project = package.join("project.json");
        std::fs::write(
            &project,
            mapping::project_file::encode_pretty(&imported.project)?,
        )?;
        for language in ["rust", "csharp"] {
            let label = format!("{design}-{language}");
            let directory = evidence.0.join(&label);
            let selected = if language == "rust" {
                GenerateTarget::Rust {
                    runtime_path: runtime.clone(),
                }
            } else {
                GenerateTarget::CSharp
            };
            let generated = generate_project(&project, &directory, selected);
            evidence.record(&format!("{label}-generation.original.txt"), &generated)?;
            let generated = generated?;
            let original = artifact_files(&directory)?;
            evidence.record(&format!("{label}-generated-files.original.txt"), &original)?;
            if generated.output_directory != directory || generated.files_written != original.len()
            {
                return Err("generated artifact census differs".into());
            }
            std::fs::write(directory.join("cases.json"), &cases)?;
            if language == "rust" {
                let manifest = directory.join("Cargo.toml");
                let text = std::fs::read_to_string(&manifest)?;
                std::fs::write(
                    manifest,
                    format!(
                        "{text}\n[dependencies.serde_json]\nversion = \"1\"\nfeatures = [\"preserve_order\"]\n"
                    ),
                )?;
                std::fs::write(
                    directory.join("src/main.rs"),
                    include_str!("nested_clone_owners/Host.rs.txt"),
                )?;
            } else {
                let host = directory.join("Harness");
                std::fs::create_dir(&host)?;
                std::fs::write(host.join("Host.csproj"), HOST_PROJECT)?;
                std::fs::write(
                    host.join("Program.cs"),
                    include_str!("nested_clone_owners/Host.cs.txt"),
                )?;
                std::fs::write(
                    directory.join("NuGet.Config"),
                    "<configuration><packageSources><clear /></packageSources></configuration>\n",
                )?;
            }
            let bound = artifact_files(&directory)?;
            evidence.record(&format!("{label}-bound-inputs.original.txt"), &bound)?;
            let observed = (|| -> TestResult<bool> {
                if language == "rust" {
                    let mut command = Command::new("cargo");
                    command
                        .args(["run", "--offline", "--jobs", "1", "--", design])
                        .arg(&directory)
                        .current_dir(&directory)
                        .env("CARGO_TARGET_DIR", &target)
                        .env("CARGO_INCREMENTAL", "0")
                        .env("RUSTFLAGS", "-Dwarnings");
                    return Ok(evidence.command(&label, &mut command)?.status.success());
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
                    .arg(format!(
                        "-p:RestoreConfigFile={}",
                        directory.join("NuGet.Config").display()
                    ))
                    .current_dir(&directory)
                    .env("MSBuildEnableWorkloadResolver", "false")
                    .env("DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE", "true")
                    .env("DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER", "1")
                    .env("MSBUILDDISABLENODEREUSE", "1");
                if !evidence
                    .command(&format!("{label}-build"), &mut build)?
                    .status
                    .success()
                {
                    return Ok(false);
                }
                let assembly = directory.join("Harness/bin/Debug/net10.0/Host.dll");
                if !assembly.is_file() {
                    return Err("successful build omitted host assembly".into());
                }
                let mut command = dotnet_command(&directory);
                command
                    .arg(assembly)
                    .arg(design)
                    .arg(&directory)
                    .current_dir(&directory);
                Ok(evidence
                    .command(&format!("{label}-host"), &mut command)?
                    .status
                    .success())
            })();
            evidence.record(&format!("{label}-complete-result.original.txt"), &observed)?;
            let after = bound
                .iter()
                .map(|(name, _)| std::fs::read(directory.join(name)))
                .collect::<Vec<_>>();
            evidence.record(&format!("{label}-source-guard.original.txt"), &after)?;
            let stable = after.iter().zip(&bound).all(|(observed, (_, expected))| matches!(observed, Ok(observed) if observed == expected));
            outcomes.push((label, observed, stable));
        }
    }
    evidence.record("cohort.original.txt", &outcomes)?;
    if outcomes.len() != 12
        || outcomes
            .iter()
            .any(|(_, outcome, stable)| !stable || !matches!(outcome, Ok(true)))
    {
        return Err("generated complete outcome cohort differs; originals retained".into());
    }
    Ok(())
}

const HOST_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
<PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
<ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors>
<NuGetAudit>false</NuGetAudit><EnableTargetingPackDownload>false</EnableTargetingPackDownload><EnableRuntimePackDownload>false</EnableRuntimePackDownload>
</PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
