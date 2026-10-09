use super::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "filter_map_csharp/projects.rs"]
mod projects;

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_FILTER_MAP193_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        fs::create_dir_all(&parent)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "ferrule-filter-map193-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        eprintln!("FILTER_MAP193_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn record(&self, label: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
        fs::write(
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
        fs::write(self.0.join(format!("{label}-stdout.bin")), &output.stdout)?;
        fs::write(self.0.join(format!("{label}-stderr.bin")), &output.stderr)?;
        Ok(output)
    }
}

fn fixtures() -> Vec<(String, Project, serde_json::Value)> {
    let corpus = projects::corpus();
    let mut fixtures = Vec::new();
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| projects::admitted(case))
    {
        fixtures.push((
            case["id"].as_str().unwrap().into(),
            projects::project(case),
            projects::profile(case, false),
        ));
    }
    for case in corpus["legacy_controls"].as_array().unwrap() {
        fixtures.push((
            case["id"].as_str().unwrap().into(),
            projects::legacy_project(case),
            projects::legacy_profile(case),
        ));
    }
    let identity = &corpus["cases"][1];
    for (label, items, work, diagnostic) in [
        ("named-complete-shared-success", 6, 34, None),
        (
            "named-shared-item-refusal",
            5,
            100,
            Some(serde_json::json!({
            "category":"ItemLimit", "output_owner":13, "phase":"source", "capture_index":null,
            "source_position":null,"function":null,"node":null,"original_cause":null,"limit":5,"used":3,"requested":3})),
        ),
        (
            "named-shared-work-refusal",
            6,
            19,
            Some(serde_json::json!({
            "category":"WorkLimit", "output_owner":13, "phase":"source", "capture_index":null,
            "source_position":null,"function":null,"node":null,"original_cause":null,"limit":19,"used":19,"requested":3})),
        ),
    ] {
        let mut profile = projects::profile(identity, true);
        profile["id"] = serde_json::Value::String(label.into());
        profile["limits"] = serde_json::json!({"source_items":items,"work":work});
        if let Some(diagnostic) = diagnostic {
            profile["expected"] = serde_json::json!({"status":"error","diagnostic":diagnostic});
        }
        fixtures.push((label.into(), projects::named_project(identity), profile));
    }
    for kind in ["exists", "item-at", "sum"] {
        let (project, profile) = projects::reducer(kind, identity);
        fixtures.push((format!("reducer-{kind}"), project, profile));
    }
    for (project, profile) in [
        projects::output_adapter(identity),
        projects::call_depth64(identity),
    ] {
        fixtures.push((profile["id"].as_str().unwrap().into(), project, profile));
    }
    for lower in [true, false] {
        let (project, profile) = projects::source_coercion(identity, lower);
        fixtures.push((profile["id"].as_str().unwrap().into(), project, profile));
    }
    fixtures
}

fn retain_bound_sources(
    root: &Evidence,
    label: &str,
    directory: &Path,
) -> TestResult<ArtifactFiles> {
    let files = artifact_files(directory)?;
    let original = root
        .0
        .join(format!("{label}-complete-bound-source-originals"));
    fs::create_dir(&original)?;
    let mut index = Vec::new();
    for (relative, bytes) in &files {
        let file = original.join(relative);
        fs::create_dir_all(file.parent().ok_or("source parent")?)?;
        fs::write(file, bytes)?;
        index.push((
            relative,
            bytes.len(),
            format!("{:x}", Sha256::digest(bytes)),
        ));
    }
    root.record(&format!("{label}-COMPLETE-BOUND-SOURCE-INDEX"), &index)?;
    Ok(files)
}

#[test]
#[ignore = "root-coordinated finite C# compiled cohort requires verified model/native/lowering merges and fixed SDK ownership"]
fn filter_map_csharp_complete_public_and_direct_runtime_oracles() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let fixtures = fixtures();
    evidence.record("COMPLETE43-PUBLIC-FIXTURE-PROFILES", &fixtures)?;
    let mut outcomes = Vec::new();
    let mut runtime_control = None;
    for (label, project, profile) in fixtures {
        let directory = evidence.0.join(&label);
        let file = evidence.0.join(format!("{label}.project.json"));
        let project_wire = mapping::project_file::encode_pretty(&project)?;
        fs::write(&file, &project_wire)?;
        let actual_generation = generate_project(&file, &directory, GenerateTarget::CSharp);
        evidence.record(
            &format!("{label}-COMPLETE-PUBLIC-GENERATION"),
            &actual_generation,
        )?;
        // Preserve each generation failure and continue the complete bounded cohort.
        let generated = match actual_generation {
            Ok(generated) => generated,
            Err(error) => {
                if directory.is_dir() {
                    retain_bound_sources(
                        &evidence,
                        &format!("{label}-FAILED-GENERATION"),
                        &directory,
                    )?;
                }
                evidence.record(
                    &format!("{label}-FAILED-GENERATION-PROJECT-AFTER"),
                    &fs::read(&file),
                )?;
                outcomes.push((label, Err::<bool, _>(error.to_string()), false));
                continue;
            }
        };
        let generated_files = artifact_files(&directory)?;
        let census = generated.output_directory == directory
            && generated.files_written == generated_files.len();
        evidence.record(
            &format!("{label}-COMPLETE-ORDINARY-ARTIFACT-CENSUS"),
            &(generated, generated_files.len(), census),
        )?;
        let harness = directory.join("Harness");
        fs::create_dir(&harness)?;
        fs::write(harness.join("Host.csproj"), HARNESS_PROJECT)?;
        for (name, source) in [
            ("Program.cs", include_str!("filter_map_csharp/Host.cs.txt")),
            (
                "Capture.cs",
                include_str!("filter_map_csharp/Capture.cs.txt"),
            ),
            (
                "RuntimeControls.cs",
                include_str!("filter_map_csharp/RuntimeControls.cs.txt"),
            ),
        ] {
            fs::write(harness.join(name), source)?;
        }
        fs::write(
            directory.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>\n",
        )?;
        let profile_file = directory.join("profile.json");
        fs::write(&profile_file, serde_json::to_vec_pretty(&profile)?)?;
        let runtime_profile = directory.join("runtime-profile.json");
        fs::write(&runtime_profile, b"{\"mode\":\"runtime-controls\"}\n")?;
        let bound = retain_bound_sources(&evidence, &label, &directory)?;
        let original = (|| -> TestResult<bool> {
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
            if !evidence
                .command(&format!("{label}-BUILD"), &mut build)?
                .status
                .success()
            {
                return Ok(false);
            }
            let assembly = harness.join("bin/Debug/net10.0/Host.dll");
            if !assembly.is_file() {
                return Err("successful build omitted host assembly".into());
            }
            let mut run = dotnet_command(&directory);
            run.arg(&assembly)
                .arg(&profile_file)
                .arg(directory.join("public-originals"))
                .current_dir(&directory);
            let public = evidence
                .command(&format!("{label}-PUBLIC-HOST"), &mut run)?
                .status
                .success();
            if label == "identity-all" {
                let mut direct = dotnet_command(&directory);
                direct
                    .arg(&assembly)
                    .arg(&runtime_profile)
                    .arg(directory.join("direct-runtime-originals"))
                    .current_dir(&directory);
                let result = evidence
                    .command("DIRECT-RUNTIME-HOST", &mut direct)
                    .map(|output| output.status.success());
                evidence.record("COMPLETE-DIRECT-RUNTIME-PROCESS-RESULT", &result)?;
                runtime_control = Some(result.map_err(|error| error.to_string()));
            }
            Ok(public)
        })();
        evidence.record(&format!("{label}-COMPLETE-BUILD-HOST-RESULT"), &original)?;
        let after = bound
            .iter()
            .map(|(path, _)| fs::read(directory.join(path)))
            .collect::<Vec<_>>();
        // Whole after bytes are retained even if a candidate changed a source input.
        let guard = evidence
            .0
            .join(format!("{label}-complete-source-after-originals"));
        fs::create_dir(&guard)?;
        for ((relative, _), bytes) in bound.iter().zip(&after) {
            if let Ok(bytes) = bytes {
                let target = guard.join(relative);
                fs::create_dir_all(target.parent().ok_or("guard parent")?)?;
                fs::write(target, bytes)?;
            }
        }
        evidence.record(
            &format!("{label}-COMPLETE-SOURCE-AFTER-READ-RESULTS"),
            &after
                .iter()
                .map(|result| {
                    result
                        .as_ref()
                        .map(|bytes| (bytes.len(), format!("{:x}", Sha256::digest(bytes))))
                })
                .collect::<Vec<_>>(),
        )?;
        let project_after = fs::read(&file);
        evidence.record(
            &format!("{label}-COMPLETE-PROJECT-AFTER-READ-RESULT"),
            &project_after,
        )?;
        if let Ok(bytes) = &project_after {
            fs::write(evidence.0.join(format!("{label}-PROJECT-AFTER.bin")), bytes)?;
        }
        let stable = census
            && matches!(&project_after, Ok(bytes) if bytes == project_wire.as_bytes())
            && after
                .iter()
                .zip(&bound)
                .all(|(actual, (_, expected))| matches!(actual, Ok(actual) if actual == expected));
        outcomes.push((label, original.map_err(|error| error.to_string()), stable));
    }
    evidence.record("ALL43-COMPLETE-PUBLIC-COHORT-RESULTS", &outcomes)?;
    evidence.record("COMPLETE-DIRECT-RUNTIME-CONTROL-RESULT", &runtime_control)?;
    if outcomes.len() != 43
        || outcomes
            .iter()
            .any(|(_, result, stable)| !stable || !matches!(result, Ok(true)))
        || !matches!(runtime_control, Some(Ok(true)))
    {
        return Err("complete C# cohort differs; all originals retained".into());
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
