use format_csv::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceData, acquire_csv_file_set,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use std::cell::Cell;

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
}
fn options() -> CsvReadOptions {
    CsvReadOptions {
        has_headers: false,
        ..Default::default()
    }
}

#[test]
fn preserves_empty_files_and_each_rows_original_location() {
    let calls = Cell::new(0);
    let value = acquire_csv_file_set(
        || Ok::<_, String>("pattern".into()),
        |name| {
            assert_eq!(name, "pattern");
            Ok(["empty.csv", "rows.csv"]
                .into_iter()
                .map(|s| Ok::<_, String>(s.into())))
        },
        |name| {
            calls.set(calls.get() + 1);
            Ok(CsvSourceData {
                bytes: if name == "empty.csv" {
                    vec![]
                } else {
                    b"a\nb\n".to_vec()
                },
                resolved_path: Some(format!(" actual/{name} ")),
            })
        },
        &schema(),
        &options(),
        CsvByteDecodingPolicy::StrictUtf8,
        2,
    )
    .unwrap();
    assert_eq!(calls.get(), 2);
    let files = value.field("Files").unwrap().as_repeated().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(
        files[0].field("Path").unwrap().as_scalar(),
        Some(&Value::String(" actual/empty.csv ".into()))
    );
    assert!(
        files[0]
            .field("Rows")
            .unwrap()
            .as_repeated()
            .unwrap()
            .is_empty()
    );
    let rows = files[1].field("Rows").unwrap().as_repeated().unwrap();
    for (index, row) in rows.iter().enumerate() {
        let Instance::Group(group) = row else {
            unreachable!()
        };
        let origin = group.source_origin().unwrap();
        assert_eq!(origin.position(), index + 1);
        assert_eq!(origin.document().source_path(), " actual/rows.csv ");
    }
}

#[test]
fn failures_return_no_partial_file_set_and_limits_precede_loading() {
    let loads = Cell::new(0);
    let result = acquire_csv_file_set(
        || Ok::<_, String>("pattern".into()),
        |_| {
            Ok(["first", "second"]
                .into_iter()
                .map(|s| Ok::<_, String>(s.into())))
        },
        |_| {
            loads.set(loads.get() + 1);
            Ok(CsvSourceData {
                bytes: b"a\n".to_vec(),
                resolved_path: None,
            })
        },
        &schema(),
        &options(),
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    );
    assert!(matches!(
        result,
        Err(CsvAcquisitionError::Documents { maximum: 1 })
    ));
    assert_eq!(loads.get(), 1);
    let result = acquire_csv_file_set(
        || Ok::<_, String>("pattern".into()),
        |_| {
            Ok(["first", "second"]
                .into_iter()
                .map(|s| Ok::<_, String>(s.into())))
        },
        |name| {
            if name == "second" {
                Err("unreadable".into())
            } else {
                Ok(CsvSourceData {
                    bytes: b"a\n".to_vec(),
                    resolved_path: None,
                })
            }
        },
        &schema(),
        &options(),
        CsvByteDecodingPolicy::StrictUtf8,
        2,
    );
    assert!(matches!(result,Err(CsvAcquisitionError::Load{path,..}) if path=="second"));
    let invalid = SchemaNode::group("Path", vec![]);
    let result = acquire_csv_file_set(
        || -> Result<String, String> { panic!("invalid schema evaluated filename") },
        |_| Ok(std::iter::empty::<Result<String, String>>()),
        |_| -> Result<CsvSourceData, String> { panic!("invalid schema loaded file") },
        &invalid,
        &options(),
        CsvByteDecodingPolicy::StrictUtf8,
        0,
    );
    assert!(matches!(
        result,
        Err(CsvAcquisitionError::InvalidFileSetSchema)
    ));
}
