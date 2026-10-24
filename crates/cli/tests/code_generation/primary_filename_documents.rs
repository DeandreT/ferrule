//! #849 primary filename documents: the interpreter, generated Rust and
//! generated C# run the portable conformance cases through the explicit local
//! filesystem host and compare complete documents, typed failures and the
//! ordered host calls each case reached.
use super::*;
use std::cell::RefCell;

use format_csv::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvSourceData, CsvWriteOptions, LocalFileError,
    LocalPrimaryFiles, PrimarySourceHost,
};
use mapping::{PrimaryFileSelection, document_project_file};
use serde_json::{Value as Json, json};

#[path = "../../../../conformance/primary-filename-documents/projects.rs"]
mod projects;

const CASES: &str = include_str!("../../../../conformance/primary-filename-documents/cases.json");
const DEFAULT_MAXIMUM: u64 = 16;

fn cases() -> TestResult<Vec<Json>> {
    let cases: Json = serde_json::from_str(CASES)?;
    Ok(cases["cases"].as_array().ok_or("cases")?.clone())
}

fn runnable(case: &Json) -> bool {
    case["unix"] != true || cfg!(unix)
}

/// Create every case's tree beneath `root`, one directory per case id.
fn materialize(root: &Path, cases: &[Json]) -> TestResult<()> {
    for case in cases.iter().filter(|case| runnable(case)) {
        let directory = root.join(case["id"].as_str().ok_or("id")?);
        std::fs::create_dir_all(&directory)?;
        for path in case["dirs"].as_array().into_iter().flatten() {
            std::fs::create_dir_all(directory.join(path.as_str().ok_or("dir")?))?;
        }
        for (path, contents) in case["files"].as_object().into_iter().flatten() {
            let path = directory.join(path);
            std::fs::create_dir_all(path.parent().ok_or("file parent")?)?;
            std::fs::write(path, contents.as_str().ok_or("contents")?)?;
        }
        #[cfg(unix)]
        for (link, target) in case["symlinks"].as_object().into_iter().flatten() {
            std::os::unix::fs::symlink(target.as_str().ok_or("target")?, directory.join(link))?;
        }
    }
    Ok(())
}

struct Recording {
    inner: LocalPrimaryFiles,
    calls: RefCell<Vec<String>>,
}

impl PrimarySourceHost for Recording {
    type Error = LocalFileError;

    fn select(
        &self,
        filename: &str,
        selection: PrimaryFileSelection,
    ) -> Result<Vec<String>, LocalFileError> {
        self.calls.borrow_mut().push(format!("select:{filename}"));
        self.inner.select(filename, selection)
    }

    fn load(&self, request: &str) -> Result<CsvSourceData, LocalFileError> {
        self.calls.borrow_mut().push(format!("load:{request}"));
        self.inner.load(request)
    }
}

fn local(error: &LocalFileError) -> (&'static str, String) {
    match error {
        LocalFileError::Root { path, .. } => ("root", path.display().to_string()),
        LocalFileError::Uri { scheme } => ("uri", scheme.clone()),
        LocalFileError::Rooted { filename } => ("rooted", filename.clone()),
        LocalFileError::Parent { filename } => ("parent", filename.clone()),
        LocalFileError::InvalidName { filename } => ("invalid_name", filename.clone()),
        LocalFileError::WildcardDirectory { filename } => ("wildcard_directory", filename.clone()),
        LocalFileError::NoMatches { filename } => ("no_matches", filename.clone()),
        LocalFileError::Missing { path } => ("missing", path.clone()),
        LocalFileError::Symlink { path } => ("symlink", path.clone()),
        LocalFileError::NotRegular { path } => ("not_regular", path.clone()),
        LocalFileError::TooLarge { path, .. } => ("too_large", path.clone()),
        LocalFileError::Io { path, .. } => ("io", path.clone()),
    }
}

fn engine_kind(error: &engine::EngineError) -> (&'static str, String) {
    match error {
        engine::EngineError::MissingRuntimeParameter { name, .. } => {
            ("missing_runtime_parameter", name.clone())
        }
        other => ("other", other.to_string()),
    }
}

fn interpreter_error(
    error: engine::PrimaryDocumentRunError<LocalFileError>,
) -> TestResult<(&'static str, &'static str, String)> {
    use engine::{PrimaryDocumentRunError as Run, PrimaryFilenameEvaluationError as Filename};
    Ok(match error {
        Run::Invalid(issues) => return Err(format!("invalid mapping: {issues:?}").into()),
        Run::Mapping(error) => {
            let (kind, subject) = engine_kind(&error);
            ("mapping", kind, subject)
        }
        Run::Acquisition(error) => match error {
            CsvAcquisitionError::Filename(Filename::Parameter(error)) => {
                let (kind, subject) = engine_kind(&error);
                ("filename", kind, subject)
            }
            CsvAcquisitionError::Filename(other) => ("filename", "other", other.to_string()),
            CsvAcquisitionError::Selection(error) => {
                let (kind, subject) = local(&error);
                ("selection", kind, subject)
            }
            CsvAcquisitionError::Load { source, .. } => {
                let (kind, subject) = local(&source);
                ("load", kind, subject)
            }
            CsvAcquisitionError::Request(error) => {
                let (kind, subject) = local(&error);
                ("request", kind, subject)
            }
            CsvAcquisitionError::Documents { maximum } => {
                ("documents", "limit", maximum.to_string())
            }
            CsvAcquisitionError::Input { path, .. } => ("input", "csv", path),
            CsvAcquisitionError::InvalidLimit { maximum } => {
                ("documents", "invalid_limit", maximum.to_string())
            }
        },
    })
}

fn parameters(case: &Json) -> TestResult<engine::RuntimeParameters> {
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case["parameters"].as_object().into_iter().flatten() {
        parameters.insert(name, Value::String(value.as_str().ok_or("value")?.into()))?;
    }
    Ok(parameters)
}

fn interpreter(root: &Path, case: &Json) -> TestResult<Json> {
    let id = case["id"].as_str().ok_or("id")?;
    let document =
        projects::document(projects::MODELS[case["model"].as_u64().ok_or("model")? as usize]);
    let mut files = LocalPrimaryFiles::new(&root.join(id))?;
    if let Some(limit) = case["max_document_bytes"].as_u64() {
        files = files.with_max_document_bytes(limit);
    }
    let host = Recording {
        inner: files,
        calls: RefCell::new(Vec::new()),
    };
    let parameters = parameters(case)?;
    let execution =
        engine::ExecutionContext::new(Path::new("authored.mapping")).with_parameters(&parameters);
    let maximum = case["maximum"].as_u64().unwrap_or(DEFAULT_MAXIMUM) as usize;
    let result = engine::run_primary_csv_document(
        &document,
        &execution,
        &host,
        CsvByteDecodingPolicy::StrictUtf8,
        maximum,
    );
    let mut record = json!({"id": id, "calls": host.calls.take()});
    match result {
        Ok(output) => {
            let Instance::Repeated(rows) = output else {
                return Err(format!("{id}: non-repeated output").into());
            };
            let project = document.project();
            let bytes = format_csv::to_bytes_with_options_bounded(
                &project.target,
                &rows,
                &CsvWriteOptions::from(&project.target_options),
                codegen_runtime::MAX_CSV_DOCUMENT_BYTES,
            )?;
            record["output"] = json!(String::from_utf8(bytes)?);
        }
        Err(error) => {
            let (stage, kind, subject) = interpreter_error(error)?;
            record["error"] = json!([stage, kind, subject]);
        }
    }
    Ok(record)
}

fn expected(case: &Json) -> Json {
    let mut record = json!({"id": case["id"], "calls": case["calls"]});
    for key in ["output", "error"] {
        if !case["expected"][key].is_null() {
            record[key] = case["expected"][key].clone();
        }
    }
    record
}

fn compare(label: &str, cases: &[Json], actual: &[Json]) -> TestResult<()> {
    let expected: Vec<_> = cases
        .iter()
        .filter(|case| runnable(case))
        .map(expected)
        .collect();
    let mismatches: Vec<_> = expected
        .iter()
        .zip(actual)
        .filter(|(expected, actual)| expected != actual)
        .map(|(expected, actual)| format!("{label}: expected {expected}\n  actual {actual}"))
        .collect();
    if expected.len() != actual.len() {
        return Err(format!(
            "{label}: {} results for {} cases",
            actual.len(),
            expected.len()
        )
        .into());
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(mismatches.join("\n").into())
    }
}

#[test]
fn primary_filename_documents_cover_every_model_and_keep_their_saved_plans() -> TestResult<()> {
    let cases = cases()?;
    let ids: Vec<_> = cases.iter().map(|case| case["id"].clone()).collect();
    let expected: Vec<_> = (1..=cases.len())
        .map(|n| json!(format!("P{n:02}")))
        .collect();
    assert_eq!(ids, expected);
    for model in 0..projects::MODELS.len() {
        assert!(cases.iter().any(|case| case["model"] == model), "{model}");
    }
    for model in projects::MODELS {
        let document = projects::document(model);
        let encoded = document_project_file::encode_pretty(&document)?;
        let decoded = document_project_file::decode_str(&encoded)?;
        assert_eq!(decoded.primary_source(), document.primary_source());
        // A direct plan keeps its original saved form; a wildcard plan names
        // its selection, so readers that predate it refuse the plan.
        let plan = serde_json::to_value(document.primary_source())?;
        assert_eq!(plan.get("selection").is_none(), model == "literal_direct");
        let program = codegen::lower_document(&document)?;
        assert_eq!(
            program.primary_selection(),
            document.primary_source().selection
        );
    }
    Ok(())
}

#[test]
fn primary_filename_documents_interpreter_matches_expected() -> TestResult<()> {
    let cases = cases()?;
    let directory = TempDir::new("primary_filename_documents_interpreter")?;
    materialize(&directory.0, &cases)?;
    let actual = cases
        .iter()
        .filter(|case| runnable(case))
        .map(|case| interpreter(&directory.0, case))
        .collect::<TestResult<Vec<_>>>()?;
    compare("interpreter", &cases, &actual)
}

#[test]
fn primary_filename_documents_refuse_source_dependent_filenames() -> TestResult<()> {
    let original = projects::document("parameter_wildcard");
    let mut project = original.project().clone();
    project
        .graph
        .nodes
        .insert(projects::FILENAME, Node::SourceDocumentPath);
    let document = mapping::DocumentProject::new(project, original.primary_source().clone());
    let unsupported = mapping::PrimaryFilenameError::UnsupportedExpression {
        node: projects::FILENAME,
    };
    assert!(matches!(
        codegen::lower_document(&document),
        Err(codegen::DocumentLowerError::Filename(error)) if error == unsupported
    ));
    let directory = TempDir::new("primary_filename_documents_unsupported")?;
    let host = Recording {
        inner: LocalPrimaryFiles::new(&directory.0)?,
        calls: RefCell::new(Vec::new()),
    };
    let parameters = engine::RuntimeParameters::new();
    let execution =
        engine::ExecutionContext::new(Path::new("authored.mapping")).with_parameters(&parameters);
    let result = engine::run_primary_csv_document(
        &document,
        &execution,
        &host,
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    );
    assert!(matches!(
        result,
        Err(engine::PrimaryDocumentRunError::Acquisition(CsvAcquisitionError::Filename(
            engine::PrimaryFilenameEvaluationError::Plan(error)
        ))) if error == unsupported
    ));
    assert!(host.calls.borrow().is_empty());
    Ok(())
}

fn emit(model: usize, language: &str) -> TestResult<codegen::ArtifactSet> {
    let document = projects::document(projects::MODELS[model]);
    let program = codegen::lower_document(&document)?;
    let policy = codegen::CsvDocumentBoundaryPolicy::from_project(document.project())?;
    Ok(if language == "rust" {
        let options = codegen_rust::Options {
            package_name: format!("primary-documents-model{model}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime_path()?.display().to_string(),
            ),
        };
        codegen_rust::emit_with_primary_csv_documents(&program, &options, &policy)?
    } else {
        codegen_csharp::emit_with_primary_csv_documents(&program, &policy)?
    })
}

#[test]
fn primary_filename_documents_emit_their_plan_deterministically() -> TestResult<()> {
    for model in 0..projects::MODELS.len() {
        for language in ["rust", "csharp"] {
            let artifacts = emit(model, language)?;
            assert_eq!(artifacts, emit(model, language)?, "{model}/{language}");
            let text: String = artifacts
                .files()
                .iter()
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect();
            let wildcard = if language == "rust" {
                "PrimaryFileSelection::FileNameWildcard"
            } else {
                "FerrulePrimaryFileSelection.FileNameWildcard("
            };
            assert_eq!(text.contains(wildcard), model != 0, "{model}/{language}");
            // The saved preview paths never reach generated code.
            assert!(!text.contains("never-preview"), "{model}/{language}");
            assert!(
                !text.contains("never-parameter-preview"),
                "{model}/{language}"
            );
        }
    }
    Ok(())
}

/// The configured identity model 1 is generated under in the host test.
const CONFIGURED_MODEL: usize = 1;
const CSHARP_IDENTITY: [&str; 3] = ["Acme.Documents", "Acme.Documents", "DocumentMapping"];

/// The canonical runtime path, so every generated crate and host binds one
/// `codegen-runtime` package.
fn runtime_path() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?)
}

/// Saves `model`'s document project and generates it with `ferrule generate
/// --primary-csv-documents`, optionally under a configured identity with the
/// compatibility manifest.
fn cli_generate(
    root: &Path,
    model: usize,
    language: &str,
    configured: bool,
    output: &Path,
) -> TestResult<()> {
    let project = root.join(format!("document-model{model}.json"));
    if !project.exists() {
        std::fs::write(
            &project,
            document_project_file::encode_pretty(&projects::document(projects::MODELS[model]))?,
        )?;
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args([
            "generate",
            "--primary-csv-documents",
            "--language",
            language,
        ])
        .arg("--project")
        .arg(&project)
        .arg("--out")
        .arg(output);
    if language == "rust" {
        command
            .arg("--rust-runtime-path")
            .arg(runtime_path()?)
            .arg("--rust-package-name")
            .arg(format!("primary-documents-model{model}"));
    } else if configured {
        for (flag, value) in [
            "--csharp-assembly-name",
            "--csharp-namespace",
            "--csharp-class",
        ]
        .into_iter()
        .zip(CSHARP_IDENTITY)
        {
            command.arg(flag).arg(value);
        }
    }
    if configured {
        command.arg("--compatibility-manifest");
    }
    let run = command.output()?;
    if !run.status.success() {
        return Err(format!(
            "generate {model}/{language}: {}",
            String::from_utf8_lossy(&run.stderr)
        )
        .into());
    }
    Ok(())
}

fn tree(directory: &Path) -> TestResult<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(next)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(directory)?
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(relative, std::fs::read(&path)?);
            }
        }
    }
    Ok(files)
}

#[test]
fn primary_filename_documents_cli_generation_matches_the_emitters() -> TestResult<()> {
    let directory = TempDir::new("primary_filename_documents_generate")?;
    for model in 0..projects::MODELS.len() {
        for language in ["rust", "csharp"] {
            let output = directory.0.join(format!("{language}-{model}"));
            cli_generate(&directory.0, model, language, false, &output)?;
            let mut emitted = BTreeMap::new();
            for file in emit(model, language)?.files() {
                emitted.insert(file.path.as_str().to_owned(), file.contents.clone());
            }
            assert!(tree(&output)? == emitted, "{model}/{language}");
        }
    }
    // An ordinary project is refused by the document profile, and a document
    // project by every ordinary profile.
    let ordinary = directory.0.join("ordinary.json");
    std::fs::write(
        &ordinary,
        mapping::project_file::encode_pretty(projects::document(projects::MODELS[0]).project())?,
    )?;
    let saved = directory.0.join("document-model0.json");
    for (project, flag) in [
        (&ordinary, Some("--primary-csv-documents")),
        (&saved, None),
        (&saved, Some("--csv-document-adapters")),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .args(["generate", "--language", "csharp", "--project"])
            .arg(project)
            .arg("--out")
            .arg(directory.0.join("refused"));
        command.args(flag);
        let run = command.output()?;
        assert!(!run.status.success(), "{project:?} {flag:?}");
        assert!(!directory.0.join("refused").exists());
    }
    Ok(())
}

#[test]
fn primary_filename_documents_generators_apply_identity_and_manifest() -> TestResult<()> {
    let document = projects::document(projects::MODELS[CONFIGURED_MODEL]);
    let program = codegen::lower_document(&document)?;
    let policy = codegen::CsvDocumentBoundaryPolicy::from_project(document.project())?;
    let mut raw = policy.clone();
    raw.target.raw_delimited_crlf = true;
    raw.target.quote_disabled = true;

    let default = codegen_csharp::Options::default();
    assert_eq!(
        codegen_csharp::Generator::new(&default)
            .emit_with_primary_csv_documents(&program, &policy)?,
        codegen_csharp::emit_with_primary_csv_documents(&program, &policy)?
    );
    let options = codegen_csharp::Options {
        identity: codegen_csharp::Identity::new(
            CSHARP_IDENTITY[0],
            CSHARP_IDENTITY[1],
            CSHARP_IDENTITY[2],
        )?,
        manifest: codegen::ManifestSelection::Included,
    };
    let generator = codegen_csharp::Generator::new(&options);
    let configured = generator.emit_with_primary_csv_documents(&program, &policy)?;
    let file = |set: &codegen::ArtifactSet, path: &str| -> TestResult<String> {
        Ok(String::from_utf8(
            set.files()
                .iter()
                .find(|file| file.path.as_str() == path)
                .ok_or_else(|| format!("missing {path}"))?
                .contents
                .clone(),
        )?)
    };
    let manifest =
        codegen::CompatibilityManifest::from_json(&file(&configured, "ferrule-manifest.json")?)?;
    assert_eq!(
        manifest.profile,
        codegen::GenerationProfile::PrimaryCsvDocument
    );
    let companion = file(&configured, "GeneratedMapping.PrimaryCsvDocument.cs")?;
    assert!(companion.starts_with(
        "namespace Acme.Documents;

public static partial class DocumentMapping
"
    ));
    assert!(
        file(&configured, "Acme.Documents.csproj")?
            .contains("GeneratedMapping.PrimaryCsvDocument.cs")
    );
    let raw_manifest = codegen::CompatibilityManifest::from_json(&file(
        &generator.emit_with_primary_csv_documents(&program, &raw)?,
        "ferrule-manifest.json",
    )?)?;
    assert_eq!(
        raw_manifest.profile,
        codegen::GenerationProfile::PrimaryRawDelimitedDocument
    );

    let options = codegen_rust::Options {
        package_name: "primary-documents".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            runtime_path()?.display().to_string(),
        ),
    };
    let plain = codegen_rust::Generator::new(&options);
    assert_eq!(
        plain.emit_with_primary_csv_documents(&program, &policy)?,
        codegen_rust::emit_with_primary_csv_documents(&program, &options, &policy)?
    );
    let with = plain
        .with_manifest(codegen::ManifestSelection::Included)
        .emit_with_primary_csv_documents(&program, &raw)?;
    let manifest =
        codegen::CompatibilityManifest::from_json(&file(&with, "ferrule-manifest.json")?)?;
    assert_eq!(
        manifest.profile,
        codegen::GenerationProfile::PrimaryRawDelimitedDocument
    );
    assert!(file(&with, "src/lib.rs")?.ends_with(
        "\nmod ferrule_compatibility;\npub use ferrule_compatibility::{COMPATIBILITY_MANIFEST, RUNTIME_IDENTITY};\n"
    ));
    Ok(())
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().ok_or("artifact parent")?)?;
        std::fs::write(path, &file.contents)?;
    }
    Ok(())
}

fn host_results(path: &Path) -> TestResult<Vec<Json>> {
    let results: Json = serde_json::from_slice(&std::fs::read(path)?)?;
    Ok(results.as_array().ok_or("host results")?.clone())
}

#[test]
#[ignore = "builds one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn primary_filename_documents_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let cases = cases()?;
    let directory = TempDir::new("primary_filename_documents_hosts")?;
    let root = directory.0.canonicalize()?;
    let trees = root.join("cases");
    materialize(&trees, &cases)?;
    let cases_path = root.join("cases.json");
    std::fs::write(&cases_path, CASES)?;

    let rust = root.join("rust-host");
    let mut dependencies = String::new();
    for model in 0..projects::MODELS.len() {
        let output = root.join(format!("rust-model{model}"));
        if model == CONFIGURED_MODEL {
            cli_generate(&root, model, "rust", true, &output)?;
        } else {
            write_artifacts(&output, &emit(model, "rust")?)?;
        }
        dependencies.push_str(&format!(
            "model{model}={{package=\"primary-documents-model{model}\",path=\"../rust-model{model}\"}}\n"
        ));
    }
    std::fs::create_dir_all(rust.join("src"))?;
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("fixtures/primary_filename_documents_rust.rs.txt"),
    )?;
    std::fs::write(
        rust.join("Cargo.toml"),
        format!(
            "[package]\nname=\"primary-documents-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&runtime_path()?.display().to_string())?
        ),
    )?;
    let rust_results = root.join("rust-results.json");
    let mut command = Command::new("cargo");
    command
        .args(["run", "--offline", "--quiet", "--"])
        .arg(&cases_path)
        .arg(&trees)
        .arg(&rust_results)
        .current_dir(&rust);
    let run = host_policy::recorded_output(&mut command, &rust, "primary-documents-rust")?;
    let mut failures = Vec::new();
    if run.status.success() {
        if let Err(error) = compare("rust", &cases, &host_results(&rust_results)?) {
            failures.push(error.to_string());
        }
    } else {
        failures.push(format!(
            "rust host failed:\n{}\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        ));
    }

    for model in 0..projects::MODELS.len() {
        let output = root.join(format!("csharp-model{model}"));
        let mut harness =
            include_str!("fixtures/primary_filename_documents_csharp.cs.txt").to_string();
        let mut library = "Ferrule.Generated";
        if model == CONFIGURED_MODEL {
            // Generated through the CLI under a configured identity and manifest.
            cli_generate(&root, model, "csharp", true, &output)?;
            harness = harness
                .replace("using Ferrule.Generated;", "using Acme.Documents;")
                .replace("GeneratedMapping.", "DocumentMapping.");
            library = CSHARP_IDENTITY[0];
        } else {
            write_artifacts(&output, &emit(model, "csharp")?)?;
        }
        std::fs::create_dir_all(output.join("Harness"))?;
        std::fs::write(output.join("Harness/Program.cs"), harness)?;
        std::fs::write(
            output.join("Harness/Harness.csproj"),
            format!(
                r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../{library}.csproj" /></ItemGroup></Project>"#
            ),
        )?;
        let results = output.join("results.json");
        let mut command = dotnet_command(&output);
        command
            .args([
                "run",
                "--project",
                "Harness/Harness.csproj",
                "--configuration",
                "Release",
                "-p:UseSharedCompilation=false",
                "--",
            ])
            .arg(&cases_path)
            .arg(&trees)
            .arg(&results)
            .arg(model.to_string())
            .current_dir(&output);
        let run = host_policy::recorded_output(
            &mut command,
            &output,
            &format!("primary-documents-csharp{model}"),
        )?;
        if !run.status.success() {
            failures.push(format!(
                "C# model {model} host failed:\n{}\n{}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            ));
            continue;
        }
        let model_cases: Vec<_> = cases
            .iter()
            .filter(|case| case["model"] == model)
            .cloned()
            .collect();
        if let Err(error) = compare(
            &format!("csharp model {model}"),
            &model_cases,
            &host_results(&results)?,
        ) {
            failures.push(error.to_string());
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n\n").into())
    }
}
