use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use format_csv::{
    CsvByteDecodingPolicy, CsvReadOptions, CsvSourceDocument, CsvSourceError, CsvSourceLoadError,
    CsvSourceRows, MAX_CSV_SOURCE_DOCUMENTS, MAX_CSV_SOURCE_PATH_BYTES,
};
use ir::{Instance, ScalarType, SchemaNode, Value};

fn schema() -> SchemaNode {
    SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
}

fn parse(path: &str, bytes: &[u8]) -> CsvSourceDocument {
    CsvSourceDocument::parse_bytes(
        path,
        Some("C:/resolved/same.csv"),
        bytes,
        &schema(),
        &CsvReadOptions::default(),
        CsvByteDecodingPolicy::StrictUtf8,
    )
    .unwrap()
}

fn text(row: &format_csv::CsvRowOccurrence) -> &str {
    match row.value().field("Value") {
        Some(Instance::Scalar(Value::String(text))) => text,
        _ => panic!("expected string field"),
    }
}

#[test]
fn rows_keep_exact_documents_positions_and_whole_parsed_values() {
    let first = parse(" ./same.csv ", b"Value\nA\nA\nB\n");
    let second = parse(" ./same.csv ", b"Value\nA\nA\nB\n");
    assert!(first.same_document(&first.clone()));
    assert!(!first.same_document(&second));
    assert_eq!(first.path(), " ./same.csv ");
    assert_eq!(first.source_path(), "C:/resolved/same.csv");
    let rows: Vec<_> = first.rows().collect();
    assert_eq!(rows.iter().map(text).collect::<Vec<_>>(), ["A", "A", "B"]);
    assert_eq!(
        rows.iter().map(|r| r.position()).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(rows[0].same_occurrence(&first.rows().next().unwrap()));
    assert!(!rows[0].same_occurrence(&rows[1]));
    assert!(!rows[0].same_occurrence(&second.rows().next().unwrap()));
    let whole =
        Instance::Group(vec![("Value".into(), Instance::Scalar(Value::String("A".into())))].into());
    assert_eq!(rows[0].value(), &whole);
    drop(first);
    assert_eq!(text(&rows[0]), "A");
}

#[test]
fn occurrences_retain_complete_typed_rows_and_missing_cells() {
    let schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Amount", ScalarType::Int),
            SchemaNode::scalar("Flag", ScalarType::Bool),
        ],
    );
    let options = CsvReadOptions {
        preserve_empty_strings: true,
        ..Default::default()
    };
    let document = CsvSourceDocument::parse_bytes(
        "original.csv",
        None,
        "Key,Amount,Flag\n\"Snow,☃\nline\",7,true\n,0,false\ntail\n".as_bytes(),
        &schema,
        &options,
        CsvByteDecodingPolicy::StrictUtf8,
    )
    .unwrap();
    let row = |key: &str, amount, flag| {
        Instance::Group(
            vec![
                ("Key".into(), Instance::Scalar(Value::String(key.into()))),
                ("Amount".into(), Instance::Scalar(amount)),
                ("Flag".into(), Instance::Scalar(flag)),
            ]
            .into(),
        )
    };
    let expected = [
        row("Snow,☃\nline", Value::Int(7), Value::Bool(true)),
        row("", Value::Int(0), Value::Bool(false)),
        row("tail", Value::Null, Value::Null),
    ];
    let actual: Vec<_> = document.rows().collect();
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual.value(), &expected);
        assert_eq!(actual.position(), index + 1);
        assert!(actual.document().same_document(&document));
        assert_eq!(actual.document().source_path(), "original.csv");
    }
}

#[test]
fn cross_file_groups_retain_each_row_origin_in_host_order() {
    let requested = Rc::new(RefCell::new(Vec::new()));
    let loaded = Rc::new(RefCell::new(Vec::new()));
    let requests = ["first", "empty", "second"].into_iter().map({
        let requested = requested.clone();
        move |name| {
            requested.borrow_mut().push(name);
            Ok::<_, String>(name.to_owned())
        }
    });
    let mut stream = CsvSourceRows::new(
        requests,
        {
            let loaded = loaded.clone();
            move |name| {
                loaded.borrow_mut().push(name.to_owned());
                Ok(parse(
                    name,
                    match name {
                        "first" => b"Value\nA\nB\n",
                        "empty" => b"Value\n",
                        "second" => b"Value\nA\n",
                        _ => panic!("unexpected request"),
                    },
                ))
            }
        },
        3,
    )
    .unwrap();
    assert!(requested.borrow().is_empty());
    assert!(loaded.borrow().is_empty());
    let first = stream.next().unwrap().unwrap();
    assert_eq!(&*requested.borrow(), &["first"]);
    let mut groups = BTreeMap::new();
    groups
        .entry(text(&first).to_owned())
        .or_insert_with(Vec::new)
        .push(first);
    for row in stream.by_ref() {
        let row = row.unwrap();
        groups
            .entry(text(&row).to_owned())
            .or_insert_with(Vec::new)
            .push(row);
    }
    assert!(stream.next().is_none());
    assert_eq!(&*requested.borrow(), &["first", "empty", "second"]);
    assert_eq!(&*loaded.borrow(), &["first", "empty", "second"]);
    let a = &groups["A"];
    assert_eq!(
        a.iter()
            .map(|r| (r.document().path(), r.position()))
            .collect::<Vec<_>>(),
        [("first", 1), ("second", 1)]
    );
    assert!(!a[0].document().same_document(a[1].document()));
    assert_eq!(groups["B"][0].position(), 2);
}

#[test]
fn load_and_enumeration_failures_stop_without_a_later_request() {
    for failed in 0..3 {
        let calls = RefCell::new(Vec::new());
        let requests = (0..3).map(|n| Ok::<_, &'static str>(n.to_string()));
        let mut rows = CsvSourceRows::new(
            requests,
            |name| {
                calls.borrow_mut().push(name.to_owned());
                if name == failed.to_string() {
                    Err("load")
                } else {
                    Ok(parse(name, b"Value\nA\n"))
                }
            },
            3,
        )
        .unwrap();
        for _ in 0..failed {
            assert!(rows.next().unwrap().is_ok());
        }
        assert!(matches!(
            rows.next(),
            Some(Err(CsvSourceLoadError::Host("load")))
        ));
        assert!(rows.next().is_none());
        assert_eq!(calls.borrow().len(), failed + 1);
    }
    let calls = RefCell::new(Vec::new());
    let requests = [
        Ok("one".to_owned()),
        Err("enumeration"),
        Ok("never".to_owned()),
    ]
    .into_iter();
    let mut rows = CsvSourceRows::new(
        requests,
        |name| {
            calls.borrow_mut().push(name.to_owned());
            Ok(parse(name, b"Value\nA\n"))
        },
        3,
    )
    .unwrap();
    assert!(rows.next().unwrap().is_ok());
    assert!(matches!(
        rows.next(),
        Some(Err(CsvSourceLoadError::Host("enumeration")))
    ));
    assert!(rows.next().is_none());
    assert_eq!(&*calls.borrow(), &["one"]);
}

#[test]
fn limits_include_empty_documents_and_do_not_invoke_extra_loader() {
    for maximum in [0, 1] {
        let calls = RefCell::new(Vec::new());
        let requests = ["a", "b"].into_iter().map(|s| Ok::<_, String>(s.into()));
        let mut rows = CsvSourceRows::new(
            requests,
            |name| {
                calls.borrow_mut().push(name.to_owned());
                Ok(parse(name, b"Value\n"))
            },
            maximum,
        )
        .unwrap();
        assert!(
            matches!(rows.next(), Some(Err(CsvSourceLoadError::Documents { maximum: n })) if n == maximum)
        );
        assert!(rows.next().is_none());
        assert_eq!(calls.borrow().len(), maximum);
    }
    assert!(matches!(
        CsvSourceRows::new(
            std::iter::empty::<Result<String, String>>(),
            |_| unreachable!(),
            MAX_CSV_SOURCE_DOCUMENTS + 1
        ),
        Err(CsvSourceLoadError::InvalidLimit)
    ));
}

#[test]
fn path_validation_and_byte_decoding_stay_explicit() {
    let parse = |path: &str, resolved, bytes: &[u8], policy| {
        CsvSourceDocument::parse_bytes(
            path,
            resolved,
            bytes,
            &schema(),
            &CsvReadOptions::default(),
            policy,
        )
    };
    for path in [
        String::new(),
        "a\0b".into(),
        "x".repeat(MAX_CSV_SOURCE_PATH_BYTES + 1),
    ] {
        assert!(matches!(
            parse(
                &path,
                None,
                b"Value\nA\n",
                CsvByteDecodingPolicy::StrictUtf8
            ),
            Err(CsvSourceError::Path { field: "path" })
        ));
    }
    assert!(
        parse(
            &"x".repeat(MAX_CSV_SOURCE_PATH_BYTES),
            None,
            b"Value\n",
            CsvByteDecodingPolicy::StrictUtf8
        )
        .is_ok()
    );
    assert!(matches!(
        parse(
            "ok",
            Some(""),
            b"Value\n",
            CsvByteDecodingPolicy::StrictUtf8
        ),
        Err(CsvSourceError::Path {
            field: "resolved path"
        })
    ));
    assert!(matches!(
        parse(
            "ok",
            None,
            b"Value\n\xff\n",
            CsvByteDecodingPolicy::StrictUtf8
        ),
        Err(CsvSourceError::Read(format_csv::CsvReadError::Encoding(_)))
    ));
    let doc = parse(
        "ok",
        None,
        b"Value\n\xff\n",
        CsvByteDecodingPolicy::BomAwareNonFlushingReplacement,
    )
    .unwrap();
    assert_eq!(doc.source_path(), "ok");
    assert_eq!(text(&doc.rows().next().unwrap()), "\u{fffd}");
    let numeric = SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    assert!(matches!(
        CsvSourceDocument::parse_bytes(
            "bad",
            None,
            b"Value\n1\ninvalid\n2\n",
            &numeric,
            &CsvReadOptions::default(),
            CsvByteDecodingPolicy::StrictUtf8
        ),
        Err(CsvSourceError::Read(_))
    ));
}

#[test]
fn partial_document_is_never_yielded_and_panics_do_not_resume_the_stream() {
    let numeric = SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    let calls = RefCell::new(Vec::new());
    let mut rows = CsvSourceRows::new(
        ["good", "bad", "never"]
            .into_iter()
            .map(|s| Ok(s.to_owned())),
        |name| {
            calls.borrow_mut().push(name.to_owned());
            CsvSourceDocument::parse_bytes(
                name,
                None,
                if name == "bad" {
                    b"Value\n1\ninvalid\n2\n"
                } else {
                    b"Value\n3\n"
                },
                &numeric,
                &CsvReadOptions::default(),
                CsvByteDecodingPolicy::StrictUtf8,
            )
        },
        3,
    )
    .unwrap();
    let first = rows.next().unwrap().unwrap();
    assert_eq!(
        first.value().field("Value"),
        Some(&Instance::Scalar(Value::Int(3)))
    );
    assert!(matches!(
        rows.next(),
        Some(Err(CsvSourceLoadError::Host(CsvSourceError::Read(_))))
    ));
    assert!(rows.next().is_none());
    assert_eq!(&*calls.borrow(), &["good", "bad"]);
    let mut calls = 0;
    let mut panics = CsvSourceRows::new(
        [Ok::<_, String>("first".into()), Ok("never".into())].into_iter(),
        |_| {
            calls += 1;
            panic!("host panic")
        },
        2,
    )
    .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| panics.next())).is_err());
    assert!(panics.next().is_none());
    drop(panics);
    assert_eq!(calls, 1);
}
