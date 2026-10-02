use ir::{Instance, ScalarType, SchemaNode};
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
fn exact_copy_keeps_attribute_absence_empty_value_and_required_use_metadata() {
    let p = project();
    assert!(engine::validate(&p).is_empty());
    for text in ["<Root/>", r#"<Root Code=""/>"#, r#"<Root Code="x"/>"#] {
        let input = format_xml::from_str(text, &p.source).unwrap();
        let result = engine::run(&p, &input).unwrap();
        assert_eq!(result, input);
        assert_eq!(
            format_xml::from_str(
                &format_xml::to_string(&p.target, &result).unwrap(),
                &p.target
            )
            .unwrap(),
            input
        );
    }
    let input = Instance::Group((vec![]).into());
    assert_eq!(engine::run(&p, &input).unwrap(), input);
}
#[test]
fn copy_rejects_different_nested_attribute_use_profiles() {
    let mut p = project();
    if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
        children[0].xml_attribute_required = false;
    }
    assert!(engine::validate(&p).iter().any(|issue| {
        issue
            .message
            .contains("matching source and target group fields")
    }));
    p.target = p.source.clone();
    assert!(engine::validate(&p).is_empty());
}
#[test]
fn malformed_required_attribute_metadata_is_a_schema_diagnostic() {
    let mut p = project();
    p.target.xml_attribute_required = true;
    assert!(engine::validate(&p).iter().any(|issue| {
        issue
            .message
            .contains("required XML attribute-use metadata")
    }));
}
