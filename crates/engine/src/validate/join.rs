use std::collections::BTreeSet;

use ir::{SchemaNode, XML_TYPE_FIELD};
use mapping::{
    ComputedJoinSource, Graph, JoinId, JoinPlan, JoinSourceCardinality, Node, NodeId,
    NodeInputRole, OwnedJoinPlan, OwnedSourceRef, Project, Scope, SequenceExpr,
    describe_owned_collection,
};

use super::ValidationIssue;
use super::graph::validate_collection_value;
use super::node_inputs::node_inputs;

/// Sources of one join scope that dominates the expressions being validated.
/// Legacy joins name collections by path alone; owned joins by full identity.
#[derive(Clone)]
pub(super) enum ActiveJoinSources {
    Legacy(Vec<Vec<String>>),
    Owned(Vec<(OwnedSourceRef, JoinSourceCardinality)>),
}

pub(super) type ActiveJoin = (JoinId, ActiveJoinSources);

pub(super) fn active_legacy_join(join: JoinId, plan: &JoinPlan) -> ActiveJoin {
    (
        join,
        ActiveJoinSources::Legacy(
            plan.collection_sources()
                .map(|source| source.collection().to_vec())
                .collect(),
        ),
    )
}

pub(super) fn active_owned_join(join: JoinId, plan: &OwnedJoinPlan) -> ActiveJoin {
    (
        join,
        ActiveJoinSources::Owned(
            plan.sources()
                .map(|source| (source.collection().clone(), source.cardinality()))
                .collect(),
        ),
    )
}
use super::schema::{
    display_path, follow_schema, follow_schema_resolved, owned_source_root,
    root_source_path_direct, source_path_crosses_repetition, source_path_matches,
    source_path_matches_direct,
};

pub(super) fn validate_plan(
    project: &Project,
    join: JoinId,
    plan: &JoinPlan,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(computed) = plan.computed_source() {
        validate_computed_key(project, join, computed, location, issues);
    }
    for source in plan.collection_sources() {
        let collection = source.collection();
        let singleton = |node: &SchemaNode| !node.repeating && node.is_scalar();
        let valid = match source.cardinality() {
            JoinSourceCardinality::Repeating => {
                collection.is_empty()
                    || source_path_matches(project, collection, |node| node.repeating)
            }
            JoinSourceCardinality::Singleton => source_path_matches(project, collection, singleton),
        };
        if valid
            && source.cardinality() == JoinSourceCardinality::Singleton
            && !source_path_matches_direct(project, collection, singleton)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} singleton collection `{}` crosses a repeating source level; join the repeating collection with a key path instead",
                    join.get(),
                    display_path(collection)
                ),
            ));
        }
        if !valid {
            let expected = match source.cardinality() {
                JoinSourceCardinality::Repeating => "missing or not repeating",
                JoinSourceCardinality::Singleton => "missing or not a singleton scalar",
            };
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} collection `{}` is {expected}",
                    join.get(),
                    display_path(collection)
                ),
            ));
        }
    }
    // A computed member is its own key, with no key path to resolve.
    for (right, conditions) in plan.stages() {
        for key in conditions.iter() {
            if let Some(left) = key.left_collection() {
                validate_collection_value(
                    project,
                    location,
                    left,
                    key.left_path(),
                    "join left key",
                    issues,
                );
            }
            if let Some(right) = right.as_collection() {
                validate_collection_value(
                    project,
                    location,
                    right.collection(),
                    key.right_path(),
                    "join right key",
                    issues,
                );
            }
        }
    }
}

/// The nodes a computed key evaluates, in visit order, stopping below the
/// first node a key does not admit.
fn computed_key_nodes<'g>(
    graph: &'g Graph,
    computed: &ComputedJoinSource,
) -> Vec<(NodeId, &'g Node, bool)> {
    let mut nodes = Vec::new();
    let mut pending = computed.key_inputs();
    pending.reverse();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        // A missing input is reported by node-input validation.
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        let admitted = admitted_in_key(node);
        if admitted {
            pending.extend(node.dependencies().into_iter().rev());
        }
        nodes.push((id, node, admitted));
    }
    nodes
}

/// A computed key is one scalar per join execution, so it admits only node
/// kinds that give one value in the receiving context. Aggregates, lookups,
/// sequences, user functions, joined aggregates, positions and every join
/// read are refused. A frame-pinned read names a repetition frame whose
/// activity at the key is not checked here, so it is refused too.
fn admitted_in_key(node: &Node) -> bool {
    matches!(
        node,
        Node::SourceField { frame: None, .. }
            | Node::Const { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::RuntimeParameterDefault { .. }
            | Node::Call { .. }
            | Node::If { .. }
            | Node::ValueMap { .. }
    )
}

/// The unframed source paths a computed key reads, with the schema path
/// each read resolves: an XML type-marker read resolves its owner group.
fn computed_key_reads<'g>(
    project: &'g Project,
    computed: &ComputedJoinSource,
) -> impl Iterator<Item = (NodeId, &'g [String], &'g [String])> {
    computed_key_nodes(&project.graph, computed)
        .into_iter()
        .filter_map(move |(id, node, _)| match node {
            Node::SourceField { path, frame: None } => {
                let owner = path
                    .strip_suffix(&[XML_TYPE_FIELD.to_string()])
                    .filter(|owner| {
                        source_path_matches(project, owner, |node| !node.alternatives().is_empty())
                    });
                Some((id, path.as_slice(), owner.unwrap_or(path)))
            }
            _ => None,
        })
}

/// Refuses key nodes outside the admitted set, and source reads that could
/// cross repetition from any anchor: the engine reads the first item there,
/// while the native key is the whole sequence. A sequence-valued key admits
/// only `tokenize`, the one sequence whose native key was observed, and its
/// inputs follow the same rules as a scalar key.
fn validate_computed_key(
    project: &Project,
    join: JoinId,
    computed: &ComputedJoinSource,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(sequence) = computed.as_sequence()
        && !matches!(sequence, SequenceExpr::Tokenize { .. })
    {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "join {} computed key {} is a `{}` sequence; a sequence-valued key admits only tokenize",
                join.get(),
                computed.id(),
                sequence.kind_name()
            ),
        ));
        return;
    }
    for (id, _, admitted) in computed_key_nodes(&project.graph, computed) {
        if !admitted {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} computed key {} reads node {id}, which a computed key does not admit; use source fields, constants, calls, conditionals, value maps, runtime values or runtime parameters",
                    join.get(),
                    computed.id()
                ),
            ));
        }
    }
    for (id, path, resolved) in computed_key_reads(project, computed) {
        if source_path_crosses_repetition(project, resolved) {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} computed key {} source field node {id} `{}` crosses or ends at a repeating source level; a computed key reads one value",
                    join.get(),
                    computed.id(),
                    display_path(path)
                ),
            ));
        }
    }
}

/// Checks each owned source against its own endpoint schema and each key
/// against the exact owned collection it names. Source-ownership admission
/// refuses owned joins for execution regardless of these results.
pub(super) fn validate_owned_plan(
    project: &Project,
    join: JoinId,
    plan: &OwnedJoinPlan,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut collections = Vec::new();
    for source in plan.sources() {
        let collection = source.collection();
        let Some(root) = owned_source_root(project, &collection.owner, location, issues) else {
            collections.push((collection, None));
            continue;
        };
        let node = follow_schema_resolved(root, &collection.path);
        let (valid, expected) = match source.cardinality() {
            JoinSourceCardinality::Repeating => (
                collection.path.is_empty() || node.is_some_and(|node| node.repeating),
                "missing or not repeating",
            ),
            JoinSourceCardinality::Singleton => (
                node.is_some_and(|node| !node.repeating && node.is_scalar()),
                "missing or not a singleton scalar",
            ),
        };
        if !valid {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} collection {} is {expected}",
                    join.get(),
                    describe_owned_collection(collection)
                ),
            ));
        } else if source.cardinality() == JoinSourceCardinality::Singleton
            && crosses_repeating(root, &collection.path)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} singleton collection {} crosses a repeating source level; join the repeating collection with a key path instead",
                    join.get(),
                    describe_owned_collection(collection)
                ),
            ));
        }
        collections.push((collection, valid.then_some(node.unwrap_or(root))));
    }
    let resolved = |wanted: &OwnedSourceRef| {
        collections
            .iter()
            .find(|(collection, _)| *collection == wanted)
            .and_then(|(_, node)| *node)
    };
    for (right, conditions) in plan.stages() {
        for key in conditions.iter() {
            for (collection, path, label) in [
                (key.left_collection(), key.left_path(), "join left key"),
                (right.collection(), key.right_path(), "join right key"),
            ] {
                // An unresolved collection already has its own issue.
                let Some(node) = resolved(collection) else {
                    continue;
                };
                if !follow_schema(node, path).is_some_and(SchemaNode::is_scalar) {
                    issues.push(ValidationIssue::new(
                        location,
                        format!(
                            "{label} `{}` is not a scalar under collection {}",
                            display_path(path),
                            describe_owned_collection(collection)
                        ),
                    ));
                }
            }
        }
    }
}

fn crosses_repeating(root: &SchemaNode, path: &[String]) -> bool {
    (1..path.len())
        .any(|end| follow_schema_resolved(root, &path[..end]).is_some_and(|node| node.repeating))
}

/// Whether `source` is the primary root schema, where expressions see no
/// active item frame. Identity iterations reuse that schema and resolve
/// paths identically.
pub(super) fn is_root_context(project: &Project, source: Option<&SchemaNode>) -> bool {
    source.is_some_and(|source| std::ptr::eq(source, &project.source))
}

/// A root-context join has no active item frame, so each singleton must
/// resolve from the primary root or a named source rather than from a
/// descendant that exists only beneath some other collection.
pub(super) fn validate_root_singletons(
    project: &Project,
    join: JoinId,
    plan: &JoinPlan,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let singleton = |node: &SchemaNode| !node.repeating && node.is_scalar();
    for source in plan.collection_sources() {
        let collection = source.collection();
        // Missing or repetition-crossing paths already have their own issue.
        if source.cardinality() == JoinSourceCardinality::Singleton
            && source_path_matches_direct(project, collection, singleton)
            && !root_source_path_direct(project, collection).is_some_and(singleton)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} singleton collection `{}` does not resolve from the root source context; use a path from the source root or a named source",
                    join.get(),
                    display_path(collection)
                ),
            ));
        }
    }
    // A computed key's reads follow the same rule; missing and crossing
    // reads already have their own issue.
    let Some(computed) = plan.computed_source() else {
        return;
    };
    for (id, path, resolved) in computed_key_reads(project, computed) {
        if source_path_matches(project, resolved, |_| true)
            && !source_path_crosses_repetition(project, resolved)
            && root_source_path_direct(project, resolved).is_none()
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} computed key {} source field node {id} `{}` does not resolve from the root source context; use a path from the source root or a named source",
                    join.get(),
                    computed.id(),
                    display_path(path)
                ),
            ));
        }
    }
}

pub(super) fn validate_scope_nodes(
    graph: &Graph,
    scope: &Scope,
    active_joins: &[ActiveJoin],
    root_context: bool,
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_roots(
        graph,
        crate::scope_slots::roots(scope, crate::scope_slots::join_validation),
        active_joins,
        root_context,
        location,
        project,
        issues,
    );
}

/// `root_context` says whether `roots` evaluate without an active item frame;
/// per-item expression inputs never do.
pub(super) fn validate_roots(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
    active_joins: &[ActiveJoin],
    root_context: bool,
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_roots_inner(
        graph,
        roots.into_iter().map(|root| (root, root_context)),
        active_joins,
        location,
        project,
        issues,
        &mut BTreeSet::new(),
    );
}

/// Each pending root carries whether it evaluates in the root context.
fn validate_roots_inner(
    graph: &Graph,
    roots: impl IntoIterator<Item = (NodeId, bool)>,
    active_joins: &[ActiveJoin],
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
    ancestors: &mut BTreeSet<NodeId>,
) {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    while let Some((id, root_context)) = pending.pop() {
        if ancestors.contains(&id) || !visited.insert((id, root_context)) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        match node {
            Node::JoinField {
                join,
                collection,
                path,
            } => match active_joins.iter().rev().find(|(active, _)| active == join) {
                None => issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "join field node {id} references inactive join {}",
                        join.get()
                    ),
                )),
                Some((_, ActiveJoinSources::Owned(_))) => issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "join field node {id} references owned join {}; read its members with owned join fields",
                        join.get()
                    ),
                )),
                Some((_, ActiveJoinSources::Legacy(collections)))
                    if !collections.contains(collection) =>
                {
                    issues.push(ValidationIssue::new(
                        location,
                        format!(
                            "join field node {id} collection `{}` does not belong to join {}",
                            display_path(collection),
                            join.get()
                        ),
                    ));
                }
                Some(_) => validate_collection_value(
                    project,
                    location,
                    collection,
                    path,
                    "join field",
                    issues,
                ),
            },
            Node::OwnedJoinField(field) => {
                if let Some(cardinality) =
                    owned_member(active_joins, id, field.join, &field.collection, location, issues)
                {
                    match cardinality {
                        JoinSourceCardinality::Singleton if !field.path.is_empty() => {
                            issues.push(ValidationIssue::new(
                                location,
                                format!(
                                    "owned join field node {id} reads `{}` below singleton collection {}; a singleton member is read with an empty path",
                                    display_path(&field.path),
                                    describe_owned_collection(&field.collection)
                                ),
                            ));
                        }
                        JoinSourceCardinality::Singleton => {}
                        JoinSourceCardinality::Repeating => {
                            super::schema::validate_owned_item_scalar(
                                project,
                                &field.collection,
                                &field.path,
                                "owned join field",
                                location,
                                issues,
                            );
                        }
                    }
                }
            }
            Node::OwnedJoinSourcePosition(position) => {
                owned_member(
                    active_joins,
                    id,
                    position.join,
                    &position.collection,
                    location,
                    issues,
                );
            }
            Node::JoinPosition { join }
                if !active_joins.iter().any(|(active, _)| active == join) =>
            {
                issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "join position node {id} references inactive join {}",
                        join.get()
                    ),
                ));
            }
            Node::JoinAggregate { join, plan, .. } => {
                if root_context {
                    validate_root_singletons(project, *join, plan, location, issues);
                }
                let mut tuple_joins = active_joins.to_vec();
                tuple_joins.push(active_legacy_join(*join, plan));
                let tuple = TupleInputs {
                    graph,
                    consumer: id,
                    joins: &tuple_joins,
                    root_context,
                };
                tuple.validate(node, &mut pending, location, project, issues, ancestors);
            }
            // Only this join is active in its tuple expression; the argument
            // keeps the receiving context and its dominating joins.
            Node::OwnedJoinAggregate(aggregate) => {
                let tuple = TupleInputs {
                    graph,
                    consumer: id,
                    joins: &[active_owned_join(aggregate.join, &aggregate.plan)],
                    root_context,
                };
                tuple.validate(node, &mut pending, location, project, issues, ancestors);
            }
            _ => {
                let item_inputs = item_context_inputs(node);
                pending.extend(
                    node_inputs(node)
                        .into_iter()
                        .map(|(_, input)| (input, root_context && !item_inputs.contains(&input))),
                );
            }
        }
    }
}

/// The inputs of a join aggregate. Its per-tuple expression is validated at
/// once with the tuple's joins active and no root context; every other input
/// keeps the consumer's context and joins.
struct TupleInputs<'j> {
    graph: &'j Graph,
    consumer: NodeId,
    joins: &'j [ActiveJoin],
    root_context: bool,
}

impl TupleInputs<'_> {
    fn validate(
        &self,
        node: &Node,
        pending: &mut Vec<(NodeId, bool)>,
        location: &str,
        project: &Project,
        issues: &mut Vec<ValidationIssue>,
        ancestors: &mut BTreeSet<NodeId>,
    ) {
        node.visit_inputs(&mut |input| match input.role {
            NodeInputRole::JoinTuple => {
                ancestors.insert(self.consumer);
                validate_roots_inner(
                    self.graph,
                    [(input.node, false)],
                    self.joins,
                    location,
                    project,
                    issues,
                    ancestors,
                );
                ancestors.remove(&self.consumer);
            }
            _ => pending.push((input.node, self.root_context)),
        });
    }
}

/// The cardinality of the member named by an owned join read, or an issue
/// when no dominating owned join with that id has a source of that identity.
fn owned_member(
    active_joins: &[ActiveJoin],
    id: NodeId,
    join: JoinId,
    collection: &OwnedSourceRef,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<JoinSourceCardinality> {
    let message = match active_joins
        .iter()
        .rev()
        .find(|(active, _)| *active == join)
    {
        None => format!(
            "owned join node {id} references inactive join {}",
            join.get()
        ),
        Some((_, ActiveJoinSources::Legacy(_))) => format!(
            "owned join node {id} references join {}, which is not an owned join",
            join.get()
        ),
        Some((_, ActiveJoinSources::Owned(sources))) => {
            match sources.iter().find(|(source, _)| source == collection) {
                Some((_, cardinality)) => return Some(*cardinality),
                None => format!(
                    "owned join node {id} collection {} does not belong to join {}",
                    describe_owned_collection(collection),
                    join.get()
                ),
            }
        }
    };
    issues.push(ValidationIssue::new(location, message));
    None
}

/// The inputs evaluated with a new item, generated-item or occurrence frame,
/// which join validation reads without the consumer's root context, in visit
/// order. Every other input, including a lookup's needle and a replacement
/// kept in the parent frame, is evaluated in the consumer's own context.
pub(super) fn item_context_inputs(node: &Node) -> Vec<NodeId> {
    let mut inputs = Vec::new();
    node.visit_inputs(&mut |input| {
        if matches!(
            input.role,
            NodeInputRole::Item | NodeInputRole::GeneratedItem | NodeInputRole::Occurrence
        ) {
            inputs.push(input.node);
        }
    });
    inputs
}
