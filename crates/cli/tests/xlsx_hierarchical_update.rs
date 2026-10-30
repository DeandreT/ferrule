//! Update-existing writes for hierarchical XLSX targets through the CLI file
//! path, against self-authored template workbooks.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, Xlsx};
use rust_xlsxwriter::{Formula, Workbook};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-xlsx-hierarchical-update-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mfd_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures")
        .join(name)
}

/// The imported hierarchical fixture with update-existing enabled. Each
/// division worksheet holds a summary header at row 2 and its title at row
/// 3, members two rows later with a header (B: name, D: rating), and a
/// one-row footer three rows after the last member.
fn update_project(scratch: &Scratch) -> PathBuf {
    let imported = mfd::import(&mfd_fixture("xlsx-hierarchical.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut project = imported.project;
    project.target_options.xlsx_update_existing = true;
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let path = scratch.0.join("project.json");
    std::fs::write(&path, serde_json::to_vec(&project).unwrap()).unwrap();
    path
}

/// A workbook left by an earlier run: Design held three members, Research
/// none. Both carry content outside the layout, and an unrelated worksheet
/// sits between them.
fn template() -> Workbook {
    let mut workbook = Workbook::new();
    let design = workbook.add_worksheet().set_name("Design").unwrap();
    design.write_string(0, 6, "Keep").unwrap();
    design.write_string(1, 0, "Division").unwrap();
    design.write_string(2, 0, "Old design").unwrap();
    design.write_string(4, 1, "Member").unwrap();
    design.write_string(4, 3, "Rating").unwrap();
    for (offset, (name, rating)) in [("Old1", 1.0), ("Old2", 2.0), ("Old3", 3.0)]
        .into_iter()
        .enumerate()
    {
        let row = 5 + offset as u32;
        design.write_string(row, 1, name).unwrap();
        design.write_number(row, 3, rating).unwrap();
    }
    design.write_string(10, 0, "Old footer").unwrap();
    design.write_string(19, 0, "Footnote").unwrap();

    let notes = workbook.add_worksheet().set_name("Notes").unwrap();
    notes.write_string(0, 0, "Untouched").unwrap();
    notes.write_number(1, 1, 42.0).unwrap();

    let research = workbook.add_worksheet().set_name("Research").unwrap();
    research.write_string(1, 0, "Division").unwrap();
    research.write_string(2, 0, "Old research").unwrap();
    research.write_string(2, 7, "Keep too").unwrap();
    research.write_string(4, 1, "Member").unwrap();
    research.write_string(4, 3, "Rating").unwrap();
    research.write_string(7, 0, "Old footer").unwrap();
    workbook
}

/// Every non-empty value as `(one-based row, one-based column) -> tag:value`.
fn cells(bytes: &[u8], sheet: &str) -> BTreeMap<(u32, u32), String> {
    let mut workbook = Xlsx::new(Cursor::new(bytes)).unwrap();
    let range = workbook.worksheet_range(sheet).unwrap();
    let (first_row, first_column) = range.start().unwrap_or((0, 0));
    range
        .used_cells()
        .filter_map(|(row, column, data)| {
            let value = match data {
                Data::Empty => return None,
                Data::String(value) => format!("s:{value}"),
                Data::Float(value) => format!("n:{value}"),
                Data::Int(value) => format!("n:{value}"),
                Data::Bool(value) => format!("b:{value}"),
                other => format!("?:{other:?}"),
            };
            Some((
                (first_row + row as u32 + 1, first_column + column as u32 + 1),
                value,
            ))
        })
        .collect()
}

fn expected(entries: &[((u32, u32), &str)]) -> BTreeMap<(u32, u32), String> {
    entries
        .iter()
        .map(|(cell, value)| (*cell, (*value).to_string()))
        .collect()
}

fn sheet_names(bytes: &[u8]) -> Vec<String> {
    Xlsx::new(Cursor::new(bytes)).unwrap().sheet_names()
}

#[test]
fn hierarchical_target_updates_existing_workbook_through_file_run() {
    let scratch = Scratch::new("update");
    let project = update_project(&scratch);
    let output = scratch.0.join("report.xlsx");
    template().save(&output).unwrap();

    let written = cli::run_project(
        &project,
        &mfd_fixture("xlsx-hierarchical-source.xml"),
        &output,
    )
    .unwrap();
    assert_eq!(written, 2);

    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(sheet_names(&bytes), ["Design", "Notes", "Research"]);
    // Old member row 8 and the old footer at row 11 are cleared; G1 and
    // A20 lie outside the layout.
    assert_eq!(
        cells(&bytes, "Design"),
        expected(&[
            ((1, 7), "s:Keep"),
            ((2, 1), "s:Division"),
            ((3, 1), "s:Design"),
            ((5, 2), "s:Member"),
            ((5, 4), "s:Rating"),
            ((6, 2), "s:Iris"),
            ((6, 4), "n:7"),
            ((7, 2), "s:Omar"),
            ((7, 4), "n:9"),
            ((10, 1), "s:Prepared internally"),
            ((20, 1), "s:Footnote"),
        ])
    );
    assert_eq!(
        cells(&bytes, "Notes"),
        expected(&[((1, 1), "s:Untouched"), ((2, 2), "n:42")])
    );
    // The member band grows into row 6, so the footer moves from 8 to 9.
    assert_eq!(
        cells(&bytes, "Research"),
        expected(&[
            ((2, 1), "s:Division"),
            ((3, 1), "s:Research"),
            ((3, 8), "s:Keep too"),
            ((5, 2), "s:Member"),
            ((5, 4), "s:Rating"),
            ((6, 2), "s:Mina"),
            ((6, 4), "n:8"),
            ((9, 1), "s:Prepared internally"),
        ])
    );
}

#[test]
fn hierarchical_update_refusal_leaves_workbook_unchanged() {
    let scratch = Scratch::new("refusal");
    let project = update_project(&scratch);
    let output = scratch.0.join("report.xlsx");
    let mut workbook = template();
    workbook
        .worksheet_from_name("Design")
        .unwrap()
        .write_formula(6, 3, Formula::new("=1+1"))
        .unwrap();
    workbook.save(&output).unwrap();
    let before = std::fs::read(&output).unwrap();

    let error = cli::run_project(
        &project,
        &mfd_fixture("xlsx-hierarchical-source.xml"),
        &output,
    )
    .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("updating output"), "{message}");
    assert!(
        message.contains("cannot update hierarchical XLSX workbook")
            && message.contains("cell D7 inside the layout holds a formula"),
        "{message}"
    );
    assert_eq!(std::fs::read(&output).unwrap(), before);
}

#[test]
fn hierarchical_update_requires_every_mapped_worksheet() {
    let scratch = Scratch::new("missing-sheet");
    let project = update_project(&scratch);
    let output = scratch.0.join("report.xlsx");
    let mut workbook = Workbook::new();
    workbook.add_worksheet().set_name("Design").unwrap();
    workbook.save(&output).unwrap();
    let before = std::fs::read(&output).unwrap();

    let error = cli::run_project(
        &project,
        &mfd_fixture("xlsx-hierarchical-source.xml"),
        &output,
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("worksheet `Research` does not exist"),
        "{error:#}"
    );
    assert_eq!(std::fs::read(&output).unwrap(), before);
}

#[test]
fn update_existing_still_refuses_composite_grid_and_worksheet_set_targets() {
    let scratch = Scratch::new("refused-layouts");
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let base: mapping::Project =
        serde_json::from_str(&std::fs::read_to_string(fixture_dir.join("project.json")).unwrap())
            .unwrap();
    let row = mapping::XlsxRow::new(1).unwrap();
    let layouts = [
        (
            "composite",
            mapping::FormatOptions {
                xlsx_composite: Some(mapping::XlsxCompositeLayout {
                    table: Some(mapping::XlsxTableRegion {
                        path: vec!["Rows".into()],
                        sheet: None,
                        start_row: row,
                        columns: Vec::new(),
                        has_header: true,
                        row_number_field: None,
                    }),
                    additional_tables: Vec::new(),
                    records: Vec::new(),
                    grids: Vec::new(),
                }),
                ..Default::default()
            },
        ),
        (
            "grid",
            mapping::FormatOptions {
                xlsx_grid: Some(mapping::XlsxGridLayout {
                    sheet: None,
                    header_row: row,
                    data_start_row: mapping::XlsxRow::new(2).unwrap(),
                    header_value_field: "Header".into(),
                    header_position_field: "Column".into(),
                    rows_field: "Rows".into(),
                    cells_field: "Cells".into(),
                    cell_value_field: "Value".into(),
                    cell_position_field: "Position".into(),
                    fixed_cells: Vec::new(),
                }),
                ..Default::default()
            },
        ),
        (
            "worksheet-set",
            mapping::FormatOptions {
                xlsx_worksheet_set: Some(mapping::XlsxWorksheetSetLayout {
                    worksheets_path: vec!["Sheets".into()],
                    worksheet_name_path: vec!["Name".into()],
                    rows_path: vec!["Rows".into()],
                    row_number_path: None,
                    start_row: row,
                    columns: Vec::new(),
                    has_header: false,
                }),
                ..Default::default()
            },
        ),
    ];
    for (layout, options) in layouts {
        let mut project = base.clone();
        project.target_options = mapping::FormatOptions {
            xlsx_update_existing: true,
            ..options
        };
        let project_path = scratch.0.join(format!("{layout}.json"));
        std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();
        let output = scratch.0.join(format!("{layout}.xlsx"));
        let mut workbook = Workbook::new();
        workbook.add_worksheet().write_string(0, 0, "Keep").unwrap();
        workbook.save(&output).unwrap();
        let before = std::fs::read(&output).unwrap();

        let error =
            cli::run_project(&project_path, &fixture_dir.join("input.csv"), &output).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains(&format!(
                "`xlsx_update_existing` cannot update a {layout} XLSX layout"
            )),
            "{layout}: {message}"
        );
        assert_eq!(std::fs::read(&output).unwrap(), before, "{layout}");
    }
}
