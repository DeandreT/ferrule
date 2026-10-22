use std::collections::BTreeSet;

use crate::{NodeId, Project};

impl Project {
    /// Removes graph expressions that no target, control, or dynamic source
    /// can evaluate. Importers can use this after lowering a richer graph
    /// format whose disconnected components have no ferrule representation.
    pub fn prune_unreachable_nodes(&mut self) {
        self.prune_unreachable_nodes_with_roots([]);
    }

    pub(crate) fn prune_unreachable_nodes_with_roots(
        &mut self,
        additional_roots: impl IntoIterator<Item = NodeId>,
    ) {
        let mut pending = additional_roots.into_iter().collect::<Vec<_>>();
        pending.extend(crate::scope_roots::project_roots(self));

        let mut reachable = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !reachable.insert(id) {
                continue;
            }
            if let Some(node) = self.graph.nodes.get(&id) {
                pending.extend(node.dependencies());
            }
        }
        self.graph.nodes.retain(|id, _| reachable.contains(id));
        self.source_options
            .mfd_decimal_input_names
            .retain(|id, _| reachable.contains(id));
        self.source_options
            .mfd_optional_parser_input_names
            .retain(|id, _| reachable.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode, Value};

    use crate::{
        Binding, FailureIteration, FailureRule, FailureSelection, Graph, NamedSource, Node,
        Project, Scope,
    };

    #[test]
    fn project_prunes_only_nodes_outside_executable_roots() {
        let mut project = Project {
            source: SchemaNode::group("Source", Vec::new()),
            target: SchemaNode::group(
                "Target",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![NamedSource {
                name: "lookup".into(),
                path: String::new(),
                schema: SchemaNode::group("Lookup", Vec::new()),
                options: Default::default(),
                dynamic_path: Some(crate::DynamicSourcePath {
                    node: 3,
                    iteration: Vec::new(),
                }),
            }],
            extra_targets: Vec::new(),
            failure_rules: vec![
                FailureRule {
                    iteration: FailureIteration::Source {
                        collection: Vec::new(),
                    },
                    selection: FailureSelection::WhenTrue { predicate: 5 },
                    message: Some(4),
                },
                FailureRule {
                    iteration: FailureIteration::Sequence {
                        sequence: crate::SequenceExpr::Generate {
                            from: Some(6),
                            to: 7,
                            item: 8,
                        },
                    },
                    selection: FailureSelection::All,
                    message: None,
                },
            ],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [
                    (0, Node::Const { value: Value::Null }),
                    (
                        1,
                        Node::Call {
                            function: "concat".into(),
                            args: vec![0],
                        },
                    ),
                    (2, Node::Const { value: Value::Null }),
                    (3, Node::Const { value: Value::Null }),
                    (
                        4,
                        Node::Call {
                            function: "concat".into(),
                            args: vec![2],
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
                    (
                        7,
                        Node::Const {
                            value: Value::Int(2),
                        },
                    ),
                    (
                        8,
                        Node::SourceField {
                            path: Vec::new(),
                            frame: None,
                        },
                    ),
                    (
                        9,
                        Node::Const {
                            value: Value::String("adjacent".into()),
                        },
                    ),
                    (
                        10,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                children: vec![
                    Scope {
                        group_adjacent_by: Some(9),
                        ..Scope::default()
                    },
                    Scope {
                        group_ending_with: Some(10),
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            },
        };

        project
            .source_options
            .mfd_decimal_input_names
            .extend([(1, "kept".into()), (11, "pruned".into())]);
        project
            .graph
            .nodes
            .insert(11, Node::Const { value: Value::Null });

        project.prune_unreachable_nodes();

        assert_eq!(
            project.graph.nodes.keys().copied().collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        );
        assert_eq!(
            project.source_options.mfd_decimal_input_names,
            [(1, "kept".to_string())].into_iter().collect()
        );
    }
}
