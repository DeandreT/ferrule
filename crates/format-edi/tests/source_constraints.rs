use format_edi::{EdiConstraintViolation, EdiFormatError};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{EdiBoundaryKind, EdiImpliedDecimal, EdiValueConstraint, FormatOptions};

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(String::from(value)))
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    let mut named = Vec::with_capacity(fields.len());
    for (name, value) in fields {
        named.push((String::from(name), value));
    }
    Instance::Group(named.into())
}

fn report(error: EdiFormatError) -> format_edi::EdiValidationReport {
    let EdiFormatError::Validation(report) = error else {
        panic!("Expected an actionable source validation report, got {error:?}");
    };
    report
}

fn segment(name: &str, count: usize) -> SchemaNode {
    let mut fields = Vec::with_capacity(count);
    for ordinal in 1..=count {
        fields.push(SchemaNode::scalar(
            format!("{name}{ordinal:02}"),
            ScalarType::String,
        ));
    }
    SchemaNode::group(name, fields)
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Interchange",
        vec![
            segment("ISA", 16),
            segment("GS", 8),
            segment("ST", 2),
            segment("G62", 3),
            SchemaNode::group(
                "Detail",
                vec![SchemaNode::group(
                    "W01",
                    vec![SchemaNode::scalar("W0101", ScalarType::Int)],
                )],
            )
            .repeating(),
            segment("SE", 2),
            segment("GE", 2),
            segment("IEA", 2),
        ],
    )
}

fn wire(time: &str, second: &str) -> String {
    format!(
        "ISA*00*          *00*          *ZZ*{:<15}*ZZ*{:<15}*261102*0700*U*00401*000000001*0*T*:~\
         GS*OW*EXAMPLE*DEST*20261102*0700*1*X*004010~\
         ST*940*0001~G62*10*20261103*{time}~W01*0004~W01*{second}~\
         SE*5*0001~GE*1*1~IEA*1*000000001~",
        "EXAMPLE", "DEST",
    )
}

fn constraint(path: &[&str], minimum: u32, maximum: u32, codes: &[&str]) -> EdiValueConstraint {
    let mut names = Vec::with_capacity(path.len());
    for part in path {
        names.push(String::from(*part));
    }
    let mut values = Vec::with_capacity(codes.len());
    for code in codes {
        values.push(String::from(*code));
    }
    EdiValueConstraint::new(names, minimum, maximum, values)
        .expect("The invented fixture constraint is valid.")
}

fn options(constraints: Vec<EdiValueConstraint>) -> FormatOptions {
    FormatOptions {
        edi_kind: Some(EdiBoundaryKind::X12),
        edi_value_constraints: constraints,
        ..FormatOptions::default()
    }
}

#[test]
fn source_lengths_are_checked_before_mapping() {
    let schema = schema();
    let options = options(vec![constraint(&["G62", "G6203"], 6, 6, &[])]);
    let valid = format_edi::document::parse(
        &schema,
        &options,
        EdiBoundaryKind::X12,
        wire("073000", "0010").as_bytes(),
    )
    .expect("A matching six-character source value must be accepted.");
    assert_eq!(
        valid
            .field("G62")
            .and_then(|group| group.field("G6203"))
            .and_then(Instance::as_scalar),
        Some(&Value::String(String::from("073000"))),
        "Source validation must not alter valid text."
    );
    let error = format_edi::document::parse(
        &schema,
        &options,
        EdiBoundaryKind::X12,
        wire("0730", "0010").as_bytes(),
    )
    .expect_err("An active six-character source constraint must refuse four characters.");
    let EdiFormatError::Validation(report) = error else {
        panic!("Expected a typed source validation report, got {error:?}");
    };
    assert_eq!(
        report.issues().len(),
        1,
        "Only the constrained time is invalid."
    );
    assert_eq!(report.issues()[0].location(), "G62/G6203");
    assert_eq!(
        report.issues()[0].violation(),
        EdiConstraintViolation::TooShort {
            minimum: 6,
            actual: 4
        }
    );
}

#[test]
fn repeated_numeric_constraints_use_wire_spelling_before_type_conversion() {
    let schema = schema();
    let options = options(vec![constraint(
        &["Detail", "W01", "W0101"],
        4,
        4,
        &["0004", "0010"],
    )]);
    let valid = format_edi::document::parse(
        &schema,
        &options,
        EdiBoundaryKind::X12,
        wire("073000", "0010").as_bytes(),
    )
    .expect("Leading zeroes satisfy the lexical policy before becoming typed integers.");
    let details = valid
        .field("Detail")
        .and_then(Instance::as_repeated)
        .expect("Both declared detail occurrences must remain present.");
    assert_eq!(details.len(), 2);
    for (detail, expected) in details.iter().zip([4, 10]) {
        assert_eq!(
            detail
                .field("W01")
                .and_then(|group| group.field("W0101"))
                .and_then(Instance::as_scalar),
            Some(&Value::Int(expected)),
            "Validation must preserve the declared numeric type."
        );
    }
    let error = format_edi::document::parse(
        &schema,
        &options,
        EdiBoundaryKind::X12,
        wire("073000", "0030").as_bytes(),
    )
    .expect_err("A present value outside the configured code list must be refused.");
    let EdiFormatError::Validation(report) = error else {
        panic!("Expected a typed code-list report, got {error:?}");
    };
    assert_eq!(report.issues().len(), 1);
    assert_eq!(report.issues()[0].location(), "Detail[2]/W01/W0101");
    assert_eq!(
        report.issues()[0].violation(),
        EdiConstraintViolation::NotAllowed { allowed_count: 2 }
    );
}

#[test]
fn lexical_constraints_precede_implied_scaling_and_preserve_null_and_boolean_types() {
    let schema = SchemaNode::group(
        "Interchange",
        vec![
            SchemaNode::group(
                "W01",
                vec![
                    SchemaNode::scalar("Amount", ScalarType::Float),
                    SchemaNode::scalar("Enabled", ScalarType::Bool),
                    SchemaNode::scalar("Optional", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    let mut options = options(vec![
        constraint(&["W01", "Amount"], 4, 4, &["0010", "0020"]),
        constraint(&["W01", "Optional"], 2, 2, &["01"]),
    ]);
    options.lenient_segments = true;
    options.edi_implied_decimals = vec![
        EdiImpliedDecimal::new(vec![String::from("W01"), String::from("Amount")], 2)
            .expect("The invented amount has two implied decimal places."),
    ];
    let wire =
        wire("073000", "0010").replace("W01*0004~W01*0010~", "W01*0010*true*~W01*0020*false~");
    let expected = group(vec![(
        "W01",
        Instance::Repeated(vec![
            group(vec![
                ("Amount", Instance::Scalar(Value::Float(0.1))),
                ("Enabled", Instance::Scalar(Value::Bool(true))),
                ("Optional", Instance::Scalar(Value::Null)),
            ]),
            group(vec![
                ("Amount", Instance::Scalar(Value::Float(0.2))),
                ("Enabled", Instance::Scalar(Value::Bool(false))),
                ("Optional", Instance::Scalar(Value::Null)),
            ]),
        ]),
    )]);
    assert_eq!(
        format_edi::document::parse(&schema, &options, EdiBoundaryKind::X12, wire.as_bytes())
            .expect("Wire lengths and codes apply before the amount becomes a scaled Float."),
        expected,
        "Every field, occurrence, type and absent value must match the independent oracle."
    );
    let too_long = wire.replace("W01*0020", "W01*00020");
    let failure = report(
        format_edi::document::parse(&schema, &options, EdiBoundaryKind::X12, too_long.as_bytes())
            .expect_err(
                "Equivalent numeric magnitude does not excuse an overlong source spelling.",
            ),
    );
    assert_eq!(
        failure.issues().len(),
        2,
        "Length and code-list failures are both actionable."
    );
    assert_eq!(failure.issues()[0].location(), "W01[2]/Amount");
    assert_eq!(
        failure.issues()[0].violation(),
        EdiConstraintViolation::TooLong {
            maximum: 4,
            actual: 5
        }
    );
    assert_eq!(
        failure.issues()[1].violation(),
        EdiConstraintViolation::NotAllowed { allowed_count: 2 }
    );

    let bad_boolean = wire.replace("*false", "*not-a-boolean");
    for constrained in [true, false] {
        let mut selected = options.clone();
        if !constrained {
            selected.edi_value_constraints.clear();
        }
        let error = format_edi::document::parse(
            &schema,
            &selected,
            EdiBoundaryKind::X12,
            bad_boolean.as_bytes(),
        )
        .expect_err("Unconstrained scalar fields must still be parsed to their declared types.");
        assert!(
            matches!(error, EdiFormatError::ElementParse {
            segment, element: 2, expected: ScalarType::Bool, value,
        } if segment == "W01" && value == "not-a-boolean"),
            "Constrained reads must preserve the native scalar diagnostic."
        );
    }
}

#[test]
fn edifact_custom_service_characters_escape_composites_and_repeated_elements() {
    let scalar = |name, ty| SchemaNode::scalar(name, ty);
    let schema = SchemaNode::group(
        "Interchange",
        vec![
            SchemaNode::group(
                "UNB",
                vec![SchemaNode::group(
                    "Syntax",
                    vec![
                        scalar("Identifier", ScalarType::String),
                        scalar("Version", ScalarType::Int),
                    ],
                )],
            ),
            SchemaNode::group(
                "BGM",
                vec![
                    SchemaNode::group(
                        "Parts",
                        vec![
                            scalar("Code", ScalarType::String),
                            scalar("Count", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            ),
        ],
    );
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Edifact),
        edi_value_constraints: vec![
            constraint(&["BGM", "Parts", "Code"], 1, 3, &["A;B", "C"]),
            constraint(&["BGM", "Parts", "Count"], 4, 4, &["0004", "0010"]),
        ],
        ..FormatOptions::default()
    };
    let wire = "UNA;|.!^$UNB|UNOC;4$BGM|A!;B;0004^C;0010$";
    let expected = group(vec![
        (
            "UNB",
            group(vec![(
                "Syntax",
                group(vec![
                    ("Identifier", text("UNOC")),
                    ("Version", Instance::Scalar(Value::Int(4))),
                ]),
            )]),
        ),
        (
            "BGM",
            group(vec![(
                "Parts",
                Instance::Repeated(vec![
                    group(vec![
                        ("Code", text("A;B")),
                        ("Count", Instance::Scalar(Value::Int(4))),
                    ]),
                    group(vec![
                        ("Code", text("C")),
                        ("Count", Instance::Scalar(Value::Int(10))),
                    ]),
                ]),
            )]),
        ),
    ]);
    assert_eq!(
        format_edi::document::parse(&schema, &options, EdiBoundaryKind::Edifact, wire.as_bytes(),)
            .expect("Validation uses decoded data, not the escaped wire character count."),
        expected
    );
    let failure = report(
        format_edi::document::parse(
            &schema,
            &options,
            EdiBoundaryKind::Edifact,
            wire.replace("0010", "0099").as_bytes(),
        )
        .expect_err("The second composite occurrence must be validated independently."),
    );
    assert_eq!(failure.issues().len(), 1);
    assert_eq!(failure.issues()[0].location(), "BGM/Parts[2]/Count");
    assert_eq!(
        failure.issues()[0].violation(),
        EdiConstraintViolation::NotAllowed { allowed_count: 2 }
    );
}

#[test]
fn hl7_decoded_escapes_repetitions_and_missing_components_keep_native_semantics() {
    let string = |name| SchemaNode::scalar(name, ScalarType::String);
    let schema = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group("MSH", vec![string("Separator"), string("Encoding")]),
            SchemaNode::group(
                "PID",
                vec![
                    SchemaNode::scalar("Id", ScalarType::Int),
                    SchemaNode::group(
                        "Name",
                        vec![string("Family"), string("Given"), string("Optional")],
                    ),
                    string("Alias").repeating(),
                ],
            ),
        ],
    );
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Hl7),
        edi_value_constraints: vec![
            constraint(&["PID", "Id"], 4, 4, &["0004"]),
            constraint(&["PID", "Name", "Family"], 4, 4, &["Fa|m"]),
            constraint(&["PID", "Name", "Given"], 6, 6, &["Gi&ven"]),
            constraint(&["PID", "Name", "Optional"], 2, 2, &["ok"]),
            constraint(&["PID", "Alias"], 1, 3, &["A~B", "C"]),
        ],
        ..FormatOptions::default()
    };
    let wire = "MSH|^~\\&\rPID|0004|Fa\\F\\m^Gi\\T\\ven|A\\R\\B~C\r";
    let expected = group(vec![
        (
            "MSH",
            group(vec![("Separator", text("|")), ("Encoding", text("^~\\&"))]),
        ),
        (
            "PID",
            group(vec![
                ("Id", Instance::Scalar(Value::Int(4))),
                (
                    "Name",
                    group(vec![
                        ("Family", text("Fa|m")),
                        ("Given", text("Gi&ven")),
                        ("Optional", Instance::Scalar(Value::Null)),
                    ]),
                ),
                ("Alias", Instance::Repeated(vec![text("A~B"), text("C")])),
            ]),
        ),
    ]);
    assert_eq!(
        format_edi::document::parse(&schema, &options, EdiBoundaryKind::Hl7, wire.as_bytes(),)
            .expect("Native HL7 escaping and optional values must survive validation."),
        expected
    );
    let failure = report(
        format_edi::document::parse(
            &schema,
            &options,
            EdiBoundaryKind::Hl7,
            wire.replace("~C", "~D").as_bytes(),
        )
        .expect_err("Every repetition must obey the retained code list."),
    );
    assert_eq!(failure.issues().len(), 1);
    assert_eq!(failure.issues()[0].location(), "PID/Alias[2]");
}

#[test]
fn tradacoms_decoded_composite_values_are_checked_before_integer_conversion() {
    let schema = SchemaNode::group(
        "Interchange",
        vec![
            SchemaNode::group(
                "STX",
                vec![SchemaNode::group(
                    "Syntax",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar_fixed("Version", ScalarType::Int, "01"),
                    ],
                )],
            ),
            SchemaNode::group(
                "REF",
                vec![SchemaNode::group(
                    "Parts",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar("Count", ScalarType::Int),
                    ],
                )],
            ),
        ],
    );
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Tradacoms),
        edi_value_constraints: vec![
            constraint(&["STX", "Syntax", "Version"], 2, 2, &["01"]),
            constraint(&["REF", "Parts", "Code"], 3, 3, &["A+B"]),
            constraint(&["REF", "Parts", "Count"], 4, 4, &["0004"]),
        ],
        ..FormatOptions::default()
    };
    let wire = "STX=ANA:01'REF=A?+B:0004'";
    let expected = group(vec![
        (
            "STX",
            group(vec![(
                "Syntax",
                group(vec![
                    ("Code", text("ANA")),
                    ("Version", Instance::Scalar(Value::Int(1))),
                ]),
            )]),
        ),
        (
            "REF",
            group(vec![(
                "Parts",
                group(vec![
                    ("Code", text("A+B")),
                    ("Count", Instance::Scalar(Value::Int(4))),
                ]),
            )]),
        ),
    ]);
    assert_eq!(
        format_edi::document::parse(
            &schema,
            &options,
            EdiBoundaryKind::Tradacoms,
            wire.as_bytes(),
        )
        .expect("Fixed qualifier spelling and release escapes must remain native."),
        expected
    );
    let failure = report(
        format_edi::document::parse(
            &schema,
            &options,
            EdiBoundaryKind::Tradacoms,
            wire.replace("0004", "0009").as_bytes(),
        )
        .expect_err("Numeric normalization must not bypass a TRADACOMS code list."),
    );
    assert_eq!(failure.issues().len(), 1);
    assert_eq!(failure.issues()[0].location(), "REF/Parts/Count");
}

#[test]
fn idoc_fixed_record_padding_and_repeated_numeric_fields_keep_native_types() {
    use mapping::{IdocFieldLayout, IdocLayout, IdocSegmentLayout};
    use std::num::NonZeroU32;
    let field = |name, first, last| {
        IdocFieldLayout::new(
            name,
            NonZeroU32::new(first).expect("Fixture field positions are one-based."),
            NonZeroU32::new(last).expect("Fixture field positions are one-based."),
        )
        .expect("Fixture field ranges are disjoint and ordered.")
    };
    let layout = IdocLayout::new(vec![
        IdocSegmentLayout::new(
            "ITEM000001",
            vec![field("Code", 12, 15), field("Count", 16, 19)],
        )
        .expect("The invented IDoc segment name is valid."),
    ])
    .expect("The fixture has one segment layout.");
    let schema = SchemaNode::group(
        "IDOC",
        vec![
            SchemaNode::group(
                "ITEM000001",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Count", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Idoc),
        idoc: Some(layout),
        edi_value_constraints: vec![
            constraint(&["ITEM000001", "Code"], 1, 1, &["A", "B"]),
            constraint(&["ITEM000001", "Count"], 4, 4, &["0004", "0010"]),
        ],
        ..FormatOptions::default()
    };
    let wire = b"EDI_DC40 ignored\rITEM000001 A   0004\rITEM000001 B   0010\r";
    let expected = group(vec![(
        "ITEM000001",
        Instance::Repeated(vec![
            group(vec![
                ("Code", text("A")),
                ("Count", Instance::Scalar(Value::Int(4))),
            ]),
            group(vec![
                ("Code", text("B")),
                ("Count", Instance::Scalar(Value::Int(10))),
            ]),
        ]),
    )]);
    assert_eq!(
        format_edi::document::parse(&schema, &options, EdiBoundaryKind::Idoc, wire,)
            .expect("IDoc validates the existing padding-trimmed native field spelling."),
        expected
    );
    let failure = report(
        format_edi::document::parse(
            &schema,
            &options,
            EdiBoundaryKind::Idoc,
            b"EDI_DC40 ignored\rITEM000001 A   0004\rITEM000001 B   0099\r",
        )
        .expect_err("The later IDoc occurrence cannot evade a retained code list."),
    );
    assert_eq!(failure.issues().len(), 1);
    assert_eq!(failure.issues()[0].location(), "ITEM000001[2]/Count");
}

#[test]
fn invalid_source_metadata_is_refused_even_when_its_optional_data_is_absent() {
    let schema = schema();
    for constraints in [
        vec![constraint(&["Absent", "Field"], 1, 1, &[])],
        vec![constraint(&["G62"], 1, 1, &[])],
        vec![constraint(&["G62", "G6203"], 1, 1, &[]); 2],
    ] {
        let error = format_edi::document::parse(
            &schema,
            &options(constraints),
            EdiBoundaryKind::X12,
            wire("073000", "0010").as_bytes(),
        )
        .expect_err("Unsupported source metadata must never be silently ignored.");
        assert!(
            matches!(error, EdiFormatError::UnsupportedSchema(_)),
            "Invalid metadata is a plan-admission error, not a successful unconstrained read."
        );
    }
}
