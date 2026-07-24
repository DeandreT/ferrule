use super::*;

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Types",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Parent", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}

fn target_schema() -> SchemaNode {
    SchemaNode::group(
        "Type",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::recursive_group("children", "Type").repeating(),
        ],
    )
}

fn adjacency_program() -> Program {
    let mut program = program();
    program.source = source_schema();
    program.target = target_schema();
    program.expressions.clear();
    program.root.bindings.clear();
    program.root.construction = TargetConstruction::AdjacencyTree {
        collection: vec!["Rows".into()],
        key: vec!["Key".into()],
        parent: vec!["Parent".into()],
        target_key: "name".into(),
        target_children: "children".into(),
        root: None,
    };
    program
}

#[test]
fn validates_adjacency_tree_source_and_target_shape() {
    assert_eq!(validate_program(&adjacency_program()), Ok(()));

    let mut key = adjacency_program();
    key.source = SchemaNode::group(
        "Types",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::Int),
                    SchemaNode::scalar("Parent", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    assert_eq!(
        validate_program(&key),
        Err(ProgramValidationError::InvalidAdjacencyTreeField {
            target_path: Vec::new(),
            role: "key",
            path: vec!["Key".into()],
        })
    );

    let mut target = adjacency_program();
    target.target = SchemaNode::group(
        "Type",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group("children", Vec::new()).repeating(),
        ],
    );
    assert_eq!(
        validate_program(&target),
        Err(ProgramValidationError::InvalidAdjacencyTreeTargetChildren {
            target_path: Vec::new(),
            field: "children".into(),
        })
    );
}

#[test]
fn validates_adjacency_tree_root_and_content() {
    let mut missing = adjacency_program();
    missing.root.construction = TargetConstruction::AdjacencyTree {
        collection: vec!["Rows".into()],
        key: vec!["Key".into()],
        parent: vec!["Parent".into()],
        target_key: "name".into(),
        target_children: "children".into(),
        root: Some(7),
    };
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingAdjacencyTreeRoot {
            target_path: Vec::new(),
            expression: 7,
        })
    );

    let mut content = adjacency_program();
    content.root.children.push(TargetScope {
        target_field: "children".into(),
        repeating: true,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    });
    assert_eq!(
        validate_program(&content),
        Err(
            ProgramValidationError::AdjacencyTreeConstructionHasContent {
                target_path: Vec::new(),
            }
        )
    );
}
