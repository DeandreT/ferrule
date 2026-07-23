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

#[test]
fn lowers_recursive_filter_and_keeps_its_predicate_reachable() {
    let Some(plan) = mapping::RecursiveFilterPlan::new("directory".into(), "file".into(), 3) else {
        panic!("valid recursive-filter plan");
    };
    let mut project = supported_project();
    project.source = directory_schema();
    project.target = directory_schema();
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec!["name".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(".keep".into()),
            },
        ),
        (
            3,
            Node::Call {
                function: "contains".into(),
                args: vec![1, 2],
            },
        ),
    ]);
    project.root = Scope {
        construction: ScopeConstruction::RecursiveFilter { plan },
        ..Scope::default()
    };

    let Ok(program) = lower(&project) else {
        panic!("recursive-filter construction lowers");
    };
    assert_eq!(
        program.root.construction,
        crate::TargetConstruction::RecursiveFilter {
            children: "directory".into(),
            items: "file".into(),
            predicate: 3,
        }
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
}
