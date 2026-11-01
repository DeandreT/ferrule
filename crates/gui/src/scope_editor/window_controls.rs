use super::{binding_node_ids_with_policy, lookup_admission::Policy, node_picker_with_policy};
use egui::Ui;
use mapping::{Graph, NodeId, Scope, SequenceWindow};

#[derive(Clone, Copy)]
enum WindowAction {
    Remove(usize),
    MoveUp(usize),
    MoveDown(usize),
}

pub(super) fn show(ui: &mut Ui, scope: &mut Scope, graph: &Graph, policy: Policy<'_>) {
    let first_node = binding_node_ids_with_policy(graph, false, policy).next();
    crate::ui_kit::section(ui, "Windows", |_| {});
    let count = scope.windows.len();
    let mut action = None;
    for (index, window) in scope.windows.iter_mut().enumerate() {
        ui.push_id(("sequence_window", index), |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("Window {}", index + 1));
                    if crate::ui_kit::icon_button(
                        ui,
                        lucide_icons::Icon::ArrowUp,
                        "Apply this window earlier",
                        (index == 0).then_some("Already the first window"),
                    )
                    .clicked()
                    {
                        action = Some(WindowAction::MoveUp(index));
                    }
                    if crate::ui_kit::icon_button(
                        ui,
                        lucide_icons::Icon::ArrowDown,
                        "Apply this window later",
                        (index + 1 == count).then_some("Already the last window"),
                    )
                    .clicked()
                    {
                        action = Some(WindowAction::MoveDown(index));
                    }
                    if crate::ui_kit::icon_button(
                        ui,
                        lucide_icons::Icon::Trash2,
                        "Remove window",
                        None,
                    )
                    .clicked()
                    {
                        action = Some(WindowAction::Remove(index));
                    }
                });
                let mut kind = WindowKind::of(*window);
                egui::ComboBox::from_id_salt(("window_kind", index))
                    .selected_text(kind.label())
                    .show_ui(ui, |ui| {
                        for candidate in WindowKind::ALL {
                            ui.selectable_value(&mut kind, candidate, candidate.label());
                        }
                    });
                if kind != WindowKind::of(*window) {
                    *window = kind.with_node(window.nodes().next().unwrap_or(0));
                }
                match window {
                    SequenceWindow::SkipFirst { count }
                    | SequenceWindow::First { count }
                    | SequenceWindow::Last { count } => {
                        ui.label("Item count");
                        node_picker_with_policy(ui, ("window_count", index), count, graph, policy);
                    }
                    SequenceWindow::From { position } => {
                        ui.label("Start position");
                        node_picker_with_policy(
                            ui,
                            ("window_position", index),
                            position,
                            graph,
                            policy,
                        );
                    }
                    SequenceWindow::FromTo { first, last } => {
                        ui.label("Start position");
                        node_picker_with_policy(ui, ("window_first", index), first, graph, policy);
                        ui.label("End position");
                        node_picker_with_policy(ui, ("window_last", index), last, graph, policy);
                    }
                }
            });
        });
    }
    match action {
        Some(WindowAction::Remove(index)) => {
            scope.windows.remove(index);
        }
        Some(WindowAction::MoveUp(index)) => scope.windows.swap(index, index - 1),
        Some(WindowAction::MoveDown(index)) => scope.windows.swap(index, index + 1),
        None => {}
    }
    if ui
        .add_enabled(first_node.is_some(), egui::Button::new("Add window"))
        .on_disabled_hover_text("Add a graph expression before creating a window")
        .clicked()
        && let Some(count) = first_node
    {
        scope.windows.push(SequenceWindow::First { count });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowKind {
    SkipFirst,
    First,
    From,
    FromTo,
    Last,
}

impl WindowKind {
    const ALL: [Self; 5] = [
        Self::SkipFirst,
        Self::First,
        Self::From,
        Self::FromTo,
        Self::Last,
    ];

    fn of(window: SequenceWindow) -> Self {
        match window {
            SequenceWindow::SkipFirst { .. } => Self::SkipFirst,
            SequenceWindow::First { .. } => Self::First,
            SequenceWindow::From { .. } => Self::From,
            SequenceWindow::FromTo { .. } => Self::FromTo,
            SequenceWindow::Last { .. } => Self::Last,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SkipFirst => "Skip first items",
            Self::First => "First items",
            Self::From => "From position",
            Self::FromTo => "Position range",
            Self::Last => "Last items",
        }
    }

    fn with_node(self, node: NodeId) -> SequenceWindow {
        match self {
            Self::SkipFirst => SequenceWindow::SkipFirst { count: node },
            Self::First => SequenceWindow::First { count: node },
            Self::From => SequenceWindow::From { position: node },
            Self::FromTo => SequenceWindow::FromTo {
                first: node,
                last: node,
            },
            Self::Last => SequenceWindow::Last { count: node },
        }
    }
}
