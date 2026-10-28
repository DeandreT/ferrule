//! The CSV document list is distinct from the XML and singular JSON routes.

use super::*;
use serde_json::{Value as Json, json};

pub(super) fn run_case(
    project: &Project,
    source: &Instance,
    extras: &[(String, Instance)],
    mapping_path: &Path,
    case: &Path,
    rust_target: &Path,
) -> TestResult<()> {
    if !project.runtime_dependencies().is_empty()
        || !extras.is_empty()
        || !project.extra_sources.is_empty()
        || !project.extra_targets.is_empty()
    {
        return Err(
            "CSV document corpus route requires one dependency-complete primary source".into(),
        );
    }
    let program = codegen::lower(project)?;
    admission::check_document_program(&program)?;
    let policy = codegen::CsvOutputPolicy::from_format_options(&project.target_options)?;
    codegen::validate_csv_document_output(&program, &policy)?;
    // The existing strict transport comparison must preserve every source row.
    // This is a host-owned typed input, not a newly inferred generated JSON API.
    let (input, restored) = admission::source_document(&project.source, source, 0)?;
    let execution = engine::ExecutionContext::new(mapping_path);
    let original = engine::run_with_context(project, source, &execution)?;
    assert_eq!(original, engine::run(project, source)?);
    assert_eq!(
        original,
        engine::run_with_context(project, &restored, &execution)?
    );
    let Instance::DocumentSet(members) = &original else {
        return Err("CSV document set required".into());
    };
    let text = format_csv::to_document_text(
        &project.target,
        &original,
        &format_csv::CsvWriteOptions::from(&project.target_options),
    )?;
    let expected = members.iter().zip(text).map(|(member, text)| {
        assert_eq!(member.path(), text.path);
        let value = admission::target_document(&project.target, member.value(), 0)?;
        Ok(json!({"path":member.path(),"value":serde_json::from_str::<Json>(&value)?,"text":text.text}))
    }).collect::<TestResult<Vec<_>>>()?;
    let expected = Json::Array(expected);
    let path = case.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(project)?)?;
    std::fs::write(case.join("source.json"), input)?;
    std::fs::write(
        case.join("source-schema.json"),
        codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        )?,
    )?;
    std::fs::write(
        case.join("target-schema.json"),
        codegen::serialize_embedded_schema(
            &project.target,
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        )?,
    )?;
    std::fs::write(
        case.join("documents.expected.json"),
        serde_json::to_vec_pretty(&expected)?,
    )?;
    let inputs = [(String::new(), case.to_path_buf())];
    let rust = super::run_rust_host_with_profile(
        &path,
        &inputs,
        case,
        rust_target,
        include_str!("csv_document_outputs_rust.rs.txt"),
        true,
        cli::GenerateProfile::CsvDocumentOutput,
    )?;
    let csharp = super::run_csharp_host_with_profile(
        &path,
        &inputs,
        case,
        include_str!("csv_document_outputs_csharp.cs.txt"),
        cli::GenerateProfile::CsvDocumentOutput,
    )?;
    for (name, output) in [("rust", rust), ("csharp", csharp)] {
        std::fs::write(
            case.join(format!("{name}-documents.actual.json")),
            &output.stdout,
        )?;
        let actual: Json = serde_json::from_slice(&output.stdout)?;
        assert!(
            envelope::ordered_json_equal(&actual.to_string(), &expected.to_string()),
            "{name} complete CSV documents differ"
        );
    }
    Ok(())
}

#[test]
fn csv_document_corpus_transport_preserves_authored_rows_and_paths() -> TestResult<()> {
    let directory = TempDir::new("csv-document-corpus")?;
    std::fs::write(
        directory.0.join("input.mfd"),
        include_str!("../../../../mfd/tests/fixtures/csv_document_output/grouped.mfd"),
    )?;
    let imported = mfd::import(&directory.0.join("input.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = imported.project;
    let source = Instance::Repeated(format_csv::from_str(
        "Code,Note\nA,one\nB,two\nC,three\n",
        &project.source,
        Some(','),
        true,
    )?);
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.0.join("target"));
    run_case(
        &project,
        &source,
        &[],
        &directory.0.join("input.mfd"),
        &directory.0,
        &target,
    )
}
