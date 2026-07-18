use std::collections::BTreeSet;

use crate::{Node, NodeId, Project, Scope, ScopeConstruction};

impl Project {
    /// Removes graph expressions that no target, control, or dynamic source
    /// can evaluate. Importers can use this after lowering a richer graph
    /// format whose disconnected components have no ferrule representation.
    pub fn prune_unreachable_nodes(&mut self) {
        let mut pending = Vec::new();
        collect_scope_roots(&self.root, &mut pending);
        for target in &self.extra_targets {
            collect_scope_roots(&target.root, &mut pending);
        }
        pending.extend(
            self.extra_sources
                .iter()
                .filter_map(|source| source.dynamic_path.as_ref())
                .map(|path| path.node),
        );

        let mut reachable = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !reachable.insert(id) {
                continue;
            }
            if let Some(node) = self.graph.nodes.get(&id) {
                pending.extend(node_dependencies(node));
            }
        }
        self.graph.nodes.retain(|id, _| reachable.contains(id));
    }
}

fn collect_scope_roots(scope: &Scope, roots: &mut Vec<NodeId>) {
    roots.extend(
        [
            scope.filter,
            scope.group_by,
            scope.group_starting_with,
            scope.group_into_blocks,
            scope.sort_by,
            scope.output_path(),
        ]
        .into_iter()
        .flatten(),
    );
    roots.extend(
        scope
            .windows
            .iter()
            .copied()
            .flat_map(|window| window.nodes()),
    );
    roots.extend(scope.sort_then_by.iter().map(|key| key.node));
    roots.extend(scope.bindings.iter().map(|binding| binding.node));
    roots.extend(
        scope
            .dynamic_bindings
            .iter()
            .flat_map(|binding| [binding.key, binding.value]),
    );
    match &scope.construction {
        ScopeConstruction::Scalar { value } => roots.push(*value),
        ScopeConstruction::RecursiveFilter { plan } => roots.push(plan.predicate()),
        ScopeConstruction::AdjacencyTree { plan } => roots.extend(plan.root()),
        ScopeConstruction::Constructed
        | ScopeConstruction::CopyCurrentSource
        | ScopeConstruction::XmlMixedContent { .. }
        | ScopeConstruction::PathHierarchy { .. } => {}
    }
    if let Some(sequence) = scope.sequence() {
        roots.extend(sequence.inputs());
        roots.push(sequence.item());
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            collect_scope_roots(segment, roots);
        }
    }
    for child in &scope.children {
        collect_scope_roots(child, roots);
    }
    for child in &scope.dynamic_children {
        roots.push(child.key);
        collect_scope_roots(&child.scope, roots);
    }
}

fn node_dependencies(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode, Value};

    use crate::{Binding, Graph, NamedSource, Node, Project, Scope};

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
                ]
                .into_iter()
                .collect(),
            },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            },
        };

        project.prune_unreachable_nodes();

        assert_eq!(
            project.graph.nodes.keys().copied().collect::<Vec<_>>(),
            vec![0, 1, 3]
        );
    }
}
