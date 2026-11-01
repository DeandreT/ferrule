//! Scope tree navigation and editing: pick which scope's bindings/filter/
//! source path you're looking at, then edit them with plain widgets (no
//! drag-and-drop onto the canvas yet -- see the module-level note in
//! `app.rs` for why that's out of scope for this first GUI pass).

use egui::Ui;
use ir::{SchemaKind, SchemaNode};
use mapping::{Binding, Graph, NodeId, Scope, ScopeConstruction, ScopeIteration};

use crate::path_picker::SourcePathCatalog;

pub(crate) mod computed_properties;
mod copy_controls;
pub(crate) mod lookup_admission;
mod output_controls;
pub(crate) mod presence_controls;
mod sort_controls;
mod source_iteration;
mod window_controls;

pub(crate) use copy_controls::{check_static_binding, copied_ancestor_at_path};
pub(crate) use output_controls::{ScopeOutputProfile, output_profile};

/// Path of child-indices from the project root to the scope being edited.
pub type ScopePath = Vec<usize>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticChildScopeCandidate {
    pub target_field: String,
    pub repeating: bool,
}

/// A bulk expansion changes only ordinary static child scopes. Repeating
/// targets still need their source iteration configured by the author.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScopeExpansion {
    pub created: usize,
    pub repeating_unconfigured: usize,
    pub skipped_incompatible: usize,
}

const MAX_SUBTREE_EXPANSION_GROUPS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeTreeError {
    InvalidScopePath(ScopePath),
    TargetScopeMissing(Vec<String>),
    TargetScopeNotGroup(Vec<String>),
    TargetChildMissing {
        parent: Vec<String>,
        target_field: String,
    },
    TargetChildNotGroup {
        parent: Vec<String>,
        target_field: String,
    },
    TargetChildAlreadyRepresented {
        parent: Vec<String>,
        target_field: String,
    },
    CannotExpandScope(Vec<String>),
    WholeGroupCopyContent(Vec<String>),
    ExpansionLimitExceeded(usize),
    CannotRemoveRoot,
    DocumentCollectionContent,
}

impl std::fmt::Display for ScopeTreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn path_label(path: &[String]) -> String {
            if path.is_empty() {
                "root".to_string()
            } else {
                path.join("/")
            }
        }

        match self {
            Self::InvalidScopePath(path) => write!(formatter, "invalid scope path {path:?}"),
            Self::TargetScopeMissing(path) => write!(
                formatter,
                "target scope {} does not exist in the target schema",
                path_label(path)
            ),
            Self::TargetScopeNotGroup(path) => write!(
                formatter,
                "target scope {} is not a group",
                path_label(path)
            ),
            Self::TargetChildMissing {
                parent,
                target_field,
            } => write!(
                formatter,
                "target group {} has no child named {target_field}",
                path_label(parent)
            ),
            Self::TargetChildNotGroup {
                parent,
                target_field,
            } => write!(
                formatter,
                "target child {}/{} is not a group",
                path_label(parent),
                target_field
            ),
            Self::TargetChildAlreadyRepresented {
                parent,
                target_field,
            } => write!(
                formatter,
                "target child {}/{} already has a scope",
                path_label(parent),
                target_field
            ),
            Self::CannotExpandScope(path) => write!(
                formatter,
                "target scope {} uses a construction or iteration mode that cannot contain an expanded subtree",
                path_label(path)
            ),
            Self::WholeGroupCopyContent(path) => write!(
                formatter,
                "target scope {} belongs to a whole source group copy; individual child scopes cannot be added",
                path_label(path)
            ),
            Self::ExpansionLimitExceeded(limit) => write!(
                formatter,
                "target subtree exceeds the {limit}-group expansion limit; select a smaller subtree"
            ),
            Self::CannotRemoveRoot => formatter.write_str("the root scope cannot be removed"),
            Self::DocumentCollectionContent => {
                formatter.write_str("document collection scopes are read-only")
            }
        }
    }
}

impl std::error::Error for ScopeTreeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupingMode {
    None,
    ByKey,
    AdjacentBy,
    StartingWith,
    EndingWith,
    IntoBlocks,
}

impl GroupingMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "No grouping",
            Self::ByKey => "By key",
            Self::AdjacentBy => "Adjacent keys",
            Self::StartingWith => "Starting predicate",
            Self::EndingWith => "Ending predicate",
            Self::IntoBlocks => "Fixed-size blocks",
        }
    }
}

fn grouping_mode(scope: &Scope) -> GroupingMode {
    if scope.group_by.is_some() {
        GroupingMode::ByKey
    } else if scope.group_adjacent_by.is_some() {
        GroupingMode::AdjacentBy
    } else if scope.group_starting_with.is_some() {
        GroupingMode::StartingWith
    } else if scope.group_ending_with.is_some() {
        GroupingMode::EndingWith
    } else if scope.group_into_blocks.is_some() {
        GroupingMode::IntoBlocks
    } else {
        GroupingMode::None
    }
}

fn set_grouping_mode(scope: &mut Scope, mode: GroupingMode, first_node: Option<NodeId>) {
    let node_for = |expected| (mode == expected).then_some(first_node).flatten();
    scope.group_by = node_for(GroupingMode::ByKey);
    scope.group_adjacent_by = node_for(GroupingMode::AdjacentBy);
    scope.group_starting_with = node_for(GroupingMode::StartingWith);
    scope.group_ending_with = node_for(GroupingMode::EndingWith);
    scope.group_into_blocks = node_for(GroupingMode::IntoBlocks);
    if mode == GroupingMode::None {
        scope.post_group_filter = None;
    }
}

fn prospective_grouping(
    scope: &Scope,
    mode: GroupingMode,
    graph: &Graph,
    paths: &SourcePathCatalog,
    ancestors: bool,
) -> Option<Scope> {
    let mut proposed = scope.clone();
    if mode == GroupingMode::None {
        set_grouping_mode(&mut proposed, mode, None);
        return lookup_admission::permits_change(graph, scope, &proposed, paths, ancestors)
            .then_some(proposed);
    }
    // Grouping changes the receiving context. Never seed the new control from
    // the old ungrouped choice list, even if its first Lookup is used elsewhere.
    for node in binding_node_ids(graph, false) {
        set_grouping_mode(&mut proposed, mode, Some(node));
        let policy = lookup_admission::Policy::for_scope(paths, &proposed, ancestors);
        if policy.allows(graph, node)
            && lookup_admission::permits_change(graph, scope, &proposed, paths, ancestors)
        {
            return Some(proposed);
        }
    }
    None
}

pub(crate) fn first_node_id(graph: &Graph) -> Option<NodeId> {
    binding_node_ids(graph, false).next()
}

fn binding_node_ids(
    graph: &Graph,
    primary_root_binding: bool,
) -> impl Iterator<Item = NodeId> + '_ {
    binding_node_ids_with_policy(
        graph,
        primary_root_binding,
        lookup_admission::Policy::default(),
    )
}

fn binding_node_ids_with_policy<'a>(
    graph: &'a Graph,
    primary_root_binding: bool,
    policy: lookup_admission::Policy<'a>,
) -> impl Iterator<Item = NodeId> + 'a {
    let unavailable =
        (!primary_root_binding).then(|| crate::primary_root_authoring::dependent_nodes(graph));
    graph.nodes.iter().filter_map(move |(&id, node)| {
        (!matches!(node, mapping::Node::Unconnected)
            && policy.allows(graph, id)
            && unavailable
                .as_ref()
                .is_none_or(|nodes| !nodes.contains(&id)))
        .then_some(id)
    })
}

fn generated_sequence_label(sequence: &mapping::SequenceExpr) -> &'static str {
    match sequence {
        mapping::SequenceExpr::Tokenize { .. } => "Split text",
        mapping::SequenceExpr::TokenizeByLength { .. } => "Fixed-length text",
        mapping::SequenceExpr::TokenizeRegex { .. } => "Regular expression",
        mapping::SequenceExpr::Generate { .. } => "Integer range",
        mapping::SequenceExpr::FilterMapV1(_) => "Filter/map",
        mapping::SequenceExpr::RecursiveCollect { .. } => "Recursive collection",
        mapping::SequenceExpr::OwnedRecursiveCollect(_) => "Owned recursive collection",
    }
}

fn join_summary(id: mapping::JoinId, plan: &mapping::JoinPlan) -> String {
    format!(
        "inner join #{} ({} sources, {} conditions)",
        id.get(),
        plan.sources().count(),
        plan.stages()
            .map(|(_, conditions)| conditions.iter().count())
            .sum::<usize>()
    )
}

fn join_source_label(source: &mapping::JoinSource) -> String {
    match source {
        mapping::JoinSource::Collection(source) if source.collection().is_empty() => {
            "Current rows".to_string()
        }
        mapping::JoinSource::Collection(source) => source.collection().join("/"),
        mapping::JoinSource::Computed(computed) => match computed.value() {
            mapping::ComputedJoinValue::Scalar(node) => {
                format!("computed key {} (node {node})", computed.id())
            }
            mapping::ComputedJoinValue::Sequence(sequence) => format!(
                "computed key {} ({} sequence)",
                computed.id(),
                sequence.kind_name()
            ),
        },
    }
}

fn join_condition_label(source: &mapping::JoinSource, key: &mapping::JoinKey) -> String {
    let left = match key.left() {
        mapping::JoinKeyLeft::Collection { collection, path } => collection
            .iter()
            .chain(path)
            .cloned()
            .collect::<Vec<_>>()
            .join("/"),
        mapping::JoinKeyLeft::Computed(id) => format!("computed key {id}"),
    };
    let right = match source {
        mapping::JoinSource::Collection(source) => source
            .collection()
            .iter()
            .chain(key.right_path())
            .cloned()
            .collect::<Vec<_>>()
            .join("/"),
        mapping::JoinSource::Computed(computed) => format!("computed key {}", computed.id()),
    };
    format!("{left} = {right}")
}

/// Lines describing a join plan with a computed key source, which the editor
/// shows read-only until authoring supports computed keys.
pub(crate) fn computed_join_lines(
    id: mapping::JoinId,
    plan: &mapping::JoinPlan,
) -> Option<Vec<String>> {
    plan.computed_source()?;
    let mut lines = vec![join_summary(id, plan)];
    lines.extend(
        plan.sources()
            .enumerate()
            .map(|(index, source)| format!("input {}: {}", index + 1, join_source_label(source))),
    );
    for (source, conditions) in plan.stages() {
        lines.extend(
            conditions
                .iter()
                .map(|condition| join_condition_label(source, condition)),
        );
    }
    Some(lines)
}

fn scope_iterates(scope: &Scope) -> bool {
    scope.iterates()
}

pub fn scope_at_mut<'a>(root: &'a mut Scope, path: &[usize]) -> &'a mut Scope {
    let mut scope = root;
    for &i in path {
        scope = &mut scope.children[i];
    }
    scope
}

fn scope_at<'a>(root: &'a Scope, path: &[usize]) -> Option<&'a Scope> {
    let mut scope = root;
    for &index in path {
        scope = scope.children.get(index)?;
    }
    Some(scope)
}

pub(crate) fn has_document_collector(root: &Scope, path: &[usize]) -> bool {
    let mut scope = root;
    for index in path.iter().copied().map(Some).chain(std::iter::once(None)) {
        if matches!(scope.construction, ScopeConstruction::CollectDocuments) {
            return true;
        }
        let Some(child) = index.and_then(|i| scope.children.get(i)) else {
            break;
        };
        scope = child;
    }
    false
}

fn scope_at_checked_mut<'a>(root: &'a mut Scope, path: &[usize]) -> Option<&'a mut Scope> {
    let Some((index, rest)) = path.split_first() else {
        return Some(root);
    };
    scope_at_checked_mut(root.children.get_mut(*index)?, rest)
}

fn target_scope_for_path<'a>(
    root: &Scope,
    target: &'a SchemaNode,
    path: &[usize],
) -> Result<(&'a SchemaNode, Vec<String>), ScopeTreeError> {
    let mut scope = root;
    let mut target_scope = target;
    let mut target_chain = Vec::new();
    for &index in path {
        let child = scope
            .children
            .get(index)
            .ok_or_else(|| ScopeTreeError::InvalidScopePath(path.to_vec()))?;
        target_chain.push(child.target_field.clone());
        target_scope = target_scope
            .child(&child.target_field)
            .ok_or_else(|| ScopeTreeError::TargetScopeMissing(target_chain.clone()))?;
        if !matches!(target_scope.kind, SchemaKind::Group { .. }) {
            return Err(ScopeTreeError::TargetScopeNotGroup(target_chain));
        }
        scope = child;
    }
    if !matches!(target_scope.kind, SchemaKind::Group { .. }) {
        return Err(ScopeTreeError::TargetScopeNotGroup(target_chain));
    }
    Ok((target_scope, target_chain))
}

pub fn available_static_child_scopes(
    root: &Scope,
    target: &SchemaNode,
    parent_path: &[usize],
) -> Result<Vec<StaticChildScopeCandidate>, ScopeTreeError> {
    if has_document_collector(root, parent_path) {
        return Err(ScopeTreeError::DocumentCollectionContent);
    }
    let parent = scope_at(root, parent_path)
        .ok_or_else(|| ScopeTreeError::InvalidScopePath(parent_path.to_vec()))?;
    let (target_parent, target_chain) = target_scope_for_path(root, target, parent_path)?;
    if copy_controls::copied_ancestor_at_path(root, parent_path).is_some() {
        return Err(ScopeTreeError::WholeGroupCopyContent(target_chain));
    }
    let SchemaKind::Group { children, .. } = &target_parent.kind else {
        return Ok(Vec::new());
    };
    Ok(children
        .iter()
        .filter(|child| matches!(child.kind, SchemaKind::Group { .. }))
        .filter(|child| {
            !parent
                .children
                .iter()
                .any(|scope| scope.target_field == child.name)
        })
        .map(|child| StaticChildScopeCandidate {
            target_field: child.name.clone(),
            repeating: child.repeating,
        })
        .collect())
}

pub fn create_static_child_scope(
    root: &mut Scope,
    target: &SchemaNode,
    parent_path: &[usize],
    target_field: &str,
) -> Result<ScopePath, ScopeTreeError> {
    if has_document_collector(root, parent_path) {
        return Err(ScopeTreeError::DocumentCollectionContent);
    }
    let parent = scope_at(root, parent_path)
        .ok_or_else(|| ScopeTreeError::InvalidScopePath(parent_path.to_vec()))?;
    let (target_parent, target_chain) = target_scope_for_path(root, target, parent_path)?;
    if copy_controls::copied_ancestor_at_path(root, parent_path).is_some() {
        return Err(ScopeTreeError::WholeGroupCopyContent(target_chain));
    }
    if parent
        .children
        .iter()
        .any(|scope| scope.target_field == target_field)
    {
        return Err(ScopeTreeError::TargetChildAlreadyRepresented {
            parent: target_chain,
            target_field: target_field.to_string(),
        });
    }
    let target_child =
        target_parent
            .child(target_field)
            .ok_or_else(|| ScopeTreeError::TargetChildMissing {
                parent: target_chain.clone(),
                target_field: target_field.to_string(),
            })?;
    if !matches!(target_child.kind, SchemaKind::Group { .. }) {
        return Err(ScopeTreeError::TargetChildNotGroup {
            parent: target_chain,
            target_field: target_field.to_string(),
        });
    }

    let parent = scope_at_checked_mut(root, parent_path)
        .ok_or_else(|| ScopeTreeError::InvalidScopePath(parent_path.to_vec()))?;
    let index = parent.children.len();
    parent.children.push(Scope {
        target_field: target_field.to_string(),
        ..Scope::default()
    });
    let mut created = parent_path.to_vec();
    created.push(index);
    Ok(created)
}

/// Adds every missing schema-declared group below `parent_path` without
/// changing existing scopes, graph nodes, bindings, or source iteration. An
/// incompatible existing branch is left intact and reported in the result.
/// Work is applied from a clone so over-budget expansion is atomic.
pub fn expand_static_target_subtree(
    root: &mut Scope,
    target: &SchemaNode,
    parent_path: &[usize],
) -> Result<ScopeExpansion, ScopeTreeError> {
    if has_document_collector(root, parent_path) {
        return Err(ScopeTreeError::DocumentCollectionContent);
    }
    let selected = scope_at(root, parent_path)
        .ok_or_else(|| ScopeTreeError::InvalidScopePath(parent_path.to_vec()))?;
    let (target_parent, target_chain) = target_scope_for_path(root, target, parent_path)?;
    if !can_expand_scope(selected) {
        return Err(ScopeTreeError::CannotExpandScope(target_chain));
    }
    if copy_controls::copied_ancestor_at_path(root, parent_path).is_some() {
        return Err(ScopeTreeError::WholeGroupCopyContent(target_chain));
    }
    let mut expanded = selected.clone();
    let mut visited = 0;
    let mut result = ScopeExpansion::default();
    expand_schema_groups(&mut expanded, target_parent, &mut visited, &mut result)?;
    if result.created > 0 {
        let selected = scope_at_checked_mut(root, parent_path)
            .ok_or_else(|| ScopeTreeError::InvalidScopePath(parent_path.to_vec()))?;
        *selected = expanded;
    }
    Ok(result)
}

fn can_expand_scope(scope: &Scope) -> bool {
    matches!(scope.construction, ScopeConstruction::Constructed) && scope.concatenated().is_none()
}

fn expand_schema_groups(
    scope: &mut Scope,
    schema: &SchemaNode,
    visited: &mut usize,
    result: &mut ScopeExpansion,
) -> Result<(), ScopeTreeError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Ok(());
    };
    for child in children
        .iter()
        .filter(|child| matches!(child.kind, SchemaKind::Group { .. }))
    {
        *visited += 1;
        if *visited > MAX_SUBTREE_EXPANSION_GROUPS {
            return Err(ScopeTreeError::ExpansionLimitExceeded(
                MAX_SUBTREE_EXPANSION_GROUPS,
            ));
        }
        let mut matches = scope
            .children
            .iter()
            .enumerate()
            .filter(|(_, existing)| existing.target_field == child.name);
        let first = matches.next().map(|(index, _)| index);
        if matches.next().is_some() {
            result.skipped_incompatible += 1;
            continue;
        }
        let index = match first {
            Some(index) => index,
            None => {
                scope.children.push(Scope {
                    target_field: child.name.clone(),
                    ..Scope::default()
                });
                result.created += 1;
                result.repeating_unconfigured += usize::from(child.repeating);
                scope.children.len() - 1
            }
        };
        let nested = &mut scope.children[index];
        if can_expand_scope(nested) {
            expand_schema_groups(nested, child, visited, result)?;
        } else {
            result.skipped_incompatible += 1;
        }
    }
    Ok(())
}

pub fn remove_child_scope(
    root: &mut Scope,
    selected_path: &[usize],
) -> Result<ScopePath, ScopeTreeError> {
    if has_document_collector(root, selected_path) {
        return Err(ScopeTreeError::DocumentCollectionContent);
    }
    let Some((&child_index, parent_path)) = selected_path.split_last() else {
        return Err(ScopeTreeError::CannotRemoveRoot);
    };
    let parent = scope_at_checked_mut(root, parent_path)
        .ok_or_else(|| ScopeTreeError::InvalidScopePath(selected_path.to_vec()))?;
    if child_index >= parent.children.len() {
        return Err(ScopeTreeError::InvalidScopePath(selected_path.to_vec()));
    }
    parent.children.remove(child_index);
    Ok(parent_path.to_vec())
}

pub fn scope_target_chain(root: &Scope, path: &[usize]) -> Vec<String> {
    let mut scope = root;
    let mut chain = Vec::new();
    for &index in path {
        let Some(child) = scope.children.get(index) else {
            break;
        };
        if !child.target_field.is_empty() {
            chain.push(child.target_field.clone());
        }
        scope = child;
    }
    chain
}

pub fn binding_target_fields(target: &SchemaNode, chain: &[String]) -> Vec<String> {
    let mut node = target;
    for segment in chain {
        let Some(child) = node.child(segment) else {
            return Vec::new();
        };
        node = child;
    }
    match &node.kind {
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => Vec::new(),
        SchemaKind::Group { children, .. } => {
            let mut fields: Vec<_> = children
                .iter()
                .filter(|child| {
                    matches!(
                        child.kind,
                        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }
                    )
                })
                .map(|child| child.name.clone())
                .collect();
            if !crate::target_xml_type::choices(node).is_empty() {
                fields.push(ir::XML_TYPE_FIELD.to_string());
            }
            fields
        }
    }
}

/// Renders the scope tree as clickable labels; returns the newly selected
/// path, if the user clicked one.
pub fn show_scope_tree(
    ui: &mut Ui,
    root: &Scope,
    selected: &ScopePath,
    reveal_selected: bool,
) -> Option<ScopePath> {
    let mut new_selection = None;
    show_scope_node(
        ui,
        root,
        &mut vec![],
        &mut vec![],
        selected,
        reveal_selected,
        &mut new_selection,
    );
    new_selection
}

fn show_scope_node(
    ui: &mut Ui,
    scope: &Scope,
    path: &mut ScopePath,
    target_path: &mut Vec<String>,
    selected: &ScopePath,
    reveal_selected: bool,
    new_selection: &mut Option<ScopePath>,
) {
    let label = target_path.last().map_or("Root scope", String::as_str);
    let is_selected = path == selected;
    let id = ui.make_persistent_id(("scope_disclosure", path.clone()));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
    if reveal_selected && selected.starts_with(path) {
        state.set_open(true);
    }
    let row = ui.horizontal(|ui| {
        if scope.children.is_empty() {
            ui.add_space(ui.spacing().indent);
        } else {
            state.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
        }
        let response = ui
            .add_sized(
                [ui.available_width(), crate::theme::METRICS.control_height],
                egui::Button::selectable(is_selected, "")
                    .left_text(label)
                    .wrap_mode(egui::TextWrapMode::Truncate),
            )
            .on_hover_text(if target_path.is_empty() {
                "Root scope".to_owned()
            } else {
                target_path.join(" / ")
            });
        if response.clicked() {
            *new_selection = Some(path.clone());
        }
        if is_selected && reveal_selected {
            response.scroll_to_me(None);
        }
    });
    if !scope.children.is_empty() {
        let body = |ui: &mut Ui| {
            for (i, child) in scope.children.iter().enumerate() {
                path.push(i);
                let child_label = if child.target_field.is_empty() {
                    format!("Child {}", i + 1)
                } else {
                    child.target_field.clone()
                };
                target_path.push(child_label.clone());
                show_scope_node(
                    ui,
                    child,
                    path,
                    target_path,
                    selected,
                    reveal_selected,
                    new_selection,
                );
                path.pop();
                target_path.pop();
            }
        };
        // Keep disclosure and selection reachable in deeply nested mappings.
        // The full target path remains available on every row's tooltip.
        if ui.available_width() > 180.0 {
            state.show_body_indented(&row.response, ui, body);
        } else {
            state.show_body_unindented(ui, body);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScopeEditorOwner<'a> {
    pub nested: bool,
    pub lookup_context: bool,
    pub primary_root_bindings: bool,
    pub target_xml_types: &'a [String],
    pub target_xml_default: Option<&'a str>,
    pub target_type_editable: bool,
}

/// Edits `scope`'s sequence controls and bindings.
pub(crate) fn show_scope_editor(
    ui: &mut Ui,
    scope: &mut Scope,
    graph: &Graph,
    source_paths: &SourcePathCatalog,
    target_fields: &[String],
    owner: ScopeEditorOwner<'_>,
    output_profile: ScopeOutputProfile,
) -> Option<crate::target_xml_type::Action> {
    lookup_admission::edit(
        ui,
        scope,
        graph,
        source_paths,
        owner.lookup_context,
        |ui, scope| {
            show_scope_editor_inner(
                ui,
                scope,
                graph,
                source_paths,
                target_fields,
                owner,
                output_profile,
            )
        },
    )
}

fn show_scope_editor_inner(
    ui: &mut Ui,
    scope: &mut Scope,
    graph: &Graph,
    source_paths: &SourcePathCatalog,
    target_fields: &[String],
    owner: ScopeEditorOwner<'_>,
    output_profile: ScopeOutputProfile,
) -> Option<crate::target_xml_type::Action> {
    let policy = lookup_admission::Policy::for_scope(source_paths, scope, owner.lookup_context);
    let first_node = binding_node_ids_with_policy(graph, false, policy).next();
    ui.strong(if scope.target_field.is_empty() {
        "Root scope".to_string()
    } else {
        format!("Scope · {}", scope.target_field)
    });

    if matches!(scope.construction, ScopeConstruction::CollectDocuments) {
        ui.label(
            "This scope collects ordered documents; its saved grouping and children are read-only.",
        );
        return None;
    }
    if matches!(scope.construction, ScopeConstruction::CopyCurrentRows) {
        ui.label("This scope writes whole CSV row blocks to named documents; its saved construction is read-only.");
        return None;
    }

    if let ScopeIteration::OwnedSource { source, .. } = &scope.iteration
        && let Some(dynamic) = source_paths.ownership().owned_dynamic_driver(&source.owner)
    {
        ui.label("This scope loads an owned dynamic input; this saved scope is read-only.");
        ui.monospace(format!(
            "loads: {}",
            mapping::describe_owned_collection(source)
        ));
        if let Some(driver) = dynamic.owned_driver() {
            ui.monospace(format!(
                "driver: {}",
                mapping::describe_owned_collection(driver)
            ));
        }
        ui.monospace(format!("document path node: {}", dynamic.node));
        return None;
    }
    if let ScopeIteration::OwnedSource {
        source,
        output_path,
    } = &scope.iteration
        && !source_iteration::owned_editable(scope)
    {
        ui.label("This owned source iteration's construction is not authorable; this saved scope is read-only.");
        ui.monospace(mapping::describe_owned_collection(source));
        if let Some(output_path) = output_path {
            ui.monospace(format!("output path node: {output_path}"));
        }
        if let ScopeConstruction::XmlMixedContent { elements } = &scope.construction {
            ui.monospace(owned_mixed_construction_label(elements));
        }
        return None;
    }
    if let Some(condition) = scope.presence() {
        ui.label("Constructs zero or one group in the parent item; item controls do not apply.");
        ui.monospace(format!("presence condition node: {condition}"));
        return None;
    }
    if let Some(lines) = scope
        .join()
        .and_then(|(id, plan)| computed_join_lines(id, plan))
    {
        ui.label("Computed join keys are not editable yet; this saved scope is read-only.");
        for line in lines {
            ui.monospace(line);
        }
        return None;
    }
    if let Some((id, plan)) = scope.iteration.owned_join() {
        ui.label("Owned join iteration is unavailable; this saved scope is read-only.");
        ui.monospace(format!("join id: {}", id.get()));
        for (index, source) in plan.sources().enumerate() {
            ui.monospace(format!(
                "input {}: {}",
                index + 1,
                mapping::describe_owned_collection(source.collection())
            ));
        }
        return None;
    }
    let whole_group_copy = matches!(scope.construction, ScopeConstruction::CopyCurrentSource);
    if whole_group_copy {
        copy_controls::show_reason(ui);
    }
    let type_action = crate::target_xml_type::show(
        ui,
        graph,
        scope,
        owner.target_xml_types,
        owner.target_xml_default,
        owner.target_type_editable
            && scope.construction == ScopeConstruction::Constructed
            && scope.concatenated().is_none(),
    );

    output_controls::show(ui, scope, output_profile);

    match &scope.iteration {
        ScopeIteration::OwnedInnerJoin { .. } | ScopeIteration::OptionalPresence { .. } => {
            return None;
        }
        ScopeIteration::Sequence(sequence) => {
            ui.horizontal(|ui| {
                ui.label("Iteration");
                ui.label(format!(
                    "generated ({})",
                    generated_sequence_label(sequence)
                ));
            });
        }
        ScopeIteration::InnerJoin { id, plan } => {
            ui.horizontal(|ui| {
                ui.label("Iteration");
                ui.label(join_summary(*id, plan));
            });
            for (index, source) in plan.sources().enumerate() {
                let collection = join_source_label(source);
                ui.horizontal(|ui| {
                    ui.label(format!("Input {}", index + 1));
                    ui.monospace(collection);
                });
            }
            for (source, conditions) in plan.stages() {
                for condition in conditions.iter() {
                    ui.horizontal(|ui| {
                        ui.label("Equal keys");
                        ui.monospace(join_condition_label(source, condition));
                    });
                }
            }
        }
        ScopeIteration::Concatenate(segments) => {
            ui.horizontal(|ui| {
                ui.label("Iteration");
                ui.label(format!("{} ordered row segments", segments.len()));
            });
            return type_action;
        }
        ScopeIteration::None
        | ScopeIteration::Source(_)
        | ScopeIteration::DynamicDocuments { .. }
        | ScopeIteration::OwnedSource { .. } => {
            source_iteration::show(ui, scope, source_paths, owner.nested);
        }
    }

    if let Some(mut output_path) = scope.output_path() {
        ui.horizontal(|ui| {
            ui.label("Output path");
            node_picker_with_policy(
                ui,
                "scope_output_path_node",
                &mut output_path,
                graph,
                policy,
            );
        });
        scope.set_output_path(Some(output_path));
    }

    if scope_iterates(scope) {
        crate::ui_kit::section(ui, "Filter & grouping", |_| {});
        ui.vertical(|ui| {
            let mut has_filter = scope.filter.is_some();
            if ui
                .add_enabled(
                    scope.filter.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_filter, "Filter items"),
                )
                .on_disabled_hover_text("Add a graph node before enabling a filter")
                .changed()
            {
                scope.filter = if has_filter { first_node } else { None };
            }
            if let Some(filter) = &mut scope.filter {
                node_picker_with_policy(ui, "filter_node", filter, graph, policy);
            }
        });

        if scope.join().is_none() {
            ui.add_enabled_ui(!whole_group_copy, |ui| {
                ui.vertical(|ui| {
                    ui.label("Grouping");
                    let mut mode = grouping_mode(scope);
                    let previous = mode;
                    ui.add_enabled_ui(mode != GroupingMode::None || first_node.is_some(), |ui| {
                        egui::ComboBox::from_id_salt("scope_grouping_mode")
                            .selected_text(mode.label())
                            .show_ui(ui, |ui| {
                                for choice in [
                                    GroupingMode::None,
                                    GroupingMode::ByKey,
                                    GroupingMode::AdjacentBy,
                                    GroupingMode::StartingWith,
                                    GroupingMode::EndingWith,
                                    GroupingMode::IntoBlocks,
                                ] {
                                    let allowed = choice == previous
                                        || prospective_grouping(
                                            scope,
                                            choice,
                                            graph,
                                            source_paths,
                                            owner.lookup_context,
                                        )
                                        .is_some();
                                    ui.add_enabled_ui(allowed, |ui| {
                                        ui.selectable_value(&mut mode, choice, choice.label());
                                    });
                                }
                            });
                    })
                    .response
                    .on_disabled_hover_text("Add a graph node before enabling grouping");
                    if mode != previous {
                        if let Some(proposed) = prospective_grouping(
                            scope,
                            mode,
                            graph,
                            source_paths,
                            owner.lookup_context,
                        ) {
                            *scope = proposed;
                        } else {
                            mode = previous;
                        }
                    }
                    let group_policy = lookup_admission::Policy::for_scope(
                        source_paths,
                        scope,
                        owner.lookup_context,
                    );
                    match mode {
                        GroupingMode::None => {}
                        GroupingMode::ByKey => {
                            if let Some(group_by) = &mut scope.group_by {
                                node_picker_with_policy(
                                    ui,
                                    "group_by_node",
                                    group_by,
                                    graph,
                                    group_policy,
                                );
                            }
                        }
                        GroupingMode::AdjacentBy => {
                            if let Some(group_by) = &mut scope.group_adjacent_by {
                                node_picker_with_policy(
                                    ui,
                                    "group_adjacent_by_node",
                                    group_by,
                                    graph,
                                    group_policy,
                                );
                            }
                        }
                        GroupingMode::StartingWith => {
                            if let Some(predicate) = &mut scope.group_starting_with {
                                node_picker_with_policy(
                                    ui,
                                    "group_starting_node",
                                    predicate,
                                    graph,
                                    group_policy,
                                );
                            }
                        }
                        GroupingMode::EndingWith => {
                            if let Some(predicate) = &mut scope.group_ending_with {
                                node_picker_with_policy(
                                    ui,
                                    "group_ending_node",
                                    predicate,
                                    graph,
                                    group_policy,
                                );
                            }
                        }
                        GroupingMode::IntoBlocks => {
                            if let Some(block_size) = &mut scope.group_into_blocks {
                                node_picker_with_policy(
                                    ui,
                                    "group_block_size_node",
                                    block_size,
                                    graph,
                                    group_policy,
                                );
                            }
                        }
                    }
                });
                if grouping_mode(scope) != GroupingMode::None {
                    let policy = lookup_admission::Policy::for_scope(
                        source_paths,
                        scope,
                        owner.lookup_context,
                    );
                    let first_node = binding_node_ids_with_policy(graph, false, policy).next();
                    ui.vertical(|ui| {
                        let mut enabled = scope.post_group_filter.is_some();
                        if ui
                            .add_enabled(
                                enabled || first_node.is_some(),
                                egui::Checkbox::new(&mut enabled, "Filter grouped items"),
                            )
                            .on_disabled_hover_text("Add a graph node before enabling a filter")
                            .changed()
                        {
                            scope.post_group_filter = enabled.then_some(first_node).flatten();
                        }
                        if let Some(filter) = &mut scope.post_group_filter {
                            node_picker_with_policy(
                                ui,
                                "post_group_filter_node",
                                filter,
                                graph,
                                policy,
                            );
                        }
                    });
                }
            });
        }

        let policy = lookup_admission::Policy::for_scope(source_paths, scope, owner.lookup_context);
        sort_controls::show(ui, scope, graph, policy);

        window_controls::show(ui, scope, graph, policy);
    }

    // Controls can change the owner in this UI frame; check the edited scope
    // again before offering root expressions for a new or replacement binding.
    let primary_root_binding = owner.primary_root_bindings
        && !owner.nested
        && crate::primary_root_authoring::flat_binding_scope(scope);
    let policy = lookup_admission::Policy::for_scope(source_paths, scope, owner.lookup_context);
    let first_binding_node =
        binding_node_ids_with_policy(graph, primary_root_binding, policy).next();
    crate::ui_kit::section(ui, "Bindings", |_| {});
    ui.add_enabled_ui(!whole_group_copy, |ui| {
        if scope.bindings.is_empty() {
            crate::ui_kit::help(ui, "Connect a target field to a graph expression.");
        }
        // A binding added by the keyboard takes focus on its first frame:
        // its fields are the next thing to set, and Add binding is disabled
        // once every target field is bound.
        let focus_new_binding = ui.scope_id().with("focus_new_binding");
        let focus_binding = ui.data_mut(|data| data.remove_temp::<usize>(focus_new_binding));
        let mut remove_idx = None;
        for (i, binding) in scope.bindings.iter_mut().enumerate() {
            ui.push_id(("binding", i), |ui| {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(format!("Binding {}", i + 1));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::ui_kit::icon_button(
                                ui,
                                lucide_icons::Icon::Trash2,
                                "Remove binding",
                                None,
                            )
                            .clicked()
                            {
                                remove_idx = Some(i);
                            }
                        });
                    });
                    crate::ui_kit::form_grid(ui, "binding_fields", |form| {
                        form.row("Target field", |ui| {
                            let target = egui::ComboBox::from_id_salt(("binding_target", i))
                                .selected_text(if binding.target_field.is_empty() {
                                    "Choose field…"
                                } else {
                                    crate::target_xml_type::field_label(&binding.target_field)
                                })
                                .width(ui.available_width().max(80.0))
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .show_ui(ui, |ui| {
                                    for field in target_fields {
                                        ui.selectable_value(
                                            &mut binding.target_field,
                                            field.clone(),
                                            crate::target_xml_type::field_label(field),
                                        );
                                    }
                                });
                            if focus_binding == Some(i) {
                                target.response.request_focus();
                            }
                        });
                        form.row("Expression", |ui| {
                            binding_node_picker(
                                ui,
                                format!("binding_{i}"),
                                &mut binding.node,
                                graph,
                                primary_root_binding,
                                policy,
                            );
                        });
                    });
                });
            });
        }
        if let Some(i) = remove_idx {
            scope.bindings.remove(i);
        }
        let next_target = target_fields
            .iter()
            .find(|field| {
                field.as_str() != ir::XML_TYPE_FIELD
                    && !scope
                        .bindings
                        .iter()
                        .any(|binding| binding.target_field.as_str() == field.as_str())
            })
            .cloned();
        let add = ui
            .add_enabled(
                first_binding_node.is_some() && next_target.is_some(),
                egui::Button::new("Add binding"),
            )
            .on_disabled_hover_text(if first_binding_node.is_none() {
                "Add a graph expression available to this target before creating a binding"
            } else {
                "Every scalar target field already has a binding"
            });
        if add.clicked()
            && let (Some(node), Some(target_field)) = (first_binding_node, next_target)
        {
            if add.has_focus() {
                let index = scope.bindings.len();
                ui.data_mut(|data| data.insert_temp(focus_new_binding, index));
                ui.ctx().request_repaint();
            }
            scope.bindings.push(Binding { target_field, node });
        }
    });
    type_action
}

pub(crate) fn node_picker(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    node_id: &mut NodeId,
    graph: &Graph,
) {
    node_picker_with_policy(
        ui,
        id_salt,
        node_id,
        graph,
        lookup_admission::Policy::default(),
    );
}

fn node_picker_with_policy(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    node_id: &mut NodeId,
    graph: &Graph,
    policy: lookup_admission::Policy<'_>,
) {
    binding_node_picker(ui, id_salt, node_id, graph, false, policy);
}

fn binding_node_picker(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    node_id: &mut NodeId,
    graph: &Graph,
    primary_root_binding: bool,
    policy: lookup_admission::Policy<'_>,
) {
    let current_label = graph.nodes.get(node_id).map_or_else(
        || egui::WidgetText::from("Missing expression"),
        |node| picker_label(ui, node, *node_id),
    );
    if !policy.allows(graph, *node_id) {
        ui.label(current_label)
            .on_hover_text(lookup_admission::READ_ONLY);
        return;
    }
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(current_label)
        .width(ui.available_width().max(80.0))
        .wrap_mode(egui::TextWrapMode::Truncate)
        .show_ui(ui, |ui| {
            let unavailable = (!primary_root_binding)
                .then(|| crate::primary_root_authoring::dependent_nodes(graph));
            for (&id, node) in &graph.nodes {
                if matches!(node, mapping::Node::Unconnected) {
                    continue;
                }
                let label = picker_label(ui, node, id);
                let allowed = policy.allows(graph, id) && unavailable.as_ref().is_none_or(|nodes| !nodes.contains(&id));
                let response = ui
                    .add_enabled_ui(allowed, |ui| {
                        ui.selectable_value(node_id, id, label)
                    })
                    .inner;
                if !allowed {
                    response.on_disabled_hover_text(
                        if !policy.allows(graph, id) { "Owned Lookup expressions require an ungrouped ordinary receiving context." } else { "Primary root expressions can only feed static fields on the flat primary target root" },
                    );
                }
            }
        });
}

fn node_label(node: &mapping::Node) -> String {
    crate::graph_viewer::readable_node_label(node)
}

fn picker_label(ui: &Ui, node: &mapping::Node, id: NodeId) -> egui::WidgetText {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &node_label(node),
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    job.append(
        &format!(" · #{id}"),
        0.0,
        egui::TextFormat {
            font_id,
            color: ui.visuals().weak_text_color(),
            ..Default::default()
        },
    );
    job.into()
}

/// The saved child renames of an owned selector's mixed-content construction.
fn owned_mixed_construction_label(elements: &[mapping::XmlMixedContentElement]) -> String {
    format!(
        "XML mixed content: {}",
        elements
            .iter()
            .map(|element| format!("{} → {}", element.source, element.target))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
#[path = "scope_editor/primary_root_tests.rs"]
mod primary_root_tests;

#[cfg(test)]
#[path = "scope_editor/ui_tests.rs"]
mod ui_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, ScalarTypeSet, Value};

    fn scope_management_target() -> SchemaNode {
        SchemaNode::group(
            "root",
            vec![
                SchemaNode::scalar("Id", ir::ScalarType::Int),
                SchemaNode::group(
                    "Orders",
                    vec![
                        SchemaNode::scalar("Number", ir::ScalarType::String),
                        SchemaNode::group(
                            "Lines",
                            vec![SchemaNode::scalar("Sku", ir::ScalarType::String)],
                        )
                        .repeating(),
                    ],
                ),
                SchemaNode::group(
                    "Customer",
                    vec![SchemaNode::scalar("Name", ir::ScalarType::String)],
                ),
            ],
        )
    }

    #[test]
    fn document_collector_cannot_be_expanded_into_ordinary_fields() {
        let target = scope_management_target();
        let mut root = Scope {
            construction: ScopeConstruction::CollectDocuments,
            ..Scope::default()
        };
        let before = root.clone();
        assert!(available_static_child_scopes(&root, &target, &[]).is_err());
        assert!(create_static_child_scope(&mut root, &target, &[], "Orders").is_err());
        assert!(expand_static_target_subtree(&mut root, &target, &[]).is_err());
        root.children.push(Scope::default());
        assert!(has_document_collector(&root, &[0]));
        assert_eq!(
            remove_child_scope(&mut root, &[0]),
            Err(ScopeTreeError::DocumentCollectionContent)
        );
        assert!(check_static_binding(&root, &["Orders".into()], "Number").is_err());
        root.children.clear();
        assert_eq!(root, before);
    }

    #[test]
    fn static_child_candidates_are_unrepresented_target_groups() {
        let root = Scope {
            children: vec![Scope {
                target_field: "Orders".into(),
                ..Scope::default()
            }],
            ..Scope::default()
        };
        let target = scope_management_target();

        assert_eq!(
            available_static_child_scopes(&root, &target, &[]),
            Ok(vec![StaticChildScopeCandidate {
                target_field: "Customer".into(),
                repeating: false,
            }])
        );
        assert_eq!(
            available_static_child_scopes(&root, &target, &[0]),
            Ok(vec![StaticChildScopeCandidate {
                target_field: "Lines".into(),
                repeating: true,
            }])
        );
        assert_eq!(
            available_static_child_scopes(&root, &target, &[7]),
            Err(ScopeTreeError::InvalidScopePath(vec![7]))
        );

        let missing_target = Scope {
            children: vec![Scope {
                target_field: "Missing".into(),
                ..Scope::default()
            }],
            ..Scope::default()
        };
        assert_eq!(
            available_static_child_scopes(&missing_target, &target, &[0]),
            Err(ScopeTreeError::TargetScopeMissing(vec!["Missing".into()]))
        );
        let scalar_target = Scope {
            children: vec![Scope {
                target_field: "Id".into(),
                ..Scope::default()
            }],
            ..Scope::default()
        };
        assert_eq!(
            available_static_child_scopes(&scalar_target, &target, &[0]),
            Err(ScopeTreeError::TargetScopeNotGroup(vec!["Id".into()]))
        );
    }

    #[test]
    fn union_leaves_are_available_as_binding_targets() {
        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Bool]) else {
            panic!("test union is valid");
        };
        let target = SchemaNode::group(
            "root",
            vec![
                SchemaNode::scalar("single", ScalarType::String),
                SchemaNode::scalar_union("union", types),
                SchemaNode::group("nested", Vec::new()),
            ],
        );

        assert_eq!(
            binding_target_fields(&target, &[]),
            ["single".to_string(), "union".to_string()]
        );
    }

    #[test]
    fn creating_static_children_validates_schema_and_duplicates() {
        let mut root = Scope {
            children: vec![Scope {
                target_field: "Orders".into(),
                ..Scope::default()
            }],
            ..Scope::default()
        };
        let target = scope_management_target();

        assert_eq!(
            create_static_child_scope(&mut root, &target, &[0], "Lines"),
            Ok(vec![0, 0])
        );
        let created = &root.children[0].children[0];
        assert_eq!(created.target_field, "Lines");
        assert!(!created.iterates());
        assert!(created.bindings.is_empty());
        assert_eq!(
            create_static_child_scope(&mut root, &target, &[0], "Lines"),
            Err(ScopeTreeError::TargetChildAlreadyRepresented {
                parent: vec!["Orders".into()],
                target_field: "Lines".into(),
            })
        );
        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(
            create_static_child_scope(&mut root, &target, &[], "Id"),
            Err(ScopeTreeError::TargetChildNotGroup {
                parent: Vec::new(),
                target_field: "Id".into(),
            })
        );
        assert_eq!(
            create_static_child_scope(&mut root, &target, &[], "Missing"),
            Err(ScopeTreeError::TargetChildMissing {
                parent: Vec::new(),
                target_field: "Missing".into(),
            })
        );
        assert_eq!(
            create_static_child_scope(&mut root, &target, &[9], "Lines"),
            Err(ScopeTreeError::InvalidScopePath(vec![9]))
        );
    }

    #[test]
    fn target_subtree_expansion_preserves_configured_scopes_and_is_idempotent() {
        let configured_order = Scope {
            target_field: "Orders".into(),
            iteration: ScopeIteration::Source(vec!["SourceOrders".into()]),
            bindings: vec![Binding {
                target_field: "Number".into(),
                node: 7,
            }],
            ..Scope::default()
        };
        let mut root = Scope {
            children: vec![configured_order.clone()],
            ..Scope::default()
        };
        let target = scope_management_target();

        assert_eq!(
            expand_static_target_subtree(&mut root, &target, &[]),
            Ok(ScopeExpansion {
                created: 2,
                repeating_unconfigured: 1,
                skipped_incompatible: 0,
            })
        );
        assert_eq!(root.children[0].source(), configured_order.source());
        assert_eq!(root.children[0].bindings.len(), 1);
        assert_eq!(root.children[0].bindings[0].target_field, "Number");
        assert_eq!(root.children[0].bindings[0].node, 7);
        assert_eq!(root.children[0].children[0].target_field, "Lines");
        assert!(!root.children[0].children[0].iterates());
        assert_eq!(root.children[1].target_field, "Customer");
        let snapshot = root.clone();
        assert_eq!(
            expand_static_target_subtree(&mut root, &target, &[]),
            Ok(ScopeExpansion::default())
        );
        assert_eq!(
            serde_json::to_value(&root).unwrap(),
            serde_json::to_value(&snapshot).unwrap()
        );
    }

    #[test]
    fn target_subtree_expansion_skips_incompatible_branches() {
        let target = SchemaNode::group(
            "root",
            vec![
                SchemaNode::group("Locked", vec![SchemaNode::group("Nested", vec![])]),
                SchemaNode::group("Sequence", vec![SchemaNode::group("Nested", vec![])]),
                SchemaNode::group("Open", vec![SchemaNode::group("Nested", vec![])]),
            ],
        );
        let locked = Scope {
            target_field: "Locked".into(),
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        };
        let sequence = Scope {
            target_field: "Sequence".into(),
            iteration: ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                Scope::default(),
                Vec::new(),
            )),
            ..Scope::default()
        };
        let mut root = Scope {
            children: vec![locked.clone(), sequence.clone()],
            ..Scope::default()
        };

        assert_eq!(
            expand_static_target_subtree(&mut root, &target, &[]),
            Ok(ScopeExpansion {
                created: 2,
                repeating_unconfigured: 0,
                skipped_incompatible: 2,
            })
        );
        assert_eq!(
            serde_json::to_value(&root.children[0]).unwrap(),
            serde_json::to_value(&locked).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&root.children[1]).unwrap(),
            serde_json::to_value(&sequence).unwrap()
        );
        assert_eq!(root.children[2].target_field, "Open");
        assert_eq!(root.children[2].children[0].target_field, "Nested");
        let snapshot = root.clone();
        assert_eq!(
            expand_static_target_subtree(&mut root, &target, &[0]),
            Err(ScopeTreeError::CannotExpandScope(vec!["Locked".into()]))
        );
        assert_eq!(
            serde_json::to_value(&root).unwrap(),
            serde_json::to_value(&snapshot).unwrap()
        );
    }

    #[test]
    fn target_subtree_expansion_limit_is_atomic() {
        let target = SchemaNode::group(
            "root",
            (0..=MAX_SUBTREE_EXPANSION_GROUPS)
                .map(|index| SchemaNode::group(format!("Group{index}"), vec![]))
                .collect(),
        );
        let mut root = Scope::default();
        assert_eq!(
            expand_static_target_subtree(&mut root, &target, &[]),
            Err(ScopeTreeError::ExpansionLimitExceeded(
                MAX_SUBTREE_EXPANSION_GROUPS
            ))
        );
        assert!(root.children.is_empty());
    }

    #[test]
    fn removing_a_child_returns_its_parent_and_protects_root() {
        let mut root = Scope {
            children: vec![
                Scope {
                    target_field: "Orders".into(),
                    children: vec![
                        Scope {
                            target_field: "Lines".into(),
                            ..Scope::default()
                        },
                        Scope {
                            target_field: "Summary".into(),
                            ..Scope::default()
                        },
                    ],
                    ..Scope::default()
                },
                Scope {
                    target_field: "Customer".into(),
                    ..Scope::default()
                },
            ],
            ..Scope::default()
        };

        assert_eq!(
            remove_child_scope(&mut root, &[]),
            Err(ScopeTreeError::CannotRemoveRoot)
        );
        assert_eq!(root.children.len(), 2);
        assert_eq!(remove_child_scope(&mut root, &[0, 0]), Ok(vec![0]));
        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].children[0].target_field, "Summary");
        assert_eq!(
            remove_child_scope(&mut root, &[3]),
            Err(ScopeTreeError::InvalidScopePath(vec![3]))
        );
        assert_eq!(root.children.len(), 2);
    }

    #[test]
    fn scope_references_default_to_an_existing_node_only() {
        let mut graph = Graph::default();
        assert_eq!(first_node_id(&graph), None);
        graph
            .nodes
            .insert(7, mapping::Node::Const { value: Value::Null });
        assert_eq!(first_node_id(&graph), Some(7));
    }

    #[test]
    fn grouping_mode_switches_clear_every_other_grouping_expression() {
        let mut scope = Scope::default();

        for mode in [
            GroupingMode::ByKey,
            GroupingMode::AdjacentBy,
            GroupingMode::StartingWith,
            GroupingMode::EndingWith,
            GroupingMode::IntoBlocks,
            GroupingMode::None,
        ] {
            set_grouping_mode(&mut scope, mode, Some(7));

            assert_eq!(grouping_mode(&scope), mode);
            let selected = [
                scope.group_by,
                scope.group_adjacent_by,
                scope.group_starting_with,
                scope.group_ending_with,
                scope.group_into_blocks,
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            if mode == GroupingMode::None {
                assert!(selected.is_empty());
            } else {
                assert_eq!(selected, [7]);
            }
        }
    }

    #[test]
    fn selected_scope_chain_drives_schema_backed_binding_choices() {
        let root = Scope {
            children: vec![Scope {
                target_field: "Orders".into(),
                children: vec![Scope {
                    target_field: "Order".into(),
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        };
        let target = SchemaNode::group(
            "root",
            vec![SchemaNode::group(
                "Orders",
                vec![SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Id", ir::ScalarType::Int),
                        SchemaNode::group(
                            "Details",
                            vec![SchemaNode::scalar("Name", ir::ScalarType::String)],
                        ),
                    ],
                )],
            )],
        );

        let chain = scope_target_chain(&root, &[0, 0]);
        assert_eq!(chain, ["Orders", "Order"]);
        assert_eq!(binding_target_fields(&target, &chain), ["Id"]);
        assert!(binding_target_fields(&target, &["Missing".into()]).is_empty());
    }

    #[test]
    fn generated_sequences_are_named_and_count_as_iteration() {
        let mut scope = Scope {
            iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
                input: 1,
                delimiter: 2,
                item: 3,
            }),
            ..Default::default()
        };
        assert!(scope_iterates(&scope));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "Split text"
        );

        scope.set_sequence(Some(mapping::SequenceExpr::TokenizeByLength {
            input: 1,
            length: 2,
            item: 3,
        }));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "Fixed-length text"
        );

        scope.set_sequence(Some(mapping::SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 3,
        }));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "Integer range"
        );

        scope.set_sequence(None);
        assert!(!scope_iterates(&scope));
        scope.set_source(Some(Vec::new()));
        assert!(scope_iterates(&scope));
    }

    #[test]
    fn joins_are_named_and_count_as_iteration() {
        let orders = mapping::JoinSource::new(vec!["orders".into()]);
        let products = mapping::JoinSource::new(vec!["products".into()]);
        let condition = mapping::JoinConditions::new(mapping::JoinKey::new(
            vec!["orders".into()],
            vec!["sku".into()],
            vec!["sku".into()],
        ));
        let plan = mapping::JoinPlan::new(orders, products, condition).unwrap();
        let scope = Scope {
            iteration: ScopeIteration::InnerJoin {
                id: mapping::JoinId::new(12),
                plan,
            },
            ..Scope::default()
        };

        assert!(scope_iterates(&scope));
        let Some((id, plan)) = scope.join() else {
            panic!("expected join iteration");
        };
        assert_eq!(
            join_summary(id, plan),
            "inner join #12 (2 sources, 1 conditions)"
        );
        let (source, conditions) = plan.stages().next().expect("join has one stage");
        assert_eq!(
            join_condition_label(source, conditions.iter().next().expect("stage has a key")),
            "orders/sku = products/sku"
        );
    }

    #[test]
    fn owned_joins_render_read_only_and_stay_unchanged() {
        use crate::test_ui::ContextExt as _;

        let rows = |owner| mapping::OwnedSourceRef {
            owner,
            path: vec!["Rows".into()],
        };
        let named = || mapping::SourceOwner::Named {
            name: "lookup".into(),
        };
        let plan = mapping::OwnedJoinPlan::new(
            mapping::OwnedJoinSource::new(rows(mapping::SourceOwner::Primary)),
            mapping::OwnedJoinSource::new(rows(named())),
            mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
                rows(mapping::SourceOwner::Primary),
                vec!["id".into()],
                vec!["id".into()],
            )),
        )
        .unwrap();
        let original = Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::OwnedInnerJoin {
                id: mapping::JoinId::new(9),
                plan,
            },
            ..Scope::default()
        };
        let mut scope = original.clone();
        let schema = ir::SchemaNode::group("Input", vec![]);
        let catalog = SourcePathCatalog::new(&schema, &[]);
        let context = egui::Context::default();
        let mut action = None;
        let _ = context.run_test_ui(Default::default(), |ui| {
            action = show_scope_editor(
                ui,
                &mut scope,
                &Graph::default(),
                &catalog,
                &[],
                ScopeEditorOwner {
                    lookup_context: true,
                    nested: true,
                    primary_root_bindings: false,
                    target_xml_types: &[],
                    target_xml_default: None,
                    target_type_editable: false,
                },
                ScopeOutputProfile::XmlElements,
            );
        });
        assert!(action.is_none());
        assert_eq!(
            serde_json::to_value(&scope).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        assert!(scope_iterates(&scope));
    }

    #[test]
    fn owned_dynamic_loading_scopes_render_read_only_and_stay_unchanged() {
        use crate::test_ui::ContextExt as _;

        let docs = mapping::SourceOwner::Named {
            name: "Docs".into(),
        };
        let original = Scope {
            target_field: "Docs".into(),
            iteration: ScopeIteration::OwnedSource {
                source: mapping::OwnedSourceRef {
                    owner: docs,
                    path: Vec::new(),
                },
                output_path: None,
            },
            bindings: vec![mapping::Binding {
                target_field: "Title".into(),
                node: 2,
            }],
            ..Scope::default()
        };
        assert!(source_iteration::owned_editable(&original));
        let schema = ir::SchemaNode::group(
            "Input",
            vec![
                ir::SchemaNode::group(
                    "Files",
                    vec![ir::SchemaNode::scalar("Path", ir::ScalarType::String)],
                )
                .repeating(),
            ],
        );
        let sources = [mapping::NamedSource {
            name: "Docs".into(),
            path: String::new(),
            schema: ir::SchemaNode::group(
                "Doc",
                vec![ir::SchemaNode::scalar("Title", ir::ScalarType::String)],
            ),
            options: Default::default(),
            dynamic_path: Some(mapping::DynamicSourcePath::owned(
                1,
                mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Primary,
                    path: vec!["Files".into()],
                },
            )),
        }];
        let catalog = SourcePathCatalog::new(&schema, &sources);
        let mut scope = original.clone();
        let context = egui::Context::default();
        let mut action = None;
        let _ = context.run_test_ui(Default::default(), |ui| {
            action = show_scope_editor(
                ui,
                &mut scope,
                &Graph::default(),
                &catalog,
                &[],
                ScopeEditorOwner {
                    lookup_context: true,
                    nested: true,
                    primary_root_bindings: false,
                    target_xml_types: &[],
                    target_xml_default: None,
                    target_type_editable: false,
                },
                ScopeOutputProfile::XmlElements,
            );
        });
        assert!(action.is_none());
        assert_eq!(
            serde_json::to_value(&scope).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
    }

    #[test]
    fn optional_presence_has_no_item_controls_and_stays_unchanged() {
        use crate::test_ui::ContextExt as _;

        let original = Scope {
            target_field: "Record".into(),
            iteration: ScopeIteration::OptionalPresence { condition: 4 },
            bindings: vec![mapping::Binding {
                target_field: "Type".into(),
                node: 2,
            }],
            ..Scope::default()
        };
        let mut scope = original.clone();
        let schema = ir::SchemaNode::group("Input", vec![]);
        let catalog = SourcePathCatalog::new(&schema, &[]);
        let context = egui::Context::default();
        let mut action = None;
        let _ = context.run_test_ui(Default::default(), |ui| {
            action = show_scope_editor(
                ui,
                &mut scope,
                &Graph::default(),
                &catalog,
                &[],
                ScopeEditorOwner {
                    lookup_context: true,
                    nested: true,
                    primary_root_bindings: false,
                    target_xml_types: &[],
                    target_xml_default: None,
                    target_type_editable: true,
                },
                ScopeOutputProfile::XmlElements,
            );
        });
        assert!(action.is_none());
        assert_eq!(
            serde_json::to_value(&scope).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
    }
}
