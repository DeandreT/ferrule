//! Independently authored output contract; no importer or reference-product data.
use format_csv::{
    CsvBoundedError, CsvDocumentsError, CsvFormatError, CsvWriteOptions, to_mapped_document_bytes,
    to_mapped_document_text,
};
use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};

const B: &[u8] =
    b"\xef\xbb\xbfClient;SKU;Total;Label\nB;x;5;\"\xc3\xa9;\"\"Q\"\"\r\nZ\"\nB;y;6;bee\n";
const A: &[u8] = b"\xef\xbb\xbfClient;SKU;Total;Label\nA;x;12;\"A\"\"x\"\nA;z;-2;\xe5\xb0\xbe\n";
const BP: &str = "out/B-2026-11-12T03:04:05Z.csv";
const AP: &str = "out/A-2026-11-12T03:04:05Z.csv";

fn schema() -> SchemaNode {
    SchemaNode::group(
        "DetailLines",
        vec![
            SchemaNode::scalar("Client", ScalarType::String),
            SchemaNode::scalar("SKU", ScalarType::String),
            SchemaNode::scalar("Total", ScalarType::Int),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    )
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn row(client: &str, sku: &str, total: i64, label: &str) -> Instance {
    Instance::Group(
        vec![
            ("Label".into(), text(label)),
            ("Total".into(), Instance::Scalar(Value::Int(total))),
            ("SKU".into(), text(sku)),
            ("Client".into(), text(client)),
        ]
        .into(),
    )
}
fn wrap(rows: Vec<Instance>) -> Instance {
    Instance::Group(vec![("DetailLines".into(), Instance::Repeated(rows))].into())
}
fn documents(second: Instance) -> Instance {
    Instance::DocumentSet(vec![
        DocumentMember::new(
            BP,
            wrap(vec![
                row("B", "x", 5, "é;\"Q\"\r\nZ"),
                row("B", "y", 6, "bee"),
            ]),
        )
        .unwrap(),
        DocumentMember::new(AP, second).unwrap(),
    ])
}
fn value() -> Instance {
    documents(wrap(vec![
        row("A", "x", 12, "A\"x"),
        row("A", "z", -2, "尾"),
    ]))
}
fn options() -> CsvWriteOptions {
    CsvWriteOptions {
        delimiter: Some(';'),
        utf8_bom: true,
        ..Default::default()
    }
}

#[test]
fn literal_documents_and_logical_paths_are_complete_ordered_and_opaque() {
    let bytes = to_mapped_document_bytes(&schema(), &value(), "DetailLines", &options()).unwrap();
    assert_eq!(
        bytes
            .iter()
            .map(|d| (d.path.as_str(), d.bytes.as_slice()))
            .collect::<Vec<_>>(),
        [(BP, B), (AP, A)]
    );
    assert_eq!((bytes[0].bytes.len(), bytes[1].bytes.len()), (56, 51));
    let texts = to_mapped_document_text(&schema(), &value(), "DetailLines", &options()).unwrap();
    assert_eq!(
        texts[0].text,
        "\u{feff}Client;SKU;Total;Label\nB;x;5;\"é;\"\"Q\"\"\r\nZ\"\nB;y;6;bee\n"
    );
    assert_eq!(
        texts[1].text,
        "\u{feff}Client;SKU;Total;Label\nA;x;12;\"A\"\"x\"\nA;z;-2;尾\n"
    );
    for path in [
        "same.csv",
        "../logical.csv",
        "C:/absolute.csv",
        "/absolute.csv",
    ] {
        let Instance::DocumentSet(members) = value() else {
            unreachable!()
        };
        let duplicate = Instance::DocumentSet(
            members
                .into_iter()
                .map(|member| DocumentMember::new(path, member.value().clone()).unwrap())
                .collect(),
        );
        let output =
            to_mapped_document_bytes(&schema(), &duplicate, "DetailLines", &options()).unwrap();
        assert_eq!(
            output
                .iter()
                .map(|d| (d.path.as_str(), d.bytes.as_slice()))
                .collect::<Vec<_>>(),
            [(path, B), (path, A)]
        );
    }
    let plain = to_mapped_document_bytes(
        &schema(),
        &value(),
        "DetailLines",
        &CsvWriteOptions {
            utf8_bom: false,
            has_headers: false,
            ..options()
        },
    )
    .unwrap();
    assert_eq!(
        plain[0].bytes,
        b"B;x;5;\"\xc3\xa9;\"\"Q\"\"\r\nZ\"\nB;y;6;bee\n"
    );
    assert_eq!(plain[1].bytes, b"A;x;12;\"A\"\"x\"\nA;z;-2;\xe5\xb0\xbe\n");
}

#[test]
fn exact_wrapper_multiplicity_and_container_types_are_not_empty_rows() {
    let repeat = Instance::Repeated(vec![]);
    for malformed in [
        Instance::Group(vec![].into()),
        Instance::Scalar(Value::Null),
        Instance::Group(vec![("DetailLines".into(), Instance::Scalar(Value::Null))].into()),
        Instance::Group(vec![("DetailLines".into(), Instance::Group(vec![].into()))].into()),
        Instance::Group(vec![("DetailLines".into(), Instance::MappedSequence(vec![]))].into()),
        Instance::Group(
            vec![
                ("DetailLines".into(), repeat.clone()),
                ("DetailLines".into(), repeat.clone()),
            ]
            .into(),
        ),
        Instance::Group(
            vec![
                ("DetailLines".into(), repeat.clone()),
                ("Spare".into(), Instance::Scalar(Value::Null)),
            ]
            .into(),
        ),
        Instance::Group(vec![("detailLines".into(), repeat)].into()),
    ] {
        assert!(
            matches!(to_mapped_document_bytes(&schema(), &documents(malformed), "DetailLines", &options()),
            Err(CsvDocumentsError::DocumentShape { document: 1, path }) if path == AP)
        );
    }
    for field in ["", "\0"] {
        assert!(matches!(
            to_mapped_document_bytes(&schema(), &Instance::DocumentSet(vec![]), field, &options()),
            Err(CsvDocumentsError::InvalidRowsField)
        ));
    }
    for (headers, bom, expected) in [
        (
            true,
            true,
            b"\xef\xbb\xbfClient;SKU;Total;Label\n".as_slice(),
        ),
        (false, true, b"\xef\xbb\xbf"),
        (false, false, b""),
    ] {
        let empty = Instance::DocumentSet(vec![DocumentMember::new(AP, wrap(vec![])).unwrap()]);
        let output = to_mapped_document_bytes(
            &schema(),
            &empty,
            "DetailLines",
            &CsvWriteOptions {
                has_headers: headers,
                utf8_bom: bom,
                ..options()
            },
        )
        .unwrap();
        assert_eq!(
            (&*output[0].path, output[0].bytes.as_slice()),
            (AP, expected)
        );
    }
    assert!(
        to_mapped_document_bytes(
            &schema(),
            &Instance::DocumentSet(vec![]),
            "DetailLines",
            &options()
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn late_row_errors_keep_document_path_row_field_and_existing_precedence() {
    let malformed = Instance::Group(
        vec![
            ("Client".into(), text("A")),
            ("SKU".into(), text("z")),
            ("Label".into(), text("尾")),
        ]
        .into(),
    );
    let error = to_mapped_document_bytes(
        &schema(),
        &documents(wrap(vec![row("A", "x", 12, "A\"x"), malformed])),
        "DetailLines",
        &options(),
    )
    .unwrap_err();
    assert!(
        matches!(error, CsvDocumentsError::Document { document: 1, path, source: CsvBoundedError::Format(CsvFormatError::MissingField { row: 1, field }) } if path == AP && field == "Total")
    );
    for (bad, got) in [
        (Instance::Scalar(Value::Float(3.0)), "float"),
        (Instance::Repeated(vec![]), "repeated"),
    ] {
        let malformed = Instance::Group(
            vec![
                ("Client".into(), text("A")),
                ("SKU".into(), text("z")),
                ("Total".into(), bad),
                ("Label".into(), text("尾")),
            ]
            .into(),
        );
        assert!(
            matches!(to_mapped_document_bytes(&schema(), &documents(wrap(vec![row("A", "x", 12, "A\"x"), malformed])), "DetailLines", &options()),
            Err(CsvDocumentsError::Document { document: 1, path, source: CsvBoundedError::Format(CsvFormatError::ValueType { row: 1, field, expected: ScalarType::Int, got: actual }) }) if path == AP && field == "Total" && actual == got)
        );
    }
    assert!(matches!(
        to_mapped_document_bytes(
            &schema(),
            &documents(wrap(vec![
                row("A", "x", 12, "A\"x"),
                Instance::Scalar(Value::Null)
            ])),
            "DetailLines",
            &options()
        ),
        Err(CsvDocumentsError::Document {
            document: 1,
            source: CsvBoundedError::Format(CsvFormatError::RowShape {
                row: 1,
                got: "null"
            }),
            ..
        })
    ));
    let no_quotes = CsvWriteOptions {
        quote_disabled: true,
        ..options()
    };
    assert!(
        matches!(to_mapped_document_bytes(&schema(), &value(), "DetailLines", &no_quotes), Err(CsvDocumentsError::Document { document: 0, path, source: CsvBoundedError::Format(CsvFormatError::UnquotedFieldBoundary { row: 0, field }) }) if path == BP && field == "Label")
    );
    let missing = Instance::Group(
        vec![
            ("Client".into(), text("B")),
            ("SKU".into(), text("y")),
            ("Label".into(), text("bee")),
        ]
        .into(),
    );
    let first = Instance::DocumentSet(vec![
        DocumentMember::new(BP, wrap(vec![row("B", "x", 5, "é;\"Q\"\r\nZ"), missing])).unwrap(),
    ]);
    assert!(
        matches!(to_mapped_document_bytes(&schema(), &first, "DetailLines", &no_quotes), Err(CsvDocumentsError::Document { document: 0, source: CsvBoundedError::Format(CsvFormatError::MissingField { row: 1, field }), .. }) if field == "Total")
    );
}

#[test]
fn static_schema_constraints_and_dialect_are_refused_even_without_documents() {
    let empty = Instance::DocumentSet(vec![]);
    let mut fixed = SchemaNode::scalar("Value", ScalarType::String);
    fixed.fixed = Some("OK".into());
    let mut tuple = schema();
    tuple.json_array_wrapper = true;
    for bad in [
        SchemaNode::group("Row", vec![]),
        SchemaNode::group("Row", vec![fixed]),
        tuple,
        SchemaNode::group(
            "Row",
            vec![
                SchemaNode::scalar("X", ScalarType::Int),
                SchemaNode::scalar("X", ScalarType::Int),
            ],
        ),
        SchemaNode::group(
            "Row",
            vec![SchemaNode::scalar("X", ScalarType::String).repeating()],
        ),
    ] {
        assert!(matches!(
            to_mapped_document_bytes(&bad, &empty, "DetailLines", &options()),
            Err(CsvDocumentsError::MappedRowSchema { .. })
        ));
    }
    assert!(matches!(
        to_mapped_document_bytes(
            &schema(),
            &empty,
            "DetailLines",
            &CsvWriteOptions {
                delimiter: Some('\n'),
                ..options()
            }
        ),
        Err(CsvDocumentsError::MappedConfiguration {
            source: CsvBoundedError::Format(CsvFormatError::BadDelimiter('\n'))
        })
    ));
}

#[test]
fn header_escaping_custom_quote_and_direct_scalar_coercion_are_literal() {
    let cases = [
        (
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("A;B", ScalarType::String),
                    SchemaNode::scalar("Q\"R", ScalarType::String),
                ],
            ),
            Instance::Group(vec![("A;B".into(), text("x")), ("Q\"R".into(), text("y"))].into()),
            None,
            b"\"A;B\";\"Q\"\"R\"\nx;y\n".as_slice(),
        ),
        (
            SchemaNode::group("Row", vec![SchemaNode::scalar("Label", ScalarType::String)]),
            Instance::Group(vec![("Label".into(), text("a'b;c"))].into()),
            Some('\''),
            b"Label\n'a''b;c'\n",
        ),
        (
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("A", ScalarType::String),
                    SchemaNode::scalar("N", ScalarType::Int),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                ],
            ),
            Instance::Group(
                vec![
                    ("A".into(), Instance::Scalar(Value::Null)),
                    ("N".into(), text(" +0007 ")),
                    ("Flag".into(), text("1")),
                ]
                .into(),
            ),
            None,
            b"A;N;Flag\n;7;true\n",
        ),
    ];
    for (schema, row, quote, expected) in cases {
        let input = Instance::DocumentSet(vec![DocumentMember::new("x", wrap(vec![row])).unwrap()]);
        let result = to_mapped_document_bytes(
            &schema,
            &input,
            "DetailLines",
            &CsvWriteOptions {
                delimiter: Some(';'),
                quote,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result[0].bytes, expected);
    }
}

#[test]
fn metadata_preflight_uses_utf8_bytes_and_runs_before_malformed_rows() {
    let options = CsvWriteOptions {
        has_headers: false,
        ..Default::default()
    };
    let good = DocumentMember::new("é".repeat(2048), wrap(vec![])).unwrap();
    let maximum = Instance::DocumentSet(vec![good.clone(); 4096]);
    assert_eq!(
        to_mapped_document_bytes(&schema(), &maximum, "DetailLines", &options)
            .unwrap()
            .len(),
        4096
    );
    let excessive = Instance::DocumentSet(vec![good; 4097]);
    assert!(matches!(
        to_mapped_document_bytes(&schema(), &excessive, "DetailLines", &options),
        Err(CsvDocumentsError::TooManyDocuments { maximum: 4096 })
    ));
    let bad = Instance::DocumentSet(vec![
        DocumentMember::new("a", Instance::Scalar(Value::Null)).unwrap(),
        DocumentMember::new("é".repeat(2049), wrap(vec![])).unwrap(),
    ]);
    assert!(matches!(
        to_mapped_document_bytes(&schema(), &bad, "DetailLines", &options),
        Err(CsvDocumentsError::PathTooLong {
            document: 1,
            maximum: 4096
        })
    ));
}

#[test]
fn direct_duplicate_extra_and_inexact_numbers_keep_their_cell_errors() {
    for duplicate in [false, true] {
        let Instance::Group(mut fields) = row("A", "z", -2, "尾") else {
            unreachable!()
        };
        fields.push((
            if duplicate { "Total" } else { "Spare" }.into(),
            Instance::Scalar(Value::Null),
        ));
        let error = to_mapped_document_bytes(
            &schema(),
            &documents(wrap(vec![
                row("A", "x", 12, "A\"x"),
                Instance::Group(fields),
            ])),
            "DetailLines",
            &options(),
        )
        .unwrap_err();
        let CsvDocumentsError::Document {
            document: 1,
            path,
            source: CsvBoundedError::Format(source),
        } = error
        else {
            panic!("document/format error")
        };
        assert_eq!(path, AP);
        if duplicate {
            assert!(
                matches!(source, CsvFormatError::DuplicateField { row: 1, field } if field == "Total")
            );
        } else {
            assert!(
                matches!(source, CsvFormatError::UnexpectedField { row: 1, field } if field == "Spare")
            );
        }
    }
    let schema = SchemaNode::group("Row", vec![SchemaNode::scalar("N", ScalarType::Float)]);
    let input = |value| {
        Instance::DocumentSet(vec![
            DocumentMember::new(
                "n",
                wrap(vec![Instance::Group(
                    vec![("N".into(), Instance::Scalar(value))].into(),
                )]),
            )
            .unwrap(),
        ])
    };
    let output = to_mapped_document_bytes(
        &schema,
        &input(Value::Int(9_007_199_254_740_992)),
        "DetailLines",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(output[0].bytes, b"N\n9007199254740992\n");
    assert!(
        matches!(to_mapped_document_bytes(&schema, &input(Value::Int(9_007_199_254_740_993)), "DetailLines", &Default::default()), Err(CsvDocumentsError::Document { source: CsvBoundedError::Format(CsvFormatError::ValueType { row: 0, field, expected: ScalarType::Float, got: "int outside the exact f64 range" }), .. }) if field == "N")
    );
    for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert!(matches!(
            to_mapped_document_bytes(
                &schema,
                &input(Value::Float(value)),
                "DetailLines",
                &Default::default()
            ),
            Err(CsvDocumentsError::Document {
                source: CsvBoundedError::Format(CsvFormatError::ValueType {
                    row: 0,
                    expected: ScalarType::Float,
                    ..
                }),
                ..
            })
        ));
    }
}

// Deliberately separate from routine tests: retained input and returned output
// are materialized, so this qualification uses substantial process memory.
#[test]
#[ignore = "large public byte boundaries; run separately under a finite grant"]
fn mapped_documents_real_byte_boundaries() {
    let schema = SchemaNode::group("Row", vec![SchemaNode::scalar("N", ScalarType::String)]);
    let options = CsvWriteOptions {
        has_headers: false,
        ..Default::default()
    };
    let document = |length| {
        DocumentMember::new(
            "same.csv",
            wrap(vec![Instance::Group(
                vec![("N".into(), text(&"x".repeat(length)))].into(),
            )]),
        )
        .unwrap()
    };
    let value = Instance::DocumentSet(vec![document(67_108_863)]);
    assert_eq!(
        to_mapped_document_bytes(&schema, &value, "DetailLines", &options).unwrap()[0]
            .bytes
            .len(),
        67_108_864
    );
    drop(value);
    assert!(matches!(
        to_mapped_document_bytes(
            &schema,
            &Instance::DocumentSet(vec![document(67_108_864)]),
            "DetailLines",
            &options
        ),
        Err(CsvDocumentsError::Document {
            document: 0,
            source: CsvBoundedError::OutputTooLarge {
                maximum: 67_108_864
            },
            ..
        })
    ));
    let value = Instance::DocumentSet((0..4).map(|_| document(67_108_863)).collect());
    let bytes = to_mapped_document_bytes(&schema, &value, "DetailLines", &options).unwrap();
    assert_eq!(
        bytes.iter().map(|d| d.bytes.len()).sum::<usize>(),
        268_435_456
    );
    drop(bytes);
    drop(value);
    let value = Instance::DocumentSet(
        (0..3)
            .map(|_| document(67_108_863))
            .chain([document(67_108_862), document(1)])
            .collect(),
    );
    assert!(matches!(
        to_mapped_document_bytes(&schema, &value, "DetailLines", &options),
        Err(CsvDocumentsError::Document {
            document: 4,
            source: CsvBoundedError::OutputTooLarge { maximum: 1 },
            ..
        })
    ));
}
