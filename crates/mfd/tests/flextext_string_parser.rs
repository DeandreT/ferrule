use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_flextext_parser_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
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
fn imports_runtime_string_parser_and_executes_each_repeated_input() -> Result<(), Box<dyn Error>> {
    let imported = mfd::import(&fixture("flextext-string-parser.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Line".into(),
        Instance::Repeated(vec![
            Instance::Scalar(Value::String("Ada*#*3".into())),
            Instance::Scalar(Value::String("Grace*#*5".into())),
        ]),
    )]);
    let output = engine::run(&imported.project, &source)?;
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );
    assert_eq!(
        rows[1].field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(5))
    );

    let dir = TempDir::new()?;
    let design = dir.0.join("flextext-parser.mfd");
    let warnings = mfd::export(&imported.project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design)?;
    assert_eq!(xml.matches("usageKind=\"stringparse\"").count(), 1);
    assert!(xml.contains("<text type=\"txt\""));
    assert!(!xml.contains("name=\"flextext_parse_field\""));
    assert_eq!(
        std::fs::read_dir(&dir.0)?
            .filter_map(Result::ok)
            .filter(
                |entry| entry.path().extension().and_then(|value| value.to_str()) == Some("mft")
            )
            .count(),
        1
    );

    let reimported = mfd::import(&design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let output = engine::run(&reimported.project, &source)?;
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );
    assert_eq!(
        rows[1].field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(5))
    );
    Ok(())
}
