//! Authored wire examples; constraints apply to decoded text, not typed display.
use format_edi::{EdiConstraintViolation, EdiFormatError, document};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{EdiBoundaryKind, EdiImpliedDecimal, EdiValueConstraint, FormatOptions};

const ISA: &str = "ISA*00*          *00*          *ZZ*SOURCE         *ZZ*DEST           *261102*1200*^*00501*000000001*0*T*:~";

fn constraint(path: &[&str], min: u32, max: u32, codes: &[&str]) -> EdiValueConstraint {
    EdiValueConstraint::new(
        path.iter().map(|part| (*part).into()).collect(),
        min,
        max,
        codes.iter().map(|code| (*code).into()).collect(),
    )
    .unwrap()
}

fn options(constraints: Vec<EdiValueConstraint>) -> FormatOptions {
    FormatOptions {
        lenient_segments: true,
        edi_value_constraints: constraints,
        ..Default::default()
    }
}

fn schema(ty: ScalarType) -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::group(
                    "G62",
                    vec![SchemaNode::scalar("Value", ty)],
                )],
            )
            .repeating(),
        ],
    )
}

fn parse(ty: ScalarType, options: &FormatOptions, body: &str) -> Result<Instance, EdiFormatError> {
    document::parse(
        &schema(ty),
        options,
        EdiBoundaryKind::X12,
        format!("{ISA}{body}").as_bytes(),
    )
}

fn first(instance: &Instance) -> &Value {
    instance.field("Rows").unwrap().as_repeated().unwrap()[0]
        .field("G62")
        .unwrap()
        .field("Value")
        .unwrap()
        .as_scalar()
        .unwrap()
}

#[test]
fn integer_leading_zeros_and_code_lists_use_the_wire_spelling() {
    let opts = options(vec![constraint(
        &["Rows", "G62", "Value"],
        6,
        6,
        &["000001"],
    )]);
    assert_eq!(
        first(&parse(ScalarType::Int, &opts, "G62*000001~").unwrap()),
        &Value::Int(1)
    );
    let EdiFormatError::Validation(report) = parse(ScalarType::Int, &opts, "G62*1~").unwrap_err()
    else {
        panic!("typed validation error")
    };
    assert_eq!(report.issues().len(), 2);
    assert_eq!(report.issues()[0].location(), "Rows[1]/G62/Value");
    assert_eq!(
        report.issues()[0].violation(),
        EdiConstraintViolation::TooShort {
            minimum: 6,
            actual: 1
        }
    );
    assert_eq!(
        report.issues()[1].violation(),
        EdiConstraintViolation::NotAllowed { allowed_count: 1 }
    );
    let codes_only = options(vec![constraint(
        &["Rows", "G62", "Value"],
        1,
        6,
        &["000001"],
    )]);
    assert!(matches!(
        parse(ScalarType::Int, &codes_only, "G62*1~"),
        Err(EdiFormatError::Validation(_))
    ));
}

#[test]
fn implied_decimals_validate_before_scaling_and_unconstrained_errors_stay_typed() {
    let mut opts = options(vec![constraint(
        &["Rows", "G62", "Value"],
        6,
        6,
        &["001234"],
    )]);
    opts.edi_implied_decimals.push(
        EdiImpliedDecimal::new(vec!["Rows".into(), "G62".into(), "Value".into()], 2).unwrap(),
    );
    assert_eq!(
        first(&parse(ScalarType::Float, &opts, "G62*001234~").unwrap()),
        &Value::Float(12.34)
    );
    assert!(matches!(
        parse(ScalarType::Float, &opts, "G62*1234~"),
        Err(EdiFormatError::Validation(_))
    ));
    let unconstrained = options(vec![]);
    assert_eq!(
        first(&parse(ScalarType::Int, &unconstrained, "G62*1~").unwrap()),
        &Value::Int(1)
    );
    assert!(matches!(
        parse(ScalarType::Int, &unconstrained, "G62*abc~"),
        Err(EdiFormatError::ElementParse {
            expected: ScalarType::Int,
            ..
        })
    ));
}

#[test]
fn repeated_groups_report_every_present_violation_but_not_null_or_absent_values() {
    let opts = options(vec![constraint(&["Rows", "G62", "Value"], 2, 3, &["AA"])]);
    let EdiFormatError::Validation(report) =
        parse(ScalarType::String, &opts, "G62*X~G62*LONG~G62*~G62~").unwrap_err()
    else {
        panic!("typed validation error")
    };
    assert_eq!(
        report
            .issues()
            .iter()
            .map(|issue| issue.location())
            .collect::<Vec<_>>(),
        [
            "Rows[1]/G62/Value",
            "Rows[1]/G62/Value",
            "Rows[2]/G62/Value",
            "Rows[2]/G62/Value"
        ]
    );
    assert_eq!(
        report.issues()[2].violation(),
        EdiConstraintViolation::TooLong {
            maximum: 3,
            actual: 4
        }
    );
    assert!(!report.truncated());
    assert!(parse(ScalarType::String, &opts, "G62*~G62~").is_ok());
    assert!(parse(ScalarType::String, &opts, "").is_ok());
}

#[test]
fn reports_are_bounded_and_display_omits_excess_issues() {
    let opts = options(vec![constraint(&["Rows", "G62", "Value"], 2, 2, &[])]);
    let EdiFormatError::Validation(report) =
        parse(ScalarType::String, &opts, &"G62*X~".repeat(10_001)).unwrap_err()
    else {
        panic!("typed validation error")
    };
    assert_eq!(report.issues().len(), 10_000);
    assert_eq!(report.issues()[9_999].location(), "Rows[10000]/G62/Value");
    assert!(report.truncated());
    assert!(report.to_string().contains("additional issues omitted"));
    assert!(!report.to_string().contains("Rows[9]"));
}

#[test]
fn all_segment_families_decode_escapes_before_validation() {
    for (kind, header, body, tag) in [
        (
            EdiBoundaryKind::Edifact,
            "UNA;*.! ~UNH*1~",
            "FTX*A!*B~",
            "FTX",
        ),
        (EdiBoundaryKind::Tradacoms, "STX=ANA:1'", "FTX=A?+B'", "FTX"),
        (
            EdiBoundaryKind::Hl7,
            "MSH|^~\\&|S\r",
            "OBX|A\\F\\B\r",
            "OBX",
        ),
    ] {
        let schema = SchemaNode::group(
            "Input",
            vec![SchemaNode::group(
                tag,
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )],
        );
        let expected = if kind == EdiBoundaryKind::Edifact {
            "A*B"
        } else if kind == EdiBoundaryKind::Hl7 {
            "A|B"
        } else {
            "A+B"
        };
        let opts = options(vec![constraint(&[tag, "Value"], 3, 3, &[expected])]);
        let parsed =
            document::parse(&schema, &opts, kind, format!("{header}{body}").as_bytes()).unwrap();
        assert_eq!(
            parsed
                .field(tag)
                .unwrap()
                .field("Value")
                .unwrap()
                .as_scalar(),
            Some(&Value::String(expected.into()))
        );
        let mut invalid = opts;
        invalid.edi_value_constraints = vec![constraint(&[tag, "Value"], 4, 4, &[])];
        assert!(
            matches!(
                document::parse(
                    &schema,
                    &invalid,
                    kind,
                    format!("{header}{body}").as_bytes()
                ),
                Err(EdiFormatError::Validation(_))
            ),
            "{kind:?}"
        );
    }
}

#[test]
fn custom_x12_syntax_repeating_composites_and_scalar_joins_keep_paths() {
    let mut opts = options(vec![
        constraint(&["G62", "Parts", "Code"], 3, 3, &["A|B"]),
        constraint(&["G62", "Joined"], 3, 3, &["A>B"]),
    ]);
    opts.x12_separators = Some(mapping::X12Separators {
        element: '|',
        component: '>',
        segment: '!',
        repetition: Some('^'),
        release: Some('?'),
    });
    let header = ISA.replace('*', "|").replace(':', ">").replace('~', "!");
    let schema = SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "G62",
            vec![
                SchemaNode::group(
                    "Parts",
                    vec![SchemaNode::scalar("Code", ScalarType::String)],
                )
                .repeating(),
                SchemaNode::scalar("Joined", ScalarType::String),
            ],
        )],
    );
    assert!(
        document::parse(
            &schema,
            &opts,
            EdiBoundaryKind::X12,
            format!("{header}G62|A?|B^A?|B|A>B!").as_bytes()
        )
        .is_ok()
    );
    let EdiFormatError::Validation(report) = document::parse(
        &schema,
        &opts,
        EdiBoundaryKind::X12,
        format!("{header}G62|A?|B^X|A>B!").as_bytes(),
    )
    .unwrap_err() else {
        panic!("typed validation error")
    };
    assert_eq!(report.issues()[0].location(), "G62/Parts[2]/Code");
}

#[test]
fn hl7_subcomponents_are_decoded_after_structural_splitting() {
    let schema = SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "OBX",
            vec![
                SchemaNode::group(
                    "Parts",
                    vec![SchemaNode::group(
                        "Nested",
                        vec![
                            SchemaNode::scalar("One", ScalarType::String),
                            SchemaNode::scalar("Two", ScalarType::Int),
                        ],
                    )],
                )
                .repeating(),
            ],
        )],
    );
    let opts = options(vec![
        constraint(&["OBX", "Parts", "Nested", "One"], 3, 3, &["A&B"]),
        constraint(&["OBX", "Parts", "Nested", "Two"], 6, 6, &["000001"]),
    ]);
    let wire = b"MSH|^~\\&|S\rOBX|A\\T\\B&000001~X&1\r";
    let EdiFormatError::Validation(report) =
        document::parse(&schema, &opts, EdiBoundaryKind::Hl7, wire).unwrap_err()
    else {
        panic!("typed validation error")
    };
    assert_eq!(report.issues().len(), 4);
    assert_eq!(report.issues()[0].location(), "OBX/Parts[2]/Nested/One");
    assert_eq!(report.issues()[2].location(), "OBX/Parts[2]/Nested/Two");
}

#[test]
fn idoc_layout_padding_is_not_part_of_the_decoded_field() {
    use mapping::{IdocFieldLayout, IdocLayout, IdocSegmentLayout};
    use std::num::NonZeroU32;
    let schema = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "ITEM000001",
                vec![SchemaNode::scalar("Code", ScalarType::Int)],
            )
            .repeating(),
        ],
    );
    let mut opts = options(vec![constraint(&["ITEM000001", "Code"], 6, 6, &["000001"])]);
    opts.idoc = Some(
        IdocLayout::new(vec![
            IdocSegmentLayout::new(
                "ITEM000001",
                vec![
                    IdocFieldLayout::new(
                        "Code",
                        NonZeroU32::new(12).unwrap(),
                        NonZeroU32::new(19).unwrap(),
                    )
                    .unwrap(),
                ],
            )
            .unwrap(),
        ])
        .unwrap(),
    );
    let parsed = document::parse(
        &schema,
        &opts,
        EdiBoundaryKind::Idoc,
        b"ITEM000001 000001  \r",
    )
    .unwrap();
    assert_eq!(
        parsed.field("ITEM000001").unwrap().as_repeated().unwrap()[0]
            .field("Code")
            .unwrap()
            .as_scalar(),
        Some(&Value::Int(1))
    );
    document::parse(
        &schema,
        &opts,
        EdiBoundaryKind::Idoc,
        b"ITEM000001 \t000001\0\r",
    )
    .unwrap();
    assert!(matches!(
        document::parse(&schema, &opts, EdiBoundaryKind::Idoc, b"ITEM000001 \xff\r"),
        Err(EdiFormatError::InvalidIdocText { .. })
    ));
    let EdiFormatError::Validation(report) = document::parse(
        &schema,
        &opts,
        EdiBoundaryKind::Idoc,
        b"ITEM000001 1       \r",
    )
    .unwrap_err() else {
        panic!("typed validation error")
    };
    assert_eq!(report.issues()[0].location(), "ITEM000001[1]/Code");
}

#[test]
fn numeric_lexical_forms_and_overflow_have_wire_level_diagnostics() {
    let opts = options(vec![constraint(
        &["Rows", "G62", "Value"],
        5,
        5,
        &["01.00"],
    )]);
    assert_eq!(
        first(&parse(ScalarType::Float, &opts, "G62*01.00~").unwrap()),
        &Value::Float(1.0)
    );
    assert!(matches!(
        parse(ScalarType::Float, &opts, "G62*1~"),
        Err(EdiFormatError::Validation(_))
    ));
    let opts = options(vec![constraint(&["Rows", "G62", "Value"], 1, 6, &[])]);
    let EdiFormatError::Validation(report) = parse(
        ScalarType::Int,
        &opts,
        "G62*999999999999999999999999999999~",
    )
    .unwrap_err() else {
        panic!("constraint precedes overflow")
    };
    assert_eq!(
        report.issues()[0].violation(),
        EdiConstraintViolation::TooLong {
            maximum: 6,
            actual: 30
        }
    );
}

#[test]
fn first_observed_constraint_precedes_later_conversion_but_not_earlier_conversion() {
    let schema = SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "G62",
            vec![
                SchemaNode::scalar("One", ScalarType::Int),
                SchemaNode::scalar("Two", ScalarType::Int),
            ],
        )],
    );
    let opts = options(vec![constraint(&["G62", "One"], 2, 2, &[])]);
    let parse = |body: &str, opts: &FormatOptions| {
        document::parse(
            &schema,
            opts,
            EdiBoundaryKind::X12,
            format!("{ISA}{body}").as_bytes(),
        )
    };
    let EdiFormatError::Validation(report) = parse("G62*1*bad~", &opts).unwrap_err() else {
        panic!("first observed wire constraint wins")
    };
    assert_eq!(report.issues()[0].location(), "G62/One");
    let opts = options(vec![constraint(&["G62", "Two"], 2, 2, &[])]);
    assert!(matches!(
        parse("G62*bad*1~", &opts),
        Err(EdiFormatError::ElementParse { element: 1, .. })
    ));
}

#[test]
fn invalid_constraint_metadata_is_refused_even_with_no_selected_values() {
    for constraints in [
        vec![constraint(&["Rows", "Missing"], 1, 6, &[])],
        vec![constraint(&["Rows", "G62"], 1, 6, &[])],
        vec![constraint(&["Rows", "G62", "Value"], 1, 6, &[]); 2],
    ] {
        assert!(matches!(
            parse(ScalarType::String, &options(constraints), ""),
            Err(EdiFormatError::UnsupportedSchema(_))
        ));
    }
}

#[test]
fn missing_and_empty_composite_positions_and_repeats_keep_null_policy() {
    let schema = SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "G62",
            vec![
                SchemaNode::group(
                    "Parts",
                    vec![
                        SchemaNode::scalar("One", ScalarType::String),
                        SchemaNode::scalar("Two", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        )],
    );
    let opts = options(vec![
        constraint(&["G62", "Parts", "One"], 2, 2, &["AA"]),
        constraint(&["G62", "Parts", "Two"], 2, 2, &["BB"]),
    ]);
    for body in ["G62~", "G62*~", "G62*:^:~", "G62*AA:^:BB~"] {
        document::parse(
            &schema,
            &opts,
            EdiBoundaryKind::X12,
            format!("{ISA}{body}").as_bytes(),
        )
        .unwrap();
    }
}

#[test]
fn renamed_schema_nodes_and_active_interchange_syntax_are_not_wire_tag_paths() {
    let schema = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Interchange",
                vec![
                    SchemaNode::group("ISA", vec![]),
                    SchemaNode::group(
                        "SG1",
                        vec![SchemaNode::group(
                            "MF_NAD_2",
                            vec![SchemaNode::scalar("Joined", ScalarType::String)],
                        )],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let opts = options(vec![constraint(
        &["Interchange", "SG1", "MF_NAD_2", "Joined"],
        3,
        3,
        &["A:B", "A>B"],
    )]);
    let second = ISA.replace('*', "|").replace(':', ">").replace('~', "!");
    let wire = format!("{ISA}NAD*A:B~{second}NAD|A>B!");
    let parsed = document::parse(&schema, &opts, EdiBoundaryKind::X12, wire.as_bytes()).unwrap();
    assert_eq!(
        parsed
            .field("Interchange")
            .unwrap()
            .as_repeated()
            .unwrap()
            .len(),
        2
    );
    let bad = wire.replace("NAD|A>B!", "NAD|X!");
    let EdiFormatError::Validation(report) =
        document::parse(&schema, &opts, EdiBoundaryKind::X12, bad.as_bytes()).unwrap_err()
    else {
        panic!("typed validation error")
    };
    assert_eq!(
        report.issues()[0].location(),
        "Interchange[2]/SG1[1]/MF_NAD_2/Joined"
    );
}

#[test]
fn only_schema_consumed_tokens_are_constrained_and_character_counts_are_not_bytes() {
    let schema = SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "G62",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )],
    );
    let opts = options(vec![constraint(&["G62", "Value"], 1, 1, &["é"])]);
    document::parse(
        &schema,
        &opts,
        EdiBoundaryKind::X12,
        format!("{ISA}ZZZ*bad~G62*é^ignored*extra~").as_bytes(),
    )
    .unwrap();
}
