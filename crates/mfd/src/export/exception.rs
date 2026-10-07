use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use mapping::{
    FailureIteration, FailureSelection, Graph, IterationOutput, Node, NodeId, Project, Scope,
    ScopeConstruction, ScopeIteration, SortFilterOrder,
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
                    "unconditional failures have no executable native exception representation",
                ));
            }
            FailureSelection::WhenTrue { predicate } => {
                validate_node(project, index, "predicate", predicate)?;
                validate_true_branch(project, index, predicate)?;
                continue;
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
                "secondary-source failures cannot own native exception filters",
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

/// Omit only a complementary negation whose sole use becomes a native filter
/// branch. Keep every other graph consumer, even a disconnected one, intact.
/// Called after preflight has validated the original, unchanged project.
pub(super) fn absorbed_filter_nodes(project: &Project) -> BTreeSet<NodeId> {
    let [rule] = project.failure_rules.as_slice() else {
        return BTreeSet::new();
    };
    let (FailureIteration::Source { collection }, FailureSelection::WhenTrue { predicate }) =
        (&rule.iteration, rule.selection)
    else {
        return BTreeSet::new();
    };
    let filters = complementary_filters(project, collection, predicate);
    let [filter] = filters.as_slice() else {
        return BTreeSet::new();
    };
    if project
        .graph
        .nodes
        .values()
        .any(|node| node.dependencies().contains(filter))
        || std::iter::once(&project.source_options)
            .chain(project.extra_sources.iter().map(|source| &source.options))
            .chain(std::iter::once(&project.target_options))
            .chain(project.extra_targets.iter().map(|target| &target.options))
            .any(|options| options.mfd_decimal_input_names.contains_key(filter))
    {
        return BTreeSet::new();
    }
    // The existing project-root traversal covers scalar bindings, every scope
    // control, failure predicates/messages, and dynamic source path expressions.
    // Use it only as a proof; neither the caller's graph nor its roots are pruned.
    let mut ordinary = project.clone();
    let uses = clear_filter_references(&mut ordinary.root, *filter)
        + ordinary
            .extra_targets
            .iter_mut()
            .map(|target| clear_filter_references(&mut target.root, *filter))
            .sum::<usize>();
    if uses != 1 {
        return BTreeSet::new();
    }
    ordinary.prune_unreachable_nodes();
    if ordinary.graph.nodes.contains_key(filter) {
        BTreeSet::new()
    } else {
        BTreeSet::from([*filter])
    }
}

fn clear_filter_references(scope: &mut Scope, filter: NodeId) -> usize {
    let mut uses = 0;
    if scope.filter == Some(filter) {
        scope.filter = None;
        uses += 1;
    }
    if let Some(segments) = scope.concatenated_mut() {
        uses += segments
            .iter_mut()
            .map(|segment| clear_filter_references(segment, filter))
            .sum::<usize>();
    }
    uses += scope
        .children
        .iter_mut()
        .map(|child| clear_filter_references(child, filter))
        .sum::<usize>();
    uses += scope
        .dynamic_children
        .iter_mut()
        .map(|child| clear_filter_references(&mut child.scope, filter))
        .sum::<usize>();
    uses
}

fn validate_true_branch(
    project: &Project,
    index: usize,
    predicate: NodeId,
) -> Result<(), MfdError> {
    if project.failure_rules.len() != 1 {
        return Err(unsupported_rule(
            index,
            "when-true failures require exactly one failure rule",
        ));
    }
    let FailureIteration::Source { collection } = &project.failure_rules[index].iteration else {
        return Err(unsupported_rule(
            index,
            "generated-sequence failures cannot share a target filter branch yet",
        ));
    };
    if project
        .extra_sources
        .iter()
        .any(|source| collection.first() == Some(&source.name))
    {
        return Err(unsupported_rule(
            index,
            "secondary-source failures cannot own native exception filters",
        ));
    }
    if !collection
        .iter()
        .try_fold(&project.source, |schema, segment| schema.child(segment))
        .is_some_and(|schema| schema.repeating)
    {
        return Err(unsupported_rule(
            index,
            "when-true failures require an exact repeating primary-source schema path",
        ));
    }
    let matches = complementary_filters(project, collection, predicate).len();
    if matches != 1 {
        return Err(unsupported_rule(
            index,
            &format!(
                "when-true failures need a complementary false-branch target consumer: requires exactly one plain repeated scope over `{}` with unary not(node {predicate}); found {matches}",
                display_collection(collection)
            ),
        ));
    }
    Ok(())
}

fn complementary_filters(
    project: &Project,
    collection: &[String],
    predicate: NodeId,
) -> Vec<NodeId> {
    let mut filters = Vec::new();
    for root in std::iter::once(&project.root)
        .chain(project.extra_targets.iter().map(|target| &target.root))
    {
        collect_complementary_filters(
            root,
            &[],
            collection,
            predicate,
            &project.graph,
            true,
            &mut filters,
        );
    }
    filters
}

fn collect_complementary_filters(
    scope: &Scope,
    parent_collection: &[String],
    expected_collection: &[String],
    predicate: NodeId,
    graph: &Graph,
    ancestors_preserve_items: bool,
    filters: &mut Vec<NodeId>,
) {
    if scope.concatenated().is_some() {
        return;
    }
    let collection = scope.source().map(|source| {
        let mut collection = parent_collection.to_vec();
        collection.extend(source.iter().cloned());
        collection
    });
    let current = collection.as_deref().unwrap_or(parent_collection);
    let plain = scope.construction == ScopeConstruction::Constructed
        && matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Source(_)
        )
        && scope.post_group_filter.is_none()
        && !scope.has_sort()
        && scope.group_by.is_none()
        && scope.group_starting_with.is_none()
        && scope.group_adjacent_by.is_none()
        && scope.group_ending_with.is_none()
        && scope.group_into_blocks.is_none()
        && scope.windows.is_empty()
        && scope.iteration_output == IterationOutput::Repeated
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields;
    if scope.source().is_some()
        && ancestors_preserve_items
        && plain
        && current == expected_collection
        && let Some(filter) = scope.filter
        && matches!(graph.nodes.get(&filter), Some(Node::Call { function, args })
            if function == "not" && args.as_slice() == std::slice::from_ref(&predicate))
    {
        filters.push(filter);
    }
    let descendants_preserve_items = ancestors_preserve_items && plain && scope.filter.is_none();
    for child in &scope.children {
        collect_complementary_filters(
            child,
            current,
            expected_collection,
            predicate,
            graph,
            descendants_preserve_items,
            filters,
        );
    }
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
        && scope.post_group_filter.is_none()
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
    true_predicates: BTreeMap<BranchKey, NodeId>,
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
        let mut true_predicates = BTreeMap::new();
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
            if let (
                FailureIteration::Source { collection },
                FailureSelection::WhenTrue { predicate },
            ) = (&rule.iteration, rule.selection)
            {
                for filter in complementary_filters(project, collection, predicate) {
                    let key = (collection.clone(), filter);
                    by_key.entry(key.clone()).or_default().push(index);
                    true_predicates.insert(key, predicate);
                }
            }
            sinks.push(Sink {
                predicate: rule.selection.predicate(),
                message: rule.message,
                trigger_output: None,
            });
        }
        Self {
            by_key,
            true_predicates,
            sinks,
        }
    }

    pub(super) fn filter_predicate(&self, collection: &[String], filter: NodeId) -> NodeId {
        self.true_predicates
            .get(&(collection.to_vec(), filter))
            .copied()
            .unwrap_or(filter)
    }

    pub(super) fn target_is_false_branch(&self, collection: &[String], filter: NodeId) -> bool {
        self.true_predicates
            .contains_key(&(collection.to_vec(), filter))
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
