use std::{cell::RefCell, error::Error, fmt, rc::Rc};

use format_csv::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceAcquisition,
    CsvSourceData, MAX_CSV_SOURCE_DOCUMENTS,
};
use ir::{Instance, ScalarType, SchemaNode, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
struct HostError(&'static str);
impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl Error for HostError {}
fn schema() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    )
}
fn data(text: &str) -> CsvSourceData {
    CsvSourceData {
        bytes: text.as_bytes().to_vec(),
        resolved_path: Some("resolved/same.csv".into()),
    }
}

#[test]
fn lazy_acquisition_retains_each_document_and_local_row_identity() {
    let schema = schema();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let a = calls.clone();
    let b = calls.clone();
    let c = calls.clone();
    let mut rows = CsvSourceAcquisition::new(
        move || {
            a.borrow_mut().push("filename".to_string());
            Ok::<_, HostError>(" opaque *.csv ".to_string())
        },
        move |filename| {
            b.borrow_mut().push(format!("select:{filename}"));
            Ok(["same.csv", "empty.csv", "same.csv"]
                .into_iter()
                .map(|name| Ok(name.into())))
        },
        move |path| {
            c.borrow_mut().push(format!("load:{path}"));
            Ok(data(if path == "empty.csv" {
                "Text,Count\n"
            } else {
                "Text,Count\n\"Snow\nline\",7\n,8\n"
            }))
        },
        &schema,
        CsvReadOptions {
            preserve_empty_strings: true,
            ..Default::default()
        },
        CsvByteDecodingPolicy::StrictUtf8,
        3,
    )
    .unwrap();
    assert!(calls.borrow().is_empty());
    let first = rows.next().unwrap().unwrap();
    assert_eq!(
        &*calls.borrow(),
        &["filename", "select: opaque *.csv ", "load:same.csv"]
    );
    let mut all = vec![first];
    all.extend(rows.by_ref().map(Result::unwrap));
    assert!(rows.next().is_none());
    assert_eq!(
        &*calls.borrow(),
        &[
            "filename",
            "select: opaque *.csv ",
            "load:same.csv",
            "load:empty.csv",
            "load:same.csv"
        ]
    );
    assert_eq!(all.len(), 4);
    for (index, row) in all.iter().enumerate() {
        assert_eq!(row.document().path(), "same.csv");
        assert_eq!(row.document().source_path(), "resolved/same.csv");
        assert_eq!(row.position(), index % 2 + 1);
        assert_eq!(
            row.value().field("Text").and_then(Instance::as_scalar),
            Some(&Value::String(
                if index % 2 == 0 { "Snow\nline" } else { "" }.into()
            ))
        );
        assert_eq!(
            row.value().field("Count").and_then(Instance::as_scalar),
            Some(&Value::Int(if index % 2 == 0 { 7 } else { 8 }))
        );
    }
    assert!(all[0].document().same_document(all[1].document()));
    assert!(!all[0].document().same_document(all[2].document()));
    assert!(!all[0].same_occurrence(&all[2]));
    assert!(all[0].same_occurrence(&all[0].clone()));
}

#[test]
fn complete_document_parse_failure_is_terminal_before_later_loads() {
    let schema = schema();
    for failure in 0..3 {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let loaded = calls.clone();
        let mut rows = CsvSourceAcquisition::new(
            || Ok::<_, HostError>("files".into()),
            |_| Ok((0..3).map(|index| Ok(index.to_string()))),
            move |path| {
                let index = path.parse::<usize>().unwrap();
                loaded.borrow_mut().push(index);
                Ok(data(if index == failure {
                    "Text,Count\nvalid,1\nbad,invalid\n"
                } else {
                    "Text,Count\nvalid,1\n"
                }))
            },
            &schema,
            Default::default(),
            CsvByteDecodingPolicy::StrictUtf8,
            3,
        )
        .unwrap();
        for _ in 0..failure {
            assert!(rows.next().unwrap().is_ok());
        }
        assert!(
            matches!(rows.next(),Some(Err(CsvAcquisitionError::Input {path,..})) if path==failure.to_string())
        );
        assert!(rows.next().is_none());
        assert!(rows.next().is_none());
        assert_eq!(*calls.borrow(), (0..=failure).collect::<Vec<_>>());
    }
}

#[test]
fn filename_selection_request_and_load_failures_keep_their_stages() {
    let schema = schema();
    let mut filename = CsvSourceAcquisition::new(
        || Err::<String, _>(HostError("filename")),
        |_| -> Result<std::iter::Empty<Result<String, HostError>>, HostError> {
            panic!("selected after filename failure")
        },
        |_| -> Result<CsvSourceData, HostError> { panic!("loaded after failure") },
        &schema,
        Default::default(),
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    )
    .unwrap();
    assert!(matches!(
        filename.next(),
        Some(Err(CsvAcquisitionError::Filename(HostError("filename"))))
    ));
    assert!(filename.next().is_none());
    let mut selection = CsvSourceAcquisition::new(
        || Ok::<_, HostError>("files".into()),
        |_| Err::<std::iter::Empty<Result<String, HostError>>, _>(HostError("selection")),
        |_| -> Result<CsvSourceData, HostError> { panic!("loaded after failure") },
        &schema,
        Default::default(),
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    )
    .unwrap();
    assert!(matches!(
        selection.next(),
        Some(Err(CsvAcquisitionError::Selection(HostError("selection"))))
    ));
    assert!(selection.next().is_none());
    let mut request = CsvSourceAcquisition::new(
        || Ok::<_, HostError>("files".into()),
        |_| Ok(std::iter::once(Err(HostError("request")))),
        |_| -> Result<CsvSourceData, HostError> { panic!("loaded after failure") },
        &schema,
        Default::default(),
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    )
    .unwrap();
    assert!(matches!(
        request.next(),
        Some(Err(CsvAcquisitionError::Request(HostError("request"))))
    ));
    assert!(request.next().is_none());
    let mut load = CsvSourceAcquisition::new(
        || Ok::<_, HostError>("files".into()),
        |_| Ok(std::iter::once(Ok("first.csv".into()))),
        |_| Err(HostError("load")),
        &schema,
        Default::default(),
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    )
    .unwrap();
    let error = load.next().unwrap().unwrap_err();
    assert!(
        matches!(&error,CsvAcquisitionError::Load {path,source:HostError("load")} if path=="first.csv")
    );
    assert_eq!(error.source().unwrap().to_string(), "load");
    assert!(load.next().is_none());
}

#[test]
fn count_and_path_limits_precede_loading_including_empty_documents() {
    let schema = schema();
    let invalid = CsvSourceAcquisition::new(
        || Ok::<_, HostError>("files".into()),
        |_| Ok(std::iter::empty()),
        |_| Ok(data("")),
        &schema,
        Default::default(),
        CsvByteDecodingPolicy::StrictUtf8,
        MAX_CSV_SOURCE_DOCUMENTS + 1,
    );
    assert!(matches!(
        invalid,
        Err(CsvAcquisitionError::InvalidLimit { .. })
    ));
    for path in ["", "bad\0path"] {
        let mut invalid = CsvSourceAcquisition::new(
            || Ok::<_, HostError>("files".into()),
            |_| Ok(std::iter::once(Ok(path.into()))),
            |_| -> Result<CsvSourceData, HostError> { panic!("invalid path loaded") },
            &schema,
            Default::default(),
            CsvByteDecodingPolicy::StrictUtf8,
            1,
        )
        .unwrap();
        assert!(matches!(
            invalid.next(),
            Some(Err(CsvAcquisitionError::Input { .. }))
        ));
        assert!(invalid.next().is_none());
    }
    let mut rows = CsvSourceAcquisition::new(
        || Ok::<_, HostError>("files".into()),
        |_| {
            Ok(["empty.csv", "second.csv"]
                .into_iter()
                .map(|s| Ok(s.into())))
        },
        |path| {
            assert_eq!(path, "empty.csv");
            Ok(data("Text,Count\n"))
        },
        &schema,
        Default::default(),
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    )
    .unwrap();
    assert!(matches!(
        rows.next(),
        Some(Err(CsvAcquisitionError::Documents { maximum: 1 }))
    ));
    assert!(rows.next().is_none());
}

#[test]
fn unwinding_from_a_callback_does_not_allow_another_request() {
    let schema = schema();
    let mut rows = CsvSourceAcquisition::new(
        || Ok::<_, HostError>("files".into()),
        |_| Ok(std::iter::once(Ok("first.csv".into()))),
        |_| -> Result<CsvSourceData, HostError> { panic!("host panic") },
        &schema,
        Default::default(),
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    )
    .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rows.next())).is_err());
    assert!(rows.next().is_none());
}

#[test]
fn acquisition_uses_the_explicit_decoding_policy() {
    let schema = schema();
    let mut bytes = vec![0xff, 0xfe];
    for unit in "Text,Count\nSnow☃,7\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    for policy in [
        CsvByteDecodingPolicy::StrictUtf8,
        CsvByteDecodingPolicy::BomAwareNonFlushingReplacement,
    ] {
        let payload = bytes.clone();
        let mut rows = CsvSourceAcquisition::new(
            || Ok::<_, HostError>("files".into()),
            |_| Ok(std::iter::once(Ok("input.csv".into()))),
            move |_| {
                Ok(CsvSourceData {
                    bytes: payload.clone(),
                    resolved_path: None,
                })
            },
            &schema,
            Default::default(),
            policy,
            1,
        )
        .unwrap();
        let result = rows.next().unwrap();
        if policy == CsvByteDecodingPolicy::StrictUtf8 {
            assert!(matches!(result, Err(CsvAcquisitionError::Input { .. })));
        } else {
            let row = result.unwrap();
            assert_eq!(
                row.value().field("Text").and_then(Instance::as_scalar),
                Some(&Value::String("Snow☃".into()))
            );
        }
        assert!(rows.next().is_none());
    }
}
