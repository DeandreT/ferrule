use codegen::{DocumentVisitorBoundaryError, validate_document_visitor};

#[path = "../../../conformance/document-clocks/project.rs"]
mod clocks;
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

#[test]
fn filename_clock_admission_is_separate_from_stable_clock_visitors() {
    let mut project = clocks::project();
    let program = codegen::lower(&project).unwrap();
    codegen::validate_document_filename_clock(&program, None).unwrap();
    project
        .graph
        .nodes
        .insert(20, mapping::Node::Raise { message: None });
    let program = codegen::lower(&project).unwrap();
    codegen::validate_document_visitor(&program, None).unwrap();
    assert!(matches!(
        codegen::validate_document_filename_clock(&program, None),
        Err(DocumentVisitorBoundaryError::Unsupported(_))
    ));
}

#[test]
fn filename_clock_udf_routes_remain_closed() {
    let mut project = clocks::project();
    let id = mapping::FunctionId::new(1);
    project.user_functions.insert(
        id,
        mapping::UserFunction {
            library: "fixture".into(),
            name: "name".into(),
            description: None,
            parameters: vec![],
            output_type: ir::ScalarType::String,
            output_name: "out".into(),
            output: 0,
            body: mapping::Graph {
                nodes: [(
                    0,
                    mapping::Node::Const {
                        value: ir::Value::String("fixed.xml".into()),
                    },
                )]
                .into_iter()
                .collect(),
            },
        },
    );
    project.graph.nodes.insert(
        20,
        mapping::Node::UserFunctionCall {
            function: id,
            args: vec![],
        },
    );
    let program = codegen::lower(&project).unwrap();
    codegen::validate_document_visitor(&program, None).unwrap();
    assert!(matches!(
        codegen::validate_document_filename_clock(&program, None),
        Err(DocumentVisitorBoundaryError::Unsupported(_))
    ));
}
