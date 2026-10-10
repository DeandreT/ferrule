use super::*;
use serde_json::Value as Json;
use std::time::{SystemTime, UNIX_EPOCH};

const CASES: &str =
    include_str!("../../../format-json/tests/fixtures/signed_integer_float_bounds.json");

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_JSON_BOUNDS_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule-json-bounds-compiled-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        std::fs::write(path.join("cases.json"), CASES)?;
        eprintln!("JSON_BOUNDS_ORIGINALS={}", path.display());
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

fn project(evidence: &Evidence, corpus: &Json, input_bounded: bool) -> TestResult<Project> {
    let bounded = format_json::json_schema::import_str(
        corpus["compiled"]["bounded_schema"]
            .as_str()
            .ok_or("bounded schema required")?,
    );
    let unbounded = format_json::json_schema::import_str(
        corpus["compiled"]["unbounded_schema"]
            .as_str()
            .ok_or("unbounded schema required")?,
    );
    evidence.record(
        &format!("{input_bounded}-COMPLETE-IMPORTS"),
        &(&bounded, &unbounded),
    )?;
    let bounded = bounded?;
    let unbounded = unbounded?;
    let (source, target) = if input_bounded {
        (bounded, unbounded)
    } else {
        (unbounded, bounded)
    };
    let mut graph = Graph::default();
    let mut bindings = Vec::new();
    for (index, name) in corpus["compiled"]["expected_ranges"]
        .as_object()
        .ok_or("ranges required")?
        .keys()
        .enumerate()
    {
        let id = u32::try_from(index)?;
        graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.clone()],
                frame: None,
            },
        );
        bindings.push(Binding {
            target_field: name.clone(),
            node: id,
        });
    }
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph,
        root: Scope {
            bindings,
            ..Scope::default()
        },
    };
    let validation = engine::validate(&project);
    let lowered = codegen::lower(&project);
    evidence.record(
        &format!("{input_bounded}-COMPLETE-PROJECT-ADMISSION"),
        &(&project, &validation, &lowered),
    )?;
    if !validation.is_empty() || lowered.is_err() {
        return Err("authored numeric project must admit".into());
    }
    Ok(project)
}

#[test]
fn signed_integer_float_bounds_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let corpus: Json = serde_json::from_str(CASES)?;
    if corpus["compiled"]["cases"].as_array().map(Vec::len) != Some(31) {
        return Err("authored compiled case count differs".into());
    }
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| evidence.0.join("cargo-target"));
    if !target.is_absolute() {
        return Err("generated host target must be absolute".into());
    }
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut results = Vec::new();
    for input_bounded in [true, false] {
        let project = project(&evidence, &corpus, input_bounded)?;
        let label = if input_bounded { "input" } else { "output" };
        let file = evidence.0.join(format!("{label}-project.json"));
        std::fs::write(&file, mapping::project_file::encode_pretty(&project)?)?;
        for language in ["rust", "csharp"] {
            let name = format!("{label}-{language}");
            let directory = evidence.0.join(&name);
            let selected = if language == "rust" {
                GenerateTarget::Rust {
                    runtime_path: runtime.clone(),
                }
            } else {
                GenerateTarget::CSharp
            };
            let generated = generate_project(&file, &directory, selected);
            evidence.record(&format!("{name}-COMPLETE-GENERATION"), &generated)?;
            generated?;
            std::fs::write(directory.join("cases.json"), CASES)?;
            if language == "rust" {
                let manifest_path = directory.join("Cargo.toml");
                let manifest = std::fs::read_to_string(&manifest_path)?;
                std::fs::write(manifest_path, manifest.replace("\n[workspace]", "\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\n\n[workspace]"))?;
                std::fs::write(
                    directory.join("src/main.rs"),
                    include_str!("signed_integer_float_bounds/Host.rs.txt"),
                )?;
            } else {
                std::fs::create_dir(directory.join("Harness"))?;
                std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
                std::fs::write(
                    directory.join("Harness/Program.cs"),
                    include_str!("signed_integer_float_bounds/Host.cs.txt"),
                )?;
                std::fs::write(
                    directory.join("NuGet.Config"),
                    "<configuration><packageSources><clear /></packageSources></configuration>\n",
                )?;
            }
            let bound = artifact_files(&directory)?;
            evidence.record(&format!("{name}-COMPLETE-BOUND-ARTIFACTS"), &bound)?;
            let original = (|| -> TestResult<bool> {
                if language == "rust" {
                    let mut command = Command::new("cargo");
                    command
                        .args(["run", "--quiet", "--offline", "--jobs", "1", "--"])
                        .arg(&directory)
                        .arg(label)
                        .current_dir(&directory)
                        .env("CARGO_TARGET_DIR", &target)
                        .env("CARGO_INCREMENTAL", "0")
                        .env("RUSTFLAGS", "-Dwarnings");
                    return Ok(evidence.command(&name, &mut command)?.status.success());
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
                    .command(&format!("{name}-BUILD"), &mut build)?
                    .status
                    .success()
                {
                    return Ok(false);
                }
                let assembly = directory.join("Harness/bin/Debug/net10.0/Host.dll");
                if !assembly.is_file() {
                    return Err("host assembly omitted".into());
                }
                let mut command = dotnet_command(&directory);
                command
                    .arg(assembly)
                    .arg(&directory)
                    .arg(label)
                    .current_dir(&directory);
                Ok(evidence.command(&name, &mut command)?.status.success())
            })();
            evidence.record(&format!("{name}-COMPLETE-RESULT"), &original)?;
            let after = bound
                .iter()
                .map(|(path, _)| std::fs::read(directory.join(path)))
                .collect::<Vec<_>>();
            evidence.record(&format!("{name}-COMPLETE-SOURCE-READBACK"), &after)?;
            let stable = after
                .iter()
                .zip(&bound)
                .all(|(actual, (_, wanted))| matches!(actual, Ok(actual) if actual == wanted));
            results.push((name, original, stable));
        }
    }
    evidence.record("ALL-COMPLETE-COMPILED-RESULTS", &results)?;
    if results
        .iter()
        .any(|(_, result, stable)| !*stable || !matches!(result, Ok(true)))
    {
        return Err("complete compiled numeric boundary cohort differs".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
