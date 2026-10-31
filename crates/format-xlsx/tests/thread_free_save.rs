//! Saving a workbook must not create a thread: sandboxed hosts refuse thread creation.
#![cfg(target_os = "linux")]

use std::process::Command;

use format_xlsx::{XlsxLimits, from_bytes, from_bytes_bounded, to_bytes, to_bytes_bounded};
use ir::{Instance, ScalarType, SchemaNode, Value};

const CHILD: &str = "FERRULE_XLSX_THREAD_FREE_CHILD";

fn schema() -> SchemaNode {
    SchemaNode::group(
        "rows",
        vec![
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("count", ScalarType::Int),
        ],
    )
}

fn rows() -> Vec<Instance> {
    (0..3)
        .map(|index| {
            Instance::Group(
                vec![
                    (
                        "label".into(),
                        Instance::Scalar(Value::String(format!("row {index}"))),
                    ),
                    ("count".into(), Instance::Scalar(Value::Int(index))),
                ]
                .into(),
            )
        })
        .collect()
}

/// Runs the child below in its own process, so the limit cannot affect other tests.
#[test]
fn workbooks_save_where_thread_creation_is_refused() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_saves_without_creating_threads",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn child_saves_without_creating_threads() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    use rustix::process::{Resource, Rlimit, setrlimit};
    setrlimit(
        Resource::Nproc,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )
    .unwrap();
    // Privileged users bypass the limit; then this check proves nothing and must not pass.
    assert!(
        std::thread::Builder::new().spawn(|| ()).is_err(),
        "thread creation is still admitted; run this test as an unprivileged user"
    );

    let bounded = to_bytes_bounded(
        &schema(),
        &rows(),
        Some("Rows"),
        2,
        &[2, 4],
        true,
        &XlsxLimits::default(),
    )
    .unwrap();
    let decoded = from_bytes_bounded(
        &bounded,
        &schema(),
        Some("Rows"),
        2,
        &[2, 4],
        true,
        &XlsxLimits::default(),
    )
    .unwrap();
    assert_eq!(decoded, rows());

    let unbounded = to_bytes(&schema(), &rows(), None, 1, &[], true).unwrap();
    assert_eq!(
        from_bytes(&unbounded, &schema(), None, 1, &[], true).unwrap(),
        rows()
    );

    // Exercise the patched loop with more than one worksheet. Distinct cell
    // values ensure neither worksheet is dropped or serialized in its place.
    let mut workbook = rust_xlsxwriter::Workbook::new();
    for (name, count) in [("First", 7), ("Second", 11)] {
        let sheet = workbook.add_worksheet();
        sheet.set_name(name).unwrap();
        sheet.write_string(0, 0, "label").unwrap();
        sheet.write_string(0, 1, "count").unwrap();
        sheet.write_string(1, 0, name).unwrap();
        sheet.write_number(1, 1, count).unwrap();
    }
    let bytes = workbook.save_to_buffer().unwrap();
    for (name, count) in [("First", 7), ("Second", 11)] {
        let expected = vec![Instance::Group(
            vec![
                ("label".into(), Instance::Scalar(Value::String(name.into()))),
                ("count".into(), Instance::Scalar(Value::Int(count))),
            ]
            .into(),
        )];
        assert_eq!(
            from_bytes(&bytes, &schema(), Some(name), 1, &[], true).unwrap(),
            expected
        );
    }
}
