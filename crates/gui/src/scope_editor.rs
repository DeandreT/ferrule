//! Scope tree navigation and editing: pick which scope's bindings/filter/
//! source path you're looking at, then edit them with plain widgets (no
//! drag-and-drop onto the canvas yet -- see the module-level note in
//! `app.rs` for why that's out of scope for this first GUI pass).

use egui::Ui;
use ir::{SchemaKind, SchemaNode};
use mapping::{Binding, Graph, NodeId, Scope, ScopeIteration};

use crate::path_picker::SourcePathCatalog;

/// Path of child-indices from the project root to the scope being edited.
pub type ScopePath = Vec<usize>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticChildScopeCandidate {
    pub target_field: String,
    pub repeating: bool,
}

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
    CannotRemoveRoot,
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
            Self::CannotRemoveRoot => formatter.write_str("the root scope cannot be removed"),
        }
    }
}

impl std::error::Error for ScopeTreeError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupingMode {
    None,
    ByKey,
    StartingWith,
    IntoBlocks,
}

impl GroupingMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ByKey => "key",
            Self::StartingWith => "starting predicate",
            Self::IntoBlocks => "fixed blocks",
        }
    }
}

fn first_node_id(graph: &Graph) -> Option<NodeId> {
    graph.nodes.keys().next().copied()
}

fn generated_sequence_label(sequence: &mapping::SequenceExpr) -> &'static str {
    match sequence {
        mapping::SequenceExpr::Tokenize { .. } => "tokenize",
        mapping::SequenceExpr::TokenizeByLength { .. } => "tokenize-by-length",
        mapping::SequenceExpr::TokenizeRegex { .. } => "tokenize-regexp",
        mapping::SequenceExpr::Generate { .. } => "generate-sequence",
        mapping::SequenceExpr::RecursiveCollect { .. } => "recursive-collect",
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

fn join_condition_label(source: &mapping::JoinSource, key: &mapping::JoinKey) -> String {
    let mut left = key.left_collection().to_vec();
    left.extend(key.left_path().iter().cloned());
    let mut right = source.collection().to_vec();
    right.extend(key.right_path().iter().cloned());
    format!("{} = {}", left.join("/"), right.join("/"))
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
    let parent = scope_at(root, parent_path)
        .ok_or_else(|| ScopeTreeError::InvalidScopePath(parent_path.to_vec()))?;
    let (target_parent, _) = target_scope_for_path(root, target, parent_path)?;
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
    let parent = scope_at(root, parent_path)
        .ok_or_else(|| ScopeTreeError::InvalidScopePath(parent_path.to_vec()))?;
    let (target_parent, target_chain) = target_scope_for_path(root, target, parent_path)?;
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

pub fn remove_child_scope(
    root: &mut Scope,
    selected_path: &[usize],
) -> Result<ScopePath, ScopeTreeError> {
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
        SchemaKind::Scalar { .. } => Vec::new(),
        SchemaKind::Group { children, .. } => children
            .iter()
            .filter(|child| matches!(child.kind, SchemaKind::Scalar { .. }))
            .map(|child| child.name.clone())
            .collect(),
    }
}

/// Renders the scope tree as clickable labels; returns the newly selected
/// path, if the user clicked one.
pub fn show_scope_tree(ui: &mut Ui, root: &Scope, selected: &ScopePath) -> Option<ScopePath> {
    let mut new_selection = None;
    show_scope_node(ui, root, "root", &mut vec![], selected, &mut new_selection);
    new_selection
}

fn show_scope_node(
    ui: &mut Ui,
    scope: &Scope,
    label: &str,
    path: &mut ScopePath,
    selected: &ScopePath,
    new_selection: &mut Option<ScopePath>,
) {
    let is_selected = path == selected;
    let title = if is_selected {
        egui::RichText::new(label).strong()
    } else {
        egui::RichText::new(label)
    };
    let response = egui::CollapsingHeader::new(title)
        .id_salt(format!("{path:?}"))
        .default_open(true)
        .show(ui, |ui| {
            for (i, child) in scope.children.iter().enumerate() {
                path.push(i);
                let child_label = if child.target_field.is_empty() {
                    format!("child {i}")
                } else {
                    child.target_field.clone()
                };
                show_scope_node(ui, child, &child_label, path, selected, new_selection);
                path.pop();
            }
        });
    if response.header_response.clicked() {
        *new_selection = Some(path.clone());
    }
}

/// Edits `scope`'s sequence controls and bindings.
pub fn show_scope_editor(
    ui: &mut Ui,
    scope: &mut Scope,
    graph: &Graph,
    source_paths: &SourcePathCatalog,
    target_fields: &[String],
    nested: bool,
) {
    let first_node = first_node_id(graph);
    ui.strong(if scope.target_field.is_empty() {
        "root scope".to_string()
    } else {
        format!("scope: {}", scope.target_field)
    });

    match &scope.iteration {
        ScopeIteration::Sequence(sequence) => {
            ui.horizontal(|ui| {
                ui.label("iteration:");
                ui.label(format!(
                    "generated ({})",
                    generated_sequence_label(sequence)
                ));
            });
        }
        ScopeIteration::InnerJoin { id, plan } => {
            ui.horizontal(|ui| {
                ui.label("iteration:");
                ui.label(join_summary(*id, plan));
            });
            for (index, source) in plan.sources().enumerate() {
                let collection = if source.collection().is_empty() {
                    "<rows>".to_string()
                } else {
                    source.collection().join("/")
                };
                ui.horizontal(|ui| {
                    ui.label(format!("  input {}:", index + 1));
                    ui.monospace(collection);
                });
            }
            for (source, conditions) in plan.stages() {
                for condition in conditions.iter() {
                    ui.horizontal(|ui| {
                        ui.label("  on:");
                        ui.monospace(join_condition_label(source, condition));
                    });
                }
            }
        }
        ScopeIteration::Concatenate(segments) => {
            ui.horizontal(|ui| {
                ui.label("iteration:");
                ui.label(format!("{} ordered row segments", segments.len()));
            });
            return;
        }
        ScopeIteration::None
        | ScopeIteration::Source(_)
        | ScopeIteration::DynamicDocuments { .. } => {
            ui.horizontal(|ui| {
                ui.label("source path:");
                let mut has_source = scope.source().is_some();
                if ui.checkbox(&mut has_source, "iterates").changed() {
                    scope.set_source(has_source.then(Vec::new));
                }
            });
            if let Some(source) = scope.source_mut() {
                ui.horizontal(|ui| {
                    ui.label("  path:");
                    source_paths.show_scope_picker(ui, "scope_source_path", source, nested);
                });
            }
        }
    }

    if let Some(mut output_path) = scope.output_path() {
        ui.horizontal(|ui| {
            ui.label("output path node:");
            node_picker(ui, "scope_output_path_node", &mut output_path, graph);
        });
        scope.set_output_path(Some(output_path));
    }

    if scope_iterates(scope) {
        ui.horizontal(|ui| {
            ui.label("  filter node:");
            let mut has_filter = scope.filter.is_some();
            if ui
                .add_enabled(
                    scope.filter.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_filter, "filtered"),
                )
                .on_disabled_hover_text("Add a graph node before enabling a filter")
                .changed()
            {
                scope.filter = if has_filter { first_node } else { None };
            }
            if let Some(filter) = &mut scope.filter {
                node_picker(ui, "filter_node", filter, graph);
            }
        });

        if scope.join().is_none() {
            ui.horizontal(|ui| {
                ui.label("  grouping:");
                let mut mode = if scope.group_by.is_some() {
                    GroupingMode::ByKey
                } else if scope.group_starting_with.is_some() {
                    GroupingMode::StartingWith
                } else if scope.group_into_blocks.is_some() {
                    GroupingMode::IntoBlocks
                } else {
                    GroupingMode::None
                };
                let previous = mode;
                ui.add_enabled_ui(mode != GroupingMode::None || first_node.is_some(), |ui| {
                    egui::ComboBox::from_id_salt("scope_grouping_mode")
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for choice in [
                                GroupingMode::None,
                                GroupingMode::ByKey,
                                GroupingMode::StartingWith,
                                GroupingMode::IntoBlocks,
                            ] {
                                ui.selectable_value(&mut mode, choice, choice.label());
                            }
                        });
                })
                .response
                .on_disabled_hover_text("Add a graph node before enabling grouping");
                if mode != previous {
                    scope.group_by = if mode == GroupingMode::ByKey {
                        first_node
                    } else {
                        None
                    };
                    scope.group_starting_with = if mode == GroupingMode::StartingWith {
                        first_node
                    } else {
                        None
                    };
                    scope.group_into_blocks = if mode == GroupingMode::IntoBlocks {
                        first_node
                    } else {
                        None
                    };
                }
                match mode {
                    GroupingMode::None => {}
                    GroupingMode::ByKey => {
                        if let Some(group_by) = &mut scope.group_by {
                            node_picker(ui, "group_by_node", group_by, graph);
                        }
                    }
                    GroupingMode::StartingWith => {
                        if let Some(predicate) = &mut scope.group_starting_with {
                            node_picker(ui, "group_starting_node", predicate, graph);
                        }
                    }
                    GroupingMode::IntoBlocks => {
                        if let Some(block_size) = &mut scope.group_into_blocks {
                            node_picker(ui, "group_block_size_node", block_size, graph);
                        }
                    }
                }
            });
        }

        ui.horizontal(|ui| {
            ui.label("  sort key:");
            let mut has_sort = scope.sort_by.is_some();
            if ui
                .add_enabled(
                    scope.sort_by.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_sort, "sorted"),
                )
                .on_disabled_hover_text("Add a graph node before enabling sorting")
                .changed()
            {
                scope.sort_by = if has_sort { first_node } else { None };
            }
            if let Some(sort_by) = &mut scope.sort_by {
                node_picker(ui, "sort_by_node", sort_by, graph);
                ui.checkbox(&mut scope.sort_descending, "descending");
            }
        });

        ui.horizontal(|ui| {
            ui.label("  item limit:");
            let mut has_take = scope.take.is_some();
            if ui
                .add_enabled(
                    scope.take.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_take, "limited"),
                )
                .on_disabled_hover_text("Add a graph node before enabling an item limit")
                .changed()
            {
                scope.take = if has_take { first_node } else { None };
            }
            if let Some(take) = &mut scope.take {
                node_picker(ui, "take_node", take, graph);
            }
        });
    }

    ui.separator();
    ui.label("bindings (target field -> graph node):");
    let mut remove_idx = None;
    for (i, binding) in scope.bindings.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("binding_target", i))
                .selected_text(if binding.target_field.is_empty() {
                    "<target field>"
                } else {
                    &binding.target_field
                })
                .width(130.0)
                .show_ui(ui, |ui| {
                    for field in target_fields {
                        ui.selectable_value(&mut binding.target_field, field.clone(), field);
                    }
                });
            ui.label("->");
            node_picker(ui, format!("binding_{i}"), &mut binding.node, graph);
            if ui
                .small_button("x")
                .on_hover_text("Remove binding")
                .clicked()
            {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(i) = remove_idx {
        scope.bindings.remove(i);
    }
    let next_target = target_fields
        .iter()
        .find(|field| {
            !scope
                .bindings
                .iter()
                .any(|binding| binding.target_field.as_str() == field.as_str())
        })
        .cloned();
    if ui
        .add_enabled(
            first_node.is_some() && next_target.is_some(),
            egui::Button::new("+ binding").small(),
        )
        .on_disabled_hover_text(if first_node.is_none() {
            "Add a graph node before creating a binding"
        } else {
            "Every scalar target field already has a binding"
        })
        .clicked()
        && let (Some(node), Some(target_field)) = (first_node, next_target)
    {
        scope.bindings.push(Binding { target_field, node });
    }
}

fn node_picker(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    node_id: &mut NodeId,
    graph: &Graph,
) {
    let current_label = graph.nodes.get(node_id).map_or_else(
        || "<missing>".to_string(),
        |node| format!("{node_id}: {}", node_label(node)),
    );
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(current_label)
        .show_ui(ui, |ui| {
            for (&id, node) in &graph.nodes {
                let label = format!("{id}: {}", node_label(node));
                ui.selectable_value(node_id, id, label);
            }
        });
}

fn node_label(node: &mapping::Node) -> String {
    match node {
        mapping::Node::SourceField { path, .. } => format!("field {}", display_path(path)),
        mapping::Node::SourceDocumentPath => "source document path".to_string(),
        mapping::Node::Position { collection } => {
            format!("position {}", display_path(collection))
        }
        mapping::Node::JoinField {
            collection, path, ..
        } => {
            let mut field = collection.clone();
            field.extend(path.iter().cloned());
            format!("joined field {}", display_path(&field))
        }
        mapping::Node::JoinPosition { join } => format!("join {} position", join.get()),
        mapping::Node::Const { value } => format!("constant {value:?}"),
        mapping::Node::RuntimeValue { value } => format!("runtime {value:?}"),
        mapping::Node::Call { function, .. } => function.clone(),
        mapping::Node::If { .. } => "if".to_string(),
        mapping::Node::ValueMap { .. } => "value map".to_string(),
        mapping::Node::Lookup { collection, .. } => {
            format!("lookup {}", display_path(collection))
        }
        mapping::Node::DynamicSourceField { object, .. } => {
            format!("dynamic field {}", display_path(object))
        }
        mapping::Node::XmlMixedContent { path, .. } => {
            format!("XML mixed content {}", display_path(path))
        }
        mapping::Node::CollectionFind { collection, .. } => {
            format!("find {}", display_path(collection))
        }
        mapping::Node::SequenceExists { sequence, .. } => {
            format!("exists {}", generated_sequence_label(sequence))
        }
        mapping::Node::SequenceItemAt { sequence, .. } => {
            format!("item-at {}", generated_sequence_label(sequence))
        }
        mapping::Node::Aggregate {
            function,
            collection,
            ..
        } => format!("{function:?} {}", display_path(collection)).to_lowercase(),
        mapping::Node::JoinAggregate { function, join, .. } => {
            format!("{function:?} join {}", join.get()).to_lowercase()
        }
    }
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<current>".to_string()
    } else {
        path.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::Value;

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
            "tokenize"
        );

        scope.set_sequence(Some(mapping::SequenceExpr::TokenizeByLength {
            input: 1,
            length: 2,
            item: 3,
        }));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "tokenize-by-length"
        );

        scope.set_sequence(Some(mapping::SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 3,
        }));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "generate-sequence"
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
}
