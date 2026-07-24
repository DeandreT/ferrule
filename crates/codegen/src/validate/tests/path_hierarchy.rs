use super::*;

fn target_schema() -> SchemaNode {
    SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    )
}

fn path_program() -> Program {
    let mut program = program();
    program.source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    program.target = target_schema();
    program.expressions.clear();
    program.root.bindings.clear();
    program.root.construction = TargetConstruction::PathHierarchy {
        collection: vec!["File".into()],
        separator: "/".into(),
        directories: "directory".into(),
        files: "file".into(),
        name: "name".into(),
    };
    program
}

#[test]
fn validates_path_hierarchy_source_and_target_shape() {
    assert_eq!(validate_program(&path_program()), Ok(()));

    let mut collection = path_program();
    collection.source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String)],
    );
    assert_eq!(
        validate_program(&collection),
        Err(ProgramValidationError::InvalidPathHierarchyCollection {
            target_path: Vec::new(),
            collection: vec!["File".into()],
        })
    );

    let mut target = path_program();
    target.target = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::group("directory", Vec::new()).repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    );
    assert_eq!(
        validate_program(&target),
        Err(ProgramValidationError::InvalidPathHierarchyDirectories {
            target_path: Vec::new(),
            field: "directory".into(),
        })
    );
}

#[test]
fn rejects_path_hierarchy_content_and_iteration() {
    let mut content = path_program();
    content.root.children.push(TargetScope {
        target_field: "directory".into(),
        repeating: true,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    });
    assert_eq!(
        validate_program(&content),
        Err(
            ProgramValidationError::PathHierarchyConstructionHasContent {
                target_path: Vec::new(),
            }
        )
    );

    let mut iteration = path_program();
    iteration.root.iteration = Some(IterationPlan::source(vec!["File".into()]));
    assert_eq!(
        validate_program(&iteration),
        Err(
            ProgramValidationError::PathHierarchyConstructionHasIteration {
                target_path: Vec::new(),
            }
        )
    );
}
