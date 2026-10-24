//! #998 primary XML filename documents: the interpreter, generated Rust and
//! generated C# read the selected XML documents as one document or document
//! set, map once and compare complete documents, typed failures and the
//! ordered host calls each case reached.
use super::primary_documents_harness::{self as harness, Cohort};
use super::*;

use mapping::{PrimaryRouteError, document_project_file};

#[path = "../../../../conformance/primary-xml-documents/projects.rs"]
mod projects;

fn emit(
    program: &codegen::DocumentProgram,
    language: &str,
    options: &codegen_rust::Options,
) -> TestResult<codegen::ArtifactSet> {
    Ok(if language == "rust" {
        codegen_rust::emit_with_primary_xml_documents(program, options)?
    } else {
        codegen_csharp::emit_with_primary_xml_documents(program)?
    })
}

const COHORT: Cohort = Cohort {
    tag: "primary-xml-documents",
    cases: include_str!("../../../../conformance/primary-xml-documents/cases.json"),
    models: &projects::MODELS,
    document: projects::document,
    emit,
    rust_fixture: include_str!("fixtures/primary_xml_documents_rust.rs.txt"),
    csharp_fixture: include_str!("fixtures/primary_xml_documents_csharp.cs.txt"),
    configured_model: 1,
};

#[test]
fn primary_xml_documents_cover_every_model_and_keep_their_saved_plans() -> TestResult<()> {
    harness::check_inventory(&COHORT)
}

#[test]
fn primary_xml_documents_interpreter_matches_expected() -> TestResult<()> {
    harness::check_interpreter(&COHORT)
}

#[test]
fn primary_xml_documents_emit_their_plan_deterministically() -> TestResult<()> {
    harness::check_emission(&COHORT)
}

#[test]
fn primary_xml_documents_cli_generation_matches_the_emitters() -> TestResult<()> {
    harness::check_cli_generation(&COHORT)
}

/// Shapes whose documents cannot form the plan's source are refused before
/// any host call, in the interpreter and at generation.
#[test]
fn primary_xml_documents_refuse_undefined_document_shapes() -> TestResult<()> {
    let direct = projects::document("literal_direct");
    let mut declared = direct.project().clone();
    declared.source_options.local_xml_file_set = true;
    let wildcard = projects::document("parameter_wildcard");
    let mut root_view = wildcard.project().clone();
    root_view.source_options.xml_root_view_read_policy = true;
    root_view
        .source_options
        .xml_allow_inactive_root_type_members = true;
    root_view.source_options.local_xml_file_set = false;
    let mut csv_target = direct.project().clone();
    csv_target.target_options.xml_document = false;
    csv_target.target_options.tabular_kind = Some(mapping::TabularBoundaryKind::Csv);
    for (project, plan, expected) in [
        (
            declared,
            direct.primary_source().clone(),
            PrimaryRouteError::DirectDocumentSet,
        ),
        (
            root_view,
            wildcard.primary_source().clone(),
            PrimaryRouteError::RootViewDocumentSet,
        ),
        (
            csv_target,
            direct.primary_source().clone(),
            PrimaryRouteError::TargetFormat {
                source: mapping::PrimaryDocumentFormat::Xml,
            },
        ),
    ] {
        let document = mapping::DocumentProject::new(project, plan);
        assert_eq!(document.primary_route(), Err(expected.clone()));
        let directory = TempDir::new("primary_xml_documents_refused")?;
        let host = harness::Recording {
            inner: format_csv::LocalPrimaryFiles::new(&directory.0)?,
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let result = cli::execute_primary_document_with_host(
            &document,
            Path::new("authored.mapping"),
            &host,
            1,
            None,
        );
        assert!(
            matches!(&result, Err(cli::PrimaryDocumentError::Route(error)) if *error == expected),
            "{result:?}"
        );
        assert!(host.calls.borrow().is_empty());
        // An observed root view needs a flat schema, so lowering this one
        // refuses before the route is consulted.
        let Ok(program) = codegen::lower_document(&document) else {
            assert_eq!(expected, PrimaryRouteError::RootViewDocumentSet);
            continue;
        };
        assert!(matches!(
            codegen_rust::emit_with_primary_xml_documents(
                &program,
                &harness::rust_options(&COHORT, 0)?
            ),
            Err(codegen_rust::PrimaryDocumentEmitError::Boundary(
                codegen::PrimaryDocumentBoundaryError::Route(error)
            )) if error == expected
        ));
        assert!(matches!(
            codegen_csharp::emit_with_primary_xml_documents(&program),
            Err(codegen_csharp::PrimaryDocumentEmitError::Boundary(
                codegen::PrimaryDocumentBoundaryError::Route(error)
            )) if error == expected
        ));
    }
    // The direct structured model is admitted; raw-delimited output is not.
    let program = codegen::lower_document(&projects::document("literal_direct"))?;
    assert!(matches!(
        codegen::validate_primary_xml_documents(&program),
        Ok(codegen::XmlDocumentInputs::Structured)
    ));
    let saved = document_project_file::encode_pretty(&projects::document("literal_direct"))?;
    let directory = TempDir::new("primary_xml_documents_raw")?;
    let path = directory.0.join("document.json");
    std::fs::write(&path, saved)?;
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["generate", "--primary-documents", "--raw-delimited-crlf"])
        .args(["--language", "csharp", "--project"])
        .arg(&path)
        .arg("--out")
        .arg(directory.0.join("out"))
        .output()?;
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("primary CSV document plan"));
    Ok(())
}

#[test]
fn primary_xml_documents_generators_apply_identity_and_manifest() -> TestResult<()> {
    let program = codegen::lower_document(&projects::document(
        projects::MODELS[COHORT.configured_model],
    ))?;
    let default = codegen_csharp::Options::default();
    assert_eq!(
        codegen_csharp::Generator::new(&default).emit_with_primary_xml_documents(&program)?,
        codegen_csharp::emit_with_primary_xml_documents(&program)?
    );
    let options = codegen_csharp::Options {
        identity: codegen_csharp::Identity::new(
            harness::CSHARP_IDENTITY[0],
            harness::CSHARP_IDENTITY[1],
            harness::CSHARP_IDENTITY[2],
        )?,
        manifest: codegen::ManifestSelection::Included,
    };
    let configured =
        codegen_csharp::Generator::new(&options).emit_with_primary_xml_documents(&program)?;
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
        codegen::GenerationProfile::PrimaryXmlDocument
    );
    assert!(
        file(&configured, "GeneratedMapping.PrimaryXmlDocument.cs")?.starts_with(
            "namespace Acme.Documents;\n\npublic static partial class DocumentMapping\n"
        )
    );
    let options = harness::rust_options(&COHORT, 0)?;
    let plain = codegen_rust::Generator::new(&options);
    assert_eq!(
        plain.emit_with_primary_xml_documents(&program)?,
        codegen_rust::emit_with_primary_xml_documents(&program, &options)?
    );
    let with = plain
        .with_manifest(codegen::ManifestSelection::Included)
        .emit_with_primary_xml_documents(&program)?;
    let manifest =
        codegen::CompatibilityManifest::from_json(&file(&with, "ferrule-manifest.json")?)?;
    assert_eq!(
        manifest.profile,
        codegen::GenerationProfile::PrimaryXmlDocument
    );
    Ok(())
}

#[test]
#[ignore = "builds one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn primary_xml_documents_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    harness::check_generated_hosts(&COHORT)
}
