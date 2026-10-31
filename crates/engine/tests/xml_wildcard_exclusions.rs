use ir::{
    ScalarType, SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD,
    XML_NAMESPACE_URI_FIELD, XmlExpandedName, XmlNamespace, XmlWildcardExcludedNames,
    XmlWildcardNamespaceConstraint,
};
use mapping::{Graph, Project, Scope, ScopeConstruction};

const ISSUE: &str = "XML wildcard excluded names";

fn names() -> XmlWildcardExcludedNames {
    XmlWildcardExcludedNames::new([XmlExpandedName::new(
        XmlNamespace::qualified("urn:ferrule:wx:a").unwrap(),
        "Secret",
    )
    .unwrap()])
    .unwrap()
}

fn wildcard(constraint: XmlWildcardNamespaceConstraint) -> SchemaNode {
    SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
        ],
    )
    .repeating()
    .with_xml_wildcard_namespace(constraint)
    .unwrap()
}

fn project(child: SchemaNode) -> Project {
    let schema = SchemaNode::group("Root", vec![child]);
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

fn reports_issue(project: &Project) -> bool {
    engine::validate(project)
        .iter()
        .any(|issue| issue.message.contains(ISSUE))
}

#[test]
fn valid_exclusions_produce_no_issue() {
    let valid = wildcard(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(names())
        .unwrap();
    let issues = engine::validate(&project(valid));
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn misplaced_or_disallowed_exclusions_are_schema_issues() {
    let mut ordinary = SchemaNode::group(
        "Ordinary",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    ordinary.xml_wildcard_excluded_names = Some(names());

    let mut unconstrained = wildcard(XmlWildcardNamespaceConstraint::Any);
    unconstrained.xml_wildcard_namespace = None;
    unconstrained.xml_wildcard_excluded_names = Some(names());

    let mut disallowed =
        wildcard(XmlWildcardNamespaceConstraint::list([XmlNamespace::Unqualified]).unwrap());
    disallowed.xml_wildcard_excluded_names = Some(names());

    let mut scalar = SchemaNode::scalar("Leaf", ScalarType::String);
    scalar.xml_wildcard_excluded_names = Some(names());

    for child in [ordinary, unconstrained, disallowed, scalar] {
        let name = child.name.clone();
        let project = project(child);
        assert!(
            reports_issue(&project),
            "{name}: {:?}",
            engine::validate(&project)
        );
    }

    // The issue names the nested path of the offending node.
    let mut nested = project(SchemaNode::scalar("Value", ScalarType::String));
    let mut inner = wildcard(XmlWildcardNamespaceConstraint::Any);
    inner.xml_wildcard_namespace = None;
    inner.xml_wildcard_excluded_names = Some(names());
    if let SchemaKind::Group { children, .. } = &mut nested.target.kind {
        children.push(SchemaNode::group("Inner", vec![inner]));
    }
    assert!(engine::validate(&nested).iter().any(|issue| {
        issue.message.contains(ISSUE) && issue.message.contains("Inner/element()")
    }));
}

fn group(fields: Vec<(&str, ir::Instance)>) -> ir::Instance {
    ir::Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> ir::Instance {
    ir::Instance::Scalar(ir::Value::String(value.into()))
}

/// `XmlSerialize` uses the native XML writer, which refuses an excluded name
/// rather than serializing a wider wildcard.
#[test]
fn xml_serialize_refuses_excluded_names() {
    let item = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Head", ScalarType::String),
            wildcard(XmlWildcardNamespaceConstraint::Any)
                .with_xml_wildcard_excluded_names(names())
                .unwrap(),
        ],
    );
    let project = Project {
        source: SchemaNode::group("Root", vec![item.clone()]),
        target: SchemaNode::group("Out", vec![SchemaNode::scalar("Xml", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                1,
                mapping::Node::XmlSerialize {
                    path: vec!["Item".into()],
                    frame: None,
                    schema: Box::new(item),
                    declaration: false,
                    indent: false,
                    namespace: None,
                },
            )]
            .into(),
        },
        root: Scope {
            bindings: vec![mapping::Binding {
                target_field: "Xml".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    let issues = engine::validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    let input = group(vec![(
        "Item",
        group(vec![
            ("Head", text("h")),
            (
                XML_ELEMENTS_FIELD,
                ir::Instance::Repeated(vec![group(vec![
                    (XML_LOCAL_NAME_FIELD, text("Secret")),
                    (XML_NAMESPACE_URI_FIELD, text("urn:ferrule:wx:a")),
                ])]),
            ),
        ]),
    )]);
    let error = engine::run(&project, &input).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("excludes element `Secret` in namespace `urn:ferrule:wx:a`"),
        "{error}"
    );
}
