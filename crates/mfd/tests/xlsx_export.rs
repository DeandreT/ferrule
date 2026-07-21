use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::TabularBoundaryKind;

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

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn assert_same_execution(
    original: &mapping::Project,
    roundtripped: &mapping::Project,
    source: &Instance,
) {
    assert_eq!(
        engine::run(original, source).unwrap(),
        engine::run(roundtripped, source).unwrap()
    );
}

#[test]
fn exports_and_reimports_flat_xlsx_target() {
    let mut project = mfd::import(&fixture("people-to-csv.mfd")).unwrap().project;
    project.target_path = Some("reports/people.xlsx".into());
    project.target_options.xlsx_sheet = Some("People & Ages".into());
    project.target_options.xlsx_start_row = Some(4);
    project.target_options.xlsx_columns = vec![5, 2];
    project.target_options.xlsx_headers = vec!["Contact".into(), "Contact".into()];
    project.target_options.has_header_row = Some(true);
    project.target_options.xlsx_update_existing = true;

    let dir = TempDir::new("target");
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("library=\"xlsx\""), "{text}");
    assert!(text.contains("kind=\"26\""), "{text}");
    assert!(
        text.contains("<excel outputinstance=\"reports/people.xlsx\" updateexistingfile=\"1\"/>"),
        "{text}"
    );
    assert!(text.contains("value=\"People &amp; Ages\""), "{text}");
    assert!(text.contains("<range id=\"1\" start=\"4\"/>"), "{text}");
    assert_eq!(text.matches("annotation=\"Contact\"").count(), 2);
    assert!(text.contains("ferrulefield=\"Name\""), "{text}");
    assert!(text.contains("ferrulefield=\"Age\""), "{text}");
    assert!(text.contains("constant value=\"2\" datatype=\"long\""));
    assert!(text.contains("constant value=\"5\" datatype=\"long\""));
    assert!(text.contains("enabletitlerow=\"1\""), "{text}");
    assert!(text.contains("updateexistingfile=\"1\""), "{text}");
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
    assert_eq!(
        imported.project.target_options.xlsx_headers,
        ["Contact", "Contact"]
    );
    assert_eq!(imported.project.target_options.has_header_row, Some(true));
    assert!(imported.project.target_options.xlsx_update_existing);
    assert_eq!(
        imported.project.target_options.tabular_kind,
        Some(TabularBoundaryKind::Xlsx)
    );
}

#[test]
fn pathless_xlsx_identity_exports_and_reimports() {
    let mut project = mfd::import(&fixture("people-to-csv.mfd")).unwrap().project;
    project.target_path = None;
    project.target_options.tabular_kind = Some(TabularBoundaryKind::Xlsx);
    project.target_options.delimiter = None;
    project.target_options.xlsx_sheet = Some("People".into());

    let dir = TempDir::new("pathless-target");
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("library=\"xlsx\""), "{text}");
    assert!(!text.contains("outputinstance="), "{text}");
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.target_path.is_none());
    assert_eq!(
        imported.project.target_options.tabular_kind,
        Some(TabularBoundaryKind::Xlsx)
    );
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
    assert_eq!(
        imported.project.source_options.tabular_kind,
        Some(TabularBoundaryKind::Xlsx)
    );
}

#[test]
fn unsupported_xlsx_schema_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("people.mfd")).unwrap().project;
    project.target_path = Some("nested.xlsx".into());
    project.target_options.xml_document = false;
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
fn exports_and_reimports_transposed_xlsx_source() {
    let project = mfd::import(&fixture("xlsx-transposed.mfd"))
        .unwrap()
        .project;

    let dir = TempDir::new("transposed");
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, project.source);
    assert_eq!(imported.project.source_path, project.source_path);
    assert_eq!(imported.project.source_options, project.source_options);
    assert!(engine::validate(&imported.project).is_empty());
    let source = Instance::Repeated(vec![
        Instance::Group(vec![
            ("Category".into(), scalar(Value::String("Hardware".into()))),
            ("Range9".into(), scalar(Value::Int(12))),
            ("n".into(), scalar(Value::Int(1))),
        ]),
        Instance::Group(vec![
            ("Category".into(), scalar(Value::String("Software".into()))),
            ("Range9".into(), scalar(Value::Int(18))),
            ("n".into(), scalar(Value::Int(2))),
        ]),
    ]);
    assert_same_execution(&project, &imported.project, &source);
}

#[test]
fn exports_and_reimports_composite_xlsx_sources() {
    for fixture_name in ["xlsx-composite-xml.mfd", "xlsx-composite-json.mfd"] {
        let project = mfd::import(&fixture(fixture_name)).unwrap().project;
        let dir = TempDir::new(fixture_name);
        let design = dir.0.join("mapping.mfd");
        let warnings = mfd::export(&project, &design).unwrap();
        assert!(warnings.is_empty(), "{fixture_name}: {warnings:?}");

        let imported = mfd::import(&design).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{fixture_name}: {:?}",
            imported.warnings
        );
        assert_eq!(imported.project.source, project.source, "{fixture_name}");
        assert_eq!(
            imported.project.source_path, project.source_path,
            "{fixture_name}"
        );
        assert_eq!(
            imported.project.source_options, project.source_options,
            "{fixture_name}"
        );
        assert!(
            engine::validate(&imported.project).is_empty(),
            "{fixture_name}"
        );
        let source = if fixture_name.ends_with("xml.mfd") {
            Instance::Group(vec![
                (
                    "Branch".into(),
                    Instance::Group(vec![
                        ("Name".into(), scalar(Value::String("North".into()))),
                        ("City".into(), scalar(Value::String("Seattle".into()))),
                    ]),
                ),
                (
                    "Roster".into(),
                    Instance::Repeated(vec![Instance::Group(vec![
                        ("First".into(), scalar(Value::String("Ada".into()))),
                        ("Team".into(), scalar(Value::String("Platform".into()))),
                    ])]),
                ),
            ])
        } else {
            Instance::Group(vec![
                (
                    "Info".into(),
                    Instance::Group(vec![(
                        "Organization".into(),
                        scalar(Value::String("Ferrule".into())),
                    )]),
                ),
                (
                    "People".into(),
                    Instance::Repeated(vec![Instance::Group(vec![
                        ("Name".into(), scalar(Value::String("Ada".into()))),
                        ("Age".into(), scalar(Value::Int(37))),
                    ])]),
                ),
            ])
        };
        assert_same_execution(&project, &imported.project, &source);
    }
}

#[test]
fn exports_and_reimports_grid_xlsx_source() {
    let project = mfd::import(&fixture("xlsx-grid.mfd")).unwrap().project;
    let dir = TempDir::new("grid");
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, project.source);
    assert_eq!(imported.project.source_path, project.source_path);
    assert_eq!(imported.project.source_options, project.source_options);
    assert!(engine::validate(&imported.project).is_empty());
    let cell = |column: i64, value: f64| {
        Instance::Group(vec![
            ("value".into(), scalar(Value::Float(value))),
            ("CellColumn".into(), scalar(Value::Int(column))),
        ])
    };
    let row = |region: f64, amount: f64| {
        Instance::Group(vec![(
            "Cells".into(),
            Instance::Repeated(vec![cell(1, region), cell(2, amount)]),
        )])
    };
    let source = Instance::Repeated(vec![Instance::Group(vec![
        ("Range1".into(), scalar(Value::String("Q1".into()))),
        ("HeaderColumn".into(), scalar(Value::Int(2))),
        ("Year".into(), scalar(Value::String("2026".into()))),
        (
            "Rows".into(),
            Instance::Repeated(vec![row(101.0, 10.5), row(202.0, 20.5)]),
        ),
    ])]);
    assert_same_execution(&project, &imported.project, &source);
}

#[test]
fn invalid_special_xlsx_layout_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("xlsx-transposed.mfd"))
        .unwrap()
        .project;
    project.source_options.xlsx_rows[1] = project.source_options.xlsx_rows[0];

    let dir = TempDir::new("special-atomic");
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "existing design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("unique one-based")
    ));
    assert_eq!(std::fs::read_to_string(&design).unwrap(), "existing design");
}

#[test]
fn exports_and_reimports_hierarchical_xlsx_target() {
    let mut project = mfd::import(&fixture("xlsx-hierarchical.mfd"))
        .unwrap()
        .project;
    assert!(project.target_options.xlsx_hierarchical.is_some());
    project.target_path = Some("report.xlsx".into());

    let dir = TempDir::new("hierarchical");
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("<excel outputinstance=\"report.xlsx\"/>"));
    assert!(text.contains("<range id=\"summary\" start=\"2\" count=\"1\"/>"));
    assert!(text.contains("<range id=\"members\" offset=\"2\"/>"));
    assert!(text.contains("<entry name=\"Worksheet\" inpkey="));

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.target, project.target);
    assert_eq!(imported.project.target_path, project.target_path);
    assert_eq!(imported.project.target_options, project.target_options);
    assert!(engine::validate(&imported.project).is_empty());

    let source =
        format_xml::read(&fixture("xlsx-hierarchical-source.xml"), &project.source).unwrap();
    assert_same_execution(&project, &imported.project, &source);
}

#[test]
fn invalid_hierarchical_xlsx_layout_does_not_replace_existing_design() {
    let mut project = mfd::import(&fixture("xlsx-hierarchical.mfd"))
        .unwrap()
        .project;
    project
        .target_options
        .xlsx_hierarchical
        .as_mut()
        .unwrap()
        .ranges[0]
        .count = None;

    let dir = TempDir::new("hierarchical-atomic");
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "existing design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("row count of one")
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
