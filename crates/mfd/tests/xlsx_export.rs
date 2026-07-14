use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xlsx_export_{tag}_{}",
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

#[test]
fn exports_and_reimports_flat_xlsx_target() {
    let mut project = mfd::import(&fixture("people-to-csv.mfd")).unwrap().project;
    project.target_path = Some("reports/people.xlsx".into());
    project.target_options.xlsx_sheet = Some("People & Ages".into());
    project.target_options.xlsx_start_row = Some(4);
    project.target_options.xlsx_columns = vec![5, 2];
    project.target_options.has_header_row = Some(false);

    let dir = TempDir::new("target");
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("library=\"xlsx\""), "{text}");
    assert!(text.contains("kind=\"26\""), "{text}");
    assert!(
        text.contains("<excel outputinstance=\"reports/people.xlsx\"/>"),
        "{text}"
    );
    assert!(text.contains("value=\"People &amp; Ages\""), "{text}");
    assert!(text.contains("<range id=\"1\" start=\"4\"/>"), "{text}");
    assert!(text.contains("annotation=\"Name\" datatype=\"string\""));
    assert!(text.contains("annotation=\"Age\" datatype=\"long\""));
    assert!(text.contains("constant value=\"2\" datatype=\"long\""));
    assert!(text.contains("constant value=\"5\" datatype=\"long\""));
    assert!(!text.contains("enabletitlerow=\"1\""), "{text}");
    assert!(!dir.0.join("mapping-target.xsd").exists());

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.target, project.target);
    assert_eq!(
        imported.project.target_path.as_deref(),
        Some("reports/people.xlsx")
    );
    assert_eq!(
        imported.project.target_options.xlsx_sheet.as_deref(),
        Some("People & Ages")
    );
    assert_eq!(imported.project.target_options.xlsx_start_row, Some(4));
    assert_eq!(imported.project.target_options.xlsx_columns, vec![5, 2]);
    assert_eq!(imported.project.target_options.has_header_row, Some(false));
}

#[test]
fn exports_and_reimports_flat_xlsx_source() {
    let mut project = mfd::import(&fixture("people-csv.mfd")).unwrap().project;
    project.source_path = Some("staff.xlsx".into());
    project.source_options.xlsx_sheet = Some("Staff".into());
    project.source_options.xlsx_start_row = Some(2);
    project.source_options.xlsx_columns = vec![1, 3, 4];
    project.source_options.has_header_row = Some(true);

    let dir = TempDir::new("source");
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("<excel inputinstance=\"staff.xlsx\"/>"));
    assert!(text.contains("outkey="), "{text}");
    assert!(text.contains("enabletitlerow=\"1\""), "{text}");

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, project.source);
    assert_eq!(imported.project.source_path.as_deref(), Some("staff.xlsx"));
    assert_eq!(
        imported.project.source_options.xlsx_sheet.as_deref(),
        Some("Staff")
    );
    assert_eq!(imported.project.source_options.xlsx_start_row, Some(2));
    assert_eq!(imported.project.source_options.xlsx_columns, vec![1, 3, 4]);
    assert_eq!(imported.project.source_options.has_header_row, Some(true));
}

#[test]
fn unsupported_xlsx_schema_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("people.mfd")).unwrap().project;
    project.target_path = Some("nested.xlsx".into());
    project.target_options.xlsx_sheet = Some("Nested".into());

    let dir = TempDir::new("atomic");
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "existing design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("XLSX worksheet") && message.contains("not a flat group")
    ));
    assert_eq!(std::fs::read_to_string(&design).unwrap(), "existing design");
    assert!(!dir.0.join("mapping-source.xsd").exists());
}

#[test]
fn transposed_xlsx_layout_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("people-csv.mfd")).unwrap().project;
    project.source_path = Some("people.xlsx".into());
    project.source_options.xlsx_rows = vec![1, 3, 5];

    let dir = TempDir::new("transposed");
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "existing design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("transposed XLSX export is not supported")
    ));
    assert_eq!(std::fs::read_to_string(&design).unwrap(), "existing design");
    assert!(!dir.0.join("mapping-source.xsd").exists());
}

#[test]
fn composite_xlsx_layout_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("people-csv.mfd")).unwrap().project;
    project.source_path = Some("people.xlsx".into());
    project.source_options.xlsx_composite = Some(mapping::XlsxCompositeLayout {
        table: mapping::XlsxTableRegion {
            path: vec!["Staff".into()],
            sheet: Some("Staff".into()),
            start_row: mapping::XlsxRow::new(1).unwrap(),
            columns: vec![],
            has_header: true,
        },
        records: vec![],
    });

    let dir = TempDir::new("composite");
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "existing design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("composite XLSX export is not supported")
    ));
    assert_eq!(std::fs::read_to_string(&design).unwrap(), "existing design");
    assert!(!dir.0.join("mapping-source.xsd").exists());
}

#[test]
fn grid_xlsx_layout_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("people-csv.mfd")).unwrap().project;
    project.source_path = Some("people.xlsx".into());
    project.source_options.xlsx_grid = Some(mapping::XlsxGridLayout {
        sheet: Some("Sales".into()),
        header_row: mapping::XlsxRow::new(1).unwrap(),
        data_start_row: mapping::XlsxRow::new(2).unwrap(),
        header_value_field: "Month".into(),
        header_position_field: "MonthColumn".into(),
        rows_field: "Rows".into(),
        cells_field: "Cells".into(),
        cell_value_field: "Value".into(),
        cell_position_field: "Column".into(),
        fixed_cells: vec![mapping::XlsxFixedCell {
            path: vec!["Year".into()],
            row: mapping::XlsxRow::new(1).unwrap(),
            column: mapping::XlsxColumn::new(1).unwrap(),
        }],
    });

    let dir = TempDir::new("grid");
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "existing design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("grid XLSX export is not supported")
    ));
    assert_eq!(std::fs::read_to_string(&design).unwrap(), "existing design");
    assert!(!dir.0.join("mapping-source.xsd").exists());
}

#[test]
fn hierarchical_xlsx_layout_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("xlsx-hierarchical.mfd"))
        .unwrap()
        .project;
    assert!(project.target_options.xlsx_hierarchical.is_some());
    project.target_path = Some("report.xlsx".into());

    let dir = TempDir::new("hierarchical");
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "existing design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("hierarchical XLSX export is not supported")
    ));
    assert_eq!(std::fs::read_to_string(&design).unwrap(), "existing design");
    assert!(!dir.0.join("mapping-source.xsd").exists());
}

#[test]
fn invalid_xlsx_coordinates_are_rejected() {
    let mut project = mfd::import(&fixture("people-to-csv.mfd")).unwrap().project;
    project.target_path = Some("people.xlsx".into());
    project.target_options.xlsx_start_row = Some(0);

    let dir = TempDir::new("coordinates");
    let design = dir.0.join("mapping.mfd");
    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("start row")
    ));
    assert!(!design.exists());

    project.target_options.xlsx_start_row = Some(1);
    project.target_options.xlsx_columns = vec![2, 2];
    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("unique numbers")
    ));
    assert!(!design.exists());

    project.target_options.xlsx_columns = vec![2, 16_385];
    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("16384")
    ));
    assert!(!design.exists());

    project.target_options.xlsx_columns = vec![2, 5];
    project.target_options.xlsx_start_row = Some(1_048_577);
    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("1048576")
    ));
    assert!(!design.exists());
}
