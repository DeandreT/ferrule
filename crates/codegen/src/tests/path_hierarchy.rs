use super::*;

fn target_schema() -> SchemaNode {
    SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![scalar("name")]).repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
            scalar("name"),
        ],
    )
}

#[test]
fn lowers_path_hierarchy_without_graph_dependencies() {
    let Some(plan) = mapping::PathHierarchyPlan::new(
        vec!["File".into()],
        "/".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    ) else {
        panic!("valid path-hierarchy plan");
    };
    let mut project = supported_project();
    project.source = SchemaNode::group("Files", vec![scalar("File").repeating()]);
    project.target = target_schema();
    project.graph = Graph::default();
    project.root = Scope {
        construction: ScopeConstruction::PathHierarchy { plan },
        ..Scope::default()
    };

    let Ok(program) = lower(&project) else {
        panic!("path-hierarchy construction lowers");
    };
    assert_eq!(
        program.root.construction,
        crate::TargetConstruction::PathHierarchy {
            collection: vec!["File".into()],
            separator: "/".into(),
            directories: "directory".into(),
            files: "file".into(),
            name: "name".into(),
        }
    );
    assert!(program.expressions.is_empty());
}
