use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicBinding, DynamicChild, FilterMapCapture, FilterMapV1, FunctionId, Graph,
    JoinId, Node, Project, ScopeConstruction, ScopeIteration, SequenceExpr, SequenceWindow,
    SortKey,
};

use super::*;

/// Every slot that can be set beside an output path, numbered in the
/// visitor's slot order.
fn every_slot() -> Scope {
    Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec!["Rows".into()],
            output_path: 9,
        },
        construction: ScopeConstruction::Scalar { value: 18 },
        filter: Some(1),
        post_group_filter: Some(2),
        group_by: Some(3),
        group_adjacent_by: Some(4),
        group_starting_with: Some(5),
        group_ending_with: Some(6),
        group_into_blocks: Some(7),
        sort_by: Some(8),
        windows: vec![SequenceWindow::FromTo {
            first: 10,
            last: 11,
        }],
        sort_then_by: vec![
            SortKey {
                node: 12,
                descending: false,
            },
            SortKey {
                node: 13,
                descending: true,
            },
        ],
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 14,
        }],
        dynamic_bindings: vec![
            DynamicBinding { key: 15, value: 16 },
            DynamicBinding { key: 17, value: 19 },
        ],
        dynamic_children: vec![DynamicChild {
            key: 20,
            scope: Scope {
                filter: Some(21),
                ..Scope::default()
            },
        }],
        ..Scope::default()
    }
}

fn presence() -> Scope {
    Scope {
        iteration: ScopeIteration::OptionalPresence { condition: 1 },
        ..Scope::default()
    }
}

/// A composed sequence owns its source's item besides its own.
fn composed_sequence() -> Scope {
    Scope {
        iteration: ScopeIteration::Sequence(SequenceExpr::FilterMapV1(FilterMapV1 {
            source: Box::new(SequenceExpr::Tokenize {
                input: 1,
                delimiter: 2,
                item: 3,
            }),
            item: 4,
            predicate: FunctionId::new(1),
            mapper: FunctionId::new(2),
            output_type: ScalarType::String,
            captures: vec![FilterMapCapture {
                node: 5,
                ty: ScalarType::Int,
            }],
        })),
        ..Scope::default()
    }
}

#[test]
fn required_source_discovery_reads_every_root_but_a_reserved_item() {
    assert_eq!(
        roots(&every_slot(), required_source),
        [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 18, 20
        ]
    );
    assert_eq!(roots(&composed_sequence(), required_source), [1, 2, 5, 4]);
}

#[test]
fn the_owned_probe_reads_controls_bindings_and_a_scalar_value() {
    assert_eq!(
        roots(&every_slot(), owned_probe),
        [1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 18]
    );
    assert_eq!(roots(&presence(), owned_probe), [1]);
    assert!(roots(&composed_sequence(), owned_probe).is_empty());
}

#[test]
fn join_validation_reads_item_and_output_roots_in_legacy_order() {
    assert_eq!(
        roots(&every_slot(), join_validation),
        [1, 2, 3, 4, 5, 6, 8, 9, 12, 13, 14, 15, 16, 17, 19, 20]
    );
    assert!(roots(&presence(), join_validation).is_empty());
    assert!(roots(&composed_sequence(), join_validation).is_empty());
}

#[test]
fn an_owned_join_tuple_reads_controls_sort_keys_bindings_and_a_scalar_value() {
    assert_eq!(
        roots(&every_slot(), join_tuple),
        [1, 2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 18]
    );
    assert!(roots(&presence(), join_tuple).is_empty());
    assert!(roots(&composed_sequence(), join_tuple).is_empty());
}

#[test]
fn owned_admission_reads_dynamic_roots_in_the_dynamic_target_context() {
    let admitted_roots = |scope: &Scope| {
        let mut found = Vec::new();
        scope.visit_roots(&mut |root| {
            if let Some(admission) = admitted(root.slot) {
                found.push((root.node, admission));
            }
        });
        found
    };
    use AdmittedRoot::{DynamicBinding as Binding, DynamicChildKey as ChildKey, Scope as Own};
    assert_eq!(
        admitted_roots(&every_slot()),
        [
            (1, Own),
            (2, Own),
            (3, Own),
            (4, Own),
            (5, Own),
            (6, Own),
            (7, Own),
            (8, Own),
            (9, Own),
            (10, Own),
            (11, Own),
            (12, Own),
            (13, Own),
            (14, Own),
            (15, Binding),
            (16, Binding),
            (17, Binding),
            (19, Binding),
            (20, ChildKey),
        ]
    );
    assert_eq!(admitted_roots(&presence()), [(1, Own)]);
    assert!(admitted_roots(&composed_sequence()).is_empty());
}

/// Join validation pops its roots last first, so a secondary sort key's
/// issue precedes the output path's.
#[test]
fn join_validation_reports_a_sort_key_before_the_output_path() {
    let join_field = |field: &str| Node::JoinField {
        join: JoinId::new(99),
        collection: vec!["Rows".into()],
        path: vec![field.into()],
    };
    let project = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Id", ScalarType::String)])
                    .repeating(),
            ],
        ),
        target: SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (0, join_field("Path")),
                (1, join_field("Key")),
                (
                    2,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["Rows".into()],
                output_path: 0,
            },
            sort_by: Some(2),
            sort_then_by: vec![SortKey {
                node: 1,
                descending: false,
            }],
            ..Scope::default()
        },
    };

    let inactive: Vec<_> = crate::validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .filter(|message| message.contains("references inactive join 99"))
        .collect();

    assert_eq!(
        inactive,
        [
            "join field node 1 references inactive join 99",
            "join field node 0 references inactive join 99",
        ]
    );
}

/// A block size is validated with the parent roots, after windows; the
/// other roots keep their issue order.
#[test]
fn join_validation_reports_a_block_size_with_the_parent_roots() {
    let join_field = |field: &str| Node::JoinField {
        join: JoinId::new(99),
        collection: vec!["Rows".into()],
        path: vec![field.into()],
    };
    let project = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Id", ScalarType::String)])
                    .repeating(),
            ],
        ),
        target: SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (1, join_field("Filter")),
                (2, join_field("Sort")),
                (3, join_field("First")),
                (4, join_field("Last")),
                (5, join_field("Size")),
                (6, join_field("Value")),
            ]
            .into(),
        },
        root: Scope {
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            filter: Some(1),
            group_into_blocks: Some(5),
            sort_by: Some(2),
            windows: vec![SequenceWindow::FromTo { first: 3, last: 4 }],
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 6,
            }],
            ..Scope::default()
        },
    };

    let inactive: Vec<_> = crate::validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .filter(|message| message.contains("references inactive join 99"))
        .collect();

    assert_eq!(
        inactive,
        [4, 3, 5, 6, 2, 1]
            .map(|node| format!("join field node {node} references inactive join 99"))
    );
}
