use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::{XlsxCellKind, XlsxRangeStart};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn child<'a>(instance: &'a Instance, name: &str) -> Option<&'a Instance> {
    let Instance::Group(fields) = instance else {
        return None;
    };
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

#[test]
fn imports_and_executes_runtime_named_hierarchical_workbook() {
    let imported = mfd::import(&fixture("xlsx-hierarchical.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let layout = imported
        .project
        .target_options
        .xlsx_hierarchical
        .as_ref()
        .unwrap();
    assert_eq!(layout.worksheets_path, ["Worksheets"]);
    assert_eq!(layout.worksheet_name_path, ["Name"]);
    assert_eq!(layout.ranges.len(), 3);
    assert!(matches!(
        layout.ranges[0].start,
        XlsxRangeStart::Absolute { row } if row.get() == 2
    ));
    assert!(layout.ranges[0].has_header);
    assert_eq!(layout.ranges[1].columns[0].column.get(), 2);
    assert_eq!(layout.ranges[1].columns[1].kind, XlsxCellKind::Number);
    assert!(matches!(
        layout.ranges[2].start,
        XlsxRangeStart::AfterPrevious { offset } if offset.get() == 3
    ));

    let source = format_xml::read(
        &fixture("xlsx-hierarchical-source.xml"),
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let worksheets = child(&target, "Worksheets")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(worksheets.len(), 2);
    assert_eq!(
        child(&worksheets[0], "Name"),
        Some(&Instance::Scalar(Value::String("Design".into())))
    );
    assert_eq!(
        child(&worksheets[0], "Rangemembers")
            .and_then(Instance::as_repeated)
            .map(<[Instance]>::len),
        Some(2)
    );

    let (bytes, worksheet_count) =
        format_xlsx::to_bytes_hierarchical(&imported.project.target, &target, layout).unwrap();
    assert_eq!(worksheet_count, 2);
    assert!(bytes.starts_with(b"PK"));
}
