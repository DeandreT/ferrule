//! Update-existing writes for hierarchical XLSX targets, exercised against
//! self-authored template workbooks built in each test.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, Xlsx};
use format_xlsx::{
    HierarchicalUpdateError, HierarchicalUpdateRefusal, XlsxFormatError, XlsxLimit,
    XlsxLimitOptions, XlsxLimits, from_bytes_hierarchical, to_bytes_hierarchical,
    update_hierarchical, update_hierarchical_bytes,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    XlsxCellKind, XlsxColumn, XlsxHierarchicalLayout, XlsxOutputColumn, XlsxOutputRange,
    XlsxRangeStart, XlsxRow,
};
use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Note, Workbook, Worksheet};

/// A per-test directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xlsx-hierarchical-update-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn workbook(&self, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("report.xlsx");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn entries(&self) -> Vec<String> {
        let mut names = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Workbook",
        vec![
            SchemaNode::group(
                "Sheets",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::group(
                        "Summary",
                        vec![
                            SchemaNode::scalar("Label", ScalarType::String),
                            SchemaNode::scalar("Started", ScalarType::String),
                            SchemaNode::scalar("Stamp", ScalarType::String),
                        ],
                    ),
                    SchemaNode::group(
                        "People",
                        vec![
                            SchemaNode::scalar("Display", ScalarType::String),
                            SchemaNode::scalar("Rank", ScalarType::Int),
                            SchemaNode::scalar("Score", ScalarType::Float),
                            SchemaNode::scalar("Active", ScalarType::Bool),
                        ],
                    )
                    .repeating(),
                    SchemaNode::group(
                        "Footer",
                        vec![SchemaNode::scalar("Label", ScalarType::String)],
                    ),
                ],
            )
            .repeating(),
        ],
    )
}

fn column(
    field: &str,
    physical: u32,
    header: Option<&str>,
    kind: XlsxCellKind,
) -> XlsxOutputColumn {
    XlsxOutputColumn {
        path: vec![field.into()],
        column: XlsxColumn::new(physical).unwrap(),
        header: header.map(str::to_string),
        kind,
    }
}

/// Summary at rows 2-3, People two rows later with an open row count, and a
/// one-row footer three rows after the last person.
fn layout() -> XlsxHierarchicalLayout {
    XlsxHierarchicalLayout {
        worksheets_path: vec!["Sheets".into()],
        worksheet_name_path: vec!["Name".into()],
        ranges: vec![
            XlsxOutputRange {
                path: vec!["Summary".into()],
                start: XlsxRangeStart::Absolute {
                    row: XlsxRow::new(2).unwrap(),
                },
                count: XlsxRow::new(1),
                has_header: true,
                columns: vec![
                    column("Label", 1, Some("Project"), XlsxCellKind::String),
                    column("Started", 3, Some("Started"), XlsxCellKind::Date),
                    column("Stamp", 5, Some("Stamp"), XlsxCellKind::DateTime),
                ],
            },
            XlsxOutputRange {
                path: vec!["People".into()],
                start: XlsxRangeStart::AfterPrevious {
                    offset: XlsxRow::new(2).unwrap(),
                },
                count: None,
                has_header: true,
                columns: vec![
                    column("Display", 2, Some("Person"), XlsxCellKind::String),
                    column("Rank", 3, Some("Rank"), XlsxCellKind::Number),
                    column("Score", 4, Some("Score"), XlsxCellKind::Number),
                    column("Active", 5, Some("Active"), XlsxCellKind::Boolean),
                ],
            },
            XlsxOutputRange {
                path: vec!["Footer".into()],
                start: XlsxRangeStart::AfterPrevious {
                    offset: XlsxRow::new(3).unwrap(),
                },
                count: XlsxRow::new(1),
                has_header: false,
                columns: vec![column("Label", 1, None, XlsxCellKind::String)],
            },
        ],
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn person(name: &str, rank: i64, score: Option<f64>, active: bool) -> Instance {
    group(vec![
        ("Display", text(name)),
        ("Rank", Instance::Scalar(Value::Int(rank))),
        (
            "Score",
            Instance::Scalar(score.map_or(Value::Null, Value::Float)),
        ),
        ("Active", Instance::Scalar(Value::Bool(active))),
    ])
}

fn sheet(name: &str, label: &str, date: &str, people: Vec<Instance>, footer: &str) -> Instance {
    group(vec![
        ("Name", text(name)),
        (
            "Summary",
            group(vec![
                ("Label", text(label)),
                ("Started", text(date)),
                ("Stamp", text(&format!("{date}T09:30:00"))),
            ]),
        ),
        ("People", Instance::Repeated(people)),
        ("Footer", group(vec![("Label", text(footer))])),
    ])
}

fn workbook(sheets: Vec<Instance>) -> Instance {
    group(vec![("Sheets", Instance::Repeated(sheets))])
}

fn people(count: i64) -> Vec<Instance> {
    (1..=count)
        .map(|rank| {
            person(
                &format!("P{rank}"),
                rank,
                Some(rank as f64 / 4.0),
                rank % 2 == 0,
            )
        })
        .collect()
}

/// North with Unicode text, a sparse score, and typed values; South with
/// more rows than its template holds.
fn report() -> Instance {
    workbook(vec![
        sheet(
            "North",
            "Release ✓",
            "2024-02-03",
            vec![
                person("Zoë", 1, Some(8.5), true),
                person("李雷", 2, None, false),
            ],
            "Ende 終",
        ),
        sheet("South", "Süd", "2024-03-04", people(4), "End"),
    ])
}

fn limits() -> XlsxLimits {
    XlsxLimits::default()
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
                Data::DateTime(value) => {
                    let (year, month, day, hour, minute, second, _) = value.to_ymd_hms_milli();
                    format!("d:{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
                }
                Data::DateTimeIso(value) | Data::DurationIso(value) => format!("d:{value}"),
                Data::Error(error) => format!("e:{error:?}"),
            };
            Some((
                (first_row + row as u32 + 1, first_column + column as u32 + 1),
                value,
            ))
        })
        .collect()
}

fn formulas(bytes: &[u8], sheet: &str) -> BTreeMap<(u32, u32), String> {
    let mut workbook = Xlsx::new(Cursor::new(bytes)).unwrap();
    let range = workbook.worksheet_formula(sheet).unwrap();
    let (first_row, first_column) = range.start().unwrap_or((0, 0));
    range
        .used_cells()
        .filter(|(_, _, formula)| !formula.is_empty())
        .map(|(row, column, formula)| {
            (
                (first_row + row as u32 + 1, first_column + column as u32 + 1),
                formula.clone(),
            )
        })
        .collect()
}

fn values_without_formulas(bytes: &[u8], sheet: &str) -> BTreeMap<(u32, u32), String> {
    let formulas = formulas(bytes, sheet);
    cells(bytes, sheet)
        .into_iter()
        .filter(|(coordinate, _)| !formulas.contains_key(coordinate))
        .collect()
}

fn expected(entries: &[((u32, u32), &str)]) -> BTreeMap<(u32, u32), String> {
    entries
        .iter()
        .map(|(coordinate, value)| (*coordinate, (*value).to_string()))
        .collect()
}

fn package_part(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut part = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut part)
        .unwrap();
    part
}

fn sheet_names(bytes: &[u8]) -> Vec<String> {
    Xlsx::new(Cursor::new(bytes))
        .unwrap()
        .sheet_names()
        .to_vec()
}

/// A template whose North sheet holds a previous, longer output plus
/// unrelated template content, a styled first data row, formulas, a merged
/// title, a note, an unrelated sheet, and a defined name.
fn rich_template() -> Vec<u8> {
    let mut book = Workbook::new();
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let stamp = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
    let score = Format::new().set_bold().set_num_format("0.00");

    let north = book.add_worksheet();
    north.set_name("North").unwrap();
    north
        .merge_range(0, 0, 0, 4, "Quarterly report", &Format::new())
        .unwrap();
    north.write_string(1, 0, "Project").unwrap();
    north.write_string(1, 2, "Started").unwrap();
    north.write_string(1, 4, "Stamp").unwrap();
    north.write_string(2, 0, "Draft").unwrap();
    north
        .write_datetime_with_format(
            2,
            2,
            ExcelDateTime::parse_from_str("2023-01-01").unwrap(),
            &date,
        )
        .unwrap();
    north
        .write_datetime_with_format(
            2,
            4,
            ExcelDateTime::parse_from_str("2023-01-01T08:00:00").unwrap(),
            &stamp,
        )
        .unwrap();
    north.write_string(2, 6, "Keep note").unwrap();
    north
        .insert_note(2, 6, &Note::new("Template note"))
        .unwrap();
    for (column, header) in [
        (1, "Person"),
        (2, "Rank"),
        (3, "Score"),
        (4, "Active"),
        (5, "Double"),
    ] {
        north.write_string(4, column, header).unwrap();
    }
    for (offset, name) in ["Old A", "Old B", "Old C"].into_iter().enumerate() {
        let row = 5 + offset as u32;
        north.write_string(row, 1, name).unwrap();
        north.write_number(row, 2, offset as f64 + 1.0).unwrap();
        if offset == 0 {
            north.write_number_with_format(row, 3, 1.0, &score).unwrap();
        } else {
            north.write_number(row, 3, offset as f64 + 1.0).unwrap();
        }
        north.write_boolean(row, 4, true).unwrap();
        north
            .write_formula(row, 5, Formula::new(format!("=D{}*2", row + 1)))
            .unwrap();
    }
    north.write_string(10, 0, "Old end").unwrap();
    north.write_string(19, 7, "Below everything").unwrap();

    let notes = book.add_worksheet();
    notes.set_name("Notes").unwrap();
    notes.write_string(0, 0, "Untouched").unwrap();
    notes.write_number(1, 7, 42.0).unwrap();
    notes
        .write_formula(2, 7, Formula::new("=North!D6+1"))
        .unwrap();

    let south = book.add_worksheet();
    south.set_name("South").unwrap();
    south.write_string(0, 7, "South title").unwrap();

    book.define_name("ScoreColumn", "=North!$D$6:$D$40")
        .unwrap();
    book.save_to_buffer().unwrap()
}

#[test]
fn updates_row_bands_and_preserves_everything_outside_them() {
    let scratch = Scratch::new("preserve");
    let template = rich_template();
    let path = scratch.workbook(&template);

    let count = update_hierarchical(&path, &schema(), &report(), &layout(), &limits()).unwrap();
    let bytes = std::fs::read(&path).unwrap();

    assert_eq!(count, 2);
    assert_eq!(scratch.entries(), ["report.xlsx"]);
    assert_eq!(sheet_names(&bytes), ["North", "Notes", "South"]);
    assert_eq!(
        values_without_formulas(&bytes, "North"),
        expected(&[
            ((1, 1), "s:Quarterly report"),
            ((2, 1), "s:Project"),
            ((2, 3), "s:Started"),
            ((2, 5), "s:Stamp"),
            ((3, 1), "s:Release ✓"),
            ((3, 3), "d:2024-02-03T00:00:00"),
            ((3, 5), "d:2024-02-03T09:30:00"),
            ((3, 7), "s:Keep note"),
            ((5, 2), "s:Person"),
            ((5, 3), "s:Rank"),
            ((5, 4), "s:Score"),
            ((5, 5), "s:Active"),
            ((5, 6), "s:Double"),
            ((6, 2), "s:Zoë"),
            ((6, 3), "n:1"),
            ((6, 4), "n:8.5"),
            ((6, 5), "b:true"),
            ((7, 2), "s:李雷"),
            ((7, 3), "n:2"),
            ((7, 5), "b:false"),
            ((10, 1), "s:Ende 終"),
            ((20, 8), "s:Below everything"),
        ])
    );
    assert_eq!(
        formulas(&bytes, "North"),
        expected(&[((6, 6), "D6*2"), ((7, 6), "D7*2"), ((8, 6), "D8*2")])
    );
    assert_eq!(
        values_without_formulas(&bytes, "Notes"),
        expected(&[((1, 1), "s:Untouched"), ((2, 8), "n:42")])
    );
    assert_eq!(
        formulas(&bytes, "Notes"),
        expected(&[((3, 8), "North!D6+1")])
    );
    assert_eq!(
        cells(&bytes, "South"),
        expected(&[
            ((1, 8), "s:South title"),
            ((2, 1), "s:Project"),
            ((2, 3), "s:Started"),
            ((2, 5), "s:Stamp"),
            ((3, 1), "s:Süd"),
            ((3, 3), "d:2024-03-04T00:00:00"),
            ((3, 5), "d:2024-03-04T09:30:00"),
            ((5, 2), "s:Person"),
            ((5, 3), "s:Rank"),
            ((5, 4), "s:Score"),
            ((5, 5), "s:Active"),
            ((6, 2), "s:P1"),
            ((6, 3), "n:1"),
            ((6, 4), "n:0.25"),
            ((6, 5), "b:false"),
            ((7, 2), "s:P2"),
            ((7, 3), "n:2"),
            ((7, 4), "n:0.5"),
            ((7, 5), "b:true"),
            ((8, 2), "s:P3"),
            ((8, 3), "n:3"),
            ((8, 4), "n:0.75"),
            ((8, 5), "b:false"),
            ((9, 2), "s:P4"),
            ((9, 3), "n:4"),
            ((9, 4), "n:1"),
            ((9, 5), "b:true"),
            ((12, 1), "s:End"),
        ])
    );

    let preserved = umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(&bytes), true).unwrap();
    let north = preserved.sheet_by_name("North").unwrap();
    assert!(
        north
            .merge_cells()
            .iter()
            .any(|range| range.range() == "A1:E1")
    );
    assert_eq!(north.comments().len(), 1);
    let workbook_part = package_part(&bytes, "xl/workbook.xml");
    assert!(
        workbook_part.contains(r#"<definedName name="ScoreColumn">North!$D$6:$D$40</definedName>"#),
        "{workbook_part}"
    );
    // The rewritten first data cell keeps its template style.
    let first_score = north.cell((4, 6)).unwrap().style();
    assert_eq!(
        first_score
            .number_format()
            .map(|format| format.format_code()),
        Some("0.00")
    );
    assert!(first_score.font().is_some_and(|font| font.bold()));
}

#[test]
fn updated_workbook_reads_back_as_the_written_instance() {
    let scratch = Scratch::new("roundtrip");
    let path = scratch.workbook(&rich_template());
    let instance = report();

    update_hierarchical(&path, &schema(), &instance, &layout(), &limits()).unwrap();
    let read =
        from_bytes_hierarchical(&std::fs::read(&path).unwrap(), &schema(), &layout()).unwrap();

    let written = sheets(&instance);
    let read = sheets(&read)
        .into_iter()
        .filter(|sheet| sheet_name(sheet) != "Notes")
        .collect::<Vec<_>>();
    assert_eq!(read, written);
}

fn sheets(instance: &Instance) -> Vec<Instance> {
    let Instance::Group(fields) = instance else {
        panic!("workbook instance must be a group");
    };
    let Some((_, Instance::Repeated(sheets))) = fields.iter().find(|(name, _)| name == "Sheets")
    else {
        panic!("workbook instance must contain sheets");
    };
    sheets.clone()
}

fn sheet_name(sheet: &Instance) -> &str {
    let Instance::Group(fields) = sheet else {
        panic!("sheet must be a group");
    };
    match fields.iter().find(|(name, _)| name == "Name") {
        Some((_, Instance::Scalar(Value::String(name)))) => name,
        _ => panic!("sheet must be named"),
    }
}

#[test]
fn update_of_a_previous_output_matches_a_new_workbook_for_the_new_instance() {
    let previous = workbook(vec![
        sheet("North", "First", "2024-01-01", people(3), "One"),
        sheet("South", "Second", "2024-01-02", people(1), "Two"),
    ]);
    let current = workbook(vec![
        sheet("North", "Third", "2024-05-06", people(5), "Three"),
        sheet("South", "Fourth", "2024-05-07", Vec::new(), "Four"),
    ]);
    let (template, _) = to_bytes_hierarchical(&schema(), &previous, &layout()).unwrap();

    let (updated, count) =
        update_hierarchical_bytes(&template, &schema(), &current, &layout(), &limits()).unwrap();
    let (fresh, _) = to_bytes_hierarchical(&schema(), &current, &layout()).unwrap();

    assert_eq!(count, 2);
    for name in ["North", "South"] {
        assert_eq!(cells(&updated, name), cells(&fresh, name), "{name}");
    }
    // A second identical update is stable.
    let (again, _) =
        update_hierarchical_bytes(&updated, &schema(), &current, &layout(), &limits()).unwrap();
    for name in ["North", "South"] {
        assert_eq!(cells(&again, name), cells(&fresh, name), "{name}");
    }
}

#[test]
fn new_data_rows_take_the_style_of_the_first_data_row() {
    let template = template_with(|north| {
        let score = Format::new().set_italic().set_num_format("0.000");
        north.write_blank(5, 3, &score).unwrap();
    });

    let (updated, _) = update_hierarchical_bytes(
        &template,
        &schema(),
        &north_only(people(3)),
        &layout(),
        &limits(),
    )
    .unwrap();

    let workbook =
        umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(&updated), true).unwrap();
    let north = workbook.sheet_by_name("North").unwrap();
    for row in 6..=8 {
        let style = north.cell((4, row)).unwrap().style();
        assert_eq!(
            style.number_format().map(|format| format.format_code()),
            Some("0.000"),
            "D{row}"
        );
        assert!(style.font().is_some_and(|font| font.italic()), "D{row}");
    }
    // Unstyled date cells receive the new-workbook writer's date format.
    let started = north.cell((3, 3)).unwrap().style();
    assert_eq!(
        started.number_format().map(|format| format.format_code()),
        Some("yyyy-mm-dd")
    );
}

fn template_with(customize: impl FnOnce(&mut Worksheet)) -> Vec<u8> {
    let mut book = Workbook::new();
    let north = book.add_worksheet();
    north.set_name("North").unwrap();
    customize(north);
    book.add_worksheet().set_name("South").unwrap();
    book.save_to_buffer().unwrap()
}

fn north_only(people: Vec<Instance>) -> Instance {
    workbook(vec![sheet("North", "Only", "2024-02-03", people, "End")])
}

/// Runs a failing update through the filesystem API and checks that the
/// workbook bytes are unchanged and no staging file remains.
fn refuse(
    name: &str,
    template: &[u8],
    instance: &Instance,
    layout: &XlsxHierarchicalLayout,
    limits: &XlsxLimits,
) -> HierarchicalUpdateError {
    let scratch = Scratch::new(name);
    let path = scratch.workbook(template);
    let error = update_hierarchical(&path, &schema(), instance, layout, limits).unwrap_err();
    assert_eq!(std::fs::read(&path).unwrap(), template, "{name}");
    assert_eq!(scratch.entries(), ["report.xlsx"], "{name}");
    error
}

#[test]
fn refuses_worksheets_missing_from_the_workbook() {
    let template = template_with(|_| {});
    let instance = workbook(vec![sheet("East", "Only", "2024-02-03", people(1), "End")]);

    let error = refuse("missing", &template, &instance, &layout(), &limits());

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Format(XlsxFormatError::MissingWorksheet(name)) if name == "East"
        ),
        "{error}"
    );
}

#[test]
fn refuses_duplicate_and_empty_worksheet_output() {
    let template = template_with(|_| {});
    let duplicate = workbook(vec![
        sheet("North", "One", "2024-02-03", people(1), "End"),
        sheet("North", "Two", "2024-02-03", people(1), "End"),
    ]);

    let error = refuse("duplicate", &template, &duplicate, &layout(), &limits());
    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Format(XlsxFormatError::DuplicateWorksheet(name)) if name == "North"
        ),
        "{error}"
    );

    let error = refuse(
        "empty",
        &template,
        &workbook(Vec::new()),
        &layout(),
        &limits(),
    );
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::NoWorksheets)
        ),
        "{error}"
    );
}

#[test]
fn refuses_overlapping_row_bands() {
    let template = template_with(|_| {});
    let mut overlapping = layout();
    overlapping.ranges[2] = XlsxOutputRange {
        path: vec!["Footer".into()],
        start: XlsxRangeStart::Absolute {
            row: XlsxRow::new(7).unwrap(),
        },
        count: XlsxRow::new(1),
        has_header: false,
        columns: vec![column("Label", 2, None, XlsxCellKind::String)],
    };

    let error = refuse(
        "overlap",
        &template,
        &north_only(people(2)),
        &overlapping,
        &limits(),
    );

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::OverlappingWrite { sheet, cell })
                if sheet == "North" && cell == "B7"
        ),
        "{error}"
    );
}

#[test]
fn refuses_formula_cells_inside_the_layout() {
    let template = template_with(|north| {
        north.write_string(5, 1, "Old").unwrap();
        north.write_formula(5, 3, Formula::new("=1+1")).unwrap();
    });

    let error = refuse(
        "formula",
        &template,
        &north_only(people(1)),
        &layout(),
        &limits(),
    );

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::FormulaCell { sheet, cell })
                if sheet == "North" && cell == "D6"
        ),
        "{error}"
    );
}

#[test]
fn refuses_merged_cells_inside_the_layout() {
    let template = template_with(|north| {
        north
            .merge_range(8, 1, 8, 3, "Merged", &Format::new())
            .unwrap();
    });

    let error = refuse(
        "merged",
        &template,
        &north_only(people(4)),
        &layout(),
        &limits(),
    );

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::MergedCell { sheet, cell, merge })
                if sheet == "North" && cell == "B9" && merge == "B9:D9"
        ),
        "{error}"
    );
}

#[test]
fn refuses_to_overwrite_content_outside_the_previous_extent() {
    // Row 8 is empty, so the previous People extent ends at row 7 and the
    // note in B9 belongs to the template, not to an earlier output.
    let template = template_with(|north| {
        north.write_string(5, 1, "Old A").unwrap();
        north.write_string(6, 1, "Old B").unwrap();
        north.write_string(8, 1, "Template note").unwrap();
    });

    let error = refuse(
        "occupied",
        &template,
        &north_only(people(4)),
        &layout(),
        &limits(),
    );

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::OccupiedCell { sheet, cell })
                if sheet == "North" && cell == "B9"
        ),
        "{error}"
    );
}

#[test]
fn refuses_inputs_and_plans_beyond_the_limits() {
    let template = template_with(|_| {});
    let small_input = XlsxLimits::new(XlsxLimitOptions {
        max_input_bytes: 64,
        ..XlsxLimitOptions::default()
    })
    .unwrap();
    let few_cells = XlsxLimits::new(XlsxLimitOptions {
        max_cells: 10,
        ..XlsxLimitOptions::default()
    })
    .unwrap();

    let error = refuse(
        "input-limit",
        &template,
        &north_only(people(1)),
        &layout(),
        &small_input,
    );
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::InputBytes,
                ..
            })
        ),
        "{error}"
    );

    let error = refuse(
        "cell-limit",
        &template,
        &north_only(people(3)),
        &layout(),
        &few_cells,
    );
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::Cells,
                ..
            })
        ),
        "{error}"
    );
}

#[test]
fn invalid_values_leave_the_workbook_unchanged() {
    let template = rich_template();
    let mut rows = people(2);
    rows.push(group(vec![
        ("Display", text("Bad")),
        ("Rank", Instance::Scalar(Value::Int(3))),
        ("Score", text("not a number")),
        ("Active", Instance::Scalar(Value::Bool(true))),
    ]));
    let instance = workbook(vec![sheet("North", "Bad", "2024-02-03", rows, "End")]);

    let error = refuse("invalid-value", &template, &instance, &layout(), &limits());

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Format(XlsxFormatError::HierarchicalValue { path, .. })
                if path == "Score"
        ),
        "{error}"
    );

    let mut bad_date = report();
    let Instance::Group(fields) = &mut bad_date else {
        unreachable!();
    };
    let Instance::Repeated(sheets) = &mut fields[0].1 else {
        unreachable!();
    };
    sheets[1] = sheet("South", "Süd", "not-a-date", people(1), "End");
    let error = refuse("invalid-date", &template, &bad_date, &layout(), &limits());
    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Format(XlsxFormatError::HierarchicalValue { path, .. })
                if path == "Started"
        ),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn replaces_the_symlink_target_and_keeps_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("symlink");
    let path = scratch.workbook(&rich_template());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let link = scratch.0.join("link.xlsx");
    std::os::unix::fs::symlink(&path, &link).unwrap();

    update_hierarchical(&link, &schema(), &report(), &layout(), &limits()).unwrap();

    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(scratch.entries(), ["link.xlsx", "report.xlsx"]);
    assert_eq!(
        cells(&std::fs::read(&path).unwrap(), "North").get(&(6, 2)),
        Some(&"s:Zoë".to_string())
    );
}

#[test]
fn missing_workbook_is_an_io_error() {
    let scratch = Scratch::new("absent");
    let path: &Path = &scratch.0.join("absent.xlsx");

    let error = update_hierarchical(path, &schema(), &report(), &layout(), &limits()).unwrap_err();

    assert!(
        matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Io(io)) if io.kind() == std::io::ErrorKind::NotFound),
        "{error}"
    );
    assert!(scratch.entries().is_empty());
}

#[test]
fn refuses_all_null_rows_in_open_ended_bands() {
    // Reads end People at its first blank row, so writing one would shift
    // the footer and leave the next update's extent walk short.
    let mut rows = people(3);
    rows[1] = group(vec![
        ("Display", Instance::Scalar(Value::Null)),
        ("Rank", Instance::Scalar(Value::Null)),
        ("Score", Instance::Scalar(Value::Null)),
        ("Active", Instance::Scalar(Value::Null)),
    ]);

    let error = refuse(
        "blank-row",
        &rich_template(),
        &north_only(rows),
        &layout(),
        &limits(),
    );

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::BlankOpenRow { sheet, band, row })
                if sheet == "North" && band == "People" && *row == 7
        ),
        "{error}"
    );
}

#[test]
fn encodes_text_like_the_new_workbook_writer() {
    let names = [
        "tab\tand\u{1}control\u{b}",
        "literal _x0041_ and _xABCD_",
        "end\u{ffff}",
    ];
    let instance = workbook(vec![sheet(
        "North",
        "Codes",
        "2024-02-03",
        names
            .iter()
            .enumerate()
            .map(|(index, name)| person(name, index as i64 + 1, None, true))
            .collect(),
        "End",
    )]);
    let (template, _) =
        to_bytes_hierarchical(&schema(), &north_only(people(1)), &layout()).unwrap();

    let (updated, _) =
        update_hierarchical_bytes(&template, &schema(), &instance, &layout(), &limits()).unwrap();
    let (fresh, _) = to_bytes_hierarchical(&schema(), &instance, &layout()).unwrap();

    assert_eq!(cells(&updated, "North"), cells(&fresh, "North"));
    let read = cells(&updated, "North");
    assert_eq!(read[&(6, 2)], "s:tab\tand\u{1}control\u{b}");
    assert_eq!(read[&(7, 2)], "s:literal _x0041_ and _xABCD_");
    // The updated workbook is valid XML for a further update.
    update_hierarchical_bytes(&updated, &schema(), &instance, &layout(), &limits()).unwrap();
}

#[test]
fn dates_override_non_date_number_formats() {
    let template = template_with(|north| {
        let number = Format::new().set_num_format("0.00");
        north.write_number_with_format(2, 2, 1.0, &number).unwrap();
        let custom_date = Format::new().set_num_format("dd/mm/yyyy");
        north
            .write_number_with_format(2, 4, 1.0, &custom_date)
            .unwrap();
    });
    let instance = north_only(people(1));

    let (updated, _) =
        update_hierarchical_bytes(&template, &schema(), &instance, &layout(), &limits()).unwrap();

    let read = cells(&updated, "North");
    assert_eq!(read[&(3, 3)], "d:2024-02-03T00:00:00");
    assert_eq!(read[&(3, 5)], "d:2024-02-03T09:30:00");
    let workbook =
        umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(&updated), true).unwrap();
    let north = workbook.sheet_by_name("North").unwrap();
    let format = |column| {
        north
            .cell((column, 3))
            .unwrap()
            .style()
            .number_format()
            .map(|format| format.format_code().to_string())
    };
    assert_eq!(format(3).as_deref(), Some("yyyy-mm-dd"));
    // A template's own date format is kept.
    assert_eq!(format(5).as_deref(), Some("dd/mm/yyyy"));
    assert_eq!(
        sheets(&from_bytes_hierarchical(&updated, &schema(), &layout()).unwrap())[0],
        sheets(&instance)[0]
    );
}

#[cfg(unix)]
#[test]
fn refuses_read_only_workbooks() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("read-only");
    let template = rich_template();
    let path = scratch.workbook(&template);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();

    let error = update_hierarchical(&path, &schema(), &report(), &layout(), &limits()).unwrap_err();

    assert!(
        matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Io(io)) if io.kind() == std::io::ErrorKind::PermissionDenied),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), template);
    assert_eq!(scratch.entries(), ["report.xlsx"]);
}

#[test]
fn output_plans_and_merge_checks_are_bounded() {
    let few_cells = XlsxLimits::new(XlsxLimitOptions {
        max_cells: 10,
        ..XlsxLimitOptions::default()
    })
    .unwrap();
    // The plan is bounded before the workbook is parsed.
    let error = update_hierarchical_bytes(
        b"not a workbook",
        &schema(),
        &north_only(people(3)),
        &layout(),
        &few_cells,
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::Cells,
                ..
            })
        ),
        "{error}"
    );

    // Merged ranges outside the layout still count toward the budget.
    let merged = template_with(|north| {
        for row in 0..50 {
            north
                .merge_range(
                    100 + row * 2,
                    10,
                    101 + row * 2,
                    11,
                    "merged",
                    &Format::new(),
                )
                .unwrap();
        }
    });
    let budget = |max_cells| {
        XlsxLimits::new(XlsxLimitOptions {
            max_cells,
            ..XlsxLimitOptions::default()
        })
        .unwrap()
    };
    let error = refuse(
        "merge-budget",
        &merged,
        &north_only(people(1)),
        &layout(),
        &budget(40),
    );
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::Cells,
                ..
            })
        ),
        "{error}"
    );
    update_hierarchical_bytes(
        &merged,
        &schema(),
        &north_only(people(1)),
        &layout(),
        &budget(1_000),
    )
    .unwrap();
}

#[cfg(unix)]
#[test]
fn refuses_workbooks_the_caller_cannot_write() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("unwritable");
    let template = rich_template();
    let path = scratch.workbook(&template);
    // A write bit is set, but not the owner's.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o464)).unwrap();
    if std::fs::OpenOptions::new().write(true).open(&path).is_ok() {
        // Privileged callers bypass the permission bits.
        return;
    }

    let error = update_hierarchical(&path, &schema(), &report(), &layout(), &limits()).unwrap_err();

    assert!(
        matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Io(io)) if io.kind() == std::io::ErrorKind::PermissionDenied),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), template);
    assert_eq!(scratch.entries(), ["report.xlsx"]);
}

#[test]
fn empty_text_is_stored_as_an_empty_cell() {
    let (template, _) =
        to_bytes_hierarchical(&schema(), &north_only(people(3)), &layout()).unwrap();
    let mut rows = people(2);
    rows[0] = person("", 1, Some(1.0), true);
    let instance = north_only(rows);
    let mut blank_header = layout();
    blank_header.ranges[1].columns[0].header = Some(String::new());

    for layout in [layout(), blank_header] {
        let (updated, _) =
            update_hierarchical_bytes(&template, &schema(), &instance, &layout, &limits()).unwrap();
        let (fresh, _) = to_bytes_hierarchical(&schema(), &instance, &layout).unwrap();
        assert_eq!(cells(&updated, "North"), cells(&fresh, "North"));
        assert!(!cells(&updated, "North").contains_key(&(6, 2)));
    }

    // Empty text with nulls is a blank row to a reader.
    let mut rows = people(2);
    rows[0] = group(vec![
        ("Display", text("")),
        ("Rank", Instance::Scalar(Value::Null)),
        ("Score", Instance::Scalar(Value::Null)),
        ("Active", Instance::Scalar(Value::Null)),
    ]);
    let error = refuse(
        "empty-text-row",
        &template,
        &north_only(rows),
        &layout(),
        &limits(),
    );
    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::BlankOpenRow { row, .. })
                if *row == 6
        ),
        "{error}"
    );
}

#[test]
fn previous_output_has_its_own_cell_allowance() {
    // 51 cells: more than half of the allowance on each side.
    let instance = north_only(people(10));
    let (template, _) = to_bytes_hierarchical(&schema(), &instance, &layout()).unwrap();
    let limits = XlsxLimits::new(XlsxLimitOptions {
        max_cells: 60,
        ..XlsxLimitOptions::default()
    })
    .unwrap();

    let (updated, _) =
        update_hierarchical_bytes(&template, &schema(), &instance, &layout(), &limits).unwrap();

    assert_eq!(cells(&updated, "North"), cells(&template, "North"));
}
