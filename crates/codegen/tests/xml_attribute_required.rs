use ir::{ScalarType, SchemaNode};
use mapping::{Graph, Project, Scope, ScopeConstruction};

fn project() -> Project {
    let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
    code.set_xml_attribute_required(true);
    let schema = SchemaNode::group("Root", vec![code]);
    Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

#[test]
fn lowered_copy_retains_required_attribute_schema_metadata() {
    let project = project();
    let lowered = codegen::lower(&project).unwrap();
    assert_eq!(lowered.source, project.source);
}

#[test]
fn lowered_copy_rejects_mismatched_attribute_use_profiles() {
    let mut project = project();
    if let ir::SchemaKind::Group { children, .. } = &mut project.target.kind {
        children[0].xml_attribute_required = false;
    }
    assert!(codegen::lower(&project).is_err());
    project.target = project.source.clone();
    assert!(codegen::lower(&project).is_ok());
}

#[test]
fn malformed_required_attribute_metadata_rejects_lowering() {
    let mut project = project();
    project.target.xml_attribute_required = true;
    assert!(codegen::lower(&project).is_err());
}
