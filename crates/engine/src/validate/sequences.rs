//! Generated item identities grant lexical permission; they do not pin a frame.
//! Value resolution remains innermost-first, including valid ancestor-item reads.

use std::collections::BTreeSet;

use mapping::{FailureIteration, Graph, Node, NodeId, Project, Scope, ScopeConstruction};

use super::graph::node_inputs;
use super::{
    ValidationEndpoint, ValidationIssue, ValidationOwner, ValidationScopeLocation,
    ValidationScopeStep,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ItemContext<'a> {
    Scope(&'a [NodeId]),
    Private(NodeId),
    Empty,
}

impl ItemContext<'_> {
    fn contains(self, item: NodeId) -> bool {
        match self {
            Self::Scope(items) => items.contains(&item),
            Self::Private(own) => own == item,
            Self::Empty => false,
        }
    }
}

#[derive(Clone, Copy)]
enum Origin<'a> {
    Site {
        location: &'a str,
        owner: &'a ValidationOwner,
    },
    Reducer(NodeId),
}

impl Origin<'_> {
    fn issue(self, expression: NodeId, item: NodeId) -> ValidationIssue {
        let message = format!(
            "expression {expression} references generated sequence item node {item} outside its owning context"
        );
        match self {
            Self::Site { location, owner } => {
                ValidationIssue::new(location, message).with_owner(owner.clone())
            }
            Self::Reducer(node) => ValidationIssue::new(format!("graph node {node}"), message)
                .with_owner(ValidationOwner::GraphNode {
                    function: None,
                    node,
                }),
        }
    }
}

/// Each reachable `(node, item context)` is checked once per expression site.
/// An explicit stack also handles graph cycles without recursive calls.
fn validate_root(
    graph: &Graph,
    items: &BTreeSet<NodeId>,
    expression: NodeId,
    context: ItemContext<'_>,
    origin: Origin<'_>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut pending = vec![(expression, context, origin)];
    let mut visited = BTreeSet::new();
    while let Some((node, context, origin)) = pending.pop() {
        if !visited.insert((node, context)) {
            continue;
        }
        if items.contains(&node) && !context.contains(node) {
            issues.push(origin.issue(expression, node));
            continue;
        }
        let Some(value) = graph.nodes.get(&node) else {
            continue;
        };
        match value {
            Node::SequenceExists {
                sequence,
                predicate,
            } => {
                let origin = Origin::Reducer(node);
                pending.push((*predicate, ItemContext::Private(sequence.item()), origin));
                pending.extend(
                    sequence
                        .inputs()
                        .into_iter()
                        .map(|input| (input, context, origin)),
                );
            }
            Node::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => {
                let origin = Origin::Reducer(node);
                let private = ItemContext::Private(sequence.item());
                pending.extend(
                    predicate
                        .iter()
                        .chain(expression)
                        .map(|input| (*input, private, origin)),
                );
                pending.extend(
                    sequence
                        .inputs()
                        .into_iter()
                        .chain(*arg)
                        .map(|input| (input, context, origin)),
                );
            }
            Node::SequenceItemAt { sequence, index } => {
                // Keep the existing ItemAt contract: it cannot consume another
                // generated context even when that context is an active parent.
                pending.extend(
                    sequence
                        .inputs()
                        .into_iter()
                        .chain([*index])
                        .map(|input| (input, ItemContext::Empty, Origin::Reducer(node))),
                );
            }
            _ => pending.extend(
                node_inputs(value)
                    .into_iter()
                    .map(|(_, input)| (input, context, origin)),
            ),
        }
    }
}

struct ScopeSite<'a> {
    scope: &'a Scope,
    parent_items: Vec<NodeId>,
    owner: ValidationScopeLocation,
    path: Vec<String>,
}

pub(super) fn validate_project(
    project: &Project,
    items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    if items.is_empty() {
        return;
    }
    // Dynamic paths run in a source driver frame, never in a target's
    // generated-item context. Reducers may still introduce their own private
    // item permission inside predicate/value expressions.
    for (index, source) in project.extra_sources.iter().enumerate() {
        let Some(dynamic) = &source.dynamic_path else {
            continue;
        };
        let location = format!("extra source `{}`", source.name.trim());
        let owner = ValidationOwner::Endpoint(ValidationEndpoint::NamedSource {
            index,
            name: source.name.clone(),
        });
        validate_root(
            &project.graph,
            items,
            dynamic.node,
            ItemContext::Empty,
            Origin::Site {
                location: &location,
                owner: &owner,
            },
            issues,
        );
    }
    let mut pending = Vec::new();
    for (index, target) in project.extra_targets.iter().enumerate().rev() {
        pending.push(ScopeSite {
            scope: &target.root,
            parent_items: Vec::new(),
            owner: ValidationScopeLocation::root(ValidationEndpoint::NamedTarget {
                index,
                name: target.name.clone(),
            }),
            path: Vec::new(),
        });
    }
    pending.push(ScopeSite {
        scope: &project.root,
        parent_items: Vec::new(),
        owner: ValidationScopeLocation::root(ValidationEndpoint::Target),
        path: Vec::new(),
    });
    while let Some(site) = pending.pop() {
        validate_scope(project, items, site, &mut pending, issues);
    }
    for (index, rule) in project.failure_rules.iter().enumerate() {
        let location = format!("failure rule {}", index + 1);
        let owner = ValidationOwner::FailureRule { index };
        let origin = Origin::Site {
            location: &location,
            owner: &owner,
        };
        let context = match &rule.iteration {
            FailureIteration::Sequence { sequence } => {
                for input in sequence.inputs() {
                    validate_root(
                        &project.graph,
                        items,
                        input,
                        ItemContext::Empty,
                        origin,
                        issues,
                    );
                }
                ItemContext::Private(sequence.item())
            }
            FailureIteration::Source { .. } => ItemContext::Empty,
        };
        for expression in rule.selection.predicate().into_iter().chain(rule.message) {
            validate_root(&project.graph, items, expression, context, origin, issues);
        }
    }
}

fn validate_scope<'a>(
    project: &Project,
    items: &BTreeSet<NodeId>,
    site: ScopeSite<'a>,
    pending: &mut Vec<ScopeSite<'a>>,
    issues: &mut Vec<ValidationIssue>,
) {
    let ScopeSite {
        scope,
        parent_items,
        owner,
        path,
    } = site;
    // The interpreter returns from concatenation before applying wrapper
    // controls/content. Segments all begin in the same enclosing context.
    if let Some(segments) = scope.concatenated() {
        for (index, segment) in segments
            .iter()
            .enumerate()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let mut path = path.clone();
            path.push(format!("<segment {}>", index + 1));
            pending.push(ScopeSite {
                scope: segment,
                parent_items: parent_items.clone(),
                owner: owner.descendant(ValidationScopeStep::Segment(index)),
                path,
            });
        }
        return;
    }
    let location = match (&owner.target, path.is_empty()) {
        (ValidationEndpoint::NamedTarget { name, .. }, true) => {
            format!("extra target `{name}` root scope")
        }
        (ValidationEndpoint::NamedTarget { name, .. }, false) => {
            format!("extra target `{name}` scope `{}`", path.join("/"))
        }
        (_, true) => "root scope".into(),
        (_, false) => format!("scope `{}`", path.join("/")),
    };
    let typed_owner = ValidationOwner::Scope(owner.clone());
    let origin = Origin::Site {
        location: &location,
        owner: &typed_owner,
    };
    let parent = ItemContext::Scope(&parent_items);
    let mut current_items = parent_items.clone();
    if let Some(sequence) = scope.sequence() {
        for input in sequence.inputs() {
            validate_root(&project.graph, items, input, parent, origin, issues);
        }
        current_items.push(sequence.item());
    }
    // Windows and block size execute once in the enclosing context, before
    // item construction. Other grouping expressions execute per candidate.
    for expression in scope
        .windows
        .iter()
        .copied()
        .flat_map(|window| window.nodes())
        .chain(scope.group_into_blocks)
    {
        validate_root(&project.graph, items, expression, parent, origin, issues);
    }
    let current = ItemContext::Scope(&current_items);
    let controls = [
        scope.filter,
        scope.post_group_filter,
        scope.group_by,
        scope.group_adjacent_by,
        scope.group_starting_with,
        scope.group_ending_with,
        scope.output_path(),
    ];
    for expression in controls
        .into_iter()
        .flatten()
        .chain(scope.sort_keys().map(|key| key.node))
    {
        validate_root(&project.graph, items, expression, current, origin, issues);
    }
    let construction = match &scope.construction {
        ScopeConstruction::Scalar { value } => Some(*value),
        ScopeConstruction::RecursiveFilter { plan } => Some(plan.predicate()),
        ScopeConstruction::AdjacencyTree { plan } => plan.root(),
        _ => None,
    };
    let bindings = scope
        .bindings
        .iter()
        .map(|binding| binding.node)
        .chain(
            scope
                .dynamic_bindings
                .iter()
                .flat_map(|binding| [binding.key, binding.value]),
        )
        .chain(scope.dynamic_children.iter().map(|child| child.key))
        .chain(construction);
    for expression in bindings {
        validate_root(&project.graph, items, expression, current, origin, issues);
    }
    // Dynamic child keys run in this parent's item context; their scope runs
    // afterwards and may introduce a different owned sequence item.
    for (index, child) in scope.dynamic_children.iter().enumerate().rev() {
        let mut path = path.clone();
        path.push("<dynamic>".into());
        pending.push(ScopeSite {
            scope: &child.scope,
            parent_items: current_items.clone(),
            owner: owner.descendant(ValidationScopeStep::DynamicChild(index)),
            path,
        });
    }
    for (index, child) in scope.children.iter().enumerate().rev() {
        let mut path = path.clone();
        path.push(child.target_field.clone());
        pending.push(ScopeSite {
            scope: child,
            parent_items: current_items.clone(),
            owner: owner.descendant(ValidationScopeStep::Child(index)),
            path,
        });
    }
}
