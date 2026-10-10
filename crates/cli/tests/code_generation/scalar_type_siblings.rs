use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

const CASES: &str = include_str!("scalar_type_siblings/cases.json");

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_JSON_SCALAR_TYPE_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule-scalar-types284-compiled-{}-{stamp}",
            std::process::id()
        ));
        host_policy::preflight_environment(&path)?;
        std::fs::create_dir_all(&path)?;
        eprintln!("SCALAR_TYPES284_COMPILED_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn record(&self, label: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
        std::fs::write(
            self.0.join(format!("{label}-ORIGINAL.txt")),
            format!("{original:#?}\n"),
        )?;
        Ok(())
    }
    fn command(&self, label: &str, directory: &Path, command: &mut Command) -> TestResult<Output> {
        self.record(&format!("{label}-COMMAND"), command)?;
        let original = host_policy::recorded_output(command, directory, label);
        self.record(&format!("{label}-COMMAND-RESULT"), &original)?;
        let output = original?;
        std::fs::write(self.0.join(format!("{label}-stdout.bin")), &output.stdout)?;
        std::fs::write(self.0.join(format!("{label}-stderr.bin")), &output.stderr)?;
        Ok(output)
    }
}

fn project(evidence: &Evidence, corpus: &serde_json::Value) -> TestResult<Project> {
    let schema =
        format_json::json_schema::import_str(&serde_json::to_string(&corpus["source_schema"])?);
    evidence.record("COMPLETE-SCHEMA-IMPORT", &schema)?;
    let schema = schema?;
    let expected: SchemaNode = serde_json::from_value(corpus["expected_model"].clone())?;
    if schema != expected {
        return Err("complete independently authored schema differs".into());
    }
    let mut graph = Graph::default();
    let mut root = Scope::default();
    let ir::SchemaKind::Group { children, .. } = &schema.kind else {
        return Err("independent identity root must be a group".into());
    };
    for (index, child) in children.iter().enumerate() {
        let node = u32::try_from(index)?;
        graph.nodes.insert(
            node,
            Node::SourceField {
                path: vec![child.name.clone()],
                frame: None,
            },
        );
        root.bindings.push(Binding {
            target_field: child.name.clone(),
            node,
        });
    }
    let project = Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root,
    };
    let validation = engine::validate(&project);
    let lowering = codegen::lower(&project);
    evidence.record(
        "COMPLETE-PROJECT-VALIDATION-LOWERING",
        &(&project, &validation, &lowering),
    )?;
    if !validation.is_empty() || lowering.is_err() {
        return Err("authored identity project must admit".into());
    }
    Ok(project)
}

#[test]
fn redundant_scalar_type_siblings_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new()?;
    std::fs::write(evidence.0.join("cases.json"), CASES)?;
    let corpus: serde_json::Value = serde_json::from_str(CASES)?;
    let project = project(&evidence, &corpus)?;
    let project_file = evidence.0.join("project.json");
    std::fs::write(
        &project_file,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let descriptor = serde_json::to_string(&project.source)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut results = Vec::new();
    for language in ["rust", "csharp"] {
        let directory = evidence.0.join(language);
        let target = if language == "rust" {
            GenerateTarget::Rust {
                runtime_path: runtime.clone(),
            }
        } else {
            GenerateTarget::CSharp
        };
        let generated = generate_project(&project_file, &directory, target);
        evidence.record(&format!("{language}-COMPLETE-GENERATION"), &generated)?;
        let generated = generated?;
        let published = artifact_files(&directory)?;
        evidence.record(
            &format!("{language}-COMPLETE-PUBLISHED-ARTIFACTS"),
            &published,
        )?;
        if generated.files_written != published.len() || generated.output_directory != directory {
            return Err("complete generated artifact census differs".into());
        }
        let mapping_file = if language == "rust" {
            "src/lib.rs"
        } else {
            "GeneratedMapping.cs"
        };
        let mapping = std::fs::read_to_string(directory.join(mapping_file))?;
        for name in if language == "rust" {
            [
                "const SOURCE_JSON_SCHEMA: &str = ",
                "const TARGET_JSON_SCHEMA: &str = ",
            ]
        } else {
            [
                "private const string SourceJsonSchema = ",
                "private const string TargetJsonSchema = ",
            ]
        } {
            if !mapping.contains(&format!("{name}{descriptor:?};")) {
                return Err("complete embedded schema differs from independent model".into());
            }
        }
        std::fs::write(directory.join("cases.json"), CASES)?;
        std::fs::write(directory.join("descriptor.json"), &descriptor)?;
        if language == "rust" {
            std::fs::write(
                directory.join("src/main.rs"),
                include_str!("scalar_type_siblings/Host.rs.txt"),
            )?;
        } else {
            std::fs::create_dir(directory.join("Harness"))?;
            std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
            std::fs::write(
                directory.join("Harness/Program.cs"),
                include_str!("scalar_type_siblings/Host.cs.txt"),
            )?;
            std::fs::write(
                directory.join("NuGet.Config"),
                "<configuration><packageSources><clear /></packageSources></configuration>\n",
            )?;
        }
        let bound = artifact_files(&directory)?;
        evidence.record(&format!("{language}-COMPLETE-BOUND-SOURCES"), &bound)?;
        let original = (|| -> TestResult<bool> {
            if language == "rust" {
                let mut command = Command::new("cargo");
                command
                    .args(["run", "--quiet", "--offline", "--jobs", "1", "--"])
                    .arg(&directory)
                    .current_dir(&directory)
                    .env("RUSTFLAGS", "-Dwarnings");
                return Ok(evidence
                    .command("rust-HOST", &directory, &mut command)?
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
                .command("csharp-BUILD", &directory, &mut build)?
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
                .arg(&directory)
                .current_dir(&directory);
            Ok(evidence
                .command("csharp-HOST", &directory, &mut command)?
                .status
                .success())
        })();
        evidence.record(&format!("{language}-COMPLETE-RESULT"), &original)?;
        let after = bound
            .iter()
            .map(|(path, _)| std::fs::read(directory.join(path)))
            .collect::<Vec<_>>();
        evidence.record(&format!("{language}-COMPLETE-SOURCE-READBACK"), &after)?;
        let unchanged = after
            .iter()
            .zip(&bound)
            .all(|(actual, (_, wanted))| matches!(actual, Ok(actual) if actual == wanted));
        results.push((language, original, unchanged));
    }
    evidence.record("ALL-COMPLETE-COMPILED-RESULTS", &results)?;
    if results
        .iter()
        .any(|(_, result, unchanged)| !unchanged || !matches!(result, Ok(true)))
    {
        return Err("full compiled scalar type-sibling outcomes differ".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
