use codegen::{DocumentVisitorBoundaryError, validate_document_visitor};

#[path = "../../../conformance/document-rows/project.rs"]
mod fixture;

#[test]
fn selected_ordinary_document_target_is_explicit() {
    let mut project = fixture::project();
    let program = codegen::lower(&project).unwrap();
    assert_eq!(validate_document_visitor(&program, None).unwrap(), None);
    assert!(
        matches!(validate_document_visitor(&program, Some("absent")), Err(DocumentVisitorBoundaryError::UnknownTarget(name)) if name == "absent")
    );
    project.extra_targets.push(mapping::NamedTarget {
        name: "chosen".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    let program = codegen::lower(&project).unwrap();
    assert_eq!(
        validate_document_visitor(&program, Some("chosen")).unwrap(),
        Some(0)
    );
    assert!(matches!(
        validate_document_visitor(&program, Some("CHOSEN")),
        Err(DocumentVisitorBoundaryError::UnknownTarget(_))
    ));
}

#[test]
fn invalid_body_keeps_its_original_typed_error() {
    let mut program = codegen::lower(&fixture::project()).unwrap();
    program.root.children.clear();
    assert!(matches!(
        validate_document_visitor(&program, None),
        Err(DocumentVisitorBoundaryError::InvalidProgram(
            codegen::ProgramValidationError::InvalidDocumentRowsConstruction { .. }
        ))
    ));
}

#[test]
fn a_static_target_is_not_silently_wrapped_as_a_document_stream() {
    let mut project = fixture::project();
    let mut body = project.root.children.remove(0);
    body.iteration = mapping::ScopeIteration::Source(vec!["Rows".into()]);
    project.root = body;
    let program = codegen::lower(&project).unwrap();
    assert!(matches!(
        validate_document_visitor(&program, None),
        Err(DocumentVisitorBoundaryError::Unsupported(_))
    ));
}
