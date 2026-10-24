//! `ferrule run-document` and the CLI document run library produce the same
//! complete documents and typed failures as the interpreter for every
//! primary filename document cohort case.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_csv::LocalPrimaryFiles;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, DocumentProject, Graph, Node, Project, Scope, ScopeIteration};
use serde_json::Value as Json;

#[path = "../../../conformance/primary-filename-documents/projects.rs"]
mod projects;
#[path = "../../../conformance/primary-xml-documents/projects.rs"]
mod xml_projects;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

struct Cohort {
    name: &'static str,
    cases: &'static str,
    models: &'static [&'static str],
    document: fn(&str) -> DocumentProject,
}

const COHORTS: [Cohort; 2] = [
    Cohort {
        name: "csv",
        cases: include_str!("../../../conformance/primary-filename-documents/cases.json"),
        models: &projects::MODELS,
        document: projects::document,
    },
    Cohort {
        name: "xml",
        cases: include_str!("../../../conformance/primary-xml-documents/cases.json"),
        models: &xml_projects::MODELS,
        document: xml_projects::document,
    },
];

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_primary_document_run_{name}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path.canonicalize()?))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cases(cohort: &Cohort) -> TestResult<Vec<Json>> {
    let cases: Json = serde_json::from_str(cohort.cases)?;
    Ok(cases["cases"]
        .as_array()
        .ok_or("cases")?
        .iter()
        .filter(|case| case["unix"] != true || cfg!(unix))
        .cloned()
        .collect())
}

fn materialize(directory: &Path, case: &Json) -> TestResult<()> {
    std::fs::create_dir_all(directory)?;
    for path in case["dirs"].as_array().into_iter().flatten() {
        std::fs::create_dir_all(directory.join(path.as_str().ok_or("dir")?))?;
    }
    for (path, contents) in case["files"].as_object().into_iter().flatten() {
        let path = directory.join(path);
        std::fs::create_dir_all(path.parent().ok_or("parent")?)?;
        std::fs::write(path, contents.as_str().ok_or("contents")?)?;
    }
    #[cfg(unix)]
    for (link, target) in case["symlinks"].as_object().into_iter().flatten() {
        std::os::unix::fs::symlink(target.as_str().ok_or("target")?, directory.join(link))?;
    }
    Ok(())
}

fn parameters(case: &Json) -> TestResult<engine::RuntimeParameters> {
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case["parameters"].as_object().into_iter().flatten() {
        parameters.insert(name, Value::String(value.as_str().ok_or("value")?.into()))?;
    }
    Ok(parameters)
}

fn policy(root: &Path, case: &Json) -> cli::PrimaryDocumentPolicy {
    let mut policy = cli::PrimaryDocumentPolicy::new(root);
    if let Some(limit) = case["max_document_bytes"].as_u64() {
        policy.max_document_bytes = limit;
    }
    policy.max_documents = case["maximum"].as_u64().unwrap_or(16) as usize;
    policy
}

/// The interpreter's complete document, or its failure's display text.
fn interpreter(
    document: &DocumentProject,
    root: &Path,
    case: &Json,
) -> TestResult<Result<Vec<u8>, String>> {
    let policy = policy(root, case);
    let host = LocalPrimaryFiles::new(root)?.with_max_document_bytes(policy.max_document_bytes);
    let parameters = parameters(case)?;
    Ok(cli::execute_primary_document_with_host(
        document,
        Path::new("authored.mapping"),
        &host,
        policy.max_documents,
        Some(&parameters),
    )
    .map(|(bytes, _)| bytes)
    .map_err(|error| error.to_string()))
}

#[test]
fn cli_document_runs_match_the_interpreter_for_every_case() -> TestResult<()> {
    for cohort in &COHORTS {
        run_cohort(cohort)?;
    }
    Ok(())
}

fn run_cohort(cohort: &Cohort) -> TestResult<()> {
    let temp = TempDir::new(cohort.name)?;
    for case in cases(cohort)? {
        let id = case["id"].as_str().ok_or("id")?;
        let model = cohort.models[case["model"].as_u64().ok_or("model")? as usize];
        let document = (cohort.document)(model);
        let root = temp.0.join("trees").join(id);
        materialize(&root, &case)?;
        let expected = interpreter(&document, &root, &case)?;
        let authored = &case["expected"];
        match &expected {
            Ok(bytes) => assert_eq!(
                Some(String::from_utf8(bytes.clone())?.as_str()),
                authored["output"].as_str(),
                "{id}"
            ),
            Err(_) => assert!(authored["error"].is_array(), "{id}"),
        }

        // The library route returns the same typed stage and complete bytes.
        let project_path = temp.0.join("projects").join(format!("{id}.json"));
        std::fs::create_dir_all(project_path.parent().ok_or("parent")?)?;
        std::fs::write(
            &project_path,
            mapping::document_project_file::encode_pretty(&document)?,
        )?;
        let parameters = parameters(&case)?;
        let library = cli::execute_primary_document(
            &document,
            &project_path,
            &policy(&root, &case),
            Some(&parameters),
        );
        match (&expected, library) {
            (Ok(bytes), Ok(output)) => assert_eq!(&output.bytes, bytes, "{id}"),
            (Err(message), Err(cli::PrimaryDocumentError::Run(error))) => {
                assert_eq!(&error.to_string(), message, "{id}")
            }
            (expected, actual) => panic!("{id}: expected {expected:?}, got {actual:?}"),
        }

        // The command publishes exactly that document or nothing.
        let output = temp.0.join("outputs").join(format!("{id}.out"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .args(["--diagnostics", "json", "run-document", "--project"])
            .arg(&project_path)
            .arg("--input-root")
            .arg(&root)
            .arg("--output")
            .arg(&output);
        let policy = policy(&root, &case);
        command
            .arg("--max-document-bytes")
            .arg(policy.max_document_bytes.to_string())
            .arg("--max-documents")
            .arg(policy.max_documents.to_string());
        for (name, value) in case["parameters"].as_object().into_iter().flatten() {
            command
                .arg("--param")
                .arg(format!("{name}={}", value.as_str().ok_or("value")?));
        }
        let run = command.output()?;
        match &expected {
            Ok(bytes) => {
                assert!(
                    run.status.success(),
                    "{id}: {}",
                    String::from_utf8_lossy(&run.stderr)
                );
                assert_eq!(&std::fs::read(&output)?, bytes, "{id}");
            }
            Err(message) => {
                assert!(!run.status.success(), "{id}");
                assert!(!output.exists(), "{id}: a failed run published output");
                let diagnostic: Json = serde_json::from_slice(
                    run.stderr
                        .split(|byte| *byte == b'\n')
                        .find(|line| !line.is_empty())
                        .ok_or("diagnostic")?,
                )?;
                let reported = diagnostic["message"].as_str().ok_or("message")?;
                assert!(
                    reported.contains(message.as_str()),
                    "{id}: {reported:?} does not report {message:?}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn document_runs_refuse_unrouted_identities_missing_roots_and_input_replacement() -> TestResult<()>
{
    let temp = TempDir::new("refusals")?;
    std::fs::create_dir_all(temp.0.join("orders"))?;
    std::fs::write(temp.0.join("orders/input.csv"), "Key,Text,Count\nk1,x,1\n")?;
    let document = projects::document("literal_direct");
    let mut json = document.project().clone();
    json.target_options.tabular_kind = None;
    json.target_path = Some("output.json".into());
    let json = mapping::DocumentProject::new(json, document.primary_source().clone());
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("Status", Value::String("ok".into()))?;
    assert!(matches!(
        cli::execute_primary_document(
            &json,
            &temp.0.join("project.json"),
            &cli::PrimaryDocumentPolicy::new(&temp.0),
            Some(&parameters),
        ),
        Err(cli::PrimaryDocumentError::Route(
            mapping::PrimaryRouteError::NotAdmitted {
                side: mapping::PrimaryRouteSide::Target,
                format: "JSON"
            }
        ))
    ));
    assert!(matches!(
        cli::execute_primary_document(
            &document,
            &temp.0.join("project.json"),
            &cli::PrimaryDocumentPolicy::new(temp.0.join("absent")),
            Some(&parameters),
        ),
        Err(cli::PrimaryDocumentError::Root(_))
    ));

    let project_path = temp.0.join("project.json");
    std::fs::write(
        &project_path,
        mapping::document_project_file::encode_pretty(&document)?,
    )?;
    for output in [temp.0.join("orders/input.csv"), project_path.clone()] {
        let before = std::fs::read(&output)?;
        let error = cli::run_primary_document_file(
            &project_path,
            Some(&output),
            &cli::PrimaryDocumentPolicy::new(&temp.0),
            Some(&parameters),
        )
        .expect_err("an input or the project is never replaced");
        assert!(error.to_string().contains("would replace"), "{error:#}");
        assert_eq!(std::fs::read(&output)?, before);
    }

    // The saved target path is the default destination; the saved source
    // path is never read.
    let outcome = cli::run_primary_document_file(
        &project_path,
        None,
        &cli::PrimaryDocumentPolicy::new(&temp.0),
        Some(&parameters),
    )?;
    assert_eq!(outcome.output_path, temp.0.join("never-preview-output.csv"));
    assert_eq!(
        std::fs::read_to_string(&outcome.output_path)?,
        "Key,Text,Count,Status\nk1,x,1,ok\n"
    );
    assert_eq!(outcome.records_written, 1);

    // An ordinary project is not a document project, and the reverse.
    let ordinary = temp.0.join("ordinary.json");
    std::fs::write(
        &ordinary,
        mapping::project_file::encode_pretty(document.project())?,
    )?;
    assert!(cli::load_document_project(&ordinary).is_err());
    assert!(cli::load_project(&project_path).is_err());
    Ok(())
}

#[test]
fn run_document_requires_an_explicit_input_root() -> TestResult<()> {
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-document", "--project", "document.json"])
        .output()?;
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("--input-root"));
    Ok(())
}
