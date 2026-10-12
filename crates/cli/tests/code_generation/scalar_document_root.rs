use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
mod models {
    include!("../../../mfd/tests/fixtures/scalar_document_root/expected_models.rs.txt");
}

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_SCALAR_ROOT_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        std::fs::create_dir_all(&parent)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule_scalar_root339_compiled_{}_{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        eprintln!("SCALAR_ROOT339_COMPILED_ORIGINALS={}", path.display());
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
fn singleton_scalar_document_roots_compiled_complete_oracles() -> TestResult<()> {
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
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/scalar_document_root");
    let cases = std::fs::read(fixtures.join("cases.json"))?;
    let mut outcomes = Vec::new();
    let parsed_cases: serde_json::Value = serde_json::from_slice(&cases)?;
    for design in [
        "source-String-modern",
        "constant-String-modern",
        "source-Bool-modern",
        "constant-Bool-modern",
        "source-Int-modern",
        "constant-Int-modern",
        "source-Float-modern",
        "constant-Float-modern",
        "named",
    ] {
        let package = evidence.0.join(format!("{design}-package"));
        std::fs::create_dir(&package)?;
        for file in std::fs::read_dir(&fixtures)? {
            let file = file?;
            std::fs::copy(file.path(), package.join(file.file_name()))?;
        }
        let case = parsed_cases
            .as_array()
            .ok_or("literal case array")?
            .iter()
            .find(|case| {
                case["name"]
                    == if design == "named" {
                        "source-String-modern"
                    } else {
                        design
                    }
            })
            .ok_or("literal design")?;
        let expected = if design == "named" {
            models::expected_named_project()
        } else {
            models::expected_project(case)
        };
        evidence.record(
            &format!("{design}-complete-project.expected.txt"),
            &expected,
        )?;
        let mapping = package.join("mapping.mfd");
        std::fs::copy(fixtures.join(format!("{design}.mfd")), &mapping)?;
        std::fs::write(package.join("expected-before-run.json"), &cases)?;
        let imported = mfd::import_with_profile(
            &mapping,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        );
        evidence.record(
            &format!("{design}-import.original.txt"),
            imported.as_ref().map(|value| {
                (
                    &value.imported.project,
                    &value.imported.warnings,
                    &value.imported.mapping_path,
                    &value.report,
                )
            }),
        )?;
        let imported = imported?.imported;
        if serde_json::to_value(&imported.project)? != serde_json::to_value(&expected)? {
            return Err("complete imported Project differs from independent literal".into());
        }
        let validation = engine::validate(&imported.project);
        evidence.record(&format!("{design}-validation.original.txt"), &validation)?;
        if !imported.warnings.is_empty() || !validation.is_empty() {
            return Err("authored project must import and validate before generation".into());
        }
        // The complete typed source is independently constructed, not parsed into expected data.
        let typed = engine::run(&imported.project, &models::source_instance());
        evidence.record(&format!("{design}-native.original.txt"), &typed)?;
        if typed? != Instance::Scalar(models::scalar(case)) {
            return Err("native scalar differs".into());
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
            assert_emitted_artifact_paths(&original, language, false);
            if language == "csharp" {
                let runtime_bodies = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../runtime/csharp/Ferrule.Runtime");
                for (path, body) in &original {
                    if let Some(relative) = path.strip_prefix("Runtime/")
                        && std::fs::read(runtime_bodies.join(relative))? != *body
                    {
                        return Err("published runtime body differs from actual source".into());
                    }
                }
            }
            evidence.record(&format!("{label}-generated-files.original.txt"), &original)?;
            if generated.output_directory != directory || generated.files_written != original.len()
            {
                return Err("generated artifact census differs".into());
            }
            std::fs::write(directory.join("cases.json"), &cases)?;
            std::fs::copy(fixtures.join("input.xml"), directory.join("input.xml"))?;
            std::fs::copy(
                fixtures.join("typed-controls.json"),
                directory.join("typed-controls.json"),
            )?;
            std::fs::write(
                directory.join("target-descriptor.json"),
                codegen::serialize_embedded_schema(
                    &expected.target,
                    codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
                )?,
            )?;
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
                    include_str!("scalar_document_root/Host.rs.txt"),
                )?;
            } else {
                let host = directory.join("Harness");
                std::fs::create_dir(&host)?;
                std::fs::write(host.join("Host.csproj"), HOST_PROJECT)?;
                std::fs::write(
                    host.join("Program.cs"),
                    include_str!("scalar_document_root/Host.cs.txt"),
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
    if outcomes.len() != 18
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
