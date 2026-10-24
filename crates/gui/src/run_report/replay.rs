use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplayStep {
    First,
    Previous,
    Next,
    NextSelectedNode,
}

pub(super) fn replay_target(view: &RunReportView, step: ReplayStep) -> Option<usize> {
    let current = view
        .replay_event
        .filter(|&index| index < view.report.trace.events.len())?;
    match step {
        ReplayStep::First => (current > 0).then_some(0),
        ReplayStep::Previous => current.checked_sub(1),
        ReplayStep::Next => (current + 1 < view.report.trace.events.len()).then_some(current + 1),
        ReplayStep::NextSelectedNode => view
            .history_node
            .and_then(|node| view.history_by_node.get(&node))
            .and_then(|indices| indices.get(indices.partition_point(|&index| index <= current)))
            .copied()
            .filter(|&index| index < view.report.trace.events.len()),
    }
}

pub(super) fn show_replay(ui: &mut egui::Ui, view: &mut RunReportView) {
    ui.weak("Recorded trace navigation for a completed run. These controls do not rerun or pause the mapping.");
    let retained = view.report.trace.events.len();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{} retained (maximum {})",
            crate::ui_kit::plural(retained, "event"),
            format_count(MAX_TRACE_EVENTS),
        ));
        if view.report.trace.dropped > 0 {
            ui.weak(format!(
                "{} later events omitted; replay ends at the retained prefix",
                view.report.trace.dropped
            ));
        }
    });
    ui.separator();

    if !view.replay_event.is_some_and(|index| index < retained) {
        ui.weak("No recorded trace events are available for this run.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (step, label) in [
            (ReplayStep::First, "First"),
            (ReplayStep::Previous, "Previous"),
            (ReplayStep::Next, "Next"),
        ] {
            let target = replay_target(view, step);
            if ui
                .add_enabled(target.is_some(), egui::Button::new(label))
                .clicked()
            {
                view.replay_event = target;
            }
        }
        if let Some(index) = view.replay_event {
            ui.strong(format!(
                "Event {} of {}",
                format_count(index + 1),
                format_count(retained)
            ));
        }
    });
    show_history_stage_selector(ui, view);
    ui.horizontal_wrapped(|ui| {
        let selected_node_label = ui.label("Selected node");
        if let Some(node) = view.history_node {
            egui::ComboBox::from_id_salt("run_replay_node")
                .selected_text(view.node_label(node))
                .show_ui(ui, |ui| {
                    for (&candidate, events) in &view.history_by_node {
                        let label = format!(
                            "{} ({})",
                            view.node_label(candidate),
                            crate::ui_kit::plural(events.len(), "event")
                        );
                        ui.selectable_value(&mut view.history_node, Some(candidate), label);
                    }
                })
                .response
                .labelled_by(selected_node_label.id);
        } else {
            ui.weak("No node events");
        }
        let target = replay_target(view, ReplayStep::NextSelectedNode);
        if ui
            .add_enabled(target.is_some(), egui::Button::new("Next node occurrence"))
            .clicked()
        {
            view.replay_event = target;
        }
    });
    ui.weak("Node occurrences include recorded input and output values.");
    ui.separator();

    let Some(index) = view.replay_event else {
        return;
    };
    let Some(details) = replay_event_details_for_view(view, index) else {
        return;
    };
    egui::ScrollArea::both()
        .id_salt("run_replay_detail")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for line in details {
                ui.add(
                    egui::Label::new(egui::RichText::new(&line).monospace())
                        .selectable(true)
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        });
}

pub(super) fn replay_event_details_for_view(
    view: &RunReportView,
    index: usize,
) -> Option<Vec<String>> {
    let event = view.report.trace.events.get(index)?;
    let mut details = replay_event_details(index, event);
    if let Some(label) = view.trace_node_label(index, event) {
        details[0] = format!("{label} · {}", details[0]);
    }
    if let Some(stage) = view.report.trace.stage_at(index) {
        details.insert(0, format!("Stage: {stage}"));
    }
    Some(details)
}

pub(super) fn replay_event_details(index: usize, event: &cli::TraceEvent) -> Vec<String> {
    let mut details = vec![trace_row(index, event)];
    if let Some(positions) = event_positions(event) {
        let context = if positions.is_empty() {
            "Root context".to_owned()
        } else {
            positions
                .iter()
                .map(format_trace_position)
                .collect::<Vec<_>>()
                .join(" > ")
        };
        details.push(format!("Positions: {context}"));
    }
    if let Some(row_details) = source_row_details(index, event) {
        details.push("Source row fields for this candidate event:".to_owned());
        details.extend(row_details.into_iter().skip(1));
    }
    details
}

fn event_positions(event: &cli::TraceEvent) -> Option<&[cli::TracePosition]> {
    match event {
        cli::TraceEvent::NodeValue { positions, .. }
        | cli::TraceEvent::NodeInputValue { positions, .. }
        | cli::TraceEvent::FunctionNodeValue { positions, .. }
        | cli::TraceEvent::FunctionNodeInputValue { positions, .. }
        | cli::TraceEvent::ScopeStarted { positions, .. }
        | cli::TraceEvent::IterationCandidate { positions, .. }
        | cli::TraceEvent::FilterDecision { positions, .. }
        | cli::TraceEvent::SortCandidate { positions, .. }
        | cli::TraceEvent::SortPosition { positions, .. }
        | cli::TraceEvent::GroupProduced { positions, .. }
        | cli::TraceEvent::TargetFieldWritten { positions, .. }
        | cli::TraceEvent::TargetProduced { positions, .. } => Some(positions),
        cli::TraceEvent::WindowApplied { .. } | cli::TraceEvent::ScopeFinished { .. } => None,
    }
}
