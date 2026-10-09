use super::*;
use std::fs;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "user_function_argument_order/projects.rs"]
mod projects;

// Always retain generated source, whole outcomes, and unsuccessful commands.
struct Evidence(PathBuf);
impl Evidence {
    fn new(label: &str) -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_CODEGEN_UDF_ORDER_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        std::fs::create_dir_all(&parent)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule-udf-order186-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        eprintln!("UDF_ORDER186_ORIGINALS={}", path.display());
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

// InstanceGroup Debug omits origin; retain every tag/payload in immutable ordered traversal.
fn retain_origins(path: &Path, value: &Instance) -> io::Result<()> {
    fn visit(file: &mut impl Write, value: &Instance, path: &str) -> io::Result<()> {
        match value {
            Instance::Group(fields) => {
                writeln!(file, "path={path:?};origin={:?}", fields.xml_type_origin())?;
                for (index, (name, child)) in fields.iter().enumerate() {
                    visit(file, child, &format!("{path}/field[{index}]={name:?}"))?;
                }
            }
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                let kind = if matches!(value, Instance::Repeated(_)) {
                    "repeated"
                } else {
                    "mapped"
                };
                for (index, item) in items.iter().enumerate() {
                    visit(file, item, &format!("{path}/{kind}[{index}]"))?;
                }
            }
            Instance::DocumentSet(documents) => {
                for (index, document) in documents.iter().enumerate() {
                    visit(
                        file,
                        document.value(),
                        &format!("{path}/document[{index}]={:?}", document.path()),
                    )?;
                }
            }
            Instance::Scalar(_) => {}
        }
        Ok(())
    }
    let mut file = io::BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
    );
    visit(&mut file, value, "root")?;
    file.flush()
}

fn fixtures() -> Vec<(&'static str, &'static str, Project)> {
    vec![
        (
            "ordinary-literal",
            "literal",
            projects::literal_project(false),
        ),
        ("nested-literal", "literal", projects::literal_project(true)),
        (
            "ordinary-controls",
            "controls",
            projects::controlled_project(false),
        ),
        (
            "nested-controls",
            "controls",
            projects::controlled_project(true),
        ),
    ]
}

#[test]
fn user_function_argument_order_native_literals_and_controls() -> TestResult<()> {
    let root = Evidence::new("native")?;
    let mut originals = Vec::new();
    for (name, mode, project) in fixtures() {
        root.record(&format!("{name}-COMPLETE-PROJECT"), &project)?;
        let validation = engine::validate(&project);
        root.record(&format!("{name}-COMPLETE-VALIDATION"), &validation)?;
        if !validation.is_empty() {
            return Err("project must validate before execution".into());
        }
        if mode == "literal" {
            let source = Instance::Group(
                vec![(
                    "Unused".into(),
                    Instance::Scalar(Value::String("unused".into())),
                )]
                .into(),
            );
            let actual = engine::run(&project, &source);
            root.record(&format!("{name}-literal-COMPLETE-OUTCOME"), &actual)?;
            if let Ok(value) = &actual {
                retain_origins(
                    &root
                        .0
                        .join(format!("{name}-literal-COMPLETE-GROUP-ORIGINS.txt")),
                    value,
                )?;
            }
            originals.push((
                name,
                "literal",
                actual,
                Err(engine::EngineError::MappingException {
                    node: 2,
                    message: None,
                }),
            ));
        } else {
            for (label, flags, expected) in projects::cases() {
                let source = projects::source(flags);
                let actual = engine::run(&project, &source);
                root.record(
                    &format!("{name}-{label}-COMPLETE-INPUT-OUTCOME"),
                    &(&source, &actual),
                )?;
                if let Ok(value) = &actual {
                    retain_origins(
                        &root
                            .0
                            .join(format!("{name}-{label}-COMPLETE-GROUP-ORIGINS.txt")),
                        value,
                    )?;
                }
                originals.push((name, label, actual, expected));
            }
        }
    }
    root.record(
        "ALL18-COMPLETE-TYPED-ORIGINALS-AND-MANUAL-EXPECTED",
        &originals,
    )?;
    let mismatches = originals
        .iter()
        .filter(|(_, _, actual, expected)| actual != expected)
        .collect::<Vec<_>>();
    root.record("ALL18-COMPLETE-COMPARISONS", &mismatches)?;
    if originals.len() != 18 || !mismatches.is_empty() {
        return Err("native complete outcome vector differs".into());
    }
    Ok(())
}

#[test]
#[ignore = "root-coordinated compiled witness must run before and after the two-emitter correction"]
fn user_function_argument_order_compiled_complete_oracles() -> TestResult<()> {
    let root = Evidence::new("compiled")?;
    let target = PathBuf::from(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .ok_or("set coordinated shared Cargo target")?,
    );
    if !target.is_absolute() {
        return Err("shared Cargo target must be absolute".into());
    }
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut outcomes = Vec::new();
    for (name, mode, project) in fixtures() {
        let validation = engine::validate(&project);
        root.record(&format!("{name}-COMPLETE-VALIDATION"), &validation)?;
        if !validation.is_empty() {
            return Err("mapping must validate before generation".into());
        }
        let file = root.0.join(format!("{name}-project.json"));
        std::fs::write(&file, mapping::project_file::encode_pretty(&project)?)?;
        for language in ["rust", "csharp"] {
            let directory = root.0.join(language).join(name);
            let selected = if language == "rust" {
                GenerateTarget::Rust {
                    runtime_path: runtime.clone(),
                }
            } else {
                GenerateTarget::CSharp
            };
            let generated = generate_project(&file, &directory, selected);
            root.record(
                &format!("{name}-{language}-COMPLETE-PUBLIC-GENERATION"),
                &generated,
            )?;
            let generated = generated?;
            let original_files = artifact_files(&directory)?;
            root.record(
                &format!("{name}-{language}-COMPLETE-ORIGINAL-ARTIFACTS"),
                &original_files,
            )?;
            if generated.output_directory != directory
                || generated.files_written != original_files.len()
            {
                return Err("whole artifact census differs".into());
            }
            if language == "rust" {
                std::fs::write(
                    directory.join("src/main.rs"),
                    include_str!("user_function_argument_order/Host.rs.txt"),
                )?;
            } else {
                let harness = directory.join("Harness");
                std::fs::create_dir(&harness)?;
                std::fs::write(harness.join("Host.csproj"), HARNESS_PROJECT)?;
                std::fs::write(
                    harness.join("Program.cs"),
                    include_str!("user_function_argument_order/Host.cs.txt"),
                )?;
                std::fs::write(
                    directory.join("NuGet.Config"),
                    "<configuration><packageSources><clear /></packageSources></configuration>\n",
                )?;
            }
            // Complete generated source and literal host bytes are bound before compiler output exists.
            let bound_inputs = artifact_files(&directory)?;
            root.record(
                &format!("{name}-{language}-COMPLETE-BOUND-INPUTS"),
                &bound_inputs,
            )?;
            let label = format!("{name}-{language}");
            let original = (|| -> TestResult<bool> {
                if language == "rust" {
                    let mut command = Command::new("cargo");
                    command
                        .args(["run", "--quiet", "--offline", "--jobs", "1", "--", mode])
                        .arg(&directory)
                        .current_dir(&directory)
                        .env("CARGO_TARGET_DIR", &target)
                        .env("CARGO_INCREMENTAL", "0")
                        .env("RUSTFLAGS", "-Dwarnings");
                    return Ok(root.command(&label, &mut command)?.status.success());
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
                        "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                        "-p:EnableTargetingPackDownload=false",
                        "-p:EnableRuntimePackDownload=false",
                        "-p:AutomaticallyUseReferenceAssemblyPackages=false",
                    ])
                    .current_dir(&directory)
                    .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
                    .env("MSBUILDDISABLENODEREUSE", "1");
                if !root
                    .command(&format!("{label}-BUILD"), &mut build)?
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
                    .arg(mode)
                    .arg(&directory)
                    .current_dir(&directory);
                Ok(root
                    .command(&format!("{label}-HOST"), &mut command)?
                    .status
                    .success())
            })();
            // Preserve failures and complete source readbacks before propagating errors or status.
            root.record(&format!("{label}-COMPLETE-RESULT"), &original)?;
            let after = bound_inputs
                .iter()
                .map(|(path, _)| std::fs::read(directory.join(path)))
                .collect::<Vec<_>>();
            root.record(&format!("{label}-COMPLETE-SOURCE-GUARD-AFTER"), &after)?;
            let stable = after
                .iter()
                .zip(&bound_inputs)
                .all(|(actual, (_, wanted))| matches!(actual, Ok(actual) if actual == wanted));
            outcomes.push((label, original, stable));
        }
    }
    root.record("ALL8-COMPLETE-GENERATED-COHORT-RESULTS", &outcomes)?;
    if outcomes.len() != 8
        || outcomes
            .iter()
            .any(|(_, actual, stable)| !stable || !matches!(actual, Ok(true)))
    {
        return Err("generated cohort differs; complete originals retained".into());
    }
    Ok(())
}

const HARNESS_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <NuGetAudit>false</NuGetAudit><DisableTransitiveFrameworkReferenceDownloads>true</DisableTransitiveFrameworkReferenceDownloads>
    <AutomaticallyUseReferenceAssemblyPackages>false</AutomaticallyUseReferenceAssemblyPackages>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#;
