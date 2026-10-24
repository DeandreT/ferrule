//! Complete strict JSON filename plans share the portable document-host harness.
use super::primary_documents_harness::{self as harness, Cohort};
use super::primary_xml_documents::projects as xml_projects;
use super::*;

#[path = "../../../../conformance/primary-json-documents/projects.rs"]
mod projects;

fn emit(
    program: &codegen::DocumentProgram,
    language: &str,
    options: &codegen_rust::Options,
) -> TestResult<codegen::ArtifactSet> {
    Ok(if language == "rust" {
        codegen_rust::emit_with_primary_json_documents(program, options)?
    } else {
        codegen_csharp::emit_with_primary_json_documents(program)?
    })
}

// Reuse the format-neutral host/exception walk from the XML cohort, replacing
// only the public format-specific entry point and typed boundary error names.
static RUST_HOST: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    include_str!("fixtures/primary_xml_documents_rust.rs.txt")
        .replace("PrimaryXmlDocumentError", "PrimaryJsonDocumentError")
        .replace(
            "execute_primary_xml_document_bytes",
            "execute_primary_json_document_bytes",
        )
        .replace("\"xml\"", "\"json\"")
        .replace(
            "    Ok(())\n}",
            "    check_output_limit()?;\n    check_input_budget()?;\n    Ok(())\n}",
        )
        + RUST_OUTPUT_LIMIT
});
static CSHARP_HOST: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    include_str!("fixtures/primary_xml_documents_csharp.cs.txt")
        .replace(
            "ExecutePrimaryXmlDocumentBytes",
            "ExecutePrimaryJsonDocumentBytes",
        )
        .replace("FerruleXmlOutputSetException", "FerruleRuntimeException")
        .replace("\"xml\"", "\"json\"")
        .replace(
            "File.WriteAllText(args[2]",
            "if (model != 4) CheckOutputLimit();\nif (model == 1) CheckInputBudget();\nFile.WriteAllText(args[2]",
        )
        .replace(
            "sealed class Recording",
            &(CSHARP_OUTPUT_LIMIT.to_owned() + "\nsealed class Recording"),
        )
});

const RUST_OUTPUT_LIMIT: &str = r#"
fn check_input_budget() -> Result<(), Box<dyn std::error::Error>> {
    struct Host(std::cell::Cell<usize>);
    impl PrimarySourceHost for Host {
        type Error = LocalFileError;
        fn select(&self, _: &str, _: PrimaryFileSelection) -> Result<Vec<String>, Self::Error> {
            Ok((0..6).map(|n| format!("{n}.json")).collect())
        }
        fn load(&self, _: &str) -> Result<CsvSourceData, Self::Error> {
            self.0.set(self.0.get()+1);
            let mut bytes = vec![b' '; codegen_runtime::MAX_JSON_DOCUMENT_BYTES];
            bytes[..2].copy_from_slice(b"{}");
            Ok(CsvSourceData { bytes, resolved_path: None })
        }
    }
    let host = Host(std::cell::Cell::new(0));
    let mut parameters = RuntimeParameters::new();
    parameters.insert("Input", Value::String("*.json".into()))?;
    let execution = ExecutionContext::new(Path::new("authored.mapping")).with_parameters(&parameters);
    assert!(matches!(model1::execute_primary_json_document_bytes(&execution, &host, 6),
        Err(PrimaryJsonDocumentError::Acquisition(PrimaryAcquisitionError::Input { path, source: codegen_runtime::JsonBoundaryError::InputTooLarge { max, .. } })) if path == "4.json" && max == 256 * 1024 * 1024));
    assert_eq!(host.0.get(), 5);
    Ok(())
}

fn check_output_limit() -> Result<(), Box<dyn std::error::Error>> {
    struct Host;
    impl PrimarySourceHost for Host {
        type Error = LocalFileError;
        fn select(&self, _: &str, _: PrimaryFileSelection) -> Result<Vec<String>, Self::Error> { Ok(vec!["input.json".into()]) }
        fn load(&self, _: &str) -> Result<CsvSourceData, Self::Error> {
            let orders: Vec<_> = (0..70).map(|key| json!({"Key":key.to_string(),"Text":"x","Count":1})).collect();
            Ok(CsvSourceData { bytes: serde_json::to_vec(&json!({"Order":orders})).unwrap(), resolved_path: None })
        }
    }
    let mut parameters = RuntimeParameters::new();
    parameters.insert("Status", Value::String("x".repeat(1_000_000)))?;
    let execution = ExecutionContext::new(Path::new("authored.mapping")).with_parameters(&parameters);
    assert!(matches!(model0::execute_primary_json_document_bytes(&execution, &Host, 1),
        Err(PrimaryJsonDocumentError::Output(codegen_runtime::JsonBoundaryError::OutputTooLarge { max, .. })) if max == 64 * 1024 * 1024));
    Ok(())
}
"#;
const CSHARP_OUTPUT_LIMIT: &str = r#"
static void CheckInputBudget()
{
    var host = new BudgetHost();
    var execution = FerruleExecutionContext.WithParameters("authored.mapping", new FerruleRuntimeParameters(
        new Dictionary<string, FerruleValue> { ["Input"] = FerruleValue.FromString("*.json") }));
    try { GeneratedMapping.ExecutePrimaryJsonDocumentBytes(execution, host, 6); }
    catch (FerrulePrimaryDocumentException error) when (error.Error == FerruleDocumentAcquisitionError.Input && error.Path == "4.json" &&
        error.InnerException is FerrulePrimaryJsonInputException { Maximum: 268435456, Bytes: 335544320 } && host.Loads == 5) { return; }
    throw new InvalidOperationException("Expected aggregate input refusal before the sixth load.");
}

static void CheckOutputLimit()
{
    var parameters = new Dictionary<string, FerruleValue> {
        ["Status"] = FerruleValue.FromString(new string('x', 1_000_000)),
        ["Input"] = FerruleValue.FromString("input.json")
    };
    var execution = FerruleExecutionContext.WithParameters("authored.mapping", new FerruleRuntimeParameters(parameters));
    try { GeneratedMapping.ExecutePrimaryJsonDocumentBytes(execution, new OverflowHost(), 1); }
    catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary &&
        error.Detail?.StartsWith("JSON output is ", StringComparison.Ordinal) == true) { return; }
    throw new InvalidOperationException("Expected JSON output byte limit refusal.");
}

sealed class OverflowHost : IFerrulePrimarySourceHost
{
    public IReadOnlyList<string> Select(string filename, FerrulePrimaryFileSelection selection) => ["input.json"];
    public FerruleCsvSourceData Load(string request)
    {
        var rows = Enumerable.Range(0, 70).Select(key => new { Key = key.ToString(), Text = "x", Count = 1 });
        return new FerruleCsvSourceData(System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(new { Order = rows }));
    }
}
sealed class BudgetHost : IFerrulePrimarySourceHost
{
    public int Loads;
    public IReadOnlyList<string> Select(string filename, FerrulePrimaryFileSelection selection) => Enumerable.Range(0, 6).Select(n => n + ".json").ToArray();
    public FerruleCsvSourceData Load(string request)
    {
        Loads++;
        var bytes = new byte[64 * 1024 * 1024];
        Array.Fill(bytes, (byte)' ');
        bytes[0] = (byte)'{'; bytes[1] = (byte)'}';
        return new FerruleCsvSourceData(bytes);
    }
}
"#;
static CASES: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let mut cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/primary-json-documents/cases.json"
    ))
    .unwrap();
    for case in cases["cases"].as_array_mut().unwrap() {
        if let Some(output) = case["expected"]["output"].as_str() {
            let document: serde_json::Value = serde_json::from_str(output).unwrap();
            case["expected"]["output"] =
                serde_json::Value::String(serde_json::to_string_pretty(&document).unwrap() + "\n");
        }
    }
    serde_json::to_string(&cases).unwrap()
});
static COHORT: std::sync::LazyLock<Cohort> = std::sync::LazyLock::new(|| Cohort {
    tag: "primary-json-documents",
    cases: &CASES,
    models: &projects::MODELS,
    document: projects::document,
    emit,
    rust_fixture: &RUST_HOST,
    csharp_fixture: &CSHARP_HOST,
    configured_model: 1,
});

#[test]
fn primary_json_documents_inventory() -> TestResult<()> {
    harness::check_inventory(&COHORT)
}
#[test]
fn primary_json_documents_interpreter() -> TestResult<()> {
    harness::check_interpreter(&COHORT)
}
#[test]
fn primary_json_documents_emission() -> TestResult<()> {
    harness::check_emission(&COHORT)
}
#[test]
fn primary_json_documents_cli_generation() -> TestResult<()> {
    harness::check_cli_generation(&COHORT)
}

#[test]
fn primary_json_documents_preflight() -> TestResult<()> {
    for model in ["literal_direct", "parameter_wildcard_map_no_documents"] {
        let base = projects::document(model);
        for kind in 0..7 {
            let mut project = base.project().clone();
            match kind {
                0 => project.extra_sources.push(mapping::NamedSource {
                    name: "Other".into(),
                    path: "missing.json".into(),
                    schema: project.source.clone(),
                    options: project.source_options.clone(),
                    dynamic_path: None,
                }),
                1 => project.extra_targets.push(mapping::NamedTarget {
                    name: "Other".into(),
                    path: Some("other.json".into()),
                    schema: project.target.clone(),
                    options: project.target_options.clone(),
                    root: project.root.clone(),
                }),
                2 => project.source_options.xml_document = true,
                3 => project.target_options.xml_document = true,
                4 => project.source_options.json5 = true,
                5 => project.source_options.json_lines = true,
                _ => project.target_options.json_lines = true,
            }
            let document = mapping::DocumentProject::new(project, base.primary_source().clone());
            let expected = document.primary_route().unwrap_err();
            let directory = TempDir::new("json-preflight")?;
            let host = harness::Recording {
                inner: format_csv::LocalPrimaryFiles::new(&directory.0)?,
                calls: Default::default(),
            };
            assert!(
                matches!(cli::execute_primary_document_with_host(&document, Path::new("authored.mapping"), &host, 1, None),
                Err(cli::PrimaryDocumentError::Route(error)) if error == expected)
            );
            assert!(host.calls.borrow().is_empty());
            let saved = directory.0.join("document.json");
            std::fs::write(
                &saved,
                mapping::document_project_file::encode_pretty(&document)?,
            )?;
            let output = directory.0.join("generated");
            for language in ["rust", "csharp"] {
                let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
                command
                    .args([
                        "generate",
                        "--primary-documents",
                        "--language",
                        language,
                        "--project",
                    ])
                    .arg(&saved)
                    .arg("--out")
                    .arg(&output);
                if language == "rust" {
                    command
                        .arg("--rust-runtime-path")
                        .arg(harness::runtime_path()?);
                }
                let run = command.output()?;
                assert!(!run.status.success());
                assert!(String::from_utf8_lossy(&run.stderr).contains(&expected.to_string()));
                assert!(!output.exists());
            }
            if let Ok(program) = codegen::lower_document(&document) {
                assert!(emit(&program, "rust", &harness::rust_options(&COHORT, 0)?).is_err());
                assert!(emit(&program, "csharp", &harness::rust_options(&COHORT, 0)?).is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn primary_json_document_paths_require_the_wildcard_context() -> TestResult<()> {
    let wildcard = projects::document("document_paths");
    assert!(!engine::validate(wildcard.project()).is_empty());
    assert!(engine::validate_primary_document(&wildcard).is_empty());
    codegen::lower_document(&wildcard)?;
    let direct = mapping::DocumentProject::new(
        wildcard.project().clone(),
        mapping::PrimarySourcePlan::new(wildcard.primary_source().filename),
    );
    assert!(!engine::validate_primary_document(&direct).is_empty());
    assert!(codegen::lower_document(&direct).is_err());
    Ok(())
}

#[test]
fn primary_json_documents_configured_generators() -> TestResult<()> {
    let program = codegen::lower_document(&projects::document("parameter_wildcard"))?;
    let options = codegen_csharp::Options {
        identity: codegen_csharp::Identity::new(
            "Acme.Documents",
            "Acme.Documents",
            "DocumentMapping",
        )?,
        manifest: codegen::ManifestSelection::Included,
    };
    let cs = codegen_csharp::Generator::new(&options).emit_with_primary_json_documents(&program)?;
    let rust_options = harness::rust_options(&COHORT, 0)?;
    let rs = codegen_rust::Generator::new(&rust_options)
        .with_manifest(codegen::ManifestSelection::Included)
        .emit_with_primary_json_documents(&program)?;
    for artifacts in [cs, rs] {
        let file = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "ferrule-manifest.json")
            .unwrap();
        let manifest =
            codegen::CompatibilityManifest::from_json(std::str::from_utf8(&file.contents)?)?;
        assert_eq!(
            manifest.profile,
            codegen::GenerationProfile::PrimaryJsonDocument
        );
    }
    Ok(())
}

#[test]
fn primary_json_documents_output_limit() -> TestResult<()> {
    struct Host;
    impl format_csv::PrimarySourceHost for Host {
        type Error = std::io::Error;
        fn select(
            &self,
            _: &str,
            _: mapping::PrimaryFileSelection,
        ) -> Result<Vec<String>, Self::Error> {
            Ok(vec!["input.json".into()])
        }
        fn load(&self, _: &str) -> Result<format_csv::CsvSourceData, Self::Error> {
            let orders: Vec<_> = (0..70)
                .map(|key| serde_json::json!({"Key":key.to_string(),"Text":"x","Count":1}))
                .collect();
            Ok(format_csv::CsvSourceData {
                bytes: serde_json::to_vec(&serde_json::json!({"Order":orders}))?,
                resolved_path: None,
            })
        }
    }
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("Status", Value::String("x".repeat(1_000_000)))?;
    let error = cli::execute_primary_document_with_host(
        &projects::document("literal_direct"),
        Path::new("authored.mapping"),
        &Host,
        1,
        Some(&parameters),
    )
    .unwrap_err();
    assert!(
        matches!(error, cli::PrimaryDocumentError::JsonOutput(error) if matches!(error.as_ref(), codegen_runtime::JsonBoundaryError::OutputTooLarge { max, .. } if *max == 64 * 1024 * 1024))
    );
    Ok(())
}

#[test]
fn primary_json_documents_aggregate_limit_stops_later_reads() -> TestResult<()> {
    struct Host(std::cell::Cell<usize>);
    impl format_csv::PrimarySourceHost for Host {
        type Error = std::io::Error;
        fn select(
            &self,
            _: &str,
            _: mapping::PrimaryFileSelection,
        ) -> Result<Vec<String>, Self::Error> {
            Ok((0..6).map(|n| format!("{n}.json")).collect())
        }
        fn load(&self, _: &str) -> Result<format_csv::CsvSourceData, Self::Error> {
            self.0.set(self.0.get() + 1);
            let mut bytes = vec![b' '; codegen_runtime::MAX_JSON_DOCUMENT_BYTES];
            bytes[..2].copy_from_slice(b"{}");
            Ok(format_csv::CsvSourceData {
                bytes,
                resolved_path: None,
            })
        }
    }
    let host = Host(std::cell::Cell::new(0));
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("Input", Value::String("*.json".into()))?;
    let error = cli::execute_primary_document_with_host(
        &projects::document("parameter_wildcard"),
        Path::new("authored.mapping"),
        &host,
        6,
        Some(&parameters),
    )
    .unwrap_err();
    assert!(
        matches!(error, cli::PrimaryDocumentError::Run(engine::PrimaryDocumentRunError::Acquisition(
        format_csv::PrimaryAcquisitionError::Input { path, source: cli::PrimaryInputError::Json(error) }
    )) if path == "4.json" && matches!(error.as_ref(), codegen_runtime::JsonBoundaryError::InputTooLarge { max, .. } if *max == 256 * 1024 * 1024))
    );
    assert_eq!(host.0.get(), 5);
    Ok(())
}

#[test]
#[ignore = "builds generated Rust and C# hosts; requires cargo and .NET 10"]
fn primary_json_documents_generated_hosts() -> TestResult<()> {
    harness::check_generated_hosts(&COHORT)
}
