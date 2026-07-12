use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, Graph, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, Node, Project, Scope,
    ScopeIteration,
};

#[test]
fn export_warns_instead_of_bypassing_inner_join_semantics() {
    let join = JoinId::new(8);
    let plan = JoinPlan::new(
        JoinSource::new(vec!["Left".into()]),
        JoinSource::new(vec!["Right".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Left".into()],
            vec!["Id".into()],
            vec!["LeftId".into()],
        )),
    )
    .unwrap();
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Left", vec![SchemaNode::scalar("Id", ScalarType::Int)]).repeating(),
            SchemaNode::group(
                "Right",
                vec![
                    SchemaNode::scalar("LeftId", ScalarType::Int),
                    SchemaNode::scalar("Value", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                .repeating(),
        ],
    );
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::JoinField {
                        join,
                        collection: vec!["Right".into()],
                        path: vec!["Value".into()],
                    },
                ),
                (1, Node::JoinPosition { join }),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin { id: join, plan },
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let stem = format!("ferrule_join_export_{}", std::process::id());
    let output = std::env::temp_dir().join(format!("{stem}.mfd"));

    let warnings = mfd::export(&project, &output).unwrap();

    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("inner join 8 is not exported"));
    assert!(warnings[0].contains("iteration and node connections are skipped"));
    let xml = std::fs::read_to_string(output).unwrap();
    assert!(!xml.contains("kind=\"32\""));
    for suffix in [".mfd", "-source.xsd", "-target.xsd"] {
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!("{stem}{suffix}")));
    }
}
