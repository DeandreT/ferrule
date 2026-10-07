use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_csv::{CsvFormatError, CsvWriteOptions, to_string_with_options, write_with_options};
use ir::{Instance, ScalarType, SchemaNode, Value};

fn schema(fields: &[(&str, ScalarType)]) -> SchemaNode {
    SchemaNode::group(
        "row",
        fields
            .iter()
            .map(|(name, ty)| SchemaNode::scalar(*name, *ty))
            .collect(),
    )
}

fn row(fields: Vec<(&str, Value)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), Instance::Scalar(value)))
            .collect::<Vec<_>>()
            .into(),
    )
}

struct TestDirectory {
    path: PathBuf,
    completed: bool,
}

impl TestDirectory {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-ordinary-csv-row-release-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            completed: false,
        })
    }

    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.completed && std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() != Ok("1") {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[test]
fn ordinary_output_matches_independent_complete_dialect_and_presence_literals() {
    let schema = schema(&[
        ("label", ScalarType::String),
        ("count", ScalarType::Int),
        ("enabled", ScalarType::Bool),
    ]);
    let rows = [
        row(vec![
            ("label", Value::String("café,\"quoted\"\r\nnext".into())),
            ("count", Value::String("  +0037  ".into())),
            ("enabled", Value::String(" 1 ".into())),
        ]),
        row(vec![
            ("label", Value::String("d'é;🙂".into())),
            ("count", Value::Int(7)),
            ("enabled", Value::Bool(false)),
        ]),
        row(vec![
            ("label", Value::json_null()),
            ("count", Value::Null),
            ("enabled", Value::Bool(false)),
        ]),
    ];
    let cases = [
        (
            CsvWriteOptions::default(),
            "label,count,enabled\n\"café,\"\"quoted\"\"\r\nnext\",37,true\nd'é;🙂,7,false\n,,false\n",
        ),
        (
            CsvWriteOptions {
                delimiter: Some(';'),
                quote: Some('\''),
                ..CsvWriteOptions::default()
            },
            "label;count;enabled\n'café,\"quoted\"\r\nnext';37;true\n'd''é;🙂';7;false\n;;false\n",
        ),
        (
            CsvWriteOptions {
                has_headers: false,
                utf8_bom: true,
                ..CsvWriteOptions::default()
            },
            "\u{feff}\"café,\"\"quoted\"\"\r\nnext\",37,true\nd'é;🙂,7,false\n,,false\n",
        ),
    ];
    for (options, expected) in cases {
        let actual = to_string_with_options(&schema, &rows, &options).unwrap();
        assert_eq!(actual.as_bytes(), expected.as_bytes());
    }
    let raw_rows = [row(vec![
        ("label", Value::String("\"raw\"🙂".into())),
        ("count", Value::Int(2)),
        ("enabled", Value::Bool(true)),
    ])];
    assert_eq!(
        to_string_with_options(
            &schema,
            &raw_rows,
            &CsvWriteOptions {
                quote_disabled: true,
                has_headers: false,
                ..CsvWriteOptions::default()
            }
        )
        .unwrap(),
        "\"raw\"🙂,2,true\n",
    );
}

#[test]
fn ordinary_empty_records_keep_exact_native_bytes_and_no_quote_refusal() {
    let empty = schema(&[]);
    let rows = [row(Vec::new())];
    for quote_disabled in [false, true] {
        assert_eq!(
            to_string_with_options(
                &empty,
                &rows,
                &CsvWriteOptions {
                    quote_disabled,
                    ..CsvWriteOptions::default()
                }
            )
            .unwrap(),
            "\"\"\n\"\"\n"
        );
    }
    let empty_name = schema(&[("", ScalarType::String)]);
    assert_eq!(
        to_string_with_options(
            &empty_name,
            &[],
            &CsvWriteOptions {
                quote_disabled: true,
                ..CsvWriteOptions::default()
            }
        )
        .unwrap(),
        "\"\"\n"
    );
    let rows = [row(vec![("", Value::String(String::new()))])];
    assert_eq!(
        to_string_with_options(
            &empty_name,
            &rows,
            &CsvWriteOptions {
                has_headers: false,
                ..CsvWriteOptions::default()
            }
        )
        .unwrap(),
        "\"\"\n"
    );
    let error = to_string_with_options(
        &empty_name,
        &rows,
        &CsvWriteOptions {
            has_headers: false,
            quote_disabled: true,
            ..CsvWriteOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        CsvFormatError::UnquotedSingleEmptyRow { row: 0 }
    ));
}

#[test]
fn ordinary_many_distinct_rows_match_the_complete_literal_pattern() {
    let schema = schema(&[("label", ScalarType::String), ("index", ScalarType::Int)]);
    let rows = (0..4096)
        .map(|index| {
            row(vec![
                ("label", Value::String(format!("row {index}, café🙂"))),
                ("index", Value::Int(index)),
            ])
        })
        .collect::<Vec<_>>();
    let mut expected = String::from("label,index\n");
    for index in 0..4096 {
        expected.push_str(&format!("\"row {index}, café🙂\",{index}\n"));
    }
    let actual = to_string_with_options(&schema, &rows, &CsvWriteOptions::default()).unwrap();
    assert_eq!(actual.as_bytes(), expected.as_bytes());
}

#[test]
fn ordinary_late_row_errors_precede_quoting_and_file_creation_or_truncation() -> std::io::Result<()>
{
    let mut directory = TestDirectory::new()?;
    let schema = schema(&[("a,b", ScalarType::String), ("number", ScalarType::Int)]);
    let first = || {
        row(vec![
            ("a,b", Value::String("early,\nrecord".into())),
            ("number", Value::Int(1)),
        ])
    };
    let options = CsvWriteOptions {
        quote_disabled: true,
        utf8_bom: true,
        ..CsvWriteOptions::default()
    };
    for (name, invalid, expected_message) in [
        (
            "shape",
            Instance::Scalar(Value::Int(9)),
            "row 1: expected a group, got int",
        ),
        (
            "unexpected",
            row(vec![
                ("a,b", Value::String("last".into())),
                ("number", Value::Int(2)),
                ("other", Value::Int(3)),
            ]),
            "row 1: unexpected column `other`",
        ),
        (
            "duplicate",
            row(vec![
                ("a,b", Value::String("last".into())),
                ("number", Value::Int(2)),
                ("number", Value::Int(3)),
            ]),
            "row 1: duplicate column `number`",
        ),
        (
            "missing",
            row(vec![("a,b", Value::String("last".into()))]),
            "row 1: missing column `number`",
        ),
        (
            "type",
            row(vec![
                ("a,b", Value::String("last".into())),
                ("number", Value::Bool(true)),
            ]),
            "row 1: column `number` expected Int, got bool",
        ),
    ] {
        let rows = [first(), invalid];
        let sentinel = directory.path.join(format!("{name}.csv"));
        std::fs::write(&sentinel, b"unchanged sentinel\n")?;
        let error = write_with_options(&sentinel, &schema, &rows, &options).unwrap_err();
        std::fs::write(
            directory.path.join(format!("{name}.error.txt")),
            format!("{error:?}\n{error}\n"),
        )?;
        assert_eq!(error.to_string(), expected_message);
        match name {
            "shape" => assert!(matches!(
                &error,
                CsvFormatError::RowShape { row: 1, got: "int" }
            )),
            "unexpected" => assert!(
                matches!(&error, CsvFormatError::UnexpectedField { row: 1, field } if field == "other")
            ),
            "duplicate" => assert!(
                matches!(&error, CsvFormatError::DuplicateField { row: 1, field } if field == "number")
            ),
            "missing" => assert!(
                matches!(&error, CsvFormatError::MissingField { row: 1, field } if field == "number")
            ),
            "type" => assert!(matches!(&error, CsvFormatError::ValueType {
                row: 1, field, expected: ScalarType::Int, got: "bool"
            } if field == "number")),
            _ => unreachable!(),
        }
        assert_eq!(std::fs::read(&sentinel)?, b"unchanged sentinel\n");
        let absent = directory
            .path
            .join("missing-parent")
            .join(format!("{name}.csv"));
        let error = write_with_options(&absent, &schema, &rows, &options).unwrap_err();
        std::fs::write(
            directory.path.join(format!("{name}-absent.error.txt")),
            format!("{error:?}\n{error}\n"),
        )?;
        assert_eq!(error.to_string(), expected_message);
        assert!(!absent.exists());
        assert!(!absent.parent().unwrap().exists());
    }
    let valid = [row(vec![
        ("a,b", Value::String("plain".into())),
        ("number", Value::Int(1)),
    ])];
    let error = to_string_with_options(&schema, &valid, &options).unwrap_err();
    assert_eq!(
        error.to_string(),
        "header `a,b` contains a separator or record boundary and cannot be written without quoting"
    );
    assert!(matches!(error, CsvFormatError::UnquotedHeaderBoundary { field } if field == "a,b"));
    let error = to_string_with_options(
        &schema,
        &[first()],
        &CsvWriteOptions {
            has_headers: false,
            ..options
        },
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "row 0: column `a,b` contains a separator or record boundary and cannot be written without quoting"
    );
    assert!(
        matches!(error, CsvFormatError::UnquotedFieldBoundary { row: 0, field } if field == "a,b")
    );
    directory.complete();
    Ok(())
}
