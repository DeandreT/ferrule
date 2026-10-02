use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_csv::{CsvFormatError, CsvReadOptions, CsvWriteOptions};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{CsvTextRepairCause, CsvTextRepairDependency, FormatOptions};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_csv_repair_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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
fn schema() -> SchemaNode {
    SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
}
fn dependency() -> CsvTextRepairDependency {
    CsvTextRepairDependency::new(CsvTextRepairCause::Encoding)
}
fn assert_repair<T>(result: Result<T, CsvFormatError>) {
    assert!(matches!(result, Err(CsvFormatError::RepairDependency(d)) if d == dependency()));
}

#[test]
fn configured_read_and_write_reject_before_open_parse_or_truncate() {
    let temp = TempDir::new();
    let options = FormatOptions {
        csv_text_repair_dependency: Some(dependency()),
        ..Default::default()
    };
    let read = CsvReadOptions::from(&options);
    let write = CsvWriteOptions::from(&options);
    fn copy<T: Copy>(_: T) {}
    copy(read);
    copy(write);
    assert_repair(format_csv::read_with_options(
        &temp.0.join("missing.json"),
        &schema(),
        &read,
    ));
    assert_repair(format_csv::from_str_with_options(
        "\"unterminated",
        &schema(),
        &read,
    ));
    assert_repair(format_csv::to_string_with_options(&schema(), &[], &write));
    let sentinel = temp.0.join("sentinel.csv");
    std::fs::write(&sentinel, b"sentinel").unwrap();
    assert_repair(format_csv::write_with_options(
        &sentinel,
        &schema(),
        &[],
        &write,
    ));
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"sentinel");
    let absent = temp.0.join("absent.csv");
    assert_repair(format_csv::write_with_options(
        &absent,
        &schema(),
        &[],
        &write,
    ));
    assert!(!absent.exists());
}

#[test]
fn ordinary_legacy_and_configured_csv_controls_keep_exact_bytes() {
    let rows = vec![Instance::Group(
        (vec![(
            "Value".into(),
            Instance::Scalar(Value::String("café".into())),
        )])
        .into(),
    )];
    assert_eq!(
        format_csv::to_string(&schema(), &rows, None, true).unwrap(),
        "Value\ncafé\n"
    );
    let options = CsvWriteOptions {
        utf8_bom: true,
        ..Default::default()
    };
    let text = format_csv::to_string_with_options(&schema(), &rows, &options).unwrap();
    assert_eq!(text.as_bytes(), b"\xef\xbb\xbfValue\ncaf\xc3\xa9\n");
    assert_eq!(
        format_csv::from_str_with_options(&text, &schema(), &CsvReadOptions::default()).unwrap(),
        rows
    );
}
