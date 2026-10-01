use super::*;

#[derive(Debug)]
pub(super) struct SourceRowFilter {
    query: String,
    stage: Option<u16>,
    indices: Vec<usize>,
}

pub(super) fn index_source_rows(events: &[cli::TraceEvent]) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(
                event,
                cli::TraceEvent::IterationCandidate {
                    source_row: Some(_),
                    ..
                }
            )
            .then_some(index)
        })
        .collect()
}

pub(super) fn filtered_source_row_indices(view: &RunReportView) -> Vec<usize> {
    let filter = view.trace_filter.trim().to_lowercase();
    if let Some(cached) = view.source_row_filter.borrow().as_ref()
        && cached.query == filter
        && cached.stage == view.history_stage
    {
        return cached.indices.clone();
    }
    let indices = view
        .source_rows
        .iter()
        .copied()
        .filter(|&index| {
            if view
                .history_stage
                .is_some_and(|stage| view.report.trace.event_stages.get(index) != Some(&stage))
            {
                return false;
            }
            view.report.trace.events.get(index).is_some_and(|event| {
                filter.is_empty()
                    || source_row_summary(index, event)
                        .is_some_and(|row| row.to_lowercase().contains(&filter))
                    || matches!(event, cli::TraceEvent::IterationCandidate {
                        source_row: Some(row), ..
                    } if row.structure.as_ref().is_some_and(|tree| tree_contains(tree, &filter)))
            })
        })
        .collect::<Vec<_>>();
    // Completed reports retain stable event indices. Search once per query or
    // selected stage; only the selected row needs formatted detail lines.
    *view.source_row_filter.borrow_mut() = Some(SourceRowFilter {
        query: filter,
        stage: view.history_stage,
        indices: indices.clone(),
    });
    indices
}

fn tree_contains(tree: &cli::TraceSourceTree, query: &str) -> bool {
    tree.name
        .as_ref()
        .is_some_and(|name| name.to_lowercase().contains(query))
        || format_output_kind(tree.kind).contains(query)
        || tree.value.as_ref().is_some_and(|value| {
            value.value_type.contains(query) || value.preview.to_lowercase().contains(query)
        })
        || tree
            .children
            .iter()
            .any(|child| tree_contains(child, query))
}

pub(super) fn show_source_rows(ui: &mut egui::Ui, view: &mut RunReportView) {
    show_history_stage_selector(ui, view);
    let stage_count = view
        .source_rows
        .iter()
        .filter(|&&index| {
            view.history_stage
                .is_none_or(|stage| view.report.trace.event_stages.get(index) == Some(&stage))
        })
        .count();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{stage_count} source rows"));
        ui.add(
            egui::TextEdit::singleline(&mut view.trace_filter)
                .hint_text("Filter source rows")
                .desired_width(280.0),
        );
        if !view.trace_filter.is_empty()
            && crate::icons::button(ui, true, lucide_icons::Icon::X, "Clear row filter").clicked()
        {
            view.trace_filter.clear();
        }
        if view.report.trace.dropped > 0 {
            ui.weak(format!(
                "Rows may be incomplete: {} later trace events omitted",
                view.report.trace.dropped
            ));
        }
    });
    ui.weak("Source candidates before filtering and sorting. Generated, join, and once iterations have no row preview.");
    ui.weak("Nested values use bounded snapshots; omitted values are marked.");
    ui.separator();

    let rows = filtered_source_row_indices(view);
    if rows.is_empty() {
        ui.weak("No matching source rows were recorded for this run.");
        return;
    }
    if !view
        .selected_source_row
        .is_some_and(|selected| rows.contains(&selected))
    {
        view.selected_source_row = rows.first().copied();
    }

    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
    egui::ScrollArea::vertical()
        .id_salt("run_source_rows")
        .max_height(180.0)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, rows.len(), |ui, range| {
            for &index in &rows[range] {
                let Some(event) = view.report.trace.events.get(index) else {
                    continue;
                };
                let Some(summary) = source_row_summary(index, event) else {
                    continue;
                };
                if ui
                    .selectable_label(view.selected_source_row == Some(index), &summary)
                    .clicked()
                {
                    view.selected_source_row = Some(index);
                }
            }
        });
    ui.separator();
    let Some(index) = view.selected_source_row else {
        return;
    };
    let Some(event) = view.report.trace.events.get(index) else {
        return;
    };
    let Some(lines) = source_row_details(index, event) else {
        return;
    };
    if replay_action(ui, index, "Replay from this row").clicked() && view.replay_from(index) {
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("run_source_row_detail")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for line in lines {
                ui.add(egui::Label::new(egui::RichText::new(&line).monospace()).selectable(true));
            }
        });
}

pub(super) fn source_row_summary(index: usize, event: &cli::TraceEvent) -> Option<String> {
    let cli::TraceEvent::IterationCandidate {
        scope,
        ordinal,
        positions,
        source_row: Some(source_row),
    } = event
    else {
        return None;
    };
    Some(format!(
        "event {:>6}  scope {}  candidate {ordinal}{}  {}",
        index + 1,
        format_trace_scope(scope),
        format_trace_positions(positions),
        format_source_row(source_row)
    ))
}

pub(super) fn source_row_details(index: usize, event: &cli::TraceEvent) -> Option<Vec<String>> {
    let cli::TraceEvent::IterationCandidate {
        source_row: Some(source_row),
        ..
    } = event
    else {
        return None;
    };
    let mut lines = vec![source_row_summary(index, event)?];
    if let Some(tree) = &source_row.structure {
        append_source_tree(&mut lines, tree, 1);
        return Some(lines);
    }
    for field in &source_row.fields {
        lines.push(format!("  {}", format_source_field(field)));
    }
    if source_row.omitted_fields > 0 {
        lines.push(format!(
            "  +{} more fields omitted",
            source_row.omitted_fields
        ));
    }
    Some(lines)
}

fn append_source_tree(lines: &mut Vec<String>, tree: &cli::TraceSourceTree, depth: usize) {
    let indent = "  ".repeat(depth);
    for (index, child) in tree.children.iter().enumerate() {
        let name = child.name.as_ref().map_or_else(
            || format!("[{}]", index + 1),
            |name| {
                if child.name_truncated {
                    format!("{name}...")
                } else {
                    name.clone()
                }
            },
        );
        let value = child.value.as_ref().map_or_else(
            || format_output_kind(child.kind).to_string(),
            format_trace_value,
        );
        lines.push(format!("{indent}{name}={value}"));
        append_source_tree(lines, child, depth + 1);
    }
    if tree.omitted_children > 0 {
        lines.push(format!(
            "{indent}+{} child values omitted{}",
            tree.omitted_children,
            if tree.depth_limited {
                " (snapshot depth limit)"
            } else {
                ""
            }
        ));
    }
}

pub(super) fn format_source_row(row: &cli::TraceSourceRow) -> String {
    if let Some(value) = &row.value {
        return format_trace_value(value);
    }
    if row.fields.is_empty() {
        return format_output_kind(row.kind).to_string();
    }
    let fields = row
        .fields
        .iter()
        .map(format_source_field)
        .collect::<Vec<_>>()
        .join(", ");
    let omitted = if row.omitted_fields > 0 {
        format!(", +{} more", row.omitted_fields)
    } else {
        String::new()
    };
    format!("{} [{fields}{omitted}]", format_output_kind(row.kind))
}

fn format_source_field(field: &cli::TraceSourceField) -> String {
    let name = if field.name_truncated {
        format!("{}...", field.name)
    } else {
        field.name.clone()
    };
    let value = field.value.as_ref().map_or_else(
        || format_output_kind(field.kind).to_string(),
        format_trace_value,
    );
    format!("{name}={value}")
}
