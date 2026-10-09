//! A bounded, compiler-free preparer for paired public typed selection hosts.
//! Compilation and host invocations are separate root-coordinated gates.
use super::*;
use serde_json::Value as Json;
use std::fs;
use std::io::Write;

#[path = "selected_targets/native.rs"]
mod native;
#[path = "selected_targets/projects.rs"]
mod projects;

const CONTROLS: &str = include_str!("selected_targets/controls.json");

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let path = PathBuf::from(
            std::env::var_os("FERRULE_SELECTED_TARGET198_PREPARE_DIR")
                .ok_or("root must supply a fresh absolute preparation directory")?,
        );
        if !path.is_absolute() || fs::symlink_metadata(&path).is_ok() {
            return Err("preparation directory must be fresh and absolute".into());
        }
        let parent = path.parent().ok_or("preparation parent")?;
        if !parent.is_dir() {
            return Err("preparation parent must already exist".into());
        }
        fs::create_dir(&path)?;
        eprintln!("SELECTED_TARGET198_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn write(&self, name: &str, bytes: &[u8]) -> TestResult<()> {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
        Ok(())
    }
    fn record(&self, name: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        self.write(name, format!("{value:#?}\n").as_bytes())
    }
    fn artifacts(
        &self,
        label: &str,
        directory: &str,
        actual: &codegen::ArtifactSet,
    ) -> TestResult<()> {
        self.record(
            &format!("{label}-COMPLETE-ARTIFACT-SET-ORIGINAL.txt"),
            actual,
        )?;
        for file in actual.files() {
            self.write(
                &format!("{directory}/{}", file.path.as_str()),
                &file.contents,
            )?;
        }
        Ok(())
    }
}

#[test]
#[ignore = "root-coordinated typed cohort preparation; verified Rust and C# backend merges and one shared compiler lane required"]
fn prepare_selected_target198_four_profiles_thirty_native_and_paired_typed_controls()
-> TestResult<()> {
    let evidence = Evidence::new()?;
    evidence.write(
        "COMPLETE-FROZEN30-CONTROLS-ORIGINAL.json",
        CONTROLS.as_bytes(),
    )?;
    let corpus: Json = serde_json::from_str(CONTROLS)?;
    let projects = projects::projects();
    // Complete constructed public schemas, metadata, graph/UDFs/options and
    // every input/origin are retained before validation or expectation checks.
    evidence.record("ALL4-COMPLETE-TYPED-PROJECTS-ORIGINAL.txt", &projects)?;
    for (index, project) in projects.iter().enumerate() {
        evidence.write(
            &format!("profile{index}-COMPLETE-PUBLIC-PROJECT-CODEC-ORIGINAL.json"),
            mapping::project_file::encode_pretty(project)?.as_bytes(),
        )?;
        evidence.write(
            &format!("profile{index}-COMPLETE-SERDE-PROJECT-ORIGINAL.json"),
            &serde_json::to_vec_pretty(project)?,
        )?;
    }
    let mut outcomes = Vec::new();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("workspace crates parent")?
        .join("codegen-runtime");
    let mut rust_modules = String::new();
    let mut rust_dispatch = String::from(
        "fn invoke(index:usize,control:&Json,source:&Instance,inputs:&[NamedInput<'_>],execution:Option<&ExecutionContext<'_>>,loader:Option<&dyn DynamicSourceLoader>,checker:&Checker,limits:FilterMapLimits)->Actual {\n    match index {\n",
    );
    for (index, project) in projects.iter().enumerate() {
        let validation = engine::validate(project);
        evidence.record(
            &format!("profile{index}-COMPLETE-VALIDATION-ORIGINAL.txt"),
            &validation,
        )?;
        let lower = codegen::lower(project);
        evidence.record(
            &format!("profile{index}-COMPLETE-LOWERING-PROGRAM-ORIGINAL.txt"),
            &lower,
        )?;
        let program = match lower {
            Ok(program) => program,
            Err(error) => {
                outcomes.push((index, false, error.to_string()));
                continue;
            }
        };
        let options = codegen_rust::Options {
            package_name: format!("selected-target198-profile{index}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        evidence.record(
            &format!("profile{index}-COMPLETE-RUST-EMISSION-OPTIONS-ORIGINAL.txt"),
            &options,
        )?;
        let rust = codegen_rust::emit(&program, &options);
        evidence.record(
            &format!("profile{index}-COMPLETE-RUST-EMISSION-ORIGINAL.txt"),
            &rust,
        )?;
        let csharp = codegen_csharp::emit(&program);
        evidence.record(
            &format!("profile{index}-COMPLETE-CSHARP-EMISSION-ORIGINAL.txt"),
            &csharp,
        )?;
        // Both original outcomes are captured before inspecting either status.
        let mut okay = validation.is_empty();
        match rust {
            Ok(artifacts) => {
                evidence.artifacts(
                    &format!("profile{index}-RUST"),
                    &format!("rust-profile{index}"),
                    &artifacts,
                )?;
                let source = artifacts
                    .files()
                    .iter()
                    .find(|file| file.path.as_str() == "src/lib.rs")
                    .ok_or("Rust emitter omitted lib.rs")?;
                evidence.write(
                    &format!("rust-aggregate/src/case_{index}.rs"),
                    &source.contents,
                )?;
                rust_modules.push_str(&format!("pub mod case_{index};\n"));
                rust_dispatch.push_str(&format!("        {index}=>route!(case_{index},control,source,inputs,execution,loader,checker,limits),\n"));
            }
            Err(error) => {
                okay = false;
                evidence.record(&format!("profile{index}-RUST-FAILURE-ORIGINAL.txt"), &error)?;
            }
        }
        match csharp {
            Ok(artifacts) => {
                evidence.artifacts(
                    &format!("profile{index}-CSHARP"),
                    &format!("csharp-profile{index}"),
                    &artifacts,
                )?;
                evidence.write(
                    &format!("csharp-profile{index}/Harness/Host.csproj"),
                    HARNESS_PROJECT.as_bytes(),
                )?;
                evidence.write(
                    &format!("csharp-profile{index}/Harness/Program.cs"),
                    include_str!("selected_targets/Host.cs.txt").as_bytes(),
                )?;
                evidence.write(
                    &format!("csharp-profile{index}/Harness/Capture.cs"),
                    include_str!("selected_targets/Capture.cs.txt").as_bytes(),
                )?;
                evidence.write(
                    &format!("csharp-profile{index}/NuGet.Config"),
                    b"<configuration><packageSources><clear /></packageSources></configuration>\n",
                )?;
            }
            Err(error) => {
                okay = false;
                evidence.record(
                    &format!("profile{index}-CSHARP-FAILURE-ORIGINAL.txt"),
                    &error,
                )?;
            }
        }
        outcomes.push((index, okay, String::new()));
    }
    rust_dispatch.push_str("        _=>panic!(\"unknown frozen profile ordinal\"),\n    }\n}\n");
    let host = include_str!("selected_targets/Host.rs.txt")
        .replace(
            "// CASE_MODULES is replaced by the preparer with four ordinary emitted modules.",
            &rust_modules,
        )
        .replace(
            "// CASE_DISPATCH is replaced with a fixed four-arm invocation of this macro.",
            &rust_dispatch,
        );
    evidence.write("rust-aggregate/src/main.rs", host.as_bytes())?;
    // Keep the existing compatible runtime and exact locked serde_json version;
    // the root chooses shared target location and records actual lock resolution.
    evidence.write("rust-aggregate/Cargo.toml",format!("[package]\nname = \"selected-target198-public-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\ncodegen-runtime = {{ path = {} }}\nserde_json = \"=1.0.150\"\n\n[workspace]\n",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;
    evidence.write("controls.json", CONTROLS.as_bytes())?;
    evidence.record("ALL4-COMPLETE-PREPARATION-OUTCOMES-ORIGINAL.txt", &outcomes)?;
    // Actual native controls have their own complete literal expectations and
    // outcomes. None is copied into the generated-language expectations.
    let native = native::run(&projects, &corpus, &evidence)?;
    evidence.record(
        "COMPLETE-PREPARATION-AND-NATIVE-RESULT-ORIGINAL.txt",
        &(&outcomes, &native),
    )?;
    if outcomes.len() != 4
        || outcomes.iter().any(|(_, okay, _)| !*okay)
        || native.len() != 30
        || native.iter().any(|(_, matched)| !*matched)
    {
        return Err(
            "complete originals retained; preparation or independent native controls differ".into(),
        );
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
