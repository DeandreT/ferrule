use ir::{Instance, ScalarType, SchemaNode};
use mapping::{Graph, Project, Scope, ScopeConstruction};
fn schema() -> SchemaNode {
    let mut scalar = SchemaNode::scalar("Value", ScalarType::String);
    scalar.set_xml_optional(true);
    let mut group = SchemaNode::group("Maybe", vec![]);
    group.set_xml_optional(true);
    SchemaNode::group("Root", vec![scalar, group])
}
fn project() -> Project {
    let s = schema();
    Project {
        source: s.clone(),
        target: s,
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
fn whole_group_copy_preserves_optional_absence_empty_and_nil() {
    let mut p = project();
    p.source.child("Value").unwrap();
    if let ir::SchemaKind::Group { children, .. } = &mut p.source.kind {
        children[0].nillable = true;
    }
    p.target = p.source.clone();
    assert!(engine::validate(&p).is_empty());
    for xml in [
        "<Root/>",
        "<Root><Value/><Maybe/></Root>",
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Value xsi:nil="true"/></Root>"#,
    ] {
        let input = format_xml::from_str(xml, &p.source).unwrap();
        let out = engine::run(&p, &input).unwrap();
        assert_eq!(input, out);
        let xml = format_xml::to_string(&p.target, &out).unwrap();
        assert_eq!(format_xml::from_str(&xml, &p.target).unwrap(), input);
    }
    let input = Instance::Group(vec![]);
    assert_eq!(engine::run(&p, &input).unwrap(), input);
}
#[test]
fn programmatic_and_nested_invalid_optional_roles_have_diagnostics() {
    let mut p = project();
    p.target.repeating = true;
    p.target.xml_optional = true;
    assert!(
        engine::validate(&p)
            .iter()
            .any(|e| e.message.contains("XML optional-occurrence metadata"))
    );
    let mut child = SchemaNode::scalar("Value", ScalarType::String).attribute();
    child.xml_optional = true;
    p.target = SchemaNode::group("Root", vec![child]);
    assert!(
        engine::validate(&p)
            .iter()
            .any(|e| e.message.contains("XML optional-occurrence metadata"))
    );
}
#[test]
fn copy_shape_validation_retains_nested_occurrence_identity() {
    let mut p = project();
    if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
        children[0].xml_optional = false;
    }
    assert!(engine::validate(&p).iter().any(|issue| {
        issue
            .message
            .contains("matching source and target group fields")
    }));
    // The existing complete-shape contract observes nested occurrence metadata;
    // the runtime copy itself never rewrites the instance's presence.
    p.target = p.source.clone();
    p.source.xml_optional = true;
    assert!(engine::validate(&p).is_empty());
    let input = Instance::Group(vec![]);
    assert_eq!(engine::run(&p, &input).unwrap(), input);
}
