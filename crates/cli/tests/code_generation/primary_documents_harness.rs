//! Shared execution of the primary filename document cohorts: every case runs
//! through the explicit local filesystem host in the interpreter, generated
//! Rust and generated C#, comparing complete documents, typed failures and the
//! ordered host calls each case reached.
use super::*;
use std::cell::RefCell;

use format_csv::{
    CsvSourceData, LocalFileError, LocalPrimaryFiles, PrimaryAcquisitionError, PrimarySourceHost,
};
use mapping::{DocumentProject, PrimaryFileSelection, document_project_file};
use serde_json::{Value as Json, json};

pub(super) const DEFAULT_MAXIMUM: u64 = 16;
pub(super) const CSHARP_IDENTITY: [&str; 3] =
    ["Acme.Documents", "Acme.Documents", "DocumentMapping"];

/// One format's cohort: its models, cases, emitters and generated hosts.
pub(super) struct Cohort {
    /// Distinguishes temporary directories and generated package names.
    pub tag: &'static str,
    pub cases: &'static str,
    pub models: &'static [&'static str],
    pub document: fn(&str) -> DocumentProject,
    /// Emits one model's library: (program, language, Rust package name).
    pub emit: fn(
        &codegen::DocumentProgram,
        &str,
        &codegen_rust::Options,
    ) -> TestResult<codegen::ArtifactSet>,
    pub rust_fixture: &'static str,
    pub csharp_fixture: &'static str,
    /// Generated through `ferrule generate` under a configured identity and
    /// the compatibility manifest in the host test.
    pub configured_model: usize,
}

pub(super) fn cases(cohort: &Cohort) -> TestResult<Vec<Json>> {
    let cases: Json = serde_json::from_str(cohort.cases)?;
    Ok(cases["cases"].as_array().ok_or("cases")?.clone())
}

pub(super) fn runnable(case: &Json) -> bool {
    case["unix"] != true || cfg!(unix)
}

/// Create every case's tree beneath `root`, one directory per case id.
pub(super) fn materialize(root: &Path, cases: &[Json]) -> TestResult<()> {
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

pub(super) struct Recording {
    pub inner: LocalPrimaryFiles,
    pub calls: RefCell<Vec<String>>,
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

pub(super) fn local(error: &LocalFileError) -> (&'static str, String) {
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

/// `[stage, kind, subject]` of an interpreter failure; `input` is the
/// route's format name.
pub(super) fn interpreter_error(
    error: cli::PrimaryDocumentError<LocalFileError>,
    input: &'static str,
) -> TestResult<(&'static str, &'static str, String)> {
    use engine::{PrimaryDocumentRunError as Run, PrimaryFilenameEvaluationError as Filename};
    let run = match error {
        cli::PrimaryDocumentError::Run(run) => run,
        other => return Ok(("output", "other", other.to_string())),
    };
    Ok(match run {
        Run::Route(error) => ("route", "refused", error.to_string()),
        Run::Invalid(issues) => return Err(format!("invalid mapping: {issues:?}").into()),
        Run::Assembly(error) => ("assembly", "refused", error.to_string()),
        Run::Mapping(error) => {
            let (kind, subject) = engine_kind(&error);
            ("mapping", kind, subject)
        }
        Run::Acquisition(error) => match error {
            PrimaryAcquisitionError::Filename(Filename::Parameter(error)) => {
                let (kind, subject) = engine_kind(&error);
                ("filename", kind, subject)
            }
            PrimaryAcquisitionError::Filename(other) => ("filename", "other", other.to_string()),
            PrimaryAcquisitionError::Selection(error) => {
                let (kind, subject) = local(&error);
                ("selection", kind, subject)
            }
            PrimaryAcquisitionError::DirectSelection { found } => {
                ("selection", "direct", found.to_string())
            }
            PrimaryAcquisitionError::Request { path } => ("request", "invalid", path),
            PrimaryAcquisitionError::Load { source, .. } => {
                let (kind, subject) = local(&source);
                ("load", kind, subject)
            }
            PrimaryAcquisitionError::Documents { maximum } => {
                ("documents", "limit", maximum.to_string())
            }
            PrimaryAcquisitionError::Input { path, .. } => ("input", input, path),
            PrimaryAcquisitionError::InvalidLimit { maximum } => {
                ("documents", "invalid_limit", maximum.to_string())
            }
        },
    })
}

pub(super) fn parameters(case: &Json) -> TestResult<engine::RuntimeParameters> {
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case["parameters"].as_object().into_iter().flatten() {
        parameters.insert(name, Value::String(value.as_str().ok_or("value")?.into()))?;
    }
    Ok(parameters)
}

pub(super) fn model(cohort: &Cohort, case: &Json) -> TestResult<DocumentProject> {
    let index = case["model"].as_u64().ok_or("model")? as usize;
    Ok((cohort.document)(
        cohort.models.get(index).ok_or("model index")?,
    ))
}

pub(super) fn interpreter(cohort: &Cohort, root: &Path, case: &Json) -> TestResult<Json> {
    let id = case["id"].as_str().ok_or("id")?;
    let document = model(cohort, case)?;
    let input = match document.primary_route()?.format {
        mapping::PrimaryDocumentFormat::Csv => "csv",
        mapping::PrimaryDocumentFormat::Xml => "xml",
        mapping::PrimaryDocumentFormat::Json => "json",
    };
    let mut files = LocalPrimaryFiles::new(&root.join(id))?;
    if let Some(limit) = case["max_document_bytes"].as_u64() {
        files = files.with_max_document_bytes(limit);
    }
    let host = Recording {
        inner: files,
        calls: RefCell::new(Vec::new()),
    };
    let parameters = parameters(case)?;
    let maximum = case["maximum"].as_u64().unwrap_or(DEFAULT_MAXIMUM) as usize;
    let result = cli::execute_primary_document_with_host(
        &document,
        Path::new("authored.mapping"),
        &host,
        maximum,
        Some(&parameters),
    );
    let mut record = json!({"id": id, "calls": host.calls.take()});
    match result {
        Ok((bytes, _)) => record["output"] = json!(String::from_utf8(bytes)?),
        Err(error) => {
            let (stage, kind, subject) = interpreter_error(error, input)?;
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

pub(super) fn compare(label: &str, cases: &[Json], actual: &[Json]) -> TestResult<()> {
    let expected: Vec<_> = cases
        .iter()
        .filter(|case| runnable(case))
        .map(expected)
        .collect();
    if expected.len() != actual.len() {
        return Err(format!(
            "{label}: {} results for {} cases",
            actual.len(),
            expected.len()
        )
        .into());
    }
    let mismatches: Vec<_> = expected
        .iter()
        .zip(actual)
        .filter(|(expected, actual)| expected != actual)
        .map(|(expected, actual)| format!("{label}: expected {expected}\n  actual {actual}"))
        .collect();
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(mismatches.join("\n").into())
    }
}

/// Case ids run `P01`.., every model has a case, and every plan keeps its
/// saved form and lowers with its selection.
pub(super) fn check_inventory(cohort: &Cohort) -> TestResult<()> {
    let cases = cases(cohort)?;
    let ids: Vec<_> = cases.iter().map(|case| case["id"].clone()).collect();
    let expected: Vec<_> = (1..=cases.len())
        .map(|n| json!(format!("P{n:02}")))
        .collect();
    assert_eq!(ids, expected);
    for model in 0..cohort.models.len() {
        assert!(cases.iter().any(|case| case["model"] == model), "{model}");
    }
    for model in cohort.models {
        let document = (cohort.document)(model);
        let encoded = document_project_file::encode_pretty(&document)?;
        let decoded = document_project_file::decode_str(&encoded)?;
        assert_eq!(decoded.primary_source(), document.primary_source());
        // A direct plan keeps its original saved form; a wildcard plan names
        // its selection, so readers that predate it refuse the plan.
        let plan = serde_json::to_value(document.primary_source())?;
        assert_eq!(
            plan.get("selection").is_none(),
            document.primary_source().selection.is_direct()
        );
        let program = codegen::lower_document(&document)?;
        assert_eq!(
            program.primary_selection(),
            document.primary_source().selection
        );
        assert_eq!(program.primary_route(), document.primary_route());
    }
    Ok(())
}

pub(super) fn check_interpreter(cohort: &Cohort) -> TestResult<()> {
    let cases = cases(cohort)?;
    let directory = TempDir::new(&format!("{}_interpreter", cohort.tag))?;
    materialize(&directory.0, &cases)?;
    let actual = cases
        .iter()
        .filter(|case| runnable(case))
        .map(|case| interpreter(cohort, &directory.0, case))
        .collect::<TestResult<Vec<_>>>()?;
    compare("interpreter", &cases, &actual)
}

/// The canonical runtime path, so every generated crate and host binds one
/// `codegen-runtime` package.
pub(super) fn runtime_path() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?)
}

pub(super) fn rust_options(cohort: &Cohort, model: usize) -> TestResult<codegen_rust::Options> {
    Ok(codegen_rust::Options {
        package_name: format!("{}-model{model}", cohort.tag),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            runtime_path()?.display().to_string(),
        ),
    })
}

pub(super) fn emit(
    cohort: &Cohort,
    model: usize,
    language: &str,
) -> TestResult<codegen::ArtifactSet> {
    let document = (cohort.document)(cohort.models[model]);
    let program = codegen::lower_document(&document)?;
    (cohort.emit)(&program, language, &rust_options(cohort, model)?)
}

/// Every model emits deterministically, embeds its selection and never its
/// saved preview paths.
pub(super) fn check_emission(cohort: &Cohort) -> TestResult<()> {
    for model in 0..cohort.models.len() {
        let wildcard = !(cohort.document)(cohort.models[model])
            .primary_source()
            .selection
            .is_direct();
        for language in ["rust", "csharp"] {
            let artifacts = emit(cohort, model, language)?;
            assert_eq!(
                artifacts,
                emit(cohort, model, language)?,
                "{model}/{language}"
            );
            let text: String = artifacts
                .files()
                .iter()
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect();
            let marker = if language == "rust" {
                "PrimaryFileSelection::FileNameWildcard"
            } else {
                "FerrulePrimaryFileSelection.FileNameWildcard("
            };
            assert_eq!(text.contains(marker), wildcard, "{model}/{language}");
            assert!(!text.contains("never-preview"), "{model}/{language}");
            assert!(
                !text.contains("never-parameter-preview"),
                "{model}/{language}"
            );
        }
    }
    Ok(())
}

/// Saves `model`'s document project and generates it with `ferrule generate
/// --primary-documents`, optionally under a configured identity with the
/// compatibility manifest.
pub(super) fn cli_generate(
    cohort: &Cohort,
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
            document_project_file::encode_pretty(&(cohort.document)(cohort.models[model]))?,
        )?;
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args(["generate", "--primary-documents", "--language", language])
        .arg("--project")
        .arg(&project)
        .arg("--out")
        .arg(output);
    if language == "rust" {
        command
            .arg("--rust-runtime-path")
            .arg(runtime_path()?)
            .arg("--rust-package-name")
            .arg(format!("{}-model{model}", cohort.tag));
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

pub(super) fn tree(directory: &Path) -> TestResult<BTreeMap<String, Vec<u8>>> {
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

/// `ferrule generate --primary-documents` writes exactly the emitters' output.
pub(super) fn check_cli_generation(cohort: &Cohort) -> TestResult<()> {
    let directory = TempDir::new(&format!("{}_generate", cohort.tag))?;
    for model in 0..cohort.models.len() {
        for language in ["rust", "csharp"] {
            let output = directory.0.join(format!("{language}-{model}"));
            cli_generate(cohort, &directory.0, model, language, false, &output)?;
            let mut emitted = BTreeMap::new();
            for file in emit(cohort, model, language)?.files() {
                emitted.insert(file.path.as_str().to_owned(), file.contents.clone());
            }
            assert!(tree(&output)? == emitted, "{model}/{language}");
        }
    }
    Ok(())
}

pub(super) fn write_artifacts(
    directory: &Path,
    artifacts: &codegen::ArtifactSet,
) -> TestResult<()> {
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

/// Builds one generated Rust host over every model and one generated C# host
/// per model, then compares every case of both languages.
pub(super) fn check_generated_hosts(cohort: &Cohort) -> TestResult<()> {
    let cases = cases(cohort)?;
    let directory = TempDir::new(&format!("{}_hosts", cohort.tag))?;
    let root = directory.0.canonicalize()?;
    let trees = root.join("cases");
    materialize(&trees, &cases)?;
    let cases_path = root.join("cases.json");
    std::fs::write(&cases_path, cohort.cases)?;

    let rust = root.join("rust-host");
    let mut dependencies = String::new();
    for model in 0..cohort.models.len() {
        let output = root.join(format!("rust-model{model}"));
        if model == cohort.configured_model {
            cli_generate(cohort, &root, model, "rust", true, &output)?;
        } else {
            write_artifacts(&output, &emit(cohort, model, "rust")?)?;
        }
        dependencies.push_str(&format!(
            "model{model}={{package=\"{}-model{model}\",path=\"../rust-model{model}\"}}\n",
            cohort.tag
        ));
    }
    std::fs::create_dir_all(rust.join("src"))?;
    std::fs::write(rust.join("src/main.rs"), cohort.rust_fixture)?;
    std::fs::write(
        rust.join("Cargo.toml"),
        format!(
            "[package]\nname=\"{}-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            cohort.tag,
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
    let run = host_policy::recorded_output(&mut command, &rust, &format!("{}-rust", cohort.tag))?;
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

    for model in 0..cohort.models.len() {
        let output = root.join(format!("csharp-model{model}"));
        let mut harness = cohort.csharp_fixture.to_string();
        let mut library = "Ferrule.Generated";
        if model == cohort.configured_model {
            cli_generate(cohort, &root, model, "csharp", true, &output)?;
            harness = harness
                .replace("using Ferrule.Generated;", "using Acme.Documents;")
                .replace("GeneratedMapping.", "DocumentMapping.");
            library = CSHARP_IDENTITY[0];
        } else {
            write_artifacts(&output, &emit(cohort, model, "csharp")?)?;
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
            &format!("{}-csharp{model}", cohort.tag),
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
