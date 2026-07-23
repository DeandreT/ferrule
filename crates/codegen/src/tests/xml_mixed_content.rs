use super::*;

#[test]
fn lowers_ordered_xml_mixed_content_and_replacement_dependencies() {
    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group(
            "Content",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::group("Em", vec![SchemaNode::scalar("Value", ScalarType::String)])
                    .repeating(),
            ],
        )],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Text", ScalarType::String)],
    );
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::XmlMixedContent {
                path: vec!["Content".into()],
                frame: None,
                replacements: vec![mapping::XmlMixedContentReplacement {
                    element: "Em".into(),
                    collection: vec!["Content".into(), "Em".into()],
                    expression: 1,
                }],
            },
        ),
    ]);
    project.root = Scope {
        bindings: vec![MappingBinding {
            target_field: "Text".into(),
            node: 2,
        }],
        ..Scope::default()
    };

    let program = lower(&project).expect("mixed-content expression lowers");
    let expression = program
        .expressions
        .iter()
        .find(|expression| expression.id == 2)
        .expect("mixed-content node remains reachable");
    assert!(matches!(
        &expression.expression,
        Expression::XmlMixedContent {
            path,
            frame: None,
            replacements,
        } if path == &["Content"]
            && replacements.len() == 1
            && replacements[0].element == "Em"
            && replacements[0].collection == ["Content", "Em"]
            && replacements[0].expression == 1
    ));
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
}
