use super::{first_node_id, node_picker};
use egui::Ui;
use mapping::{Graph, NodeId, Scope, SequenceWindow};

#[derive(Clone, Copy)]
enum WindowAction {
    Remove(usize),
    MoveUp(usize),
    MoveDown(usize),
}

pub(super) fn show(ui: &mut Ui, scope: &mut Scope, graph: &Graph) {
    let first_node = first_node_id(graph);
    ui.label("  sequence windows:");
    let count = scope.windows.len();
    let mut action = None;
    for (index, window) in scope.windows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
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
                    node_picker(ui, ("window_count", index), count, graph);
                }
                SequenceWindow::From { position } => {
                    node_picker(ui, ("window_position", index), position, graph);
                }
                SequenceWindow::FromTo { first, last } => {
                    node_picker(ui, ("window_first", index), first, graph);
                    node_picker(ui, ("window_last", index), last, graph);
                }
            }
            if ui
                .add_enabled(index > 0, egui::Button::new("Up").small())
                .on_hover_text("Apply this window earlier")
                .clicked()
            {
                action = Some(WindowAction::MoveUp(index));
            }
            if ui
                .add_enabled(index + 1 < count, egui::Button::new("Down").small())
                .on_hover_text("Apply this window later")
                .clicked()
            {
                action = Some(WindowAction::MoveDown(index));
            }
            if ui
                .small_button("x")
                .on_hover_text("Remove window")
                .clicked()
            {
                action = Some(WindowAction::Remove(index));
            }
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
        .add_enabled(first_node.is_some(), egui::Button::new("+"))
        .on_hover_text("Add sequence window")
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
            Self::SkipFirst => "skip first",
            Self::First => "first",
            Self::From => "from",
            Self::FromTo => "from to",
            Self::Last => "last",
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
