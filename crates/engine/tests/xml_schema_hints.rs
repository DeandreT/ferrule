use ir::{ScalarType, SchemaNode, XmlSchemaHints};
use mapping::{FormatOptions, Graph, NamedSource, NamedTarget, Project, Scope};
fn options() -> FormatOptions {
    FormatOptions {
        xml_document: true,
        xml_schema_hints: Some(XmlSchemaHints {
            no_namespace_location: Some("root.xsd".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}
fn project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group("Root", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: options(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}
fn hint_issues(project: &Project) -> Vec<engine::ValidationIssue> {
    engine::validate(project)
        .into_iter()
        .filter(|i| i.message.contains("schema") && i.location.contains("format options"))
        .collect()
}
#[test]
fn primary_and_named_xml_targets_accept_with_owned_validation() {
    let mut p = project();
    assert!(engine::validate(&p).is_empty());
    p.extra_targets.push(NamedTarget {
        name: "other".into(),
        path: Some("other.xml".into()),
        schema: SchemaNode::group("Other", vec![]),
        options: options(),
        root: Scope::default(),
    });
    assert!(engine::validate(&p).is_empty());
    p.extra_targets[0].options.xml_document = false;
    let issues = hint_issues(&p);
    assert_eq!(issues.len(), 1);
    assert!(format!("{:?}", issues[0].owner).contains("other"));
}
#[test]
fn primary_and_disconnected_named_sources_refuse() {
    let mut p = project();
    p.source_options = options();
    let issues = hint_issues(&p);
    assert_eq!(issues.len(), 1);
    assert!(format!("{:?}", issues[0].owner).contains("Source"));
    p.source_options = Default::default();
    p.extra_sources.push(NamedSource {
        name: "unused".into(),
        path: "unused.xml".into(),
        schema: SchemaNode::scalar("Unused", ScalarType::String),
        options: options(),
        dynamic_path: None,
    });
    let issues = hint_issues(&p);
    assert_eq!(issues.len(), 1);
    assert!(format!("{:?}", issues[0].owner).contains("unused"));
}
#[test]
fn non_xml_conflicting_and_invalid_metadata_refuse() {
    let base = project();
    for mutation in [0, 1, 2, 3, 4] {
        let mut p = base.clone();
        match mutation {
            0 => p.target_options.xml_document = false,
            1 => p.target_options.json_document = true,
            2 => p.target_options.local_xml_file_set = true,
            3 => p.target_options.xml_allow_inactive_root_type_members = true,
            _ => {
                p.target_options
                    .xml_schema_hints
                    .as_mut()
                    .unwrap()
                    .no_namespace_location = Some("a b.xsd".into())
            }
        }
        assert!(!hint_issues(&p).is_empty());
    }
}
