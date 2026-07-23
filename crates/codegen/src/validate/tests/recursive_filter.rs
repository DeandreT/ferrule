use super::*;

fn directory_schema() -> SchemaNode {
    SchemaNode::group(
        "Directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", "Directory").repeating(),
        ],
    )
}

fn recursive_program() -> Program {
    let mut program = program();
    program.source = directory_schema();
    program.target = directory_schema();
    program.expressions = vec![ExpressionNode {
        id: 7,
        expression: Expression::Const {
            value: Value::Bool(true),
        },
    }];
    program.root.bindings.clear();
    program.root.construction = TargetConstruction::RecursiveFilter {
        children: "directory".into(),
        items: "file".into(),
        predicate: 7,
    };
    program
}

#[test]
fn validates_recursive_filter_shape_and_predicate() {
    assert_eq!(validate_program(&recursive_program()), Ok(()));

    let mut missing = recursive_program();
    missing.expressions.clear();
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingRecursiveFilterPredicate {
            target_path: Vec::new(),
            expression: 7,
        })
    );

    let mut invalid_children = recursive_program();
    invalid_children.root.construction = TargetConstruction::RecursiveFilter {
        children: "missing".into(),
        items: "file".into(),
        predicate: 7,
    };
    assert_eq!(
        validate_program(&invalid_children),
        Err(ProgramValidationError::InvalidRecursiveFilterChildren {
            target_path: Vec::new(),
            field: "missing".into(),
        })
    );
}

#[test]
fn rejects_recursive_filter_content_and_mismatched_targets() {
    let mut content = recursive_program();
    content.root.bindings.push(Binding {
        target_field: "name".into(),
        expression: 7,
        target_type: ScalarType::String,
        repeating: false,
    });
    assert_eq!(
        validate_program(&content),
        Err(
            ProgramValidationError::RecursiveFilterConstructionHasContent {
                target_path: Vec::new(),
            }
        )
    );

    let mut mismatch = recursive_program();
    mismatch.target = SchemaNode::group(
        "Directory",
        vec![SchemaNode::scalar("name", ScalarType::String)],
    );
    assert_eq!(
        validate_program(&mismatch),
        Err(
            ProgramValidationError::RecursiveFilterConstructionRequiresMatchingGroups {
                target_path: Vec::new(),
            }
        )
    );
}
