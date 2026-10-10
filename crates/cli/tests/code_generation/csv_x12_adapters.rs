//! Complete authored compiled CSV input and singular X12 output contracts.
use super::*;
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const HOST: &str = include_str!("csv_x12_adapters/Host.cs.txt");
const FIXTURES: &[(&str, &[u8])] = &[
    (
        "all-row/cases.json",
        include_bytes!("csv_x12_adapters/all-row/cases.json"),
    ),
    (
        "all-row/context.json",
        include_bytes!("csv_x12_adapters/all-row/context.json"),
    ),
    (
        "all-row/multiple-expected.x12",
        include_bytes!("csv_x12_adapters/all-row/multiple-expected.x12"),
    ),
    (
        "all-row/multiple-source-tags.json",
        include_bytes!("csv_x12_adapters/all-row/multiple-source-tags.json"),
    ),
    (
        "all-row/multiple-typed-target-tags.json",
        include_bytes!("csv_x12_adapters/all-row/multiple-typed-target-tags.json"),
    ),
    (
        "all-row/multiple.csv",
        include_bytes!("csv_x12_adapters/all-row/multiple.csv"),
    ),
    (
        "all-row/one-expected.x12",
        include_bytes!("csv_x12_adapters/all-row/one-expected.x12"),
    ),
    (
        "all-row/one-source-tags.json",
        include_bytes!("csv_x12_adapters/all-row/one-source-tags.json"),
    ),
    (
        "all-row/one-typed-target-tags.json",
        include_bytes!("csv_x12_adapters/all-row/one-typed-target-tags.json"),
    ),
    (
        "all-row/one.csv",
        include_bytes!("csv_x12_adapters/all-row/one.csv"),
    ),
    (
        "all-row/project.json",
        include_bytes!("csv_x12_adapters/all-row/project.json"),
    ),
    (
        "all-row/zero-expected.x12",
        include_bytes!("csv_x12_adapters/all-row/zero-expected.x12"),
    ),
    (
        "all-row/zero-source-tags.json",
        include_bytes!("csv_x12_adapters/all-row/zero-source-tags.json"),
    ),
    (
        "all-row/zero-typed-target-tags.json",
        include_bytes!("csv_x12_adapters/all-row/zero-typed-target-tags.json"),
    ),
    (
        "all-row/zero.csv",
        include_bytes!("csv_x12_adapters/all-row/zero.csv"),
    ),
    (
        "dialect-expected.x12",
        include_bytes!("csv_x12_adapters/dialect-expected.x12"),
    ),
    (
        "dialect-source-tags.json",
        include_bytes!("csv_x12_adapters/dialect-source-tags.json"),
    ),
    (
        "dialect-typed-target-tags.json",
        include_bytes!("csv_x12_adapters/dialect-typed-target-tags.json"),
    ),
    (
        "direct-cases.json",
        include_bytes!("csv_x12_adapters/direct-cases.json"),
    ),
    (
        "generated-cases.json",
        include_bytes!("csv_x12_adapters/generated-cases.json"),
    ),
    (
        "inputs/dialect.csv",
        include_bytes!("csv_x12_adapters/inputs/dialect.csv"),
    ),
    (
        "inputs/invalid-second-row.csv",
        include_bytes!("csv_x12_adapters/inputs/invalid-second-row.csv"),
    ),
    (
        "inputs/reserved-target.csv",
        include_bytes!("csv_x12_adapters/inputs/reserved-target.csv"),
    ),
    (
        "projects/base.json",
        include_bytes!("csv_x12_adapters/projects/base.json"),
    ),
    (
        "projects/dialect.json",
        include_bytes!("csv_x12_adapters/projects/dialect.json"),
    ),
    (
        "projects/failure.json",
        include_bytes!("csv_x12_adapters/projects/failure.json"),
    ),
];

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-x12-275-compiled-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        eprintln!("CSV_X12_275_ORIGINALS={}", path.display());
        for (name, body) in FIXTURES {
            let file = path.join("fixtures").join(name);
            std::fs::create_dir_all(file.parent().ok_or("fixture parent")?)?;
            std::fs::write(file, body)?;
        }
        Ok(Self(path))
    }
    fn record(&self, label: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        std::fs::write(
            self.0.join(format!("{label}-ORIGINAL.txt")),
            format!("{value:#?}\n"),
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

const X12_PATHS: [&str; 8] = [
    "Runtime/X12/FerruleX12.cs",
    "Runtime/X12/FerruleX12.Schema.cs",
    "Runtime/X12/FerruleX12.Reader.cs",
    "Runtime/X12/FerruleX12.Numeric.cs",
    "Runtime/X12/FerruleX12.Writer.cs",
    "Runtime/X12/FerruleX12.Completion.cs",
    "Runtime/X12/FerruleX12.Lexical.cs",
    "Runtime/X12/FerruleX12Exception.cs",
];

#[test]
fn csv_x12_adapters_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let fixture_root = evidence.0.join("fixtures");
    let mut results = Vec::new();
    let generated: Json =
        serde_json::from_slice(&std::fs::read(fixture_root.join("generated-cases.json"))?)?;
    let direct: Json =
        serde_json::from_slice(&std::fs::read(fixture_root.join("direct-cases.json"))?)?;
    for family in ["base", "failure", "dialect"] {
        let directory = evidence.0.join(family);
        let input = fixture_root.join("projects").join(format!("{family}.json"));
        let outcome =
            cli::generate_project_with_csv_x12_adapters(&input, &directory, GenerateTarget::CSharp);
        evidence.record(&format!("{family}-COMPLETE-GENERATION"), &outcome)?;
        outcome?;
        let emitted = artifact_files(&directory)?;
        evidence.record(&format!("{family}-COMPLETE-EMITTED-ARTIFACTS"), &emitted)?;
        let mut expected = expected_csharp_artifact_paths(false)
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        expected.extend(X12_PATHS.into_iter().map(str::to_owned));
        expected.insert("GeneratedMapping.CsvX12.cs".into());
        expected.insert("Runtime/FerruleCsvInput.cs".into());
        let actual = emitted
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<BTreeSet<_>>();
        evidence.record(
            &format!("{family}-COMPLETE-PATH-MEMBERSHIP"),
            &(&actual, &expected),
        )?;
        if actual != expected || actual.len() != 87 {
            return Err("complete CSV/X12 publish set differs".into());
        }
        for (name, body) in &emitted {
            if name.starts_with("GeneratedMapping") {
                let text = std::str::from_utf8(body)?;
                if ["ExecuteJson", "SourceJsonSchema", "ParseNamedJsonInputs"]
                    .iter()
                    .any(|method| text.contains(method))
                {
                    return Err("CSV profile exposes a JSON source boundary".into());
                }
            }
        }
        let runtime =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/csharp/Ferrule.Runtime");
        let bodies = emitted
            .iter()
            .filter(|(name, _)| name.starts_with("Runtime/"))
            .map(|(name, body)| {
                (
                    name.clone(),
                    std::fs::read(runtime.join(name.strip_prefix("Runtime/").unwrap())),
                    body.clone(),
                )
            })
            .collect::<Vec<_>>();
        evidence.record(&format!("{family}-COMPLETE-RUNTIME-BODY-READBACK"), &bodies)?;
        if bodies.len() != 83
            || bodies
                .iter()
                .any(|(_, original, body)| !matches!(original, Ok(original) if original == body))
        {
            return Err("complete CSV/X12 runtime body differs".into());
        }
        std::fs::create_dir(directory.join("Harness"))?;
        std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
        std::fs::write(directory.join("Harness/Program.cs"), HOST)?;
        std::fs::write(
            directory.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>\n",
        )?;
        let bound = artifact_files(&directory)?;
        evidence.record(&format!("{family}-COMPLETE-BOUND-ARTIFACTS"), &bound)?;
        let expected_calls = generated
            .as_array()
            .ok_or("generated rows")?
            .iter()
            .filter(|row| row["family"] == family)
            .count()
            + if family == "base" {
                direct
                    .as_array()
                    .ok_or("direct rows")?
                    .iter()
                    .map(|row| {
                        row["routes"]
                            .as_array()
                            .map(Vec::len)
                            .ok_or("direct routes")
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .sum::<usize>()
            } else {
                0
            };
        let host_result = (|| -> TestResult<bool> {
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
                .env("DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER", "1")
                .env("MSBUILDDISABLENODEREUSE", "1");
            if !evidence
                .command(&format!("{family}-BUILD"), &mut build)?
                .status
                .success()
            {
                return Ok(false);
            }
            let mut host = dotnet_command(&directory);
            host.arg(directory.join("Harness/bin/Debug/net10.0/Host.dll"))
                .arg(&fixture_root)
                .arg(family)
                .arg(directory.join("host-evidence"))
                .current_dir(&directory);
            if !evidence
                .command(&format!("{family}-HOST"), &mut host)?
                .status
                .success()
            {
                return Ok(false);
            }
            let summary: Json = serde_json::from_slice(&std::fs::read(
                directory.join("host-evidence/summary.json"),
            )?)?;
            let calls: Json = serde_json::from_slice(&std::fs::read(
                directory.join("host-evidence/results.json"),
            )?)?;
            evidence.record(
                &format!("{family}-COMPLETE-HOST-SUMMARY"),
                &(&summary, &calls),
            )?;
            Ok(summary["calls"].as_u64() == Some(expected_calls as u64)
                && summary["failures"].as_u64() == Some(0)
                && calls
                    .as_array()
                    .is_some_and(|calls| calls.len() == expected_calls))
        })();
        evidence.record(&format!("{family}-COMPLETE-HOST-RESULT"), &host_result)?;
        let readback = bound
            .iter()
            .map(|(name, _)| std::fs::read(directory.join(name)))
            .collect::<Vec<_>>();
        evidence.record(&format!("{family}-COMPLETE-BOUND-READBACK"), &readback)?;
        let stable = readback
            .iter()
            .zip(&bound)
            .all(|(actual, (_, expected))| matches!(actual, Ok(actual) if actual == expected));
        results.push((family, host_result, stable));
    }
    let readback = FIXTURES
        .iter()
        .map(|(name, _)| std::fs::read(fixture_root.join(name)))
        .collect::<Vec<_>>();
    evidence.record("ALL-COMPLETE-FIXTURE-READBACK", &readback)?;
    evidence.record("ALL-COMPLETE-COMPILED-RESULTS", &results)?;
    if results.iter().any(|(_, outcome, stable)| !*stable || !matches!(outcome, Ok(true)))
        || !readback.iter().zip(FIXTURES).all(|(actual, (_, expected))| matches!(actual, Ok(actual) if actual.as_slice() == *expected)) {
        return Err("complete CSV/X12 compiled outcomes or source identity differ".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
