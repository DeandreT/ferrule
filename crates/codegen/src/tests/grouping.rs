use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration, SequenceExpr, SequenceWindow};

use crate::{GeneratedSequence, GroupingPlan, IterationSource, lower};

fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}

fn source_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        scalar("Key", ScalarType::String),
                        scalar("Starts", ScalarType::Bool),
                        scalar("Value", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::group("Group", vec![scalar("First", ScalarType::Int)]).repeating()],
        ),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Key".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Starts".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                filter: Some(5),
                sort_by: Some(4),
                windows: vec![SequenceWindow::First { count: 6 }],
                bindings: vec![Binding {
                    target_field: "First".into(),
                    node: 4,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: BTreeMap::new(),
        failure_rules: Vec::new(),
    }
}

#[test]
fn lowers_each_grouping_mode_inside_the_candidate_pipeline() {
    for expected in [
        GroupingPlan::By { key: 1 },
        GroupingPlan::AdjacentBy { key: 1 },
        GroupingPlan::StartingWith { predicate: 2 },
        GroupingPlan::EndingWith { predicate: 2 },
        GroupingPlan::IntoBlocks { size: 3 },
    ] {
        let mut project = source_project();
        let scope = &mut project.root.children[0];
        match expected {
            GroupingPlan::By { key } => scope.group_by = Some(key),
            GroupingPlan::AdjacentBy { key } => scope.group_adjacent_by = Some(key),
            GroupingPlan::StartingWith { predicate } => {
                scope.group_starting_with = Some(predicate);
            }
            GroupingPlan::EndingWith { predicate } => {
                scope.group_ending_with = Some(predicate);
            }
            GroupingPlan::IntoBlocks { size } => scope.group_into_blocks = Some(size),
        }
        scope.post_group_filter = Some(2);

        let program = lower(&project).expect("portable grouping should lower");
        let iteration = program.root.children[0]
            .iteration
            .as_ref()
            .expect("grouping retains its iteration");

        assert_eq!(iteration.grouping(), Some(expected));
        assert_eq!(iteration.post_group_filter(), Some(2));
        assert_eq!(iteration.filter(), Some(5));
        assert_eq!(
            iteration.sort().expect("sort is retained").keys().count(),
            1
        );
        assert_eq!(iteration.windows().len(), 1);
        assert_eq!(
            iteration.roots().collect::<Vec<_>>(),
            vec![5, 4, expected.expression(), 2, 6]
        );
    }
}

#[test]
fn lowers_grouping_over_a_generated_sequence() {
    let mut project = source_project();
    project.graph.nodes.insert(
        7,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    let scope = &mut project.root.children[0];
    scope.iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: Some(6),
        to: 3,
        item: 7,
    });
    scope.group_by = Some(7);
    scope.sort_by = None;
    scope.bindings[0].node = 7;

    let program = lower(&project).expect("generated grouping should lower");
    let iteration = program.root.children[0]
        .iteration
        .as_ref()
        .expect("generated grouping retains its iteration");

    assert!(matches!(
        iteration.input(),
        IterationSource::Generated(GeneratedSequence::Range { item: 7, .. })
    ));
    assert_eq!(iteration.grouping(), Some(GroupingPlan::By { key: 7 }));
}
