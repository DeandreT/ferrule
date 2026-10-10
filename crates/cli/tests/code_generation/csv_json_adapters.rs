//! Complete authored CSV input and selected JSON document boundary contracts.
use super::*;
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "csv_json_adapters/native_contract.rs"]
mod native_contract;
const HOST: &str = include_str!("csv_json_adapters/Host.cs.txt");
const FIXTURES: &[(&str, &[u8])] = &[
    (
        "cases.json",
        include_bytes!("csv_json_adapters/fixtures/cases.json"),
    ),
    (
        "expected/parsed-rows.json",
        include_bytes!("csv_json_adapters/fixtures/expected/parsed-rows.json"),
    ),
    (
        "expected/wire/missing-Archive.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/missing-Archive.json"),
    ),
    (
        "expected/wire/missing-ExplicitNull.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/missing-ExplicitNull.json"),
    ),
    (
        "expected/wire/missing-NullObject.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/missing-NullObject.json"),
    ),
    (
        "expected/wire/missing-Primary.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/missing-Primary.json"),
    ),
    (
        "expected/wire/multi-Archive.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/multi-Archive.json"),
    ),
    (
        "expected/wire/multi-ExplicitNull.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/multi-ExplicitNull.json"),
    ),
    (
        "expected/wire/multi-FirstScalar.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/multi-FirstScalar.json"),
    ),
    (
        "expected/wire/multi-HostValue.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/multi-HostValue.json"),
    ),
    (
        "expected/wire/multi-NullObject.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/multi-NullObject.json"),
    ),
    (
        "expected/wire/multi-Primary.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/multi-Primary.json"),
    ),
    (
        "expected/wire/multi-RequiredFirst.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/multi-RequiredFirst.json"),
    ),
    (
        "expected/wire/one-Archive.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/one-Archive.json"),
    ),
    (
        "expected/wire/one-ExplicitNull.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/one-ExplicitNull.json"),
    ),
    (
        "expected/wire/one-FirstScalar.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/one-FirstScalar.json"),
    ),
    (
        "expected/wire/one-NullObject.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/one-NullObject.json"),
    ),
    (
        "expected/wire/one-Primary.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/one-Primary.json"),
    ),
    (
        "expected/wire/one-RequiredFirst.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/one-RequiredFirst.json"),
    ),
    (
        "expected/wire/short-Archive.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/short-Archive.json"),
    ),
    (
        "expected/wire/short-ExplicitNull.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/short-ExplicitNull.json"),
    ),
    (
        "expected/wire/short-FirstScalar.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/short-FirstScalar.json"),
    ),
    (
        "expected/wire/short-NullObject.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/short-NullObject.json"),
    ),
    (
        "expected/wire/short-Primary.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/short-Primary.json"),
    ),
    (
        "expected/wire/short-RequiredFirst.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/short-RequiredFirst.json"),
    ),
    (
        "expected/wire/unicode-Archive.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/unicode-Archive.json"),
    ),
    (
        "expected/wire/unicode-ExplicitNull.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/unicode-ExplicitNull.json"),
    ),
    (
        "expected/wire/unicode-FirstScalar.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/unicode-FirstScalar.json"),
    ),
    (
        "expected/wire/unicode-NullObject.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/unicode-NullObject.json"),
    ),
    (
        "expected/wire/unicode-Primary.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/unicode-Primary.json"),
    ),
    (
        "expected/wire/unicode-RequiredFirst.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/unicode-RequiredFirst.json"),
    ),
    (
        "expected/wire/zero-Archive.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/zero-Archive.json"),
    ),
    (
        "expected/wire/zero-ExplicitNull.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/zero-ExplicitNull.json"),
    ),
    (
        "expected/wire/zero-NullObject.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/zero-NullObject.json"),
    ),
    (
        "expected/wire/zero-Primary.json",
        include_bytes!("csv_json_adapters/fixtures/expected/wire/zero-Primary.json"),
    ),
    (
        "facade-cases.json",
        include_bytes!("csv_json_adapters/fixtures/facade-cases.json"),
    ),
    (
        "inputs/later_bad.csv",
        include_bytes!("csv_json_adapters/fixtures/inputs/later_bad.csv"),
    ),
    (
        "inputs/missing.csv",
        include_bytes!("csv_json_adapters/fixtures/inputs/missing.csv"),
    ),
    (
        "inputs/multi.csv",
        include_bytes!("csv_json_adapters/fixtures/inputs/multi.csv"),
    ),
    (
        "inputs/one.csv",
        include_bytes!("csv_json_adapters/fixtures/inputs/one.csv"),
    ),
    (
        "inputs/short.csv",
        include_bytes!("csv_json_adapters/fixtures/inputs/short.csv"),
    ),
    (
        "inputs/unicode.csv",
        include_bytes!("csv_json_adapters/fixtures/inputs/unicode.csv"),
    ),
    (
        "inputs/zero.csv",
        include_bytes!("csv_json_adapters/fixtures/inputs/zero.csv"),
    ),
    (
        "policy-cases.json",
        include_bytes!("csv_json_adapters/fixtures/policy-cases.json"),
    ),
    (
        "policy/invalid_unselected_schema.json",
        include_bytes!("csv_json_adapters/fixtures/policy/invalid_unselected_schema.json"),
    ),
    (
        "policy/named_source.json",
        include_bytes!("csv_json_adapters/fixtures/policy/named_source.json"),
    ),
    (
        "policy/non_json_named_target.json",
        include_bytes!("csv_json_adapters/fixtures/policy/non_json_named_target.json"),
    ),
    (
        "policy/root_iteration.json",
        include_bytes!("csv_json_adapters/fixtures/policy/root_iteration.json"),
    ),
    (
        "policy/top_level_array.json",
        include_bytes!("csv_json_adapters/fixtures/policy/top_level_array.json"),
    ),
    (
        "projects/base.json",
        include_bytes!("csv_json_adapters/fixtures/projects/base.json"),
    ),
    (
        "projects/failure.json",
        include_bytes!("csv_json_adapters/fixtures/projects/failure.json"),
    ),
];

struct Evidence(PathBuf);
impl Evidence {
    fn new(kind: &str) -> TestResult<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-json-314-{kind}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        eprintln!("CSV_JSON_314_ORIGINALS={}", path.display());
        for (name, body) in FIXTURES {
            let target = path.join("fixtures").join(name);
            std::fs::create_dir_all(target.parent().ok_or("fixture parent")?)?;
            std::fs::write(target, body)?;
        }
        Ok(Self(path))
    }
    fn record(&self, name: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        std::fs::write(
            self.0.join(format!("{name}-ORIGINAL.txt")),
            format!("{value:#?}\n"),
        )?;
        Ok(())
    }
    fn command(&self, name: &str, command: &mut Command) -> TestResult<Output> {
        self.record(&format!("{name}-COMMAND"), command)?;
        let actual = command.isolated_output();
        self.record(&format!("{name}-COMMAND-RESULT"), &actual)?;
        let actual = actual?;
        std::fs::write(self.0.join(format!("{name}-stdout.bin")), &actual.stdout)?;
        std::fs::write(self.0.join(format!("{name}-stderr.bin")), &actual.stderr)?;
        Ok(actual)
    }
    fn fixture_hold(&self) -> TestResult<bool> {
        let actual = FIXTURES
            .iter()
            .map(|(name, _)| std::fs::read(self.0.join("fixtures").join(name)))
            .collect::<Vec<_>>();
        self.record("ALL-COMPLETE-FIXTURE-READBACK", &actual)?;
        Ok(actual.iter().zip(FIXTURES).all(|(actual, (_, expected))| matches!(actual, Ok(actual) if actual.as_slice() == *expected)))
    }
}

#[test]
fn csv_json_adapters_native_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new("native")?;
    let actual = native_contract::run(
        &evidence.0.join("fixtures"),
        &evidence.0.join("native-evidence"),
    );
    evidence.record("COMPLETE-NATIVE-RESULT", &actual)?;
    let fixture_hold = evidence.fixture_hold()?;
    let originals = std::fs::read(evidence.0.join("native-evidence/FINAL.json"));
    evidence.record("COMPLETE-NATIVE-OUTCOMES", &originals)?;
    let originals: Json = serde_json::from_slice(&originals?)?;
    let rows = originals.as_array().ok_or("complete native outcomes")?;
    if actual.is_err()
        || !fixture_hold
        || rows.len() != 44
        || rows.iter().any(|row| row["passed"] != true)
    {
        return Err("complete authored CSV/JSON native outcome differs; originals retained".into());
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq)]
struct Counters {
    physical: usize,
    parse: usize,
    typed: usize,
    serialize: usize,
}
fn expected_comparisons(
    cases: &Json,
    facades: &Json,
    family: &str,
) -> TestResult<(BTreeSet<String>, Counters)> {
    let mut paths = BTreeSet::new();
    let mut counts = Counters::default();
    for case in cases
        .as_array()
        .ok_or("authored cases")?
        .iter()
        .filter(|row| row["family"] == family)
    {
        let id = case["id"].as_str().ok_or("case id")?;
        let stage = case["expected_error"]["stage"].as_str();
        for route in case["routes"].as_array().ok_or("routes")? {
            let route = route.as_str().ok_or("route name")?;
            let independent = format!("{id}-{route}-independent");
            paths.insert(format!("{independent}/COMPARISON.json"));
            counts.parse += 1;
            if stage != Some("parse") {
                paths.insert(format!("{independent}/typed/COMPARISON.json"));
                counts.typed += 1;
                if stage != Some("mapping") {
                    for serialization in ["text", "bytes"] {
                        paths.insert(format!(
                            "{independent}/typed/serialize-{serialization}/COMPARISON.json"
                        ));
                        counts.serialize += 1;
                    }
                }
            }
            paths.insert(format!("{id}-{route}-WithHost/COMPARISON.json"));
            counts.physical += 1;
            if case["context"].as_object().ok_or("context")?.is_empty() {
                paths.insert(format!("{id}-{route}-simple/COMPARISON.json"));
                counts.physical += 1;
            }
        }
    }
    if family == "base" {
        for case in facades.as_array().ok_or("facade controls")? {
            paths.insert(format!(
                "{}-facade/COMPARISON.json",
                case["id"].as_str().ok_or("facade id")?
            ));
            counts.physical += 1;
        }
    }
    Ok((paths, counts))
}

fn compiled_family(
    evidence: &Evidence,
    family: &str,
    cases: &Json,
    facades: &Json,
) -> TestResult<bool> {
    let directory = evidence.0.join(family);
    let fixture = evidence.0.join("fixtures");
    let generation = cli::generate_project_with_csv_json_adapters(
        &fixture.join(format!("projects/{family}.json")),
        &directory,
        GenerateTarget::CSharp,
    );
    evidence.record(&format!("{family}-COMPLETE-GENERATION"), &generation)?;
    generation?;
    let emitted = artifact_files(&directory)?;
    evidence.record(&format!("{family}-COMPLETE-EMITTED-ARTIFACTS"), &emitted)?;
    let mut expected = expected_csharp_artifact_paths(false)
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    expected.extend([
        "GeneratedMapping.CsvJson.cs".to_owned(),
        "Runtime/FerruleCsvInput.cs".to_owned(),
    ]);
    let actual = emitted
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    evidence.record(
        &format!("{family}-COMPLETE-PUBLISH-MEMBERSHIP"),
        &(&actual, &expected),
    )?;
    if actual != expected || actual.len() != 79 {
        return Err("complete CSV/JSON publication differs".into());
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
    evidence.record(&format!("{family}-COMPLETE-RUNTIME-BODIES"), &bodies)?;
    if bodies.len() != 75
        || bodies
            .iter()
            .any(|(_, source, body)| !matches!(source,Ok(source) if source==body))
    {
        return Err("complete CSV/JSON runtime bodies differ".into());
    }
    std::fs::create_dir(directory.join("Harness"))?;
    std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
    std::fs::write(directory.join("Harness/Program.cs"), HOST)?;
    std::fs::write(
        directory.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )?;
    let bound = artifact_files(&directory)?;
    evidence.record(&format!("{family}-COMPLETE-BOUND-SOURCES"), &bound)?;
    let host_outcome = (|| -> TestResult<bool> {
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
        let host_evidence = directory.join("host-evidence");
        let mut host = dotnet_command(&directory);
        host.arg(directory.join("Harness/bin/Debug/net10.0/Host.dll"))
            .arg(&fixture)
            .arg(family)
            .arg(&host_evidence)
            .current_dir(&directory);
        if !evidence
            .command(&format!("{family}-HOST"), &mut host)?
            .status
            .success()
        {
            return Ok(false);
        }
        let final_bytes = std::fs::read(host_evidence.join("FINAL.json"))?;
        evidence.record(&format!("{family}-COMPLETE-HOST-FINAL"), &final_bytes)?;
        let summary: Json = serde_json::from_slice(&final_bytes)?;
        let (expected_paths, counts) = expected_comparisons(cases, facades, family)?;
        let comparisons = artifact_files(&host_evidence)?
            .into_iter()
            .filter(|(name, _)| name.ends_with("/COMPARISON.json"))
            .collect::<Vec<_>>();
        evidence.record(&format!("{family}-COMPLETE-COMPARISONS"), &comparisons)?;
        let actual_paths = comparisons
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>();
        evidence.record(
            &format!("{family}-COMPLETE-COMPARISON-MEMBERSHIP"),
            &(&actual_paths, &expected_paths, &counts),
        )?;
        let values = comparisons
            .iter()
            .map(|(_, body)| serde_json::from_slice::<Json>(body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(actual_paths == expected_paths
            && summary["failures"] == 0
            && summary["physical_calls"].as_u64() == Some(counts.physical as u64)
            && summary["parse_calls"].as_u64() == Some(counts.parse as u64)
            && summary["typed_calls"].as_u64() == Some(counts.typed as u64)
            && summary["serialization_calls"].as_u64() == Some(counts.serialize as u64)
            && values.iter().all(|row| {
                row["equal"] == true && row["unchanged"] == true && row["noPartial"] == true
            }))
    })();
    evidence.record(&format!("{family}-COMPLETE-HOST-OUTCOME"), &host_outcome)?;
    let readback = bound
        .iter()
        .map(|(name, _)| std::fs::read(directory.join(name)))
        .collect::<Vec<_>>();
    evidence.record(&format!("{family}-COMPLETE-BOUND-READBACK"), &readback)?;
    let stable = readback
        .iter()
        .zip(&bound)
        .all(|(actual, (_, wanted))| matches!(actual,Ok(actual) if actual==wanted));
    Ok(stable && host_outcome?)
}

#[test]
fn csv_json_adapters_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new("compiled")?;
    let cases: Json =
        serde_json::from_slice(&std::fs::read(evidence.0.join("fixtures/cases.json"))?)?;
    let facades: Json = serde_json::from_slice(&std::fs::read(
        evidence.0.join("fixtures/facade-cases.json"),
    )?)?;
    let mut outcomes = Vec::new();
    for family in ["base", "failure"] {
        let actual = compiled_family(&evidence, family, &cases, &facades);
        evidence.record(&format!("{family}-COMPLETE-FAMILY-OUTCOME"), &actual)?;
        outcomes.push((family, actual));
    }
    evidence.record("ALL-COMPLETE-COMPILED-OUTCOMES", &outcomes)?;
    let hold = evidence.fixture_hold()?;
    if !hold
        || outcomes
            .iter()
            .any(|(_, actual)| !matches!(actual, Ok(true)))
    {
        return Err("complete CSV/JSON compiled outcomes differ; originals retained".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
