use std::collections::BTreeSet;

use mapping::{
    FailureIteration, NamedTarget, Node, NodeId, Project, Scope, ScopeConstruction, SequenceExpr,
};

use crate::{EngineError, TargetSelection, scope_slots};

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
    crate::source_ownership::check_project(project, Some(selection))?;
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
        match scope.source_selector() {
            Some(mapping::SourceSelectorRef::Legacy(source)) => self.inspect_path(source),
            Some(mapping::SourceSelectorRef::Owned(source)) => self.inspect_owner(&source.owner),
            None => {}
        }
        if let Some(sequence) = scope.sequence() {
            self.inspect_sequence_sources(sequence);
        }
        if let Some((_, join)) = scope.join() {
            for source in join.sources() {
                self.inspect_path(source.collection());
            }
        }
        if let Some((_, join)) = scope.iteration.owned_join() {
            for source in join.sources() {
                self.inspect_owner(&source.collection().owner);
            }
        }
        match &scope.construction {
            ScopeConstruction::PathHierarchy { plan } => self.inspect_path(plan.collection()),
            ScopeConstruction::OwnedPathHierarchy { plan } => {
                self.inspect_owner(&plan.collection().owner)
            }
            ScopeConstruction::AdjacencyTree { plan } => self.inspect_path(plan.collection()),
            ScopeConstruction::OwnedAdjacencyTree { plan } => {
                self.inspect_owner(&plan.collection().owner)
            }
            ScopeConstruction::Constructed
            | ScopeConstruction::CopyCurrentSource
            | ScopeConstruction::XmlMixedContent { .. }
            | ScopeConstruction::Scalar { .. }
            | ScopeConstruction::RecursiveFilter { .. } => {}
        }
        self.pending
            .extend(scope_slots::roots(scope, scope_slots::required_source));
        for (_, nested) in scope.nested_scopes() {
            self.inspect_scope(nested);
        }
    }

    fn inspect_failure_rules(&mut self) {
        for rule in &self.project.failure_rules {
            match &rule.iteration {
                FailureIteration::Source { collection } => self.inspect_path(collection),
                FailureIteration::Sequence { sequence } => self.inspect_sequence_sources(sequence),
                FailureIteration::OwnedSource { source } => self.inspect_owner(&source.owner),
            }
            let pending = &mut self.pending;
            rule.visit_roots(&mut |root| {
                if scope_slots::required_source(root.slot) {
                    pending.push(root.node);
                }
            });
        }
    }

    fn inspect_sequence_sources(&mut self, sequence: &SequenceExpr) {
        if let SequenceExpr::RecursiveCollect { collection, .. } = sequence {
            self.inspect_path(collection);
        }
        if let Some(source) = sequence.owned_source() {
            self.inspect_owner(&source.owner);
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
            // Every input, whatever its role, including a sequence's
            // inputs and item identities.
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
            | Node::SequenceAggregate { sequence, .. } => self.inspect_sequence_sources(sequence),
            Node::JoinAggregate { plan, .. } => {
                for source in plan.sources() {
                    self.inspect_path(source.collection());
                }
            }
            Node::OwnedSourceField(field) => self.inspect_owner(&field.source.owner),
            Node::OwnedPosition(position) => self.inspect_owner(&position.owner),
            Node::OwnedRowDocumentPath(path) => self.inspect_owner(&path.owner),
            Node::OwnedSourceDocumentPath(path) => self.inspect_owner(&path.owner),
            Node::OwnedDynamicSourceField(field) => self.inspect_owner(&field.object.owner),
            Node::OwnedLookup(lookup) => self.inspect_owner(&lookup.collection.owner),
            Node::OwnedCollectionFind(find) => self.inspect_owner(&find.collection.owner),
            Node::OwnedXmlSerialize(serializer) => self.inspect_owner(&serializer.source.owner),
            Node::OwnedAggregate(aggregate) => self.inspect_owner(&aggregate.collection.owner),
            Node::OwnedJoinField(field) => self.inspect_owner(&field.collection.owner),
            Node::OwnedJoinSourcePosition(position) => {
                self.inspect_owner(&position.collection.owner)
            }
            Node::OwnedXmlMixedContent(mixed) => self.inspect_owner(&mixed.source.owner),
            Node::OwnedJoinAggregate(aggregate) => {
                for source in aggregate.plan.sources() {
                    self.inspect_owner(&source.collection().owner);
                }
            }
            Node::SourceDocumentPath
            | Node::SourceRootXmlTypeEquals { .. }
            | Node::SourceRootField { .. }
            | Node::JoinPosition { .. }
            | Node::GeneratedPosition { .. }
            | Node::Unconnected
            | Node::Const { .. }
            | Node::FunctionParameter { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::RuntimeParameterDefault { .. }
            | Node::InvocationCounter { .. }
            | Node::Call { .. }
            | Node::UserFunctionCall { .. }
            | Node::If { .. }
            | Node::Raise { .. }
            | Node::ValueMap { .. } => {}
        }
    }

    fn inspect_owner(&mut self, owner: &mapping::SourceOwner) {
        if let mapping::SourceOwner::Named { name } = owner {
            self.required.extend(
                self.project
                    .extra_sources
                    .iter()
                    .enumerate()
                    .filter(|(_, source)| source.name == *name && source.dynamic_path.is_none())
                    .map(|(index, _)| index),
            );
            // An owned dynamic owner loads through its own exact driver.
            let project = self.project;
            for (index, source) in project.extra_sources.iter().enumerate() {
                let Some(dynamic) = source.dynamic_path.as_ref() else {
                    continue;
                };
                if source.name == *name
                    && let Some(driver) = dynamic.owned_driver()
                    && self.inspected_dynamic.insert(index)
                {
                    self.pending.push(dynamic.node);
                    self.inspect_owner(&driver.owner);
                }
            }
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
            match &dynamic.iteration {
                mapping::SourceSelector::Legacy(path) => {
                    if let Some(driver) = path.first().cloned() {
                        self.inspect_path(&[driver]);
                    }
                }
                mapping::SourceSelector::Owned(driver) => self.inspect_owner(&driver.owner),
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/required_sources_roles.rs"]
mod role_tests;

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
        dynamic.dynamic_path = Some(DynamicSourcePath::legacy(0, vec!["driver_data".into()]));
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
