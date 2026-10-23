use codegen::{Expression, ProgramValidationError};

#[path = "../../../conformance/document-clocks/project.rs"]
mod base;
#[path = "../../../conformance/distinct-text-map/project.rs"]
mod fixture;

#[test]
fn distinct_text_paths_are_checked_for_direct_programs() {
    let mut program = codegen::lower(&fixture::project(base::project())).unwrap();
    codegen::validate_document_clocks(&program, None).unwrap();
    let node = program
        .expressions
        .iter_mut()
        .find(|node| node.id == 50)
        .unwrap();
    let Expression::DistinctTextMapLast { key, .. } = &mut node.expression else {
        panic!("distinct node")
    };
    *key = vec!["Fail".into()];
    assert!(matches!(
        codegen::validate_program(&program),
        Err(ProgramValidationError::InvalidDistinctTextKey { node: 50, .. })
    ));
}

#[test]
fn the_owned_profile_does_not_inherit_distinct_text_mapping() {
    let mut project = fixture::project(base::project());
    project.root.iteration = mapping::ScopeIteration::None;
    project.source_options = Default::default();
    project.target_options = Default::default();
    project.graph.nodes = std::collections::BTreeMap::from([
        (
            50,
            mapping::Node::DistinctTextMapLast {
                collection: vec!["Rows".into()],
                key: vec!["Key".into()],
                expression: 17,
            },
        ),
        (
            17,
            mapping::Node::Const {
                value: ir::Value::String("value".into()),
            },
        ),
    ]);
    let program = codegen::lower(&project).unwrap();
    assert!(codegen::validate_basic_owned_emission(&program).is_err());
    let mut plain = program.clone();
    plain
        .expressions
        .iter_mut()
        .find(|node| node.id == 50)
        .unwrap()
        .expression = Expression::Const {
        value: ir::Value::String("value".into()),
    };
    codegen::validate_basic_owned_emission(&plain).unwrap();
    // An unused special producer must not become an accidental capability.
    let mut unused = program;
    unused.root.bindings[0].expression = 17;
    assert!(codegen::validate_basic_owned_emission(&unused).is_err());
}
