use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, SchemaKind, Value};
use mapping::PdfCommand;

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
    assert!(layout.commands().iter().any(|command| {
        matches!(
            command,
            PdfCommand::EdgeRows(rows) if rows.minimum_extent == Some(30.0)
        )
    }));
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
