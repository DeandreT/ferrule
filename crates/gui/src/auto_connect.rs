//! Deterministic schema-to-schema binding suggestions for the visual editor.

use std::collections::{BTreeMap, BTreeSet};

use mapping::{Binding, Graph, Node, Scope, ScopeConstruction};

use crate::canvas::{SourceLeaf, TargetLeaf, source_leaves, target_leaves};
use crate::schema_scalar::ScalarDomain;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedConnection {
    pub source_frame: Option<Vec<String>>,
    pub source_path: Vec<String>,
    pub target_chain: Vec<String>,
    pub target_field: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoConnectPlan {
    pub connections: Vec<PlannedConnection>,
    pub skipped_ambiguous: usize,
    pub skipped_incompatible: usize,
}

impl AutoConnectPlan {
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingRoute {
    Available,
    Existing,
    Unavailable,
}

enum CandidateResolution<'a> {
    Missing,
    Match(&'a SourceLeaf),
    Ambiguous,
    Incompatible,
}

/// Plans bindings below one selected target scope. Source fields from the
/// selected iteration frame outrank outward, non-repeating broadcast fields.
pub fn plan_auto_connect(
    source_schema: &ir::SchemaNode,
    target_schema: &ir::SchemaNode,
    selected_scope: &Scope,
    selected_target_chain: &[String],
    source_frame_hint: Option<&[String]>,
) -> AutoConnectPlan {
    let sources = source_leaves(source_schema);
    let targets = target_leaves(target_schema);
    let mut plan = AutoConnectPlan::default();

    for target in targets {
        let Some(relative_parent) = target.chain.strip_prefix(selected_target_chain) else {
            continue;
        };
        match binding_route(selected_scope, relative_parent, &target.field) {
            BindingRoute::Existing => continue,
            BindingRoute::Unavailable => {
                plan.skipped_incompatible += 1;
                continue;
            }
            BindingRoute::Available => {}
        }

        match resolve_source(&sources, &target, relative_parent, source_frame_hint) {
            CandidateResolution::Match(source) => {
                plan.connections.push(PlannedConnection {
                    source_frame: source.frame.clone(),
                    source_path: source.path.clone(),
                    target_chain: relative_parent.to_vec(),
                    target_field: target.field,
                });
            }
            CandidateResolution::Ambiguous => plan.skipped_ambiguous += 1,
            CandidateResolution::Missing | CandidateResolution::Incompatible => {
                plan.skipped_incompatible += 1;
            }
        }
    }
    plan
}

/// Returns the innermost source iteration path that can identify a repetition
/// frame for the selected scope. Empty-path nested scopes inherit their parent.
pub fn source_frame_hint(root: &Scope, selected_path: &[usize]) -> Option<Vec<String>> {
    let mut scope = root;
    let mut hint = scope.source().map(<[String]>::to_vec);
    for &index in selected_path {
        scope = scope.children.get(index)?;
        if let Some(source) = scope.source()
            && (!source.is_empty() || hint.is_none())
        {
            hint = Some(source.to_vec());
        }
    }
    hint
}

pub fn scope_at<'a>(root: &'a Scope, path: &[usize]) -> Option<&'a Scope> {
    let mut scope = root;
    for &index in path {
        scope = scope.children.get(index)?;
    }
    Some(scope)
}

pub fn scope_at_mut<'a>(root: &'a mut Scope, path: &[usize]) -> Option<&'a mut Scope> {
    let mut scope = root;
    for &index in path {
        scope = scope.children.get_mut(index)?;
    }
    Some(scope)
}

/// Applies a previously confirmed plan without replacing bindings that may
/// have appeared since planning. Node identifiers are reserved before any
/// mutation, keeping identifier exhaustion atomic.
pub fn apply_auto_connect(
    graph: &mut Graph,
    selected_scope: &mut Scope,
    plan: &AutoConnectPlan,
) -> Result<usize, &'static str> {
    let accepted = plan
        .connections
        .iter()
        .filter(|connection| {
            binding_route(
                selected_scope,
                &connection.target_chain,
                &connection.target_field,
            ) == BindingRoute::Available
        })
        .collect::<Vec<_>>();
    let existing_fields = graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| match node {
            Node::SourceField { frame, path } => Some(((frame.clone(), path.clone()), id)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let required = accepted
        .iter()
        .map(|connection| {
            (
                connection.source_frame.clone(),
                connection.source_path.clone(),
            )
        })
        .filter(|source| !existing_fields.contains_key(source))
        .collect::<BTreeSet<_>>();
    let ids = reserve_node_ids(graph, required.len()).ok_or("mapping node IDs are exhausted")?;
    let mut fields = existing_fields;
    for (source, id) in required.into_iter().zip(ids) {
        graph.nodes.insert(
            id,
            Node::SourceField {
                frame: source.0.clone(),
                path: source.1.clone(),
            },
        );
        fields.insert(source, id);
    }

    for connection in &accepted {
        let key = (
            connection.source_frame.clone(),
            connection.source_path.clone(),
        );
        let Some(&node) = fields.get(&key) else {
            return Err("planned source field was not materialized");
        };
        let scope = ensure_scope(selected_scope, &connection.target_chain);
        scope.bindings.push(Binding {
            target_field: connection.target_field.clone(),
            node,
        });
    }
    Ok(accepted.len())
}

fn resolve_source<'a>(
    sources: &'a [SourceLeaf],
    target: &TargetLeaf,
    relative_parent: &[String],
    frame_hint: Option<&[String]>,
) -> CandidateResolution<'a> {
    let mut target_path = relative_parent.to_vec();
    target_path.push(target.field.clone());
    let normalized_target = normalize_name(&target.field);

    if let Some(frame_hint) = frame_hint {
        let framed = sources
            .iter()
            .filter(|source| frame_matches(source.frame.as_deref(), frame_hint))
            .collect::<Vec<_>>();
        let outward = sources
            .iter()
            .filter(|source| source.frame.is_none())
            .collect::<Vec<_>>();
        for candidates in [
            candidates_by_path(&framed, &target_path),
            candidates_by_path(&outward, &target_path),
            candidates_by_name(&framed, &normalized_target),
            candidates_by_name(&outward, &normalized_target),
        ] {
            match resolve_candidates(candidates, target.ty) {
                CandidateResolution::Missing => {}
                resolution => return resolution,
            }
        }
        CandidateResolution::Missing
    } else {
        for candidates in [
            sources
                .iter()
                .filter(|source| source.path == target_path)
                .collect(),
            sources
                .iter()
                .filter(|source| {
                    source
                        .path
                        .last()
                        .is_some_and(|name| normalize_name(name) == normalized_target)
                })
                .collect(),
        ] {
            match resolve_candidates(candidates, target.ty) {
                CandidateResolution::Missing => {}
                resolution => return resolution,
            }
        }
        CandidateResolution::Missing
    }
}

fn candidates_by_path<'a>(
    sources: &[&'a SourceLeaf],
    target_path: &[String],
) -> Vec<&'a SourceLeaf> {
    sources
        .iter()
        .copied()
        .filter(|source| source.path == target_path)
        .collect()
}

fn candidates_by_name<'a>(
    sources: &[&'a SourceLeaf],
    normalized_target: &str,
) -> Vec<&'a SourceLeaf> {
    sources
        .iter()
        .copied()
        .filter(|source| {
            source
                .path
                .last()
                .is_some_and(|name| normalize_name(name) == normalized_target)
        })
        .collect()
}

fn resolve_candidates(
    candidates: Vec<&SourceLeaf>,
    target_type: ScalarDomain,
) -> CandidateResolution<'_> {
    if candidates.is_empty() {
        return CandidateResolution::Missing;
    }
    let compatible = candidates
        .into_iter()
        .filter(|source| target_type.accepts_all_from(source.ty))
        .collect::<Vec<_>>();
    match compatible.as_slice() {
        [] => CandidateResolution::Incompatible,
        [source] => CandidateResolution::Match(source),
        [_, _, ..] => CandidateResolution::Ambiguous,
    }
}

fn frame_matches(frame: Option<&[String]>, hint: &[String]) -> bool {
    let Some(frame) = frame else {
        return false;
    };
    if hint.is_empty() {
        frame.is_empty()
    } else {
        frame == hint || frame.ends_with(hint)
    }
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn binding_route(scope: &Scope, chain: &[String], field: &str) -> BindingRoute {
    if !matches!(scope.construction, ScopeConstruction::Constructed)
        || scope.concatenated().is_some()
    {
        return BindingRoute::Unavailable;
    }
    let Some((first, rest)) = chain.split_first() else {
        return if scope
            .bindings
            .iter()
            .any(|binding| binding.target_field == field)
        {
            BindingRoute::Existing
        } else {
            BindingRoute::Available
        };
    };
    let mut matching = scope
        .children
        .iter()
        .filter(|child| child.target_field == *first);
    match (matching.next(), matching.next()) {
        (None, _) => BindingRoute::Available,
        (Some(child), None) => binding_route(child, rest, field),
        (Some(_), Some(_)) => BindingRoute::Unavailable,
    }
}

fn ensure_scope<'a>(scope: &'a mut Scope, chain: &[String]) -> &'a mut Scope {
    let Some((first, rest)) = chain.split_first() else {
        return scope;
    };
    let index = scope
        .children
        .iter()
        .position(|child| child.target_field == *first)
        .unwrap_or_else(|| {
            scope.children.push(Scope {
                target_field: first.clone(),
                ..Scope::default()
            });
            scope.children.len() - 1
        });
    ensure_scope(&mut scope.children[index], rest)
}

fn reserve_node_ids(graph: &Graph, count: usize) -> Option<Vec<mapping::NodeId>> {
    let mut ids = Vec::with_capacity(count);
    let mut candidate = 0_u32;
    while ids.len() < count {
        if !graph.nodes.contains_key(&candidate) {
            ids.push(candidate);
        }
        if ids.len() == count {
            break;
        }
        candidate = candidate.checked_add(1)?;
    }
    Some(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, ScalarTypeSet, SchemaNode};

    fn group(name: &str, fields: Vec<SchemaNode>) -> SchemaNode {
        SchemaNode::group(name, fields)
    }

    fn scalar_union(name: &str, types: impl IntoIterator<Item = ScalarType>) -> SchemaNode {
        let Some(types) = ScalarTypeSet::new(types) else {
            panic!("test union must contain distinct scalar types");
        };
        SchemaNode::scalar_union(name, types)
    }

    #[test]
    fn exact_relative_path_outranks_normalized_leaf_matches() {
        let source = group(
            "source",
            vec![
                group(
                    "customer",
                    vec![SchemaNode::scalar("CustomerID", ScalarType::String)],
                ),
                group(
                    "other",
                    vec![SchemaNode::scalar("customer_id", ScalarType::String)],
                ),
            ],
        );
        let target = group(
            "target",
            vec![group(
                "customer",
                vec![SchemaNode::scalar("CustomerID", ScalarType::String)],
            )],
        );

        let plan = plan_auto_connect(&source, &target, &Scope::default(), &[], None);

        assert_eq!(plan.connections.len(), 1);
        assert_eq!(
            plan.connections[0].source_path,
            vec!["customer", "CustomerID"]
        );
        assert_eq!(plan.skipped_ambiguous, 0);
    }

    #[test]
    fn unique_normalized_leaf_name_is_accepted_but_ambiguity_is_not_guessed() {
        let unique_source = group(
            "source",
            vec![SchemaNode::scalar("customer_id", ScalarType::String)],
        );
        let target = group(
            "target",
            vec![SchemaNode::scalar("CustomerID", ScalarType::String)],
        );
        let unique = plan_auto_connect(&unique_source, &target, &Scope::default(), &[], None);
        assert_eq!(unique.connections.len(), 1);

        let ambiguous_source = group(
            "source",
            vec![
                group(
                    "a",
                    vec![SchemaNode::scalar("customer_id", ScalarType::String)],
                ),
                group(
                    "b",
                    vec![SchemaNode::scalar("Customer-Id", ScalarType::String)],
                ),
            ],
        );
        let ambiguous = plan_auto_connect(&ambiguous_source, &target, &Scope::default(), &[], None);
        assert!(ambiguous.connections.is_empty());
        assert_eq!(ambiguous.skipped_ambiguous, 1);
    }

    #[test]
    fn iteration_frame_hint_selects_one_repeating_sibling() {
        let source = group(
            "source",
            vec![
                group("A", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
                group("B", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
            ],
        );
        let target = group("target", vec![SchemaNode::scalar("id", ScalarType::String)]);
        let mut scope = Scope::default();
        scope.set_source(Some(vec!["B".to_owned()]));

        let hint = source_frame_hint(&scope, &[]).expect("source frame is available");
        let plan = plan_auto_connect(&source, &target, &scope, &[], Some(&hint));

        assert_eq!(plan.connections.len(), 1);
        assert_eq!(plan.connections[0].source_frame, Some(vec!["B".to_owned()]));
    }

    #[test]
    fn incompatible_and_already_bound_targets_are_not_connected() {
        let source = group(
            "source",
            vec![
                SchemaNode::scalar("enabled", ScalarType::Bool),
                SchemaNode::scalar("kept", ScalarType::String),
            ],
        );
        let target = group(
            "target",
            vec![
                SchemaNode::scalar("enabled", ScalarType::String),
                SchemaNode::scalar("kept", ScalarType::String),
            ],
        );
        let mut scope = Scope::default();
        scope.bindings.push(Binding {
            target_field: "kept".to_owned(),
            node: 9,
        });

        let plan = plan_auto_connect(&source, &target, &scope, &[], None);

        assert!(plan.connections.is_empty());
        assert_eq!(plan.skipped_incompatible, 1);
    }

    #[test]
    fn unions_auto_connect_only_when_the_complete_source_domain_is_accepted() {
        let source = group(
            "source",
            vec![
                scalar_union("safe", [ScalarType::String, ScalarType::Int]),
                scalar_union("unsafe", [ScalarType::String, ScalarType::Bool]),
                scalar_union("partial", [ScalarType::String, ScalarType::Int]),
            ],
        );
        let target = group(
            "target",
            vec![
                scalar_union("safe", [ScalarType::String, ScalarType::Float]),
                scalar_union("unsafe", [ScalarType::String, ScalarType::Int]),
                SchemaNode::scalar("partial", ScalarType::String),
            ],
        );

        let plan = plan_auto_connect(&source, &target, &Scope::default(), &[], None);

        assert_eq!(plan.connections.len(), 1);
        assert_eq!(plan.connections[0].target_field, "safe");
        assert_eq!(plan.skipped_incompatible, 2);
    }

    #[test]
    fn scalar_to_union_auto_connect_keeps_integer_widening() {
        let source = group("source", vec![SchemaNode::scalar("value", ScalarType::Int)]);
        let target = group(
            "target",
            vec![scalar_union(
                "value",
                [ScalarType::String, ScalarType::Float],
            )],
        );

        let plan = plan_auto_connect(&source, &target, &Scope::default(), &[], None);

        assert_eq!(plan.connections.len(), 1);
        assert_eq!(plan.skipped_incompatible, 0);
    }

    #[test]
    fn apply_reuses_source_fields_and_never_creates_placeholders() {
        let source = group(
            "source",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        );
        let target = group(
            "target",
            vec![
                SchemaNode::scalar("value", ScalarType::String),
                group(
                    "nested",
                    vec![SchemaNode::scalar("value", ScalarType::String)],
                ),
            ],
        );
        let mut graph = Graph::default();
        let mut scope = Scope::default();
        let plan = plan_auto_connect(&source, &target, &scope, &[], None);

        assert_eq!(apply_auto_connect(&mut graph, &mut scope, &plan), Ok(2));
        assert_eq!(graph.nodes.len(), 1);
        assert!(
            graph
                .nodes
                .values()
                .all(|node| matches!(node, Node::SourceField { .. }))
        );
        assert_eq!(scope.bindings.len(), 1);
        assert_eq!(scope.children[0].bindings.len(), 1);
    }
}
