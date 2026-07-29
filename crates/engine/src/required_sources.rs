use std::collections::BTreeSet;

use mapping::{
    FailureIteration, NamedTarget, Node, NodeId, Project, Scope, ScopeConstruction, SequenceExpr,
};

use crate::{EngineError, TargetSelection};

pub(crate) fn selected_target<'a>(
    project: &'a Project,
    selection: TargetSelection<'_>,
) -> Result<Option<&'a NamedTarget>, EngineError> {
    let TargetSelection::Named(name) = selection else {
        return Ok(None);
    };
    let mut matches = project
        .extra_targets
        .iter()
        .filter(|target| target.name == name);
    let target = matches.next().ok_or_else(|| EngineError::UnknownTarget {
        name: name.to_string(),
    })?;
    if matches.next().is_some() {
        return Err(EngineError::AmbiguousTarget {
            name: name.to_string(),
        });
    }
    Ok(Some(target))
}

pub(crate) fn required_for_target<'a>(
    project: &'a Project,
    selection: TargetSelection<'_>,
) -> Result<crate::RequiredTargetSources<'a>, EngineError> {
    let selected = selected_target(project, selection)?;
    let mut analysis = Analysis::new(project);
    analysis.inspect_scope(selected.map_or(&project.root, |target| &target.root));
    analysis.inspect_failure_rules();
    analysis.inspect_graph();
    let static_sources = project
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(index, source)| {
            source.dynamic_path.is_none() && analysis.required.contains(index)
        })
        .map(|(_, source)| source)
        .collect();
    let dynamic_sources = project
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(index, source)| {
            source.dynamic_path.is_some() && analysis.inspected_dynamic.contains(index)
        })
        .map(|(_, source)| source)
        .collect();
    Ok(crate::RequiredTargetSources {
        static_sources,
        dynamic_sources,
    })
}

struct Analysis<'a> {
    project: &'a Project,
    required: BTreeSet<usize>,
    inspected_dynamic: BTreeSet<usize>,
    pending: Vec<NodeId>,
    visited: BTreeSet<NodeId>,
}

impl<'a> Analysis<'a> {
    fn new(project: &'a Project) -> Self {
        Self {
            project,
            required: BTreeSet::new(),
            inspected_dynamic: BTreeSet::new(),
            pending: Vec::new(),
            visited: BTreeSet::new(),
        }
    }

    fn inspect_scope(&mut self, scope: &Scope) {
        if let Some(source) = scope.source() {
            self.inspect_path(source);
        }
        if let Some(sequence) = scope.sequence() {
            self.inspect_sequence(sequence);
        }
        if let Some((_, join)) = scope.join() {
            for source in join.sources() {
                self.inspect_path(source.collection());
            }
        }
        if let Some(segments) = scope.concatenated() {
            for segment in segments.iter() {
                self.inspect_scope(segment);
            }
        }

        self.pending.extend(scope.filter);
        self.pending.extend(scope.post_group_filter);
        self.pending.extend(scope.grouping_nodes());
        self.pending.extend(scope.sort_by);
        self.pending.extend(scope.output_path());
        self.pending.extend(
            scope
                .windows
                .iter()
                .copied()
                .flat_map(|window| window.nodes()),
        );
        self.pending
            .extend(scope.sort_then_by.iter().map(|key| key.node));
        self.pending
            .extend(scope.bindings.iter().map(|binding| binding.node));
        self.pending.extend(
            scope
                .dynamic_bindings
                .iter()
                .flat_map(|binding| [binding.key, binding.value]),
        );

        match &scope.construction {
            ScopeConstruction::Scalar { value } => self.pending.push(*value),
            ScopeConstruction::RecursiveFilter { plan } => self.pending.push(plan.predicate()),
            ScopeConstruction::PathHierarchy { plan } => self.inspect_path(plan.collection()),
            ScopeConstruction::AdjacencyTree { plan } => {
                self.inspect_path(plan.collection());
                self.pending.extend(plan.root());
            }
            ScopeConstruction::Constructed
            | ScopeConstruction::CopyCurrentSource
            | ScopeConstruction::XmlMixedContent { .. } => {}
        }

        for child in &scope.children {
            self.inspect_scope(child);
        }
        for child in &scope.dynamic_children {
            self.pending.push(child.key);
            self.inspect_scope(&child.scope);
        }
    }

    fn inspect_failure_rules(&mut self) {
        for rule in &self.project.failure_rules {
            match &rule.iteration {
                FailureIteration::Source { collection } => self.inspect_path(collection),
                FailureIteration::Sequence { sequence } => self.inspect_sequence(sequence),
            }
            self.pending.extend(rule.selection.predicate());
            self.pending.extend(rule.message);
        }
    }

    fn inspect_sequence(&mut self, sequence: &SequenceExpr) {
        self.pending.extend(sequence.inputs());
        self.pending.push(sequence.item());
        if let SequenceExpr::RecursiveCollect { collection, .. } = sequence {
            self.inspect_path(collection);
        }
    }

    fn inspect_graph(&mut self) {
        while let Some(id) = self.pending.pop() {
            if !self.visited.insert(id) {
                continue;
            }
            let Some(node) = self.project.graph.nodes.get(&id) else {
                continue;
            };
            self.pending.extend(node.dependencies());
            self.inspect_node(node);
        }
    }

    fn inspect_node(&mut self, node: &Node) {
        match node {
            Node::SourceField { path, frame }
            | Node::DynamicSourceField {
                object: path,
                frame,
                ..
            }
            | Node::XmlSerialize { path, frame, .. } => {
                self.inspect_path(frame.as_deref().unwrap_or(path));
            }
            Node::Position { collection }
            | Node::JoinField { collection, .. }
            | Node::Lookup { collection, .. }
            | Node::CollectionFind { collection, .. }
            | Node::Aggregate { collection, .. } => self.inspect_path(collection),
            Node::XmlMixedContent {
                path,
                frame,
                replacements,
            } => {
                self.inspect_path(frame.as_deref().unwrap_or(path));
                for replacement in replacements {
                    self.inspect_path(&replacement.collection);
                }
            }
            Node::SequenceExists { sequence, .. }
            | Node::SequenceItemAt { sequence, .. }
            | Node::SequenceAggregate { sequence, .. } => self.inspect_sequence(sequence),
            Node::JoinAggregate { plan, .. } => {
                for source in plan.sources() {
                    self.inspect_path(source.collection());
                }
            }
            Node::SourceDocumentPath
            | Node::JoinPosition { .. }
            | Node::Unconnected
            | Node::Const { .. }
            | Node::FunctionParameter { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::Call { .. }
            | Node::UserFunctionCall { .. }
            | Node::If { .. }
            | Node::ValueMap { .. } => {}
        }
    }

    fn inspect_path(&mut self, path: &[String]) {
        let Some(first) = path.first() else {
            return;
        };
        let matches = self
            .project
            .extra_sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.name == *first)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for index in matches {
            let source = &self.project.extra_sources[index];
            let Some(dynamic) = &source.dynamic_path else {
                self.required.insert(index);
                continue;
            };
            if !self.inspected_dynamic.insert(index) {
                continue;
            }
            self.pending.push(dynamic.node);
            if let Some(driver) = dynamic.iteration.first().cloned() {
                self.inspect_path(&[driver]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode};
    use mapping::{
        Binding, DynamicSourcePath, FailureRule, FailureSelection, Graph, JoinConditions, JoinId,
        JoinKey, JoinPlan, JoinSource, NamedSource, NamedTarget, Project, Scope, ScopeIteration,
    };

    use super::*;

    fn source(name: &str) -> NamedSource {
        NamedSource {
            name: name.into(),
            path: format!("{name}.json"),
            schema: SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)]),
            options: Default::default(),
            dynamic_path: None,
        }
    }

    fn scope(node: NodeId) -> Scope {
        Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node,
            }],
            ..Scope::default()
        }
    }

    #[test]
    fn selected_target_keeps_its_sources_and_global_failure_sources() {
        let project = Project {
            source: SchemaNode::group("Input", Vec::new()),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![
                source("primary_data"),
                source("named_data"),
                source("guard_data"),
            ],
            extra_targets: vec![NamedTarget {
                name: "named".into(),
                path: None,
                schema: SchemaNode::group(
                    "Named",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                ),
                options: Default::default(),
                root: scope(1),
            }],
            failure_rules: vec![FailureRule {
                iteration: FailureIteration::Source {
                    collection: vec!["guard_data".into()],
                },
                selection: FailureSelection::All,
                message: None,
            }],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [
                    (
                        0,
                        Node::SourceField {
                            path: vec!["primary_data".into(), "Value".into()],
                            frame: None,
                        },
                    ),
                    (
                        1,
                        Node::Aggregate {
                            function: mapping::AggregateOp::Join,
                            collection: vec!["named_data".into()],
                            value: vec!["Value".into()],
                            expression: None,
                            arg: None,
                        },
                    ),
                ]
                .into(),
            },
            root: scope(0),
        };

        let primary = required_for_target(&project, TargetSelection::Primary)
            .expect("primary selection should resolve");
        assert_eq!(
            primary
                .static_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            ["primary_data", "guard_data"]
        );

        let named = required_for_target(&project, TargetSelection::Named("named"))
            .expect("named selection should resolve");
        assert_eq!(
            named
                .static_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            ["named_data", "guard_data"]
        );
    }

    #[test]
    fn selected_dynamic_source_keeps_static_path_and_driver_dependencies() {
        let mut dynamic = source("dynamic");
        dynamic.dynamic_path = Some(DynamicSourcePath {
            node: 0,
            iteration: vec!["driver_data".into()],
        });
        let project = Project {
            source: SchemaNode::group("Input", Vec::new()),
            target: SchemaNode::group("Primary", Vec::new()),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![
                source("path_data"),
                source("driver_data"),
                source("unselected_data"),
                dynamic,
            ],
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph {
                nodes: [(
                    0,
                    Node::SourceField {
                        path: vec!["path_data".into(), "Value".into()],
                        frame: None,
                    },
                )]
                .into(),
            },
            root: Scope {
                iteration: ScopeIteration::Source(vec!["dynamic".into()]),
                ..Scope::default()
            },
        };

        let required = required_for_target(&project, TargetSelection::Primary)
            .expect("primary selection should resolve");
        assert_eq!(
            required
                .static_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            ["path_data", "driver_data"]
        );
        assert_eq!(
            required
                .dynamic_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            ["dynamic"]
        );
    }

    #[test]
    fn selected_scope_keeps_direct_and_joined_collections() {
        let plan = match JoinPlan::new(
            JoinSource::new(vec!["join_left".into()]),
            JoinSource::new(vec!["join_right".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["join_left".into()],
                vec!["Value".into()],
                vec!["Value".into()],
            )),
        ) {
            Ok(plan) => plan,
            Err(error) => panic!("test join plan should be valid: {error}"),
        };
        let project = Project {
            source: SchemaNode::group("Input", Vec::new()),
            target: SchemaNode::group("Primary", Vec::new()),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![
                source("scope_data"),
                source("join_left"),
                source("join_right"),
                source("unselected_data"),
            ],
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph::default(),
            root: Scope {
                children: vec![
                    Scope {
                        iteration: ScopeIteration::Source(vec!["scope_data".into()]),
                        ..Scope::default()
                    },
                    Scope {
                        iteration: ScopeIteration::InnerJoin {
                            id: JoinId::new(1),
                            plan,
                        },
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            },
        };

        let required = required_for_target(&project, TargetSelection::Primary)
            .expect("primary selection should resolve");
        assert_eq!(
            required
                .static_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            ["scope_data", "join_left", "join_right"]
        );
    }
}
