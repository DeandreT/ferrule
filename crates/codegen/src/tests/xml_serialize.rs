use super::*;
use crate::ExpressionNode;

#[test]
fn lowers_xml_serialization_with_exact_schema_and_document_policy() {
    let item = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("id", ScalarType::String).attribute(),
            SchemaNode::scalar("Name", ScalarType::String),
        ],
    );
    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group("Rows", vec![item.clone()]).repeating()],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Xml", ScalarType::String).repeating()],
    );
    project.graph.nodes = BTreeMap::from([(
        1,
        Node::XmlSerialize {
            path: vec!["Item".into()],
            frame: Some(vec!["Rows".into()]),
            schema: item.clone(),
            declaration: true,
            indent: false,
            namespace: Some("urn:ferrule:test".into()),
        },
    )]);
    project.root = Scope {
        children: vec![Scope {
            target_field: "Xml".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            construction: ScopeConstruction::Scalar { value: 1 },
            ..Scope::default()
        }],
        ..Scope::default()
    };

    let program = lower(&project).expect("XML serialization lowers");
    assert_eq!(
        program.expressions,
        vec![ExpressionNode {
            id: 1,
            expression: Expression::XmlSerialize {
                frame: Some(vec!["Rows".into()]),
                path: vec!["Item".into()],
                schema: item,
                declaration: true,
                indent: false,
                namespace: Some("urn:ferrule:test".into()),
            },
        }]
    );
}
