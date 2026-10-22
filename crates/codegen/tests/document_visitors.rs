use codegen::{DocumentVisitorBoundaryError, validate_document_visitor};

#[path = "../../../conformance/document-body-clocks/project.rs"]
mod body_clocks;
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

#[test]
fn body_clock_admission_is_explicit_and_rejects_unqualified_graphs() {
    let mut project = body_clocks::project(clocks::project());
    codegen::validate_document_clocks(&codegen::lower(&project).unwrap(), None).unwrap();
    let named = body_clocks::named(project.clone());
    codegen::validate_document_clocks(&codegen::lower(&named).unwrap(), Some("selected")).unwrap();
    project
        .graph
        .nodes
        .insert(2, mapping::Node::Raise { message: None });
    let program = codegen::lower(&project).unwrap();
    codegen::validate_document_filename_clock(&program, None).unwrap();
    assert!(matches!(
        codegen::validate_document_clocks(&program, None),
        Err(DocumentVisitorBoundaryError::Unsupported(_))
    ));
}

#[test]
fn body_clock_udf_routes_remain_closed_even_when_the_filename_is_ordinary() {
    let mut project = body_clocks::project(clocks::project());
    let id = mapping::FunctionId::new(1);
    project.user_functions.insert(
        id,
        mapping::UserFunction {
            library: "fixture".into(),
            name: "body".into(),
            description: None,
            parameters: vec![],
            output_type: ir::ScalarType::String,
            output_name: "out".into(),
            output: 0,
            body: mapping::Graph {
                nodes: [(
                    0,
                    mapping::Node::Const {
                        value: ir::Value::String("body".into()),
                    },
                )]
                .into(),
            },
        },
    );
    project.graph.nodes.insert(
        23,
        mapping::Node::UserFunctionCall {
            function: id,
            args: vec![],
        },
    );
    project.root.bindings[0].node = 23;
    let program = codegen::lower(&project).unwrap();
    codegen::validate_document_filename_clock(&program, None).unwrap();
    assert!(matches!(
        codegen::validate_document_clocks(&program, None),
        Err(DocumentVisitorBoundaryError::Unsupported(_))
    ));
}
