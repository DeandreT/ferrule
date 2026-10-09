use egui::Ui;
use mapping::{NodeId, Scope};

/// Identity remains separate from its printable label: equal imported names
/// do not merge distinct target slots or scope steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TargetOwner<'a> {
    CurrentPrimary,
    CurrentNamed,
    Named { index: usize, name: &'a str },
    InactivePrimary,
    InactiveNamed { index: usize, name: &'a str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScopeStep<'a> {
    Child { index: usize, field: &'a str },
    Segment(usize),
    DynamicChild(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReducerKind {
    Exists,
    ItemAt,
    Aggregate,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SequenceItemOwner<'a> {
    Scope {
        target: TargetOwner<'a>,
        path: Vec<ScopeStep<'a>>,
    },
    Graph {
        node: NodeId,
        kind: ReducerKind,
    },
    FailureRule {
        index: usize,
    },
}

impl SequenceItemOwner<'_> {
    pub(super) fn label(&self) -> String {
        match self {
            Self::Scope { target, path } => {
                let mut label = match target {
                    TargetOwner::CurrentPrimary => "current primary target".to_string(),
                    TargetOwner::CurrentNamed => "current named target".to_string(),
                    TargetOwner::Named { index, name } => {
                        format!("named target {} '{name}'", index + 1)
                    }
                    TargetOwner::InactivePrimary => "primary target".to_string(),
                    TargetOwner::InactiveNamed { index, name } => {
                        format!("other named target {} '{name}'", index + 1)
                    }
                };
                if path.is_empty() {
                    label.push_str(" / root scope");
                }
                for step in path {
                    match step {
                        ScopeStep::Child { index, field } => {
                            label.push_str(&format!(" / child {} '{field}'", index + 1));
                        }
                        ScopeStep::Segment(index) => {
                            label.push_str(&format!(" / segment {}", index + 1));
                        }
                        ScopeStep::DynamicChild(index) => {
                            label.push_str(&format!(" / dynamic child {}", index + 1));
                        }
                    }
                }
                label
            }
            Self::Graph { node, kind } => {
                let kind = match kind {
                    ReducerKind::Exists => "sequence-exists",
                    ReducerKind::ItemAt => "sequence-item-at",
                    ReducerKind::Aggregate => "sequence-aggregate",
                };
                format!("graph node {node} ({kind})")
            }
            Self::FailureRule { index } => format!("failure rule {}", index + 1),
        }
    }
}

pub(super) fn collect_scope_owners<'a>(
    root: &'a Scope,
    target: TargetOwner<'a>,
    item: NodeId,
    owners: &mut Vec<SequenceItemOwner<'a>>,
) {
    // The traversal borrows only scope metadata and uses an explicit stack;
    // schema/options/project data are not copied for node presentation.
    let mut pending = vec![(root, Vec::new())];
    while let Some((scope, path)) = pending.pop() {
        if scope
            .sequence()
            .is_some_and(|sequence| sequence.owned_items().contains(&item))
        {
            owners.push(SequenceItemOwner::Scope {
                target,
                path: path.clone(),
            });
        }
        for (index, child) in scope.dynamic_children.iter().enumerate().rev() {
            let mut child_path = path.clone();
            child_path.push(ScopeStep::DynamicChild(index));
            pending.push((&child.scope, child_path));
        }
        for (index, child) in scope.children.iter().enumerate().rev() {
            let mut child_path = path.clone();
            child_path.push(ScopeStep::Child {
                index,
                field: &child.target_field,
            });
            pending.push((child, child_path));
        }
        if let Some(segments) = scope.concatenated() {
            for (index, segment) in segments
                .iter()
                .enumerate()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let mut segment_path = path.clone();
                segment_path.push(ScopeStep::Segment(index));
                pending.push((segment, segment_path));
            }
        }
    }
}

pub(super) fn collect_scope_item_ids(root: &Scope, items: &mut std::collections::BTreeSet<NodeId>) {
    let mut pending = vec![root];
    while let Some(scope) = pending.pop() {
        if let Some(sequence) = scope.sequence() {
            items.extend(sequence.owned_items());
        }
        if let Some(segments) = scope.concatenated() {
            pending.extend(segments.iter());
        }
        pending.extend(scope.children.iter());
        pending.extend(scope.dynamic_children.iter().map(|child| &child.scope));
    }
}

pub(super) fn show(
    ui: &mut Ui,
    owners: &[SequenceItemOwner<'_>],
    path: &[String],
    frame: Option<&[String]>,
) {
    ui.vertical(|ui| {
        ui.set_max_width(super::SOURCE_FIELD_EDIT_WIDTH);
        ui.label("Generated item (read-only)").on_hover_text(
            "This node is owned by a generated sequence. Its source path must stay empty and unframed.",
        );
        for owner in owners {
            let label = owner.label();
            ui.add_sized(
                [super::SOURCE_FIELD_EDIT_WIDTH, ui.spacing().interact_size.y],
                egui::Label::new(&label).truncate(),
            )
            .on_hover_text(label);
        }
        if owners.len() > 1 {
            ui.colored_label(ui.visuals().error_fg_color, "Multiple sequence owners (unchanged)")
                .on_hover_text("Each generated sequence requires a unique item node. Existing ownership is retained for repair.");
        }
        if !path.is_empty() || frame.is_some() {
            let current_path = if path.is_empty() {
                "<empty>".to_string()
            } else {
                path.join("/")
            };
            let current_frame = frame.map_or_else(|| "<unframed>".to_string(), |value| value.join("/"));
            ui.colored_label(ui.visuals().error_fg_color, "Invalid item shape (unchanged)")
                .on_hover_text(format!(
                    "Owned items require an empty path with no frame. Existing metadata is retained for repair.\nPath: {current_path}\nFrame: {current_frame}"
                ));
        }
    });
}
