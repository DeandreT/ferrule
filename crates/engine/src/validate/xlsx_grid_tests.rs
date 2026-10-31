use super::*;
use mapping::XlsxRow;

fn composite_grid(sheet: &str) -> mapping::XlsxGridRegion {
    mapping::XlsxGridRegion {
        path: vec![sheet.into()],
        layout: mapping::XlsxGridLayout {
            sheet: Some(sheet.into()),
            header_row: XlsxRow::new(1).unwrap(),
            data_start_row: XlsxRow::new(2).unwrap(),
            header_value_field: "Header".into(),
            header_position_field: "HeaderColumn".into(),
            rows_field: "Rows".into(),
            cells_field: "Cells".into(),
            cell_value_field: "value".into(),
            cell_position_field: "CellColumn".into(),
            fixed_cells: Vec::new(),
        },
    }
}

#[test]
fn composite_grid_regions_refuse_targets_shared_worksheets_and_empty_layouts() {
    let grids = |grids| mapping::XlsxCompositeLayout {
        table: None,
        additional_tables: Vec::new(),
        records: Vec::new(),
        grids,
    };
    let messages = |project: &mapping::Project| {
        validate(project)
            .into_iter()
            .filter(|issue| issue.message.contains("XLSX") || issue.message.contains("grid"))
            .map(|issue| format!("{}: {}", issue.location, issue.message))
            .collect::<Vec<_>>()
    };

    let mut project = tests::valid_project();
    project.source_options.xlsx_composite =
        Some(grids(vec![composite_grid("Q"), composite_grid("A")]));
    assert!(messages(&project).is_empty(), "{:?}", messages(&project));

    project.source_options.xlsx_composite =
        Some(grids(vec![composite_grid("Q"), composite_grid("Q")]));
    assert!(
        messages(&project).iter().any(|message| message.ends_with(
            "several XLSX grids cannot share one worksheet; each grid needs its own worksheet"
        )),
        "{:?}",
        messages(&project)
    );

    project.source_options.xlsx_composite = Some(grids(Vec::new()));
    assert!(
        messages(&project)
            .iter()
            .any(|message| message
                .ends_with("a composite XLSX layout needs at least one table or grid")),
        "{:?}",
        messages(&project)
    );

    let mut project = tests::valid_project();
    project.target_options.xlsx_composite = Some(grids(vec![composite_grid("Q")]));
    project.target_options.xlsx_update_existing = true;
    let found = messages(&project);
    for expected in [
        "target format options: grid XLSX output is not supported; XLSX grids are valid only for mapping sources",
        "target format options: `xlsx_update_existing` cannot update a grid XLSX layout; only flat tables and hierarchical layouts update existing workbooks",
    ] {
        assert!(found.iter().any(|message| message == expected), "{found:?}");
    }
}
