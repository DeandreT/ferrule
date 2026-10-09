use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "variadic_logical/projects.rs"]
mod projects;

const CASES: &str = include_str!("variadic_logical/cases.tsv");

struct Evidence(PathBuf);
impl Evidence {
    fn new(label: &str) -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_LOGICAL_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule-logical237-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        eprintln!("LOGICAL237_ORIGINALS={}", path.display());
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

fn expected(row: &[&str]) -> Result<Instance, engine::EngineError> {
    let function = if row[2] == "and" { "and" } else { "or" };
    match row[9] {
        "T" | "F" => Ok(Instance::Group(
            vec![(
                "Result".into(),
                Instance::Scalar(Value::Bool(row[9] == "T")),
            )]
            .into(),
        )),
        "type:null" | "type:string" | "type:int" => Err(engine::EngineError::Function(
            codegen_runtime::FunctionError::TypeMismatch {
                function,
                got: match row[9] {
                    "type:null" => "null",
                    "type:string" => "string",
                    _ => "int",
                },
            },
        )),
        text => Err(engine::EngineError::MappingException {
            node: text
                .strip_prefix("raise:")
                .expect("frozen category")
                .parse()
                .expect("frozen node"),
            message: None,
        }),
    }
}

#[test]
fn variadic_logical_native_complete_oracles_and_admission() -> TestResult<()> {
    let evidence = Evidence::new("native")?;
    std::fs::write(evidence.0.join("cases.tsv"), CASES)?;
    let project = projects::project();
    evidence.record("COMPLETE-PROJECT", &project)?;
    let validation = engine::validate(&project);
    let lowering = codegen::lower(&project);
    evidence.record("COMPLETE-VALIDATION", &validation)?;
    evidence.record("COMPLETE-LOWERING", &lowering)?;
    if !validation.is_empty() || lowering.is_err() {
        return Err("authored calls must admit before execution".into());
    }
    let rows = CASES
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let originals = rows
        .iter()
        .map(|row| {
            let input = projects::source(row);
            let actual = engine::run(&project, &input);
            (row[0], input, actual, expected(row))
        })
        .collect::<Vec<_>>();
    evidence.record("ALL-COMPLETE-INPUTS-OUTCOMES-EXPECTED", &originals)?;
    evidence.record(
        "ALL-COMPLETE-GROUP-ORIGINS",
        &originals
            .iter()
            .map(|(label, source, actual, wanted)| {
                (
                    *label,
                    projects::origins(source),
                    actual.as_ref().ok().map(projects::origins),
                    wanted.as_ref().ok().map(projects::origins),
                )
            })
            .collect::<Vec<_>>(),
    )?;
    let mut arity = Vec::new();
    for function in ["and", "or"] {
        for count in [0, 1] {
            let mut refused = project.clone();
            refused.graph.nodes.insert(
                300,
                Node::Call {
                    function: function.into(),
                    args: (31..31 + count).collect(),
                },
            );
            let validation = engine::validate(&refused);
            let lowered = codegen::lower(&refused);
            let direct = codegen_runtime::call(function, &vec![Value::Bool(true); count as usize]);
            arity.push((function, count, validation, lowered, direct));
        }
    }
    evidence.record("ALL-COMPLETE-ARITY-CONTROLS", &arity)?;
    let mismatches = originals
        .iter()
        .filter(|(_, _, actual, wanted)| actual != wanted)
        .collect::<Vec<_>>();
    evidence.record("COMPLETE-COMPARISONS", &mismatches)?;
    if originals.len() != 378
        || !mismatches.is_empty()
        || arity
            .iter()
            .any(|(function, count, validation, lowered, actual)| {
                validation.is_empty()
                    || lowered.is_ok()
                    || *actual
                        != Err(codegen_runtime::RuntimeError::Function(
                            codegen_runtime::FunctionError::ArityMismatch {
                                function,
                                expected: 2,
                                got: *count as usize,
                            },
                        ))
            })
    {
        return Err("complete logical native/admission cohort differs".into());
    }
    Ok(())
}

#[test]
fn variadic_logical_compiled_complete_oracles() -> TestResult<()> {
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
                include_str!("variadic_logical/Host.rs.txt"),
            )?;
        } else {
            std::fs::create_dir(directory.join("Harness"))?;
            std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
            std::fs::write(
                directory.join("Harness/Program.cs"),
                include_str!("variadic_logical/Host.cs.txt"),
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
        return Err("complete compiled logical cohort differs".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
