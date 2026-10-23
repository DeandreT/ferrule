use mapping::{Node, PrimarySourcePlan};

#[path = "../../../conformance/mapforce-json5-documents/project.rs"]
mod fixture;

#[test]
fn row_paths_lower_without_xml_flags_and_roundtrip_without_path_changes() {
    let project = fixture::with_row_paths(fixture::project(false));
    let document = mapping::DocumentProject::new(project, PrimarySourcePlan { filename: 90 });
    let program = codegen::lower_document(&document).unwrap();
    assert!(program.program().expressions.iter().any(|node| node.id == 4
        && matches!(&node.expression,
        codegen::Expression::SourceRowDocumentPath { collection } if collection.is_empty())));
    assert!(codegen::validate_document_clocks(program.program(), None).is_ok());
    let node = Node::SourceRowDocumentPath {
        collection: vec!["Rows".into(), " exact/path ".into()],
    };
    let encoded = serde_json::to_string(&node).unwrap();
    let restored = serde_json::from_str::<Node>(&encoded).unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), encoded);
}

#[test]
fn nonexistent_and_non_group_paths_are_refused_in_direct_programs() {
    let project = fixture::with_row_paths(fixture::project(false));
    let mut program = codegen::lower(&project).unwrap();
    for collection in [vec!["Missing".into()], vec!["Qty".into()]] {
        program
            .expressions
            .iter_mut()
            .find(|node| node.id == 4)
            .unwrap()
            .expression = codegen::Expression::SourceRowDocumentPath { collection };
        assert!(matches!(
            codegen::validate_program(&program),
            Err(codegen::ProgramValidationError::InvalidSourceRowPath { node: 4, .. })
        ));
    }
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == 4)
        .unwrap()
        .expression = codegen::Expression::SourceRowDocumentPath { collection: vec![] };
    program.extra_sources.push(codegen::NamedSourceProgram {
        name: "other".into(),
        source: program.source.clone(),
        dynamic: None,
    });
    assert!(matches!(
        codegen::validate_program(&program),
        Err(codegen::ProgramValidationError::InvalidSourceRowPath { node: 4, .. })
    ));
}
