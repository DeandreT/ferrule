use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_protobuf_{}_{}",
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

fn scalar<'a>(instance: &'a Instance, name: &str) -> Option<&'a Value> {
    instance.field(name).and_then(Instance::as_scalar)
}

#[test]
fn imports_executes_and_encodes_proto2_target() {
    let imported = mfd::import(&fixture("protobuf-target.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported.project.target_path.as_deref(),
        Some("directory.bin")
    );

    let options = imported.project.target_options.protobuf.as_ref().unwrap();
    assert_eq!(options.root_message, "ferrule.fixture.Directory");
    assert_eq!(
        options.schema,
        std::fs::read_to_string(fixture("protobuf-target.proto")).unwrap()
    );
    let records = imported.project.target.child("records").unwrap();
    assert!(records.repeating);
    assert_eq!(
        records.child("code").map(|field| &field.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    );

    let source = format_xml::read(
        &fixture("protobuf-target-source.xml"),
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        scalar(&target, "title"),
        Some(&Value::String("Demo".into()))
    );
    let rows = target
        .field("records")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(scalar(&rows[0], "code"), Some(&Value::Int(7)));
    assert_eq!(
        scalar(&rows[1], "label"),
        Some(&Value::String("Two".into()))
    );
    let notes = rows[0]
        .field("notes")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(scalar(&notes[0], "text"), Some(&Value::String("A".into())));

    let layout = format_protobuf::Layout::parse(&options.schema).unwrap();
    let bytes = format_protobuf::to_vec(&layout, &options.root_message, &target).unwrap();
    assert_eq!(
        bytes,
        vec![
            0x0a, 0x04, b'D', b'e', b'm', b'o', 0x12, 0x0e, 0x08, 0x07, 0x12, 0x03, b'O', b'n',
            b'e', 0x18, 0x01, 0x22, 0x03, 0x0a, 0x01, b'A', 0x12, 0x0e, 0x08, 0x09, 0x12, 0x03,
            b'T', b'w', b'o', 0x18, 0x00, 0x22, 0x03, 0x0a, 0x01, b'B',
        ]
    );
}

#[test]
fn protobuf_options_reject_export_before_replacing_the_design() {
    let imported = mfd::import(&fixture("protobuf-target.mfd")).unwrap();
    let temp = TempDir::new();
    let design = temp.0.join("mapping.mfd");
    std::fs::write(&design, "keep this design").unwrap();

    let result = mfd::export(&imported.project, &design);
    assert!(
        matches!(result, Err(mfd::MfdError::Unsupported(message)) if message.contains("protobuf component export is not supported"))
    );
    assert_eq!(std::fs::read_to_string(design).unwrap(), "keep this design");
}
