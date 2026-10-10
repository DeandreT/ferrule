use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
#[path = "format_guid_string/projects.rs"]
mod projects;
const CASES: &str = include_str!("../../../functions/tests/fixtures/format_guid_string.tsv");

struct Evidence(PathBuf);
impl Evidence {
    fn new(label: &str) -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_GUID_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule-guid269-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        eprintln!("GUID269_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn record(&self, label: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
        std::fs::write(
            self.0.join(format!("{label}-ORIGINAL.txt")),
            format!("{original:#?}\n"),
        )?;
        Ok(())
    }
    fn command(&self, label: &str, command: &mut Command) -> TestResult<Output> {
        self.record(&format!("{label}-COMMAND"), command)?;
        let original = command.isolated_output();
        self.record(&format!("{label}-COMMAND-RESULT"), &original)?;
        let output = original?;
        std::fs::write(self.0.join(format!("{label}-stdout.bin")), &output.stdout)?;
        std::fs::write(self.0.join(format!("{label}-stderr.bin")), &output.stderr)?;
        Ok(output)
    }
}

#[test]
fn format_guid_string_native_complete_oracles_and_admission() -> TestResult<()> {
    let evidence = Evidence::new("native")?;
    std::fs::write(evidence.0.join("cases.tsv"), CASES)?;
    let project = projects::project();
    let validation = engine::validate(&project);
    let lowered = codegen::lower(&project);
    evidence.record(
        "COMPLETE-PROJECT-VALIDATION-LOWERING",
        &(&project, &validation, &lowered),
    )?;
    if !validation.is_empty() || lowered.is_err() {
        return Err("authored GUID project must admit".into());
    }
    let outcomes = CASES
        .lines()
        .map(|line| {
            let row = line.split('\t').collect::<Vec<_>>();
            let source = Instance::Group(
                vec![("Hex".into(), Instance::Scalar(projects::argument(&row)))].into(),
            );
            let actual = engine::run(&project, &source);
            (row[0], source, actual, projects::expected(row[3]))
        })
        .collect::<Vec<_>>();
    evidence.record("ALL-COMPLETE-INPUTS-OUTCOMES-EXPECTED", &outcomes)?;
    let mut arity = Vec::new();
    for count in [0, 2, 3] {
        let mut refused = project.clone();
        refused.graph.nodes.insert(
            2,
            Node::Call {
                function: "format_guid_string".into(),
                args: vec![1; count],
            },
        );
        arity.push((count, engine::validate(&refused), codegen::lower(&refused)));
    }
    evidence.record("ALL-COMPLETE-ARITY-ADMISSION", &arity)?;
    if outcomes.len() != 22
        || outcomes
            .iter()
            .any(|(_, _, actual, wanted)| actual != wanted)
        || arity
            .iter()
            .any(|(_, validation, lowered)| validation.is_empty() || lowered.is_ok())
    {
        return Err("complete native GUID formatter cohort differs".into());
    }
    Ok(())
}

#[test]
fn format_guid_string_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new("compiled")?;
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| evidence.0.join("cargo-target"));
    if !target.is_absolute() {
        return Err("shared Cargo target must be absolute".into());
    }
    let project = projects::project();
    let file = evidence.0.join("project.json");
    std::fs::write(&file, mapping::project_file::encode_pretty(&project)?)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut results = Vec::new();
    for language in ["rust", "csharp"] {
        let directory = evidence.0.join(language);
        let selected = if language == "rust" {
            GenerateTarget::Rust {
                runtime_path: runtime.clone(),
            }
        } else {
            GenerateTarget::CSharp
        };
        let generated = generate_project(&file, &directory, selected);
        evidence.record(&format!("{language}-COMPLETE-GENERATION"), &generated)?;
        generated?;
        std::fs::write(directory.join("cases.tsv"), CASES)?;
        if language == "rust" {
            std::fs::write(
                directory.join("src/main.rs"),
                include_str!("format_guid_string/Host.rs.txt"),
            )?;
        } else {
            std::fs::create_dir(directory.join("Harness"))?;
            std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
            std::fs::write(
                directory.join("Harness/Program.cs"),
                include_str!("format_guid_string/Host.cs.txt"),
            )?;
            std::fs::write(
                directory.join("NuGet.Config"),
                "<configuration><packageSources><clear /></packageSources></configuration>\n",
            )?;
        }
        let bound = artifact_files(&directory)?;
        evidence.record(&format!("{language}-COMPLETE-BOUND-ARTIFACTS"), &bound)?;
        let original = (|| -> TestResult<bool> {
            if language == "rust" {
                let mut command = Command::new("cargo");
                command
                    .args(["run", "--quiet", "--offline", "--jobs", "1", "--"])
                    .arg(&directory)
                    .current_dir(&directory)
                    .env("CARGO_TARGET_DIR", &target)
                    .env("CARGO_INCREMENTAL", "0")
                    .env("RUSTFLAGS", "-Dwarnings");
                return Ok(evidence
                    .command("rust-HOST", &mut command)?
                    .status
                    .success());
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
                .env("MSBuildEnableWorkloadResolver", "false")
                .env("DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE", "true")
                .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
                .env("MSBUILDDISABLENODEREUSE", "1");
            if !evidence
                .command("csharp-BUILD", &mut build)?
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
                .arg(&assembly)
                .arg(&directory)
                .current_dir(&directory);
            Ok(evidence
                .command("csharp-HOST", &mut command)?
                .status
                .success())
        })();
        evidence.record(&format!("{language}-COMPLETE-RESULT"), &original)?;
        let after = bound
            .iter()
            .map(|(path, _)| std::fs::read(directory.join(path)))
            .collect::<Vec<_>>();
        evidence.record(&format!("{language}-COMPLETE-SOURCE-READBACK"), &after)?;
        let stable = after
            .iter()
            .zip(&bound)
            .all(|(actual, (_, wanted))| matches!(actual, Ok(actual) if actual == wanted));
        results.push((language, original, stable));
    }
    evidence.record("ALL-COMPLETE-COMPILED-RESULTS", &results)?;
    if results
        .iter()
        .any(|(_, result, stable)| !*stable || !matches!(result, Ok(true)))
    {
        return Err("complete compiled GUID formatter cohort differs".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
