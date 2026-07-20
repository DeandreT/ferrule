use std::collections::BTreeMap;
use std::fmt::Write as _;

use mapping::{
    FailureIteration, FailureSelection, Graph, IterationOutput, NodeId, Project, Scope,
    ScopeIteration, SortFilterOrder,
};

use crate::MfdError;

use super::schema::KeyAlloc;

type BranchKey = (Vec<String>, NodeId);

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
    for (index, rule) in project.failure_rules.iter().enumerate() {
        if let Some(message) = rule.message {
            validate_node(project, index, "message", message)?;
        }
        let predicate = match rule.selection {
            FailureSelection::WhenFalse { predicate } => {
                validate_node(project, index, "predicate", predicate)?;
                predicate
            }
            FailureSelection::All => {
                return Err(unsupported_rule(
                    index,
                    "unconditional failures have no executable MapForce exception representation",
                ));
            }
            FailureSelection::WhenTrue { .. } => {
                return Err(unsupported_rule(
                    index,
                    "when-true failures need a complementary false-branch target consumer",
                ));
            }
        };
        let collection = match &rule.iteration {
            FailureIteration::Source { collection } => collection,
            FailureIteration::Sequence { .. } => {
                return Err(unsupported_rule(
                    index,
                    "generated-sequence failures cannot share a target filter branch yet",
                ));
            }
        };
        if project
            .extra_sources
            .iter()
            .any(|source| collection.first() == Some(&source.name))
        {
            return Err(unsupported_rule(
                index,
                "secondary-source failures cannot own MapForce exception filters",
            ));
        }
        let matches = matching_scopes(project, collection, predicate);
        if matches != 1 {
            return Err(unsupported_rule(
                index,
                &format!(
                    "requires exactly one target scope that iterates `{}` and keeps predicate node {predicate}; found {matches}",
                    display_collection(collection)
                ),
            ));
        }
    }
    Ok(())
}

fn validate_node(
    project: &Project,
    rule_index: usize,
    role: &str,
    node: NodeId,
) -> Result<(), MfdError> {
    if project.graph.nodes.contains_key(&node) {
        return Ok(());
    }
    Err(MfdError::Unsupported(format!(
        "failure rule {} references missing {role} node {node}",
        rule_index + 1
    )))
}

fn unsupported_rule(index: usize, reason: &str) -> MfdError {
    MfdError::Unsupported(format!("failure rule {} {reason}", index + 1))
}

fn display_collection(collection: &[String]) -> String {
    if collection.is_empty() {
        "<root>".to_string()
    } else {
        collection.join("/")
    }
}

fn matching_scopes(project: &Project, collection: &[String], predicate: NodeId) -> usize {
    std::iter::once(&project.root)
        .chain(project.extra_targets.iter().map(|target| &target.root))
        .map(|root| count_scope_matches(root, &[], collection, predicate, true))
        .sum()
}

fn count_scope_matches(
    scope: &Scope,
    parent_collection: &[String],
    expected_collection: &[String],
    predicate: NodeId,
    ancestors_preserve_items: bool,
) -> usize {
    if let Some(segments) = scope.concatenated() {
        return segments
            .iter()
            .map(|segment| {
                count_scope_matches(
                    segment,
                    parent_collection,
                    expected_collection,
                    predicate,
                    ancestors_preserve_items,
                )
            })
            .sum();
    }
    let explicit_source = scope.source();
    let collection = explicit_source.map(|source| {
        let mut collection = parent_collection.to_vec();
        collection.extend(source.iter().cloned());
        collection
    });
    let current = collection.as_deref().unwrap_or(parent_collection);
    let filter_precedes_sort =
        scope.sort_filter_order == SortFilterOrder::FilterThenSort || !scope.has_sort();
    let own_match = usize::from(
        explicit_source.is_some()
            && ancestors_preserve_items
            && current == expected_collection
            && scope.filter == Some(predicate)
            && filter_precedes_sort,
    );
    let descendants_preserve_items = ancestors_preserve_items && preserves_descendant_items(scope);
    own_match
        + scope
            .children
            .iter()
            .map(|child| {
                count_scope_matches(
                    child,
                    current,
                    expected_collection,
                    predicate,
                    descendants_preserve_items,
                )
            })
            .sum::<usize>()
}

fn preserves_descendant_items(scope: &Scope) -> bool {
    matches!(
        scope.iteration,
        ScopeIteration::None | ScopeIteration::Source(_) | ScopeIteration::DynamicDocuments { .. }
    ) && scope.filter.is_none()
        && !scope.has_sort()
        && scope.group_by.is_none()
        && scope.group_starting_with.is_none()
        && scope.group_adjacent_by.is_none()
        && scope.group_ending_with.is_none()
        && scope.group_into_blocks.is_none()
        && scope.windows.is_empty()
        && scope.iteration_output == IterationOutput::Repeated
}

struct Sink {
    predicate: Option<NodeId>,
    message: Option<NodeId>,
    trigger_output: Option<u32>,
}

pub(super) struct Branches {
    by_key: BTreeMap<BranchKey, Vec<usize>>,
    sinks: Vec<Sink>,
}

pub(super) struct RenderArgs<'a> {
    pub(super) graph: &'a Graph,
    pub(super) node_out_key: &'a BTreeMap<NodeId, u32>,
    pub(super) position_contexts: &'a BTreeMap<NodeId, Option<u32>>,
    pub(super) keys: &'a mut KeyAlloc,
    pub(super) uid: &'a mut u32,
    pub(super) components: &'a mut String,
    pub(super) edges: &'a mut Vec<(u32, u32)>,
}

impl Branches {
    pub(super) fn new(project: &Project) -> Self {
        let mut by_key = BTreeMap::<BranchKey, Vec<usize>>::new();
        let mut sinks = Vec::with_capacity(project.failure_rules.len());
        for (index, rule) in project.failure_rules.iter().enumerate() {
            if let (
                FailureIteration::Source { collection },
                FailureSelection::WhenFalse { predicate },
            ) = (&rule.iteration, rule.selection)
            {
                by_key
                    .entry((collection.clone(), predicate))
                    .or_default()
                    .push(index);
            }
            sinks.push(Sink {
                predicate: rule.selection.predicate(),
                message: rule.message,
                trigger_output: None,
            });
        }
        Self { by_key, sinks }
    }

    pub(super) fn has_branch(&self, collection: &[String], predicate: NodeId) -> bool {
        self.by_key.contains_key(&(collection.to_vec(), predicate))
    }

    pub(super) fn message_nodes(
        &self,
        collection: &[String],
        predicate: NodeId,
    ) -> impl Iterator<Item = NodeId> + '_ {
        self.by_key
            .get(&(collection.to_vec(), predicate))
            .into_iter()
            .flatten()
            .filter_map(|index| self.sinks.get(*index).and_then(|sink| sink.message))
    }

    pub(super) fn claim(&mut self, collection: &[String], predicate: NodeId, trigger_output: u32) {
        let Some(indexes) = self.by_key.get(&(collection.to_vec(), predicate)) else {
            return;
        };
        for index in indexes {
            if let Some(sink) = self.sinks.get_mut(*index) {
                sink.trigger_output = Some(trigger_output);
            }
        }
    }

    pub(super) fn render(&self, args: RenderArgs<'_>) -> Result<(), MfdError> {
        let RenderArgs {
            graph,
            node_out_key,
            position_contexts,
            keys,
            uid,
            components,
            edges,
        } = args;
        for (index, sink) in self.sinks.iter().enumerate() {
            for position in super::position::position_nodes_for_roots(
                sink.predicate.into_iter().chain(sink.message),
                graph,
            ) {
                if !matches!(position_contexts.get(&position), Some(Some(_))) {
                    return Err(unsupported_rule(
                        index,
                        &format!(
                            "position node {position} has no unambiguous failure-item export context"
                        ),
                    ));
                }
            }
            let trigger_output = sink.trigger_output.ok_or_else(|| {
                unsupported_rule(
                    index,
                    "could not claim its complementary target filter branch",
                )
            })?;
            let message_output = sink
                .message
                .map(|message| {
                    node_out_key.get(&message).copied().ok_or_else(|| {
                        MfdError::Unsupported(format!(
                            "failure rule {} message node {message} has no exportable output",
                            index + 1
                        ))
                    })
                })
                .transpose()?;
            render_sink(trigger_output, message_output, keys, uid, components, edges);
        }
        Ok(())
    }
}

fn render_sink(
    trigger_output: u32,
    message_output: Option<u32>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
    edges: &mut Vec<(u32, u32)>,
) {
    let trigger_input = keys.next();
    let message_input = keys.next();
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"exception\" library=\"core\" uid=\"{uid}\" kind=\"18\">\n\
         \t\t\t\t\t<properties/>\n\
         \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{trigger_input}\"/><datapoint pos=\"1\" key=\"{message_input}\"/></sources>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
         \t\t\t\t\t<data><wsdl/><exception/></data>\n\
         \t\t\t\t</component>\n"
    );
    edges.push((trigger_output, trigger_input));
    if let Some(message_output) = message_output {
        edges.push((message_output, message_input));
    }
}
