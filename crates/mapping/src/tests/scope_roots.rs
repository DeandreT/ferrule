use super::*;
use crate::{
    AdjacencyTreePlan, Binding, DynamicBinding, DynamicChild, FailureSelection, FilterMapCapture,
    FilterMapV1, FunctionId, RecursiveFilterPlan, ScopeIteration, ScopeSequence, SequenceWindow,
    SortKey,
};
use ScopeRootRole::{GeneratedItem, GroupMember, Item, Lazy, Output, Parent, RecursiveGroup};
use ScopeRootSlot as Slot;

fn roots(scope: &Scope) -> Vec<(NodeId, ScopeRootSlot, ScopeRootRole)> {
    let mut roots = Vec::new();
    scope.visit_roots(&mut |root| roots.push((root.node, root.slot, root.role)));
    roots
}

/// Every scalar slot set at once; grouping exclusivity is a validation rule,
/// not part of the visitor's structure.
fn every_slot() -> Scope {
    Scope {
        iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 3,
        }),
        construction: ScopeConstruction::Scalar { value: 4 },
        filter: Some(5),
        post_group_filter: Some(6),
        group_by: Some(7),
        group_adjacent_by: Some(8),
        group_starting_with: Some(9),
        group_ending_with: Some(10),
        group_into_blocks: Some(11),
        sort_by: Some(12),
        sort_then_by: vec![SortKey {
            node: 13,
            descending: true,
        }],
        windows: vec![
            SequenceWindow::First { count: 14 },
            SequenceWindow::FromTo {
                first: 15,
                last: 16,
            },
        ],
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 17,
        }],
        dynamic_bindings: vec![DynamicBinding { key: 18, value: 19 }],
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

#[test]
fn every_scope_slot_has_its_evaluation_role() {
    assert_eq!(
        roots(&every_slot()),
        [
            (5, Slot::Filter, Item),
            (6, Slot::PostGroupFilter, GroupMember),
            (7, Slot::GroupBy, Item),
            (8, Slot::GroupAdjacentBy, Item),
            (9, Slot::GroupStartingWith, Item),
            (10, Slot::GroupEndingWith, Item),
            (11, Slot::GroupIntoBlocks, Parent),
            (12, Slot::SortBy, Item),
            (14, Slot::Window(0), Parent),
            (15, Slot::Window(1), Parent),
            (16, Slot::Window(1), Parent),
            (13, Slot::SortThenBy(0), Item),
            (17, Slot::Binding(0), Output),
            (18, Slot::DynamicBindingKey(0), Output),
            (19, Slot::DynamicBindingValue(0), Output),
            (4, Slot::ScalarValue, Output),
            (1, Slot::SequenceInput, Parent),
            (2, Slot::SequenceInput, Parent),
            (3, Slot::SequenceItem, GeneratedItem),
            (20, Slot::DynamicChildKey(0), Output),
        ]
    );
}

#[test]
fn construction_and_output_path_roots_keep_their_roles() {
    let output = Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec!["Rows".into()],
            output_path: 1,
        },
        construction: ScopeConstruction::RecursiveFilter {
            plan: RecursiveFilterPlan::new("Children".into(), "Items".into(), 2)
                .expect("distinct recursive fields"),
        },
        ..Scope::default()
    };
    assert_eq!(
        roots(&output),
        [
            (1, Slot::OutputPath, Output),
            (2, Slot::RecursiveFilterPredicate, RecursiveGroup),
        ]
    );
    let tree = Scope {
        construction: ScopeConstruction::AdjacencyTree {
            plan: AdjacencyTreePlan::new(
                vec!["Rows".into()],
                vec!["Key".into()],
                vec!["Parent".into()],
                "Key".into(),
                "Children".into(),
                Some(3),
            )
            .expect("valid adjacency plan"),
        },
        ..Scope::default()
    };
    assert_eq!(roots(&tree), [(3, Slot::AdjacencyTreeRoot, Output)]);
    let presence = Scope {
        iteration: ScopeIteration::OptionalPresence { condition: 4 },
        ..Scope::default()
    };
    assert_eq!(roots(&presence), [(4, Slot::Presence, Parent)]);
}

#[test]
fn a_composed_sequence_reports_its_reserved_source_item() {
    let scope = Scope {
        iteration: ScopeIteration::Sequence(SequenceExpr::FilterMapV1(FilterMapV1 {
            source: Box::new(SequenceExpr::Tokenize {
                input: 1,
                delimiter: 2,
                item: 3,
            }),
            item: 4,
            predicate: FunctionId::new(1),
            mapper: FunctionId::new(2),
            output_type: ir::ScalarType::String,
            captures: vec![FilterMapCapture {
                node: 5,
                ty: ir::ScalarType::Int,
            }],
        })),
        ..Scope::default()
    };
    assert_eq!(
        roots(&scope),
        [
            (1, Slot::SequenceInput, Parent),
            (2, Slot::SequenceInput, Parent),
            (5, Slot::SequenceInput, Parent),
            (3, Slot::SequenceReservedItem, GeneratedItem),
            (4, Slot::SequenceItem, GeneratedItem),
        ]
    );
}

#[test]
fn nested_scopes_are_segments_then_children_then_dynamic_children() {
    let leaf = |node| Scope {
        filter: Some(node),
        ..Scope::default()
    };
    let scope = Scope {
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(leaf(1), vec![leaf(2)])),
        children: vec![leaf(3)],
        dynamic_children: vec![DynamicChild {
            key: 4,
            scope: leaf(5),
        }],
        ..Scope::default()
    };
    assert_eq!(
        scope
            .nested_scopes()
            .map(|(kind, nested)| (kind, nested.filter))
            .collect::<Vec<_>>(),
        [
            (NestedScopeKind::Segment(0), Some(1)),
            (NestedScopeKind::Segment(1), Some(2)),
            (NestedScopeKind::Child(0), Some(3)),
            (NestedScopeKind::DynamicChild(0), Some(5)),
        ]
    );
    let mut tree = Vec::new();
    scope.visit_tree_roots(&mut |root| tree.push(root.node));
    assert_eq!(tree, [4, 1, 2, 3, 5]);
}

#[test]
fn a_failure_message_is_lazy_beside_its_item_predicate() {
    let rule = FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 1,
                item: 2,
            },
        },
        selection: FailureSelection::WhenFalse { predicate: 3 },
        message: Some(4),
    };
    let mut roots = Vec::new();
    rule.visit_roots(&mut |root| roots.push((root.node, root.slot, root.role)));
    assert_eq!(
        roots,
        [
            (1, Slot::SequenceInput, Parent),
            (2, Slot::SequenceItem, GeneratedItem),
            (3, Slot::FailurePredicate, Item),
            (4, Slot::FailureMessage, Lazy),
        ]
    );
}

/// Pruning keeps every root the visitor reports, in every target, nested
/// scope, dynamic source path and failure rule, and nothing else.
#[test]
fn pruning_keeps_exactly_the_visited_roots() {
    let mut root = every_slot();
    root.children.push(Scope {
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(
            Scope {
                filter: Some(22),
                ..Scope::default()
            },
            Vec::new(),
        )),
        ..Scope::default()
    });
    let mut project = crate::Project {
        source: ir::SchemaNode::group("Source", Vec::new()),
        target: ir::SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![crate::NamedSource {
            name: "lookup".into(),
            path: String::new(),
            schema: ir::SchemaNode::group("Lookup", Vec::new()),
            options: Default::default(),
            dynamic_path: Some(crate::DynamicSourcePath {
                node: 23,
                iteration: Vec::new(),
            }),
        }],
        extra_targets: vec![crate::NamedTarget {
            name: "extra".into(),
            path: None,
            schema: ir::SchemaNode::group("Extra", Vec::new()),
            options: Default::default(),
            root: Scope {
                iteration: ScopeIteration::DynamicDocuments {
                    source: vec!["Rows".into()],
                    output_path: 24,
                },
                ..Scope::default()
            },
        }],
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source {
                collection: Vec::new(),
            },
            selection: FailureSelection::WhenTrue { predicate: 25 },
            message: Some(26),
        }],
        user_functions: Default::default(),
        graph: crate::Graph {
            nodes: (1..=27)
                .map(|id| {
                    (
                        id,
                        crate::Node::Const {
                            value: ir::Value::Null,
                        },
                    )
                })
                .collect(),
        },
        root,
    };

    project.prune_unreachable_nodes();

    assert_eq!(
        project.graph.nodes.keys().copied().collect::<Vec<_>>(),
        (1..=26).collect::<Vec<_>>()
    );
}
