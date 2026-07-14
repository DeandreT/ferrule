use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, SchemaKind, Value};
use mapping::{PdfCommand, PdfPageSelection};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_pdf_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn has_minimum_extent(commands: &[PdfCommand], expected: f64) -> bool {
    commands.iter().any(|command| match command {
        PdfCommand::EdgeRows(rows) => {
            rows.minimum_extent == Some(expected) || has_minimum_extent(&rows.children, expected)
        }
        PdfCommand::GroupPerPage(group) => has_minimum_extent(&group.children, expected),
        PdfCommand::Pages(pages) => has_minimum_extent(&pages.children, expected),
        PdfCommand::Merge(merge) => has_minimum_extent(&merge.children, expected),
        PdfCommand::Capture(_) | PdfCommand::Anchor(_) | PdfCommand::BoundaryFindVertical(_) => {
            false
        }
    })
}

#[test]
fn imports_case_insensitive_pdf_references_and_table_layout() {
    let temp = TempDir::new();
    std::fs::copy(fixture("pdf-table.mfd"), temp.0.join("mapping.mfd")).unwrap();
    std::fs::copy(fixture("pdf-table.pxt"), temp.0.join("Garden-Layout.PXT")).unwrap();
    std::fs::write(temp.0.join("Garden-Input.PDF"), b"").unwrap();

    let imported = mfd::import(&temp.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("Garden-Input.PDF")
    );
    let layout = imported.project.source_options.pdf.as_ref().unwrap();
    assert!(has_minimum_extent(layout.commands(), 30.0));
    assert_eq!(imported.project.source.name, "GardenReport");
    assert!(matches!(
        imported.project.source.child("Heading").unwrap().kind,
        SchemaKind::Scalar { .. }
    ));
    let rows = imported.project.source.child("Plant").unwrap();
    assert!(rows.repeating);
    assert!(matches!(rows.kind, SchemaKind::Group { .. }));
    assert!(matches!(
        rows.child("Name").unwrap().kind,
        SchemaKind::Scalar { .. }
    ));
    assert!(matches!(
        rows.child("Quantity").unwrap().kind,
        SchemaKind::Scalar { .. }
    ));
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![
        (
            "Heading".into(),
            Instance::Scalar(Value::String("Summer stock".into())),
        ),
        (
            "Plant".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![
                    (
                        "Name".into(),
                        Instance::Scalar(Value::String("Basil".into())),
                    ),
                    (
                        "Quantity".into(),
                        Instance::Scalar(Value::String("8".into())),
                    ),
                ]),
                Instance::Group(vec![
                    (
                        "Name".into(),
                        Instance::Scalar(Value::String("heading".into())),
                    ),
                    (
                        "Quantity".into(),
                        Instance::Scalar(Value::String("not a number".into())),
                    ),
                ]),
            ]),
        ),
    ]);
    let output = engine::run(&imported.project, &source).unwrap();
    let rows = output.as_repeated().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Basil".into()))
    );
    assert_eq!(
        rows[0].field("Quantity").and_then(Instance::as_scalar),
        Some(&Value::Float(4.0))
    );

    let design = temp.0.join("export.mfd");
    std::fs::write(&design, "keep this design").unwrap();
    assert!(matches!(
        mfd::export(&imported.project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("PDF component export is not supported")
    ));
    assert_eq!(std::fs::read_to_string(design).unwrap(), "keep this design");
}

#[test]
fn imports_exact_page_regions_into_a_named_merge() {
    let temp = TempDir::new();
    assert!(std::fs::copy(fixture("pdf-merge.mfd"), temp.0.join("mapping.mfd")).is_ok());
    assert!(std::fs::copy(fixture("pdf-merge.pxt"), temp.0.join("ledger.pxt")).is_ok());
    assert!(std::fs::write(temp.0.join("ledger.pdf"), b"").is_ok());

    let Ok(imported) = mfd::import(&temp.0.join("mapping.mfd")) else {
        panic!("self-authored PDF merge fixture must import");
    };
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let Some(layout) = imported.project.source_options.pdf.as_ref() else {
        panic!("PDF merge import must retain its source layout");
    };
    assert!(matches!(
        layout.commands().first(),
        Some(PdfCommand::Pages(pages))
            if pages.selection == PdfPageSelection::First
                && matches!(pages.children.as_slice(), [PdfCommand::Capture(capture)] if capture.name == "ReportName")
    ));
    let Some(merge) = layout.commands().iter().find_map(|command| match command {
        PdfCommand::Merge(merge) => Some(merge),
        _ => None,
    }) else {
        panic!("PDF merge import must retain the named merge");
    };
    assert_eq!(merge.name, "Records");
    assert_eq!(merge.sources.len(), 2);
    assert_eq!(merge.sources[0].page_selection, PdfPageSelection::First);
    assert!(matches!(
        merge.sources[1].page_selection,
        PdfPageSelection::Range { first, last }
            if first.get() == 2 && last.get() == 2
    ));
    assert!(matches!(
        merge.children.as_slice(),
        [PdfCommand::EdgeRows(rows)]
            if rows.fallback_anchor.as_ref().is_some_and(|anchor| anchor.left.offset == 180.0)
    ));

    let Some(records) = imported.project.source.child("Record") else {
        panic!("PDF merge schema must contain repeating records");
    };
    assert!(records.repeating);
    assert!(matches!(records.kind, SchemaKind::Group { .. }));
    assert!(records.child("Code").is_some());
    assert!(records.child("Amount").is_some());
    assert!(engine::validate(&imported.project).is_empty());
}

#[test]
#[ignore = "needs the local MapForce sample set; informational only"]
fn imports_and_executes_the_local_multiline_book_catalog() {
    let samples =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("BookCatalogPDFToXML.mfd");
    let pdf = samples.join("BookCatalog.pdf");
    if !design.is_file() || !pdf.is_file() {
        return;
    }

    let Ok(imported) = mfd::import(&design) else {
        panic!("local BookCatalog PDF mapping must import");
    };
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let Some(layout) = imported.project.source_options.pdf.as_ref() else {
        panic!("local BookCatalog PDF mapping must retain its layout");
    };
    let Ok(source) = format_pdf::read(&pdf, layout) else {
        panic!("local BookCatalog PDF must extract");
    };
    let Ok(output) = engine::run(&imported.project, &source) else {
        panic!("local BookCatalog PDF mapping must execute");
    };
    let Some(books) = output.field("Book").and_then(Instance::as_repeated) else {
        panic!("local BookCatalog output must contain repeated books");
    };
    assert_eq!(books.len(), 52);
    assert!(books.iter().all(|book| {
        book.field("ISBN13").and_then(Instance::as_scalar).is_some()
            && book.field("Title").and_then(Instance::as_scalar).is_some()
            && book.field("Year").and_then(Instance::as_scalar).is_some()
            && book.field("Price").and_then(Instance::as_scalar).is_some()
            && book
                .field("Author")
                .and_then(Instance::as_repeated)
                .is_some_and(|authors| authors.len() == 1)
    }));
    assert!(format_xml::to_string(&imported.project.target, &output).is_ok());
}
