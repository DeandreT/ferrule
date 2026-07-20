use super::*;

#[test]
fn lowers_collection_find_with_only_its_context_expressions() {
    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            scalar("First"),
            scalar("NestedValue"),
            SchemaNode::group(
                "Departments",
                vec![SchemaNode::group("People", vec![scalar("Name")]).repeating()],
            )
            .repeating(),
        ],
    );
    project.graph.nodes.extend([
        (
            40,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            41,
            Node::SourceField {
                path: vec!["Name".into()],
                frame: Some(vec!["Departments".into(), "People".into()]),
            },
        ),
        (
            42,
            Node::CollectionFind {
                collection: vec!["Departments".into(), "People".into()],
                predicate: 40,
                value: 41,
            },
        ),
        (
            99,
            Node::Const {
                value: Value::String("unreachable".into()),
            },
        ),
    ]);
    project.root.bindings[1].node = 42;

    let program = lower(&project).expect("collection-find is portable");

    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        vec![10, 30, 40, 41, 42]
    );
    assert!(matches!(
        program.expressions.last().map(|node| &node.expression),
        Some(Expression::CollectionFind {
            collection,
            predicate: 40,
            value: 41,
        }) if collection == &["Departments", "People"]
    ));
}
