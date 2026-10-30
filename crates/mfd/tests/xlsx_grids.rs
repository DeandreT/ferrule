//! XLSX sources with a header-driven grid per worksheet, alone or beside
//! tables, from self-authored designs.

use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::{Project, XlsxCompositeLayout};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xlsx_grids_{tag}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn composite(project: &Project) -> &XlsxCompositeLayout {
    project.source_options.xlsx_composite.as_ref().unwrap()
}

/// Text cells as zero-based (row, column, value).
type Cells = [(u32, u16, &'static str)];

fn workbook(sheets: &[(&str, &Cells)]) -> Vec<u8> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    for (name, cells) in sheets {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(*name).unwrap();
        for (row, column, value) in *cells {
            worksheet.write_string(*row, *column, *value).unwrap();
        }
    }
    workbook.save_to_buffer().unwrap()
}

const QUARTERLY: &Cells = &[
    (0, 0, "2026"),
    (0, 1, "Q1"),
    (0, 2, "Q2"),
    (1, 0, "101"),
    (1, 1, "10.5"),
    (1, 2, "20.5"),
    (2, 0, "202"),
    (2, 1, "30.5"),
    (2, 2, "40.5"),
];

/// The header is on row 3 and data starts on row 5; row 4 lies between them
/// and is not data. The fixed unit is on row 1.
const ANNUAL: &Cells = &[
    (0, 1, "USD"),
    (2, 1, "FY2025"),
    (2, 2, "FY2026"),
    (3, 0, "999"),
    (4, 0, "301"),
    (4, 1, "1.5"),
    (4, 2, "2.5"),
    (5, 0, "302"),
    (5, 1, "3.5"),
    (5, 2, "4.5"),
];

const STAFF: &Cells = &[(0, 0, "Name"), (1, 0, "Ada"), (2, 0, "Grace")];

fn run(project: &Project, bytes: &[u8]) -> Instance {
    let source =
        format_xlsx::from_bytes_composite(bytes, &project.source, composite(project)).unwrap();
    engine::run(project, &source).unwrap()
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn amounts(name: &str, rows: &[(f64, f64)]) -> (String, Instance) {
    (
        name.into(),
        Instance::Repeated(
            rows.iter()
                .map(|(region, amount)| {
                    Instance::Group(
                        (vec![
                            ("Region".into(), scalar(Value::Float(*region))),
                            ("Amount".into(), scalar(Value::Float(*amount))),
                        ])
                        .into(),
                    )
                })
                .collect(),
        ),
    )
}

fn header(
    names: [&str; 3],
    label: &str,
    column: i64,
    fixed: &str,
    lines: (String, Instance),
) -> Instance {
    Instance::Group(
        (vec![
            (names[0].into(), scalar(Value::String(label.into()))),
            (names[1].into(), scalar(Value::Int(column))),
            (names[2].into(), scalar(Value::String(fixed.into()))),
            lines,
        ])
        .into(),
    )
}

fn periods() -> (String, Instance) {
    let period = |month, column, first, second| {
        header(
            ["Month", "Column", "Year"],
            month,
            column,
            "2026",
            amounts("Sale", &[(101.0, first), (202.0, second)]),
        )
    };
    (
        "Period".into(),
        Instance::Repeated(vec![
            period("Q1", 2, 10.5, 30.5),
            period("Q2", 3, 20.5, 40.5),
        ]),
    )
}

fn totals() -> (String, Instance) {
    let total = |label, column, first, second| {
        header(
            ["Label", "Column", "Unit"],
            label,
            column,
            "USD",
            amounts("Line", &[(301.0, first), (302.0, second)]),
        )
    };
    (
        "Total".into(),
        Instance::Repeated(vec![
            total("FY2025", 2, 1.5, 3.5),
            total("FY2026", 3, 2.5, 4.5),
        ]),
    )
}

fn people() -> (String, Instance) {
    let person = |name: &str| {
        Instance::Group((vec![("Name".into(), scalar(Value::String(name.into())))]).into())
    };
    (
        "Person".into(),
        Instance::Repeated(vec![person("Ada"), person("Grace")]),
    )
}

#[test]
fn two_grid_worksheets_import_warning_free_and_read_each_grid() {
    let imported = mfd::import(&fixture("xlsx-grid-pair.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;
    assert!(project.source_options.xlsx_grid.is_none());
    let layout = composite(project);
    assert!(layout.table.is_none());
    assert!(layout.additional_tables.is_empty());
    assert!(layout.records.is_empty());
    let grids = layout
        .grids
        .iter()
        .map(|grid| {
            (
                grid.path.clone(),
                grid.layout.sheet.clone(),
                grid.layout.header_row.get(),
                grid.layout.data_start_row.get(),
                grid.layout.fixed_cells[0].path.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        grids,
        [
            (
                vec!["Quarterly".to_string()],
                Some("Quarterly".to_string()),
                1,
                2,
                vec!["Year".to_string()],
            ),
            (
                vec!["Annual".to_string()],
                Some("Annual".to_string()),
                3,
                5,
                vec!["Unit".to_string()],
            ),
        ]
    );
    for name in ["Quarterly", "Annual"] {
        let group = project.source.child(name).unwrap();
        assert!(group.repeating, "{name}");
        assert!(group.child("Rows").unwrap().repeating, "{name}");
    }
    assert!(
        engine::validate(project).is_empty(),
        "{:?}",
        engine::validate(project)
    );

    let bytes = workbook(&[("Quarterly", QUARTERLY), ("Annual", ANNUAL)]);
    assert_eq!(
        run(project, &bytes),
        Instance::Group((vec![periods(), totals()]).into())
    );
}

#[test]
fn grid_beside_a_table_imports_and_reads_both() {
    let imported = mfd::import(&fixture("xlsx-grid-table.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;
    let layout = composite(project);
    let table = layout.table.as_ref().unwrap();
    assert_eq!(table.path, ["Staff"]);
    assert_eq!(layout.grids.len(), 1);
    assert_eq!(layout.grids[0].path, ["Quarterly"]);
    assert!(
        engine::validate(project).is_empty(),
        "{:?}",
        engine::validate(project)
    );

    let bytes = workbook(&[("Staff", STAFF), ("Quarterly", QUARTERLY)]);
    assert_eq!(
        run(project, &bytes),
        Instance::Group((vec![periods(), people()]).into())
    );
}

#[test]
fn single_grid_keeps_its_serialized_layout() {
    let imported = mfd::import(&fixture("xlsx-grid.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let options = serde_json::to_value(&imported.project.source_options).unwrap();
    assert!(options.get("xlsx_composite").is_none(), "{options}");
    assert_eq!(
        options["xlsx_grid"],
        serde_json::json!({
            "sheet": "Quarterly",
            "header_row": 1,
            "data_start_row": 2,
            "header_value_field": "Range1",
            "header_position_field": "HeaderColumn",
            "rows_field": "Rows",
            "cells_field": "Cells",
            "cell_value_field": "value",
            "cell_position_field": "CellColumn",
            "fixed_cells": [{"path": ["Year"], "row": 1, "column": 1}]
        })
    );
}

#[test]
fn combined_grid_layouts_export_and_reimport_without_drift() {
    for (name, sheets) in [
        (
            "xlsx-grid-pair.mfd",
            &[("Quarterly", QUARTERLY), ("Annual", ANNUAL)][..],
        ),
        (
            "xlsx-grid-table.mfd",
            &[("Staff", STAFF), ("Quarterly", QUARTERLY)][..],
        ),
    ] {
        let project = mfd::import(&fixture(name)).unwrap().project;
        let dir = TempDir::new(name.trim_end_matches(".mfd"));
        let design = dir.0.join("mapping.mfd");
        let warnings = mfd::export(&project, &design).unwrap();
        assert!(warnings.is_empty(), "{name}: {warnings:?}");

        let reimported = mfd::import(&design).unwrap();
        assert!(
            reimported.warnings.is_empty(),
            "{name}: {:?}",
            reimported.warnings
        );
        assert_eq!(reimported.project.source, project.source, "{name}");
        assert_eq!(
            reimported.project.source_options, project.source_options,
            "{name}"
        );
        assert!(engine::validate(&reimported.project).is_empty(), "{name}");
        let bytes = workbook(sheets);
        assert_eq!(
            run(&reimported.project, &bytes),
            run(&project, &bytes),
            "{name}"
        );
    }
}

#[test]
fn several_grids_on_one_worksheet_refuse_with_the_component_reason() {
    // The pair design's two grids, placed on one worksheet.
    let Err(error) = mfd::import(&fixture("xlsx-grid-shared-sheet.mfd")) else {
        panic!("a worksheet with several grids must not import");
    };
    let error = error.to_string();
    assert!(
        error.contains(
            "skipped xlsx component `Workbook`: xlsx component `Workbook` has several nested grids on worksheet `Quarterly`; ferrule imports one grid per worksheet"
        ),
        "{error}"
    );
    assert!(
        !error.contains("which ferrule cannot import yet"),
        "{error}"
    );
}
