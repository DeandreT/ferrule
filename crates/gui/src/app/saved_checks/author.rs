use std::collections::BTreeSet;

use editor_ui::mapping_checks::{Assertion, KeyPart, NamedCheck};

use super::store::{CheckSet, Output, Reference};
use super::{Action, PreviewTable, headers};

pub(super) struct Draft {
    pub index: Option<usize>,
    pub set: CheckSet,
    pub table: Option<PreviewTable>,
    row: usize, // one-based data row in the reference picker only
    keys: BTreeSet<usize>,
    field: Option<usize>,
    cell_name: String,
    error: String,
}

impl Draft {
    pub fn new(output: Output, reference: Reference, table: PreviewTable) -> Self {
        let name = reference
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut checks = vec![NamedCheck {
            name: "Data rows".into(),
            assertion: Assertion::RowCount {
                expected: table.rows.len(),
            },
        }];
        if let Ok(assertion) = Assertion::capture_columns(headers(&table)) {
            checks.push(NamedCheck {
                name: "Columns in order".into(),
                assertion,
            });
        }
        Self {
            index: None,
            set: CheckSet {
                name,
                output,
                reference,
                checks,
            },
            table: Some(table),
            row: 1,
            keys: BTreeSet::new(),
            field: None,
            cell_name: String::new(),
            error: String::new(),
        }
    }

    pub fn edit(index: usize, set: CheckSet) -> Self {
        Self {
            index: Some(index),
            set,
            table: None,
            row: 1,
            keys: BTreeSet::new(),
            field: None,
            cell_name: String::new(),
            error: String::new(),
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) -> Action {
        ui.label(format!("Output: {}", self.set.output.label()));
        ui.label(format!(
            "Independent reference: {}",
            self.set.reference.path.display()
        ));
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Set name");
            ui.text_edit_singleline(&mut self.set.name)
                .labelled_by(label.id);
        });
        ui.separator();
        ui.strong("Checks to save");
        let mut remove = None;
        for (index, check) in self.set.checks.iter_mut().enumerate() {
            ui.push_id(("check", index), |ui| {
                ui.horizontal_wrapped(|ui| {
                    let label = ui.label("Check name");
                    ui.text_edit_singleline(&mut check.name)
                        .labelled_by(label.id);
                    if ui.button("Remove").clicked() {
                        remove = Some(index);
                    }
                });
                ui.weak(check.assertion.expected_detail());
            });
        }
        if let Some(index) = remove {
            self.set.checks.remove(index);
        }
        ui.separator();
        let Some(table) = &self.table else {
            ui.weak("Saved expectations can be renamed or removed. To add one, open the saved reference rows with the selected output’s CSV settings.");
            return if ui.button("Open saved reference rows").clicked() {
                Action::LoadRows
            } else {
                Action::None
            };
        };
        let count = table.rows.len();
        if !self
            .set
            .checks
            .iter()
            .any(|c| matches!(c.assertion, Assertion::RowCount { .. }))
            && ui.button("Add row count").clicked()
        {
            self.set.checks.push(NamedCheck {
                name: "Data rows".into(),
                assertion: Assertion::RowCount { expected: count },
            });
        }
        if !self
            .set
            .checks
            .iter()
            .any(|c| matches!(c.assertion, Assertion::Columns { .. }))
        {
            match Assertion::capture_columns(headers(table)) {
                Ok(assertion) => {
                    if ui.button("Add ordered columns").clicked() {
                        self.set.checks.push(NamedCheck {
                            name: "Columns in order".into(),
                            assertion,
                        });
                    }
                }
                Err(reason) => {
                    ui.weak(reason);
                }
            }
        }
        ui.strong("Check a cell by key");
        if !table.has_headers || table.headers.is_empty() {
            ui.weak("Named-cell checks need a physical CSV header.");
            return Action::None;
        }
        if count == 0 {
            ui.weak("The reference has no data rows to select.");
            return Action::None;
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Reference data row");
            ui.add(egui::DragValue::new(&mut self.row).range(1..=count))
                .labelled_by(label.id);
            ui.label(format!("of {count} (header excluded)"));
        });
        let row = &table.rows[self.row - 1];
        ui.weak(
            "Choose the columns that together identify this record and the one field to check.",
        );
        for (column, name) in table.headers.iter().enumerate() {
            ui.push_id(("reference_column", column), |ui| {
                if name.is_empty() {
                    ui.label(format!("Column {}: missing header name", column + 1));
                } else if table.column(name).is_err() {
                    ui.label(format!(
                        "{name}: duplicate header, cannot identify a unique column"
                    ));
                } else {
                    ui.horizontal_wrapped(|ui| {
                        let mut key = self.keys.contains(&column);
                        if ui.checkbox(&mut key, format!("Key: {name}")).changed() {
                            if key {
                                self.keys.insert(column);
                            } else {
                                self.keys.remove(&column);
                            }
                        }
                        ui.radio_value(&mut self.field, Some(column), format!("Check {name}"));
                        if let Some(value) = row.get(column) {
                            ui.monospace(format!("{value:?}"));
                        }
                    });
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Cell check name");
            ui.text_edit_singleline(&mut self.cell_name)
                .labelled_by(label.id);
        });
        if ui.button("Add keyed cell check").clicked() {
            match self.add_cell() {
                Ok(()) => self.error.clear(),
                Err(reason) => self.error = reason,
            }
        }
        if !self.error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.error);
        }
        Action::None
    }

    fn add_cell(&mut self) -> Result<(), String> {
        if self.cell_name.trim().is_empty() {
            return Err("Name the cell check.".into());
        }
        let table = self
            .table
            .as_ref()
            .ok_or("Open the saved reference rows first.")?;
        let field = self
            .field
            .and_then(|i| table.headers.get(i))
            .ok_or("Choose the field to check.")?;
        let row = self
            .row
            .checked_sub(1)
            .and_then(|i| table.rows.get(i))
            .ok_or("Choose a reference data row.")?;
        let key = self
            .keys
            .iter()
            .map(|&column| {
                Ok(KeyPart {
                    column: table
                        .headers
                        .get(column)
                        .ok_or("Missing reference key column.")?
                        .clone(),
                    value: row
                        .get(column)
                        .ok_or("Missing reference key cell.")?
                        .clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let assertion = Assertion::capture_cell(key, field.clone(), headers(table), &table.rows)?;
        self.set.checks.push(NamedCheck {
            name: self.cell_name.trim().to_owned(),
            assertion,
        });
        self.cell_name.clear();
        Ok(())
    }
}
