//! #849 primary CSV filename documents: the interpreter, generated Rust and
//! generated C# run the portable conformance cases through the explicit local
//! filesystem host and compare complete documents, typed failures and the
//! ordered host calls each case reached.
use super::primary_documents_harness::{self as harness, Cohort, Recording};
use super::*;
use std::cell::RefCell;

use format_csv::{CsvByteDecodingPolicy, LocalPrimaryFiles, PrimaryAcquisitionError};
use mapping::document_project_file;

#[path = "../../../../conformance/primary-filename-documents/projects.rs"]
pub(super) mod projects;

fn emit(
    program: &codegen::DocumentProgram,
    language: &str,
    options: &codegen_rust::Options,
) -> TestResult<codegen::ArtifactSet> {
    let policy = codegen::CsvDocumentBoundaryPolicy::from_project(&csv_project())?;
    Ok(if language == "rust" {
        codegen_rust::emit_with_primary_csv_documents(program, options, &policy)?
    } else {
        codegen_csharp::emit_with_primary_csv_documents(program, &policy)?
    })
}

/// Every model shares the CSV dialect of the first.
fn csv_project() -> Project {
    projects::document(projects::MODELS[0]).project().clone()
}

const COHORT: Cohort = Cohort {
    tag: "primary-documents",
    cases: include_str!("../../../../conformance/primary-filename-documents/cases.json"),
    models: &projects::MODELS,
    document: projects::document,
    emit,
    rust_fixture: include_str!("fixtures/primary_filename_documents_rust.rs.txt"),
    csharp_fixture: include_str!("fixtures/primary_filename_documents_csharp.cs.txt"),
    configured_model: 1,
};

#[test]
fn primary_filename_documents_cover_every_model_and_keep_their_saved_plans() -> TestResult<()> {
    harness::check_inventory(&COHORT)
}

#[test]
fn primary_filename_documents_interpreter_matches_expected() -> TestResult<()> {
    harness::check_interpreter(&COHORT)
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
        Err(engine::PrimaryDocumentRunError::Acquisition(PrimaryAcquisitionError::Filename(
            engine::PrimaryFilenameEvaluationError::Plan(error)
        ))) if error == unsupported
    ));
    assert!(host.calls.borrow().is_empty());
    Ok(())
}

#[test]
fn primary_filename_documents_emit_their_plan_deterministically() -> TestResult<()> {
    harness::check_emission(&COHORT)
}

#[test]
fn primary_filename_documents_cli_generation_matches_the_emitters() -> TestResult<()> {
    harness::check_cli_generation(&COHORT)?;
    // An ordinary project is refused by the document profile, and a document
    // project by every ordinary profile.
    let directory = TempDir::new("primary_filename_documents_refused")?;
    let ordinary = directory.0.join("ordinary.json");
    std::fs::write(
        &ordinary,
        mapping::project_file::encode_pretty(&csv_project())?,
    )?;
    let saved = directory.0.join("document.json");
    std::fs::write(
        &saved,
        document_project_file::encode_pretty(&projects::document(projects::MODELS[0]))?,
    )?;
    for (project, flag) in [
        (&ordinary, Some("--primary-documents")),
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
    let document = projects::document(projects::MODELS[COHORT.configured_model]);
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
            harness::CSHARP_IDENTITY[0],
            harness::CSHARP_IDENTITY[1],
            harness::CSHARP_IDENTITY[2],
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
    assert!(
        companion.starts_with(
            "namespace Acme.Documents;\n\npublic static partial class DocumentMapping\n"
        )
    );
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

    let options = harness::rust_options(&COHORT, 0)?;
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

#[test]
#[ignore = "builds one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn primary_filename_documents_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    harness::check_generated_hosts(&COHORT)
}
