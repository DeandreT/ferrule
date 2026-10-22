use codegen::{CsvOutputError, CsvOutputPolicy, IterationPlan, ProgramValidationError};

#[path = "../../../conformance/document-rows/project.rs"]
mod fixture;

#[test]
fn document_sets_have_a_distinct_explicit_csv_admission() {
    let program = codegen::lower(&fixture::project()).unwrap();
    let policy = CsvOutputPolicy::default();
    assert!(matches!(
        codegen::validate_csv_output(&program, &policy),
        Err(CsvOutputError::DynamicDocuments)
    ));
    codegen::validate_csv_document_set_output(&program, &policy).unwrap();

    let mut single = program.clone();
    single.root = single.root.children.remove(0);
    single.root.iteration = Some(IterationPlan::source(vec!["Rows".into()]));
    codegen::validate_csv_output(&single, &policy).unwrap();
    assert!(matches!(
        codegen::validate_csv_document_set_output(&single, &policy),
        Err(CsvOutputError::DocumentRows)
    ));
}

#[test]
fn malformed_document_body_is_rejected_before_adapter_admission() {
    let program = codegen::lower(&fixture::project()).unwrap();
    for case in 0..3 {
        let mut candidate = program.clone();
        match case {
            0 => candidate.root.children.clear(),
            1 => candidate.root.children[0].target_field = "invented".into(),
            _ => candidate.root.children[0].iteration = None,
        }
        assert!(matches!(
            codegen::validate_csv_document_set_output(&candidate, &CsvOutputPolicy::default()),
            Err(CsvOutputError::InvalidProgram(
                ProgramValidationError::InvalidDocumentRowsConstruction { .. }
            ))
        ));
    }
}

#[test]
fn document_set_adapter_keeps_existing_csv_dialect_refusals() {
    let program = codegen::lower(&fixture::project()).unwrap();
    let policy = CsvOutputPolicy {
        raw_delimited_crlf: true,
        ..Default::default()
    };
    assert!(matches!(
        codegen::validate_csv_document_set_output(&program, &policy),
        Err(CsvOutputError::RawDelimitedRequiresDisabledQuotes)
    ));
}

#[test]
fn document_set_adapter_refuses_inputs_not_exposed_by_its_entry_points() {
    let mut project = fixture::project();
    project.extra_sources.push(mapping::NamedSource {
        name: "unavailable".into(),
        path: "extra.json".into(),
        schema: ir::SchemaNode::group("Extra", vec![]),
        options: Default::default(),
        dynamic_path: None,
    });
    let program = codegen::lower(&project).unwrap();
    assert!(matches!(
        codegen::validate_csv_document_set_output(&program, &CsvOutputPolicy::default()),
        Err(CsvOutputError::NamedInputs)
    ));
}
