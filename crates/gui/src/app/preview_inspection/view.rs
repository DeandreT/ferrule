use super::*;
use egui::{Key, RichText, Ui};

enum Action {
    Refresh,
    EditInputs,
    Wiring(String),
    Pin,
}

impl FerruleApp {
    pub(in crate::app) fn show_captured_results(&mut self, ctx: &egui::Context) {
        let Some(view) = self.run_report.as_ref() else {
            self.show_run_report = false;
            return;
        };
        if !self.show_run_report {
            return;
        }
        let Some(capture) = self
            .preview_inspection
            .capture
            .as_ref()
            .filter(|_| view.report.kind == RunReportKind::Preview)
        else {
            crate::run_report::show(
                ctx,
                &mut self.show_run_report,
                self.run_report.as_mut().unwrap(),
            );
            return;
        };
        let stale = self
            .observed_editor
            .state
            .layout
            .project_fingerprint
            .as_ref()
            != Some(&capture.fingerprint)
            || !self.preview_inputs_current(capture);
        let can_refresh =
            !self.rest_run_busy() && self.pending_preview.is_none() && self.preview_draft.is_none();
        let inspection = &mut self.preview_inspection;
        let report = self.run_report.as_mut().unwrap();
        let response =
            crate::ui_kit::dialog(ctx, crate::ui_kit::DialogKind::Docked, "Preview results")
                .id_salt("run_results")
                .width(crate::ui_kit::DialogWidth::Wide)
                .show_body(|ui| inspection.show(ui, report, stale, can_refresh));
        self.show_run_report = !response.close_requested;
        match response.body {
            Some(Action::Refresh | Action::EditInputs) if can_refresh => {
                let edit = matches!(response.body, Some(Action::EditInputs));
                if let Some(draft) = self.preview_inspection.last_draft.as_ref() {
                    self.preview_draft = Some(draft.clone());
                    let captured = self.preview_inspection.capture.as_ref().unwrap();
                    if !captured.documents {
                        self.preview_input_source = captured.source;
                    }
                    self.show_run_report = false;
                    if !edit {
                        self.execute_preview();
                    }
                }
            }
            Some(Action::Wiring(field)) if self.fresh_preview().is_ok() => {
                let target = self
                    .preview_inspection
                    .capture
                    .as_ref()
                    .unwrap()
                    .target
                    .clone();
                if self.reveal_preview_field(&target, &field) {
                    self.show_run_report = false;
                } else {
                    self.preview_inspection.note =
                        Some(format!("Field `{field}` has no current canvas port."));
                }
            }
            Some(Action::Pin) if self.fresh_preview().is_ok() => {
                self.preview_inspection
                    .pin(self.run_report.as_ref().unwrap());
            }
            _ => {}
        }
    }
}

impl Inspection {
    pub(super) fn pin(&mut self, report: &RunReportView) {
        let Some(c) = &self.capture else { return };
        let Some(Ok(table)) = &self.table else { return };
        let index = report.selected_output();
        let (Some(output), Some(artifact)) =
            (report.report.outputs.get(index), c.artifacts.get(index))
        else {
            return;
        };
        self.pinned = Some(Pinned {
            target: c.target.clone(),
            path: output.path.clone(),
            input_path: c.input_path.clone(),
            input_sha: c.input_sha.clone(),
            sha: artifact.sha.clone(),
            table: Arc::clone(table),
        });
        self.change = 0;
        self.diff = None;
        self.pair_open = false;
        self.page = Page::Compare;
        self.row_focus = true;
    }

    fn show(
        &mut self,
        ui: &mut Ui,
        report: &mut RunReportView,
        stale: bool,
        can_refresh: bool,
    ) -> Option<Action> {
        let mut action = None;
        let fresh = !stale && self.attempt.is_none();
        let palette = crate::theme::active_palette(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.action, "MAPPING PREVIEW");
            ui.weak("In memory · No files written");
            if ui
                .add_enabled(can_refresh, egui::Button::new("Refresh preview"))
                .clicked()
            {
                action = Some(Action::Refresh);
            }
            if ui
                .add_enabled(can_refresh, egui::Button::new("Edit preview inputs"))
                .clicked()
            {
                action = Some(Action::EditInputs);
            }
        });
        if let Some(reason) = &self.attempt {
            ui.colored_label(palette.warning, reason);
            ui.weak("Showing the last completed snapshot.");
        } else if stale {
            ui.colored_label(
                palette.warning,
                "Stale preview · The mapping or preview inputs have changed.",
            );
        }
        ui.horizontal_wrapped(|ui| {
            for (page, label) in [
                (Page::Inspect, "Inspect row"),
                (Page::Compare, "Compare previews"),
                (Page::Scope, "Explain row counts"),
                (Page::Summary, "Summary & trace"),
            ] {
                if ui.selectable_value(&mut self.page, page, label).changed() {
                    self.row_focus = true;
                }
            }
        });
        ui.separator();
        if let Some(path) = &self.unavailable_output {
            ui.colored_label(
                palette.warning,
                format!(
                    "The previously selected artifact `{}` has no unique match in this preview.",
                    path.display()
                ),
            );
            if report.report.outputs.is_empty() {
                ui.weak("No output artifacts were produced.");
            } else {
                let mut chosen = None;
                egui::ComboBox::from_label("Replacement artifact")
                    .selected_text("Select a captured output")
                    .show_ui(ui, |ui| {
                        for (index, output) in report.report.outputs.iter().enumerate() {
                            let label = format!("{} · {}", index + 1, output.path.display());
                            if ui.selectable_label(false, label).clicked() {
                                chosen = Some(index);
                            }
                        }
                    });
                if let Some(index) = chosen {
                    self.unavailable_output = None;
                    report.select_output(index);
                    self.select(report);
                }
            }
            return action;
        }
        if self.page == Page::Summary {
            crate::run_report::show_report(ui, report);
            self.select(report);
            return action;
        }
        if report.report.outputs.len() > 1 {
            let mut chosen = report.selected_output();
            egui::ComboBox::from_label("Selected output")
                .selected_text(&report.report.outputs[chosen].name)
                .show_ui(ui, |ui| {
                    for (i, output) in report.report.outputs.iter().enumerate() {
                        ui.selectable_value(&mut chosen, i, &output.name);
                    }
                });
            report.select_output(chosen);
        }
        self.select(report);
        let Some(output) = report.report.outputs.get(report.selected_output()) else {
            ui.weak("No output artifacts were produced.");
            return action;
        };
        let identity = selected_output_identity(report);
        if let Err(reason) = &identity {
            ui.colored_label(palette.warning, reason);
        }
        ui.horizontal_wrapped(|ui| {
            ui.strong(self.capture.as_ref().unwrap().target.label());
            ui.monospace(output.path.display().to_string());
            if ui
                .add_enabled(
                    fresh && identity.is_ok() && matches!(self.table, Some(Ok(_))),
                    egui::Button::new(if self.pinned.is_some() {
                        "Replace pinned A"
                    } else {
                        "Pin as A"
                    }),
                )
                .clicked()
            {
                action = Some(Action::Pin);
            }
        });
        let table = match &self.table {
            Some(Ok(table)) => Arc::clone(table),
            Some(Err(message)) => {
                ui.weak(message);
                crate::run_report::show_output_content(ui, output, report.selected_output());
                return action;
            }
            None => {
                ui.weak("No captured output available.");
                return action;
            }
        };
        match self.page {
            Page::Inspect => {
                if let Some(field) = self.inspect(ui, report, &table, fresh && identity.is_ok()) {
                    action = Some(Action::Wiring(field));
                }
            }
            Page::Compare => self.compare(ui, report, &table),
            Page::Scope => self.explain(ui, &table),
            Page::Summary => {}
        }
        action
    }

    fn inspect(
        &mut self,
        ui: &mut Ui,
        report: &RunReportView,
        table: &PreviewTable,
        fresh: bool,
    ) -> Option<String> {
        if let Some(note) = &self.note {
            ui.colored_label(ui.visuals().warn_fg_color, note);
        }
        if table.rows.is_empty() {
            ui.strong("No data rows.");
            ui.weak(format!("{} columns captured.", table.headers.len()));
            return None;
        }
        let capture = self.capture.as_ref().unwrap();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Inspect the output");
            if ui
                .add_enabled(self.row > 0, egui::Button::new("Previous row"))
                .clicked()
            {
                self.row -= 1;
            }
            ui.strong(format!("Row {} of {}", self.row + 1, table.rows.len()));
            if ui
                .add_enabled(
                    self.row + 1 < table.rows.len(),
                    egui::Button::new("Next row"),
                )
                .clicked()
            {
                self.row += 1;
            }
        });
        let linked = self
            .matches
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|rows| {
                rows.items
                    .iter()
                    .position(|m| *m == RowMatch::Row(self.row))
            });
        let mut sample = self.sample;
        let mut selected_col = self.column;
        let mut focus = self.row_focus;
        let input_panel = |ui: &mut Ui| {
            panel(
                ui,
                "INPUT SAMPLE",
                crate::theme::active_palette(ui.ctx()).source,
                |ui| {
                    ui.weak(capture.input_path.display().to_string());
                    if capture.input_sha.is_none() {
                        ui.weak("Source bytes were not captured for this file preview.");
                        return;
                    }
                    let mut text = capture.excerpt.as_str();
                    let mut line = 1;
                    if let Ok(scope) = &capture.scope
                        && !scope.items.is_empty()
                    {
                        sample = sample.min(scope.items.len() - 1);
                        if let Some(index) = linked {
                            sample = index;
                            ui.weak("Input occurrence matched by selected key.");
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.weak(format!(
                                    "Reference item {} of {}",
                                    sample + 1,
                                    scope.items.len()
                                ));
                                if ui
                                    .add_enabled(sample > 0, egui::Button::new("‹"))
                                    .on_hover_text("Previous input sample")
                                    .clicked()
                                {
                                    sample -= 1;
                                }
                                if ui
                                    .add_enabled(
                                        sample + 1 < scope.items.len(),
                                        egui::Button::new("›"),
                                    )
                                    .on_hover_text("Next input sample")
                                    .clicked()
                                {
                                    sample += 1;
                                }
                            });
                            ui.weak("Independent input excerpt");
                        }
                        let item = &scope.items[sample];
                        line = item.line;
                        text = &text[item.excerpt.clone()];
                    } else {
                        ui.weak("Independent input excerpt");
                    }
                    let excerpt = &text[..text.floor_char_boundary(text.len().min(4096))];
                    egui::ScrollArea::both()
                        .id_salt("preview_input_excerpt")
                        .max_height(170.0)
                        .show(ui, |ui| {
                            for (i, text) in excerpt.lines().take(24).enumerate() {
                                ui.monospace(format!("{:>3}  {}", line + i as u32, text));
                            }
                        });
                    if capture.input_truncated
                        || excerpt.len() != text.len()
                        || excerpt.lines().nth(24).is_some()
                    {
                        ui.weak("Excerpt shortened.");
                    }
                },
            )
        };
        let mut output_panel = |ui: &mut Ui| {
            panel(
                ui,
                "OUTPUT / CSV",
                crate::theme::active_palette(ui.ctx()).target,
                |ui| {
                    if !table.has_headers {
                        ui.weak("Headerless CSV · Schema column names");
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("preview_fields")
                        .max_height(230.0)
                        .show(ui, |ui| {
                            for (col, (name, value)) in
                                table.headers.iter().zip(&table.rows[self.row]).enumerate()
                            {
                                ui.horizontal(|ui| {
                                    let response = ui.selectable_label(selected_col == col, name);
                                    if response.clicked() {
                                        selected_col = col;
                                    }
                                    if selected_col == col && focus {
                                        response.request_focus();
                                        response.scroll_to_me(None);
                                        focus = false;
                                    }
                                    short_label(ui, value);
                                });
                            }
                        });
                },
            )
        };
        two_panels(ui, input_panel, &mut output_panel);
        self.sample = sample;
        self.column = selected_col;
        self.row_focus = focus;
        let key = (self.row, self.column);
        if self
            .cell_trace
            .as_ref()
            .is_none_or(|(cached, _)| *cached != key)
        {
            let trace = capture
                .trace
                .as_ref()
                .and_then(|t| t.cell(&report.report.trace, table, self.row, self.column));
            self.cell_trace = Some((key, trace));
        }
        let field = table.headers.get(self.column)?;
        ui.separator();
        ui.strong(format!("Selected cell · Row {} · {field}", self.row + 1));
        let value = &table.rows[self.row][self.column];
        ui.horizontal_wrapped(|ui| {
            if crate::icons::button(ui, true, lucide_icons::Icon::Copy, "Copy full cell value")
                .clicked()
            {
                ui.ctx().copy_text(value.clone());
            }
            short_label(ui, value);
        });
        match self
            .cell_trace
            .as_ref()
            .and_then(|(_, value)| value.as_ref())
        {
            Some(trace) => {
                ui.label(trace);
            }
            None => {
                ui.weak("No trace captured for this cell.");
            }
        }
        ui.add_enabled(
            fresh && table.column(field).is_ok(),
            egui::Button::new("Show wiring"),
        )
        .on_hover_text("Return to this target field and inspect its static wiring")
        .clicked()
        .then(|| field.clone())
    }
}

fn panel(ui: &mut Ui, title: &str, color: egui::Color32, body: impl FnOnce(&mut Ui)) {
    egui::Frame::group(ui.style())
        .fill(crate::theme::active_palette(ui.ctx()).surface)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).strong().color(color));
            body(ui);
        });
}

fn two_panels(ui: &mut Ui, first: impl FnOnce(&mut Ui), second: impl FnOnce(&mut Ui)) {
    if ui.available_width() >= 520.0 {
        ui.columns(2, |cols| {
            first(&mut cols[0]);
            second(&mut cols[1]);
        });
    } else {
        first(ui);
        second(ui);
    }
}

fn short_label(ui: &mut Ui, value: &str) {
    // A single cell can fill the entire payload budget; do not shape a
    // megabyte of unbroken text in every column on every frame.
    let shown = &value[..value.floor_char_boundary(value.len().min(512))];
    ui.add(
        egui::Label::new(
            RichText::new(if value.is_empty() { "(empty)" } else { shown }).monospace(),
        )
        .truncate(),
    )
    .on_hover_text(shown);
    if shown.len() != value.len() {
        ui.weak("…");
    }
}

fn key_picker(
    ui: &mut Ui,
    id: &str,
    label: egui::Id,
    names: &[String],
    keys: &mut Vec<String>,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .selected_text(if keys.is_empty() {
            "Choose key fields".into()
        } else {
            keys.join(" + ")
        })
        .show_ui(ui, |ui| {
            if !keys.is_empty() && ui.button("Clear key").clicked() {
                keys.clear();
                changed = true;
            }
            for name in names {
                let mut checked = keys.contains(name);
                if ui.checkbox(&mut checked, name).changed() {
                    if checked {
                        keys.push(name.clone());
                    } else {
                        keys.retain(|k| k != name);
                    }
                    changed = true;
                }
            }
            if ui.button("Use selected key").clicked() {
                ui.close();
            }
        })
        .response
        .labelled_by(label);
    changed
}

/// Focusable list heading; Tab enters/leaves the list, arrows move its
/// selection even when the next row is virtualized.
fn list_keys(ui: &mut Ui, label: &str, focus: &mut bool) -> (egui::Response, Option<Key>) {
    let response = ui.button(label);
    if *focus {
        response.request_focus();
        *focus = false;
    }
    let key = if response.has_focus() {
        crate::canvas_keyboard::focus::take_key(
            ui.ctx(),
            response.id,
            &[
                Key::ArrowUp,
                Key::ArrowDown,
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::Enter,
            ],
        )
    } else {
        None
    };
    (response, key)
}

impl Inspection {
    fn compare(&mut self, ui: &mut Ui, report: &RunReportView, table: &PreviewTable) {
        ui.heading("Compare previews");
        let Some(a) = &self.pinned else {
            ui.weak("Pin the selected output as baseline A, then refresh the working preview B.");
            return;
        };
        let c = self.capture.as_ref().unwrap();
        let b = &report.report.outputs[report.selected_output()];
        let artifact = &c.artifacts[report.selected_output()];
        let palette = crate::theme::active_palette(ui.ctx());
        two_panels(
            ui,
            |ui| {
                panel(ui, "A / PINNED BASELINE", palette.action, |ui| {
                    snapshot_label(ui, &a.target, &a.path, &a.sha, &a.input_path)
                })
            },
            |ui| {
                panel(ui, "B / WORKING PREVIEW", palette.target, |ui| {
                    snapshot_label(ui, &c.target, &b.path, &artifact.sha, &c.input_path)
                })
            },
        );
        let input_status = match (&a.input_sha, &c.input_sha) {
            (Some(a_sha), Some(b_sha)) if a_sha == b_sha && a.input_path == c.input_path => {
                "Same primary input identity and bytes"
            }
            (Some(a_sha), Some(b_sha)) if a_sha == b_sha => {
                "Different input identity · same primary input bytes"
            }
            (Some(_), Some(_)) if a.input_path == c.input_path => {
                "Same input identity · different primary input bytes"
            }
            (Some(_), Some(_)) => "Different input identities and primary input bytes",
            _ if a.input_path == c.input_path => "Same input identity · file bytes not captured",
            _ => "Different input identities · file bytes not captured",
        };
        ui.label(input_status).on_hover_text(format!(
            "A: {} · {}\nB: {} · {}",
            a.input_path.display(),
            a.input_sha.as_deref().unwrap_or("uncaptured"),
            c.input_path.display(),
            c.input_sha.as_deref().unwrap_or("uncaptured")
        ));
        if a.target != c.target || a.path != b.path {
            ui.weak("Pinned A belongs to another target or artifact. Select its output or explicitly replace A.");
            return;
        }
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Match rows by");
            changed |= ui
                .radio_value(&mut self.key.position, false, "Key fields")
                .changed();
            if !self.key.position {
                changed |= key_picker(
                    ui,
                    "compare_keys",
                    label.id,
                    &table.headers,
                    &mut self.key.keys,
                );
            }
            changed |= ui
                .radio_value(&mut self.key.position, true, "Row position")
                .changed();
        });
        if changed {
            self.change = 0;
            self.diff = None;
            self.pair_open = false;
            self.row_focus = false;
        }
        let diff = self
            .diff
            .get_or_insert_with(|| compare::compare(&a.table, table, &self.key));
        let diff = match diff {
            Ok(diff) => diff,
            Err(reason) => {
                ui.weak(reason.as_str());
                return;
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!(
                "{} matched · {} added · {} removed",
                diff.matched, diff.added, diff.removed
            ));
            ui.weak(if self.key.position {
                "Explicit positional comparison"
            } else {
                "Key match, regardless of row order"
            });
        });
        for &(ac, bc) in &diff.fields {
            match (ac, bc) {
                (None, Some(c)) => {
                    ui.label(format!("Added field: {}", table.headers[c]));
                }
                (Some(c), None) => {
                    ui.label(format!("Removed field: {}", a.table.headers[c]));
                }
                _ => {}
            }
        }
        if diff.changes.is_empty() {
            ui.strong("No changed values.");
            return;
        }
        let (list, key) = list_keys(ui, "Changed values", &mut self.row_focus);
        self.change = self.change.min(diff.changes.len() - 1);
        match key {
            Some(Key::ArrowUp) => self.change = self.change.saturating_sub(1),
            Some(Key::ArrowDown) => self.change = (self.change + 1).min(diff.changes.len() - 1),
            Some(Key::Enter) => self.pair_open = true,
            _ => {}
        }
        let column_for = |ch: &compare::Change| {
            ch.field
                .map(|field| diff.fields[field])
                .unwrap_or((None, None))
        };
        let field_name = |ch: &compare::Change| match column_for(ch) {
            (Some(col), _) => a.table.headers[col].as_str(),
            (_, Some(col)) => table.headers[col].as_str(),
            _ if ch.a.is_none() => "Added row",
            _ => "Removed row",
        };
        let row_label = |ch: &compare::Change| {
            let (table, row) = match (ch.a, ch.b) {
                (Some(row), _) => (a.table.as_ref(), row),
                (_, Some(row)) => (table, row),
                _ => return String::new(),
            };
            if self.key.position {
                return format!("Row {}", row + 1);
            }
            self.key
                .keys
                .iter()
                .filter_map(|name| table.column(name).ok())
                .map(|col| compact(&table.rows[row][col]))
                .collect::<Vec<_>>()
                .join(" / ")
        };
        ui.weak("Row key  ·  Field  ·  A → B");
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("preview_comparison_changes")
            .max_height(205.0);
        let height = ui
            .text_style_height(&egui::TextStyle::Button)
            .max(ui.spacing().interact_size.y);
        if matches!(key, Some(Key::ArrowUp | Key::ArrowDown)) {
            scroll = scroll.vertical_scroll_offset(
                (height + ui.spacing().item_spacing.y) * self.change as f32,
            );
        }
        scroll.show_rows(ui, height, diff.changes.len(), |ui, range| {
            for i in range {
                let ch = &diff.changes[i];
                let (ac, bc) = column_for(ch);
                let mut label = format!("{}   ·   {}", row_label(ch), compact(field_name(ch)));
                if ch.field.is_some() {
                    let old =
                        ch.a.zip(ac)
                            .map(|(r, c)| compact(&a.table.rows[r][c]))
                            .unwrap_or("(absent)");
                    let new =
                        ch.b.zip(bc)
                            .map(|(r, c)| compact(&table.rows[r][c]))
                            .unwrap_or("(absent)");
                    label.push_str(&format!("   {old} → {new}"));
                }
                if ui.selectable_label(self.change == i, label).clicked() {
                    self.change = i;
                    self.pair_open = true;
                    list.request_focus();
                }
            }
        });
        ui.weak("↑ ↓ Move between changes · Enter opens the selected pair");
        if self.pair_open {
            let ch = &diff.changes[self.change];
            ui.separator();
            ui.strong(format!("{} · {}", row_label(ch), field_name(ch)));
            two_panels(
                ui,
                |ui| pair_panel(ui, "A", ch.a, &a.table),
                |ui| pair_panel(ui, "B", ch.b, table),
            );
            if let Some(row) = ch.b
                && ui.button("Inspect working row").clicked()
            {
                self.row = row;
                self.column = column_for(ch).1.unwrap_or(0);
                self.page = Page::Inspect;
                self.row_focus = true;
            }
        }
    }

    fn explain(&mut self, ui: &mut Ui, table: &PreviewTable) {
        let capture = self.capture.as_ref().unwrap();
        ui.heading(format!(
            "Why {}?",
            crate::ui_kit::plural(table.rows.len(), "row")
        ));
        let scope = match &capture.scope {
            Ok(scope) => scope,
            Err(reason) => {
                if let Some((candidates, produced)) = capture.scope_counts {
                    ui.label(format!("Captured root scope · {candidates} iteration candidates · {produced} items produced."));
                }
                ui.weak(reason);
                return;
            }
        };
        ui.strong(&scope.path);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} input groups · {} input occurrences · {} output rows",
                scope.groups.len(),
                scope.items.len(),
                table.rows.len()
            ));
        });
        if scope.groups.len() <= 20 {
            let counts = scope
                .groups
                .iter()
                .map(|group| group.items.len().to_string())
                .collect::<Vec<_>>();
            if !counts.is_empty() {
                ui.monospace(format!(
                    "{} = {} input occurrences",
                    counts.join(" + "),
                    scope.items.len()
                ));
            }
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Match input and output by");
            if key_picker(
                ui,
                "preview_scope_keys",
                label.id,
                &scope.columns,
                &mut self.scope_keys,
            ) {
                self.matches = None;
                self.row_focus = false;
            }
        });
        let matches = self
            .matches
            .get_or_insert_with(|| scope.match_rows(table, &self.scope_keys));
        match matches.as_ref() {
            Ok(matches) => {
                if matches.unmatched_output_rows > 0 {
                    ui.label(format!(
                        "{} output rows have no unique input match.",
                        matches.unmatched_output_rows
                    ));
                }
                if !matches.missing_output_keys.is_empty() {
                    let examples = matches
                        .missing_output_keys
                        .iter()
                        .take(5)
                        .map(|i| (i + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!(
                            "Missing output key in {} data rows (including {examples}).",
                            matches.missing_output_keys.len()
                        ),
                    );
                }
                if matches.duplicate_output_keys > 0 {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("Ambiguous output keys: {}.", matches.duplicate_output_keys),
                    );
                }
            }
            Err(reason) => {
                ui.weak(reason);
            }
        }
        ui.weak("Compared by key · Data row numbers exclude the CSV header. This is not runtime lineage.");
        if scope.groups.is_empty() {
            ui.weak("No input groups at this scope.");
            return;
        }
        let (list, key) = list_keys(ui, "Input groups", &mut self.row_focus);
        self.group = self.group.min(scope.groups.len() - 1);
        let selected = &scope.groups[self.group];
        match key {
            Some(Key::ArrowRight | Key::Enter) => self.expanded = Some(self.group),
            Some(Key::ArrowLeft) => {
                self.expanded = None;
                self.group_item = None;
            }
            Some(Key::ArrowDown) => {
                let next = self
                    .group_item
                    .map_or(selected.items.start, |item| item + 1);
                if self.expanded == Some(self.group) && selected.items.contains(&next) {
                    self.group_item = Some(next);
                } else if self.group + 1 < scope.groups.len() {
                    self.group += 1;
                    self.group_item = None;
                }
            }
            Some(Key::ArrowUp) => {
                if let Some(item) = self.group_item {
                    self.group_item = item
                        .checked_sub(1)
                        .filter(|item| selected.items.contains(item));
                } else if self.group > 0 {
                    self.group -= 1;
                    if self.expanded == Some(self.group) {
                        self.group_item = scope.groups[self.group].items.clone().last();
                    }
                }
            }
            _ => {}
        }
        // Only one group is expanded. Virtual indices map directly to either a
        // group or that group's item; no flattened tree of cloned records.
        let expanded = self.expanded;
        let children = expanded.map_or(0, |i| scope.groups[i].items.len());
        let visual_row = |visible: usize| match expanded {
            Some(g) if visible > g && visible <= g + children => {
                (g, Some(scope.groups[g].items.start + visible - g - 1))
            }
            Some(g) if visible > g + children => (visible - children, None),
            _ => (visible, None),
        };
        let selected_visible = match (expanded, self.group_item) {
            (Some(g), Some(item)) if g == self.group => g + 1 + item - scope.groups[g].items.start,
            (Some(g), _) if self.group > g => self.group + children,
            _ => self.group,
        };
        let mut clicked = None;
        let mut open_row = None;
        two_panels(
            ui,
            |ui| {
                panel(
                    ui,
                    "INPUT GROUPS",
                    crate::theme::active_palette(ui.ctx()).source,
                    |ui| {
                        let mut scroll = egui::ScrollArea::vertical()
                            .id_salt("preview_groups")
                            .max_height(240.0);
                        let height = ui.spacing().interact_size.y;
                        if matches!(key, Some(Key::ArrowUp | Key::ArrowDown)) {
                            scroll = scroll.vertical_scroll_offset(
                                (height + ui.spacing().item_spacing.y) * selected_visible as f32,
                            );
                        }
                        scroll.show_rows(ui, height, scope.groups.len() + children, |ui, range| {
                            for i in range {
                                let (g, item) = visual_row(i);
                                let label = scope.label(g, item, &self.scope_keys);
                                let name = match item {
                                    Some(_) => format!("    ↳  {label}"),
                                    None => format!(
                                        "{} {label}   ·   {}",
                                        if expanded == Some(g) { "▾" } else { "▸" },
                                        crate::ui_kit::plural(scope.groups[g].items.len(), "item")
                                    ),
                                };
                                if ui.selectable_label(selected_visible == i, name).clicked() {
                                    clicked = Some((g, item));
                                    list.request_focus();
                                }
                            }
                        });
                    },
                )
            },
            |ui| {
                panel(
                    ui,
                    "CAPTURED OUTPUT",
                    crate::theme::active_palette(ui.ctx()).target,
                    |ui| {
                        let group = &scope.groups[self.group];
                        if group.items.is_empty() {
                            ui.weak("Empty input group · 0 items");
                            return;
                        }
                        let Ok(matches) = matches.as_ref() else {
                            ui.weak("Select matching fields to locate output rows.");
                            return;
                        };
                        let found = matches.items[group.items.clone()]
                            .iter()
                            .filter(|r| matches!(r, RowMatch::Row(_)))
                            .count();
                        ui.weak(format!(
                            "{} of {} input items match a unique output row.",
                            found,
                            group.items.len()
                        ));
                        let items = self
                            .group_item
                            .map_or_else(|| group.items.clone(), |item| item..item + 1);
                        let height = ui.spacing().interact_size.y
                            + ui.text_style_height(&egui::TextStyle::Small)
                            + ui.spacing().item_spacing.y;
                        egui::ScrollArea::vertical()
                            .id_salt("scope_output_rows")
                            .max_height(235.0)
                            .show_rows(ui, height, items.len(), |ui, range| {
                                for i in range {
                                    let item = i + items.start;
                                    let label =
                                        scope.label(self.group, Some(item), &self.scope_keys);
                                    match matches.items[item] {
                                        RowMatch::Row(r) => {
                                            if ui
                                                .selectable_label(
                                                    false,
                                                    format!("Data row {}   ·   {label}", r + 1),
                                                )
                                                .clicked()
                                            {
                                                open_row = Some(r);
                                            }
                                            let details = table
                                                .headers
                                                .iter()
                                                .zip(&table.rows[r])
                                                .filter(|(name, _)| !self.scope_keys.contains(name))
                                                .take(4)
                                                .map(|(name, v)| {
                                                    format!("{}: {}", compact(name), compact(v))
                                                })
                                                .collect::<Vec<_>>()
                                                .join(" · ");
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&details).small().weak(),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(details);
                                        }
                                        RowMatch::MissingKey => {
                                            ui.weak(format!("{label} · Missing input key"));
                                        }
                                        RowMatch::Ambiguous => {
                                            ui.weak(format!("{label} · Ambiguous key"));
                                        }
                                        RowMatch::NoOutput => {
                                            ui.weak(format!("{label} · No matching output row"));
                                        }
                                    }
                                }
                            });
                    },
                )
            },
        );
        if let Some((g, item)) = clicked {
            self.group = g;
            self.group_item = item;
            if item.is_none() {
                self.expanded = (self.expanded != Some(g)).then_some(g);
            }
        }
        if let Some(row) = open_row {
            self.row = row;
            self.page = Page::Inspect;
            self.row_focus = true;
        }
        ui.weak("↑ ↓ Move · → Expand · ← Collapse");
    }
}

fn snapshot_label(ui: &mut Ui, target: &PreviewTarget, path: &Path, sha: &str, input: &Path) {
    ui.strong(target.label());
    ui.label(path.display().to_string());
    ui.monospace(format!("SHA-256  {}", &sha[..12]))
        .on_hover_text(sha);
    ui.weak(format!("Primary input: {}", input.display()));
}

fn compact(s: &str) -> &str {
    &s[..s.floor_char_boundary(s.len().min(96))]
}

fn pair_panel(ui: &mut Ui, side: &str, row: Option<usize>, table: &PreviewTable) {
    let Some(row) = row else {
        ui.weak(format!("{side}: row absent"));
        return;
    };
    ui.strong(format!("{side} · Data row {}", row + 1));
    egui::ScrollArea::vertical()
        .id_salt(("preview_pair", side))
        .max_height(130.0)
        .show(ui, |ui| {
            for (name, value) in table.headers.iter().zip(&table.rows[row]) {
                ui.horizontal(|ui| {
                    ui.label(name);
                    short_label(ui, value);
                });
            }
        });
}
