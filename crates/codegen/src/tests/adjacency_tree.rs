use super::*;

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Types",
        vec![
            scalar("Root"),
            SchemaNode::group("Rows", vec![scalar("Key"), scalar("Parent")]).repeating(),
        ],
    )
}

fn target_schema() -> SchemaNode {
    SchemaNode::group(
        "Type",
        vec![
            scalar("name"),
            SchemaNode::recursive_group("children", "Type").repeating(),
        ],
    )
}

#[test]
fn lowers_adjacency_tree_and_keeps_the_optional_root_reachable() {
    let Some(plan) = mapping::AdjacencyTreePlan::new(
        vec!["Rows".into()],
        vec!["Key".into()],
        vec!["Parent".into()],
        "name".into(),
        "children".into(),
        Some(7),
    ) else {
        panic!("valid adjacency-tree plan");
    };
    let mut project = supported_project();
    project.source = source_schema();
    project.target = target_schema();
    project.graph = Graph {
        nodes: BTreeMap::from([(
            7,
            Node::SourceField {
                path: vec!["Root".into()],
                frame: None,
            },
        )]),
    };
    project.root = Scope {
        construction: ScopeConstruction::AdjacencyTree { plan },
        ..Scope::default()
    };

    let Ok(program) = lower(&project) else {
        panic!("adjacency-tree construction lowers");
    };
    assert_eq!(
        program.root.construction,
        crate::TargetConstruction::AdjacencyTree {
            collection: vec!["Rows".into()],
            key: vec!["Key".into()],
            parent: vec!["Parent".into()],
            target_key: "name".into(),
            target_children: "children".into(),
            root: Some(7),
        }
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        [7]
    );
}
