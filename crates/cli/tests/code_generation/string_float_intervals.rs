use super::*;
use serde_json::Value as Json;
use std::time::{SystemTime, UNIX_EPOCH};

const CASES: &str = include_str!("../../../format-json/tests/fixtures/string_float_intervals.json");

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_STRING_FLOAT_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule-string-float-compiled-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        std::fs::write(path.join("cases.json"), CASES)?;
        eprintln!("STRING_FLOAT_ORIGINALS={}", path.display());
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
    for (index, name) in corpus["compiled"]["fields"]
        .as_array()
        .ok_or("fields required")?
        .iter()
        .enumerate()
    {
        let name = name.as_str().ok_or("field name")?.to_owned();
        let id = u32::try_from(index)?;
        graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.clone()],
                frame: None,
            },
        );
        bindings.push(Binding {
            target_field: name,
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
        return Err("authored String-or-Float project must admit".into());
    }
    Ok(project)
}

fn malformed_program_admission(
    evidence: &Evidence,
    corpus: &Json,
    runtime: &Path,
) -> TestResult<()> {
    let valid_project = project(evidence, corpus, true)?;
    let valid_program = codegen::lower(&valid_project)?;
    let mut comparisons = Vec::new();
    for row in corpus["malformed_ir_cases"]
        .as_array()
        .ok_or("malformed IR cases required")?
    {
        let raw = &row["schema"];
        let mut invalid = SchemaNode::scalar_union(
            "Reading",
            serde_json::from_value(raw["kind"]["types"].clone())?,
        );
        invalid.numeric_range = Some(serde_json::from_value(raw["numeric_range"].clone())?);
        invalid.fixed = raw["fixed"].as_str().map(str::to_owned);
        invalid.string_length_range = raw
            .get("string_length_range")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()?;
        let mut project = valid_project.clone();
        let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
            return Err("authored object source required".into());
        };
        children[0] = invalid.clone();
        let mut program = valid_program.clone();
        let ir::SchemaKind::Group { children, .. } = &mut program.source.kind else {
            return Err("authored program source required".into());
        };
        children[0] = invalid;
        let validation = engine::validate(&project);
        let lowering = codegen::lower(&project);
        let admitted = codegen::validate_program(&program);
        let rust = codegen_rust::emit(
            &program,
            &codegen_rust::Options {
                package_name: "authored_invalid".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    runtime.display().to_string(),
                ),
            },
        );
        let csharp = codegen_csharp::emit(&program);
        evidence.record(
            &format!(
                "{}-MALFORMED-COMPLETE-ADMISSION",
                row["id"].as_str().unwrap()
            ),
            &(
                &row,
                &project,
                &program,
                &validation,
                &lowering,
                &admitted,
                &rust,
                &csharp,
            ),
        )?;
        let wanted = codegen::ProgramValidationError::InvalidSchemaMetadata {
            boundary: "source".into(),
            path: vec!["Reading".into()],
        };
        let numeric_owner = engine::ValidationOwner::SchemaNode(engine::ValidationSchemaLocation {
            endpoint: engine::ValidationEndpoint::Source,
            path: vec![engine::ValidationSchemaStep::Child(0)],
        });
        let correct_native = validation
            .iter()
            .filter(|issue| issue.message.starts_with("numeric-range metadata"))
            .collect::<Vec<_>>();
        let expected_diagnostics = validation
            .iter()
            .map(|issue| codegen::Diagnostic::Validation {
                location: issue.location.clone(),
                message: issue.message.clone(),
            })
            .collect::<Vec<_>>();
        let correct = correct_native.len() == 1
            && correct_native[0].location == "source schema"
            && correct_native[0].message
                == "numeric-range metadata at `Reading` requires one matching concrete numeric scalar and must contain its fixed value"
            && correct_native[0].owner.as_ref() == Some(&numeric_owner)
            && matches!(&lowering, Err(error) if error.diagnostics() == expected_diagnostics.as_slice())
            && matches!(&admitted, Err(error) if error == &wanted)
            && matches!(&rust, Err(codegen_rust::EmitError::InvalidProgram(error)) if error == &wanted)
            && matches!(&csharp, Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == &wanted);
        comparisons.push((row["id"].clone(), correct));
    }
    evidence.record("MALFORMED-COMPLETE-COMPARISONS", &comparisons)?;
    if comparisons.iter().any(|(_, correct)| !correct) {
        return Err("malformed String-or-Float program admission differs".into());
    }
    Ok(())
}

#[test]
fn string_float_intervals_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let corpus: Json = serde_json::from_str(CASES)?;
    if corpus["compiled"]["cases"].as_array().map(Vec::len) != Some(16) {
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
    malformed_program_admission(&evidence, &corpus, &runtime)?;
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
                    include_str!("string_float_intervals/Host.rs.txt"),
                )?;
            } else {
                std::fs::create_dir(directory.join("Harness"))?;
                std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
                std::fs::write(
                    directory.join("Harness/Program.cs"),
                    include_str!("string_float_intervals/Host.cs.txt"),
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
        return Err("complete compiled String-or-Float boundary cohort differs".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
