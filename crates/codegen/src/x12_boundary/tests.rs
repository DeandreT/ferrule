use super::*;
use crate::{Binding, Expression, ExpressionNode, TargetScope};
use ir::{ItemCountRange, StringLengthRange, Value};

type SchemaMutation = Box<dyn Fn(&mut SchemaNode)>;

fn retained_capture(
    label: &str,
    options: &FormatOptions,
) -> Result<X12BoundaryOptions, X12BoundaryPolicyError> {
    let root = std::env::temp_dir().join(format!(
        "ferrule-x12-capture-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("OPTIONS.original.txt"), format!("{options:#?}")).unwrap();
    let result = X12BoundaryOptions::from_format_options(options);
    std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
    eprintln!("retained X12 capture evidence: {}", root.display());
    result
}

fn retained_profile(
    label: &str,
    schema: &SchemaNode,
    options: &X12BoundaryOptions,
    side: X12BoundarySide,
) -> Result<String, X12BoundaryPolicyError> {
    let root = std::env::temp_dir().join(format!(
        "ferrule-x12-profile-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("SCHEMA.original.txt"), format!("{schema:#?}")).unwrap();
    std::fs::write(root.join("OPTIONS.original.txt"), format!("{options:#?}")).unwrap();
    let result = descriptor(schema, Some(options), side);
    std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
    if let Ok(encoded) = &result {
        std::fs::write(root.join("descriptor.json"), encoded).unwrap();
    }
    eprintln!("retained X12 profile evidence: {}", root.display());
    result
}

fn detail_leaf(schema: &mut SchemaNode, ty: ScalarType) {
    schema
        .child_mut("Detail")
        .unwrap()
        .child_mut("W01")
        .unwrap()
        .child_mut("W0101")
        .unwrap()
        .kind = SchemaKind::Scalar { ty };
}

#[test]
fn saved_options_are_retained_with_native_directionality_and_inactive_repetition() {
    let mut candidate = schema();
    detail_leaf(&mut candidate, ScalarType::Float);
    let path = vec!["Detail".into(), "W01".into(), "W0101".into()];
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::X12),
        x12_interchange_version: Some("00401".into()),
        lenient_segments: true,
        edi_implied_decimals: vec![EdiImpliedDecimal::new(path.clone(), 2).unwrap()],
        edi_lexical_formats: vec![
            EdiLexicalFormat::new(path, EdiLexicalKind::Decimal { max_chars: 8 }).unwrap(),
        ],
        edi_autocomplete: Some(EdiAutocomplete::X12(X12Autocomplete {
            request_acknowledgement: true,
            transaction_set: Some("940".into()),
        })),
        x12_separators: Some(X12Separators {
            element: '*',
            component: ':',
            segment: '~',
            repetition: Some('^'),
            release: None,
        }),
        ..Default::default()
    };
    let captured = retained_capture("saved-profile", &options).unwrap();
    for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
        let encoded = retained_profile("supported", &candidate, &captured, side).unwrap();
        let wrapper: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(wrapper["lenient_segments"], true);
        assert_eq!(
            wrapper["implied_decimals"],
            serde_json::json!([{ "path":["Detail","W01","W0101"], "places":2 }])
        );
        assert_eq!(
            wrapper["lexical_formats"],
            serde_json::json!([{ "path":["Detail","W01","W0101"], "kind":{"decimal":{"max_chars":8}} }])
        );
        assert_eq!(
            wrapper["autocomplete"],
            serde_json::json!({"request_acknowledgement":true,"transaction_set":"940"})
        );
        assert_eq!(wrapper["separators"]["repetition"], "^");
    }
}

#[test]
fn implied_decimal_input_is_float_only_and_every_option_path_is_checked() {
    let path = vec!["Detail".into(), "W01".into(), "W0101".into()];
    let implied = EdiImpliedDecimal::new(path.clone(), 18).unwrap();
    for ty in [ScalarType::String, ScalarType::Int, ScalarType::Float] {
        let mut candidate = schema();
        detail_leaf(&mut candidate, ty);
        for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
            let options = X12BoundaryOptions {
                implied_decimals: vec![implied.clone()],
                ..Default::default()
            };
            let result = retained_profile("implied-type", &candidate, &options, side);
            let accepted =
                ty == ScalarType::Float || side == X12BoundarySide::Target && ty == ScalarType::Int;
            assert_eq!(result.is_ok(), accepted, "{ty:?}/{side:?}: {result:?}");
        }
    }
    let mut candidate = schema();
    detail_leaf(&mut candidate, ScalarType::Float);
    for path in [vec!["Missing".into()], vec!["Detail".into()]] {
        for implied in [true, false] {
            let options = if implied {
                X12BoundaryOptions {
                    implied_decimals: vec![EdiImpliedDecimal::new(path.clone(), 2).unwrap()],
                    ..Default::default()
                }
            } else {
                X12BoundaryOptions {
                    lexical_formats: vec![
                        EdiLexicalFormat::new(path.clone(), EdiLexicalKind::CompactDate8).unwrap(),
                    ],
                    ..Default::default()
                }
            };
            let result =
                retained_profile("bad-path", &candidate, &options, X12BoundarySide::Source);
            assert!(
                matches!(result, Err(X12BoundaryPolicyError::Constraint { .. })),
                "{result:?}"
            );
        }
    }
    for options in [
        X12BoundaryOptions {
            implied_decimals: vec![implied.clone(), implied],
            ..Default::default()
        },
        X12BoundaryOptions {
            lexical_formats: vec![
                EdiLexicalFormat::new(
                    path.clone(),
                    EdiLexicalKind::Decimal { max_chars: 1 }
                )
                .unwrap();
                2
            ],
            ..Default::default()
        },
    ] {
        let result = retained_profile(
            "duplicate-path",
            &candidate,
            &options,
            X12BoundarySide::Source,
        );
        assert!(
            matches!(result, Err(X12BoundaryPolicyError::Constraint { .. })),
            "{result:?}"
        );
    }
}

#[test]
fn lexical_target_types_and_completion_transaction_identity_fail_closed() {
    let path = vec!["Detail".into(), "W01".into(), "W0101".into()];
    for ty in [ScalarType::String, ScalarType::Int, ScalarType::Float] {
        let mut candidate = schema();
        detail_leaf(&mut candidate, ty);
        for kind in [
            EdiLexicalKind::CompactDate6,
            EdiLexicalKind::CompactDate8,
            EdiLexicalKind::CompactTime {
                min_digits: 4,
                max_digits: 8,
            },
            EdiLexicalKind::Decimal { max_chars: 255 },
        ] {
            let options = X12BoundaryOptions {
                lexical_formats: vec![EdiLexicalFormat::new(path.clone(), kind).unwrap()],
                ..Default::default()
            };
            for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
                let result = retained_profile("lexical-type", &candidate, &options, side);
                assert_eq!(
                    result.is_ok(),
                    side == X12BoundarySide::Source
                        || ty == ScalarType::String
                        || matches!(kind, EdiLexicalKind::Decimal { .. }),
                    "{ty:?}/{kind:?}/{side:?}: {result:?}"
                );
            }
        }
    }
    let mut candidate = schema();
    candidate
        .child_mut("ST")
        .unwrap()
        .child_mut("ST01")
        .unwrap()
        .fixed = Some("940".into());
    for transaction in [None, Some("940"), Some("945"), Some("94"), Some("９４０")] {
        for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
            let options = X12BoundaryOptions {
                autocomplete: Some(X12Autocomplete {
                    request_acknowledgement: false,
                    transaction_set: transaction.map(str::to_owned),
                }),
                ..Default::default()
            };
            let result = retained_profile("transaction-set", &candidate, &options, side);
            let accepted = transaction.is_none_or(|text| {
                text == "940" || side == X12BoundarySide::Source && text == "945"
            });
            assert_eq!(
                result.is_ok(),
                accepted,
                "{transaction:?}/{side:?}: {result:?}"
            );
        }
    }
}

#[test]
fn profile_collection_limits_and_other_autocomplete_dialects_remain_refused() {
    let mut candidate = schema();
    detail_leaf(&mut candidate, ScalarType::Float);
    let path = vec!["Detail".into(), "W01".into(), "W0101".into()];
    let options = X12BoundaryOptions {
        implied_decimals: vec![
            EdiImpliedDecimal::new(path.clone(), 2).unwrap();
            MAX_X12_SCHEMA_NODES + 1
        ],
        ..Default::default()
    };
    let result = retained_profile(
        "option-bound",
        &candidate,
        &options,
        X12BoundarySide::Source,
    );
    assert!(
        matches!(
            result,
            Err(X12BoundaryPolicyError::Constraint {
                reason: "profile option collection limit exceeded",
                ..
            })
        ),
        "{result:?}"
    );
    let retained = FormatOptions {
        edi_implied_decimals: options.implied_decimals,
        ..Default::default()
    };
    let captured = retained_capture("collection-bound", &retained);
    assert!(matches!(
        captured,
        Err(X12BoundaryPolicyError::FormatOptions)
    ));
    for dialect in [
        EdiAutocomplete::Hl7,
        EdiAutocomplete::Tradacoms,
        EdiAutocomplete::Idoc,
        EdiAutocomplete::SwiftMt,
    ] {
        let retained = FormatOptions {
            edi_autocomplete: Some(dialect),
            ..Default::default()
        };
        let captured = retained_capture("other-dialect", &retained);
        assert!(matches!(
            captured,
            Err(X12BoundaryPolicyError::FormatOptions)
        ));
    }
}

trait ChildMut {
    fn child_mut(&mut self, name: &str) -> Option<&mut SchemaNode>;
}
impl ChildMut for SchemaNode {
    fn child_mut(&mut self, name: &str) -> Option<&mut SchemaNode> {
        let SchemaKind::Group { children, .. } = &mut self.kind else {
            return None;
        };
        children.iter_mut().find(|child| child.name == name)
    }
}

fn segment(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| SchemaNode::scalar(format!("{name}{index:02}"), ScalarType::String))
            .collect(),
    )
}

fn schema() -> SchemaNode {
    let mut isa = segment("ISA", 16);
    isa.child_mut("ISA12").unwrap().fixed = Some("00401".into());
    let mut gs = segment("GS", 8);
    gs.child_mut("GS08").unwrap().fixed = Some("004010".into());
    SchemaNode::group(
        "Interchange",
        vec![
            isa,
            gs,
            segment("ST", 2),
            SchemaNode::group("Detail", vec![segment("LX", 1), segment("W01", 7)]).repeating(),
            segment("SE", 2),
            segment("GE", 2),
            segment("IEA", 2),
        ],
    )
}

fn program() -> Program {
    Program {
        xml_boundary: None,
        source: schema(),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Order", vec![SchemaNode::scalar("id", ScalarType::String)]),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::Const {
                value: Value::String("synthetic".into()),
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: vec![Binding {
                target_field: "id".into(),
                expression: 1,
                target_domain: ScalarType::String.into(),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

fn source_policy() -> X12BoundaryPolicy {
    X12BoundaryPolicy {
        source: Some(X12BoundaryOptions::default()),
        target: None,
    }
}

#[test]
fn constructed_ir_is_bounded_before_recursive_validation_or_metadata_copies() {
    let discard_iteratively = |schema: SchemaNode| {
        let mut pending = vec![schema];
        while let Some(node) = pending.pop() {
            if let SchemaKind::Group { children, .. } = node.kind {
                pending.extend(children);
            }
        }
    };
    for source in [true, false] {
        let mut deep = SchemaNode::scalar("value", ScalarType::String);
        for _ in 0..10_000 {
            deep = SchemaNode::group("Nested", vec![deep]);
        }
        let mut candidate = program();
        let selected = if source {
            &mut candidate.source
        } else {
            &mut candidate.target
        };
        *selected = deep;
        let policy = X12BoundaryPolicy {
            source: source.then(X12BoundaryOptions::default),
            target: (!source).then(X12BoundaryOptions::default),
        };
        let result = prepare_x12_boundary(&candidate, &policy);
        assert!(
            matches!(
                result,
                Err(X12BoundaryPolicyError::Schema {
                    reason: "schema depth or node limit exceeded",
                    ..
                })
            ),
            "{result:?}"
        );
        let selected = if source {
            &mut candidate.source
        } else {
            &mut candidate.target
        };
        let deep = std::mem::replace(selected, SchemaNode::group("Consumed", vec![]));
        discard_iteratively(deep);
    }
    let mut candidate = program();
    candidate.source = SchemaNode::group(
        "Wide",
        (0..MAX_X12_SCHEMA_NODES)
            .map(|index| SchemaNode::scalar(format!("value_{index}"), ScalarType::String))
            .collect(),
    );
    assert!(matches!(
        prepare_x12_boundary(&candidate, &source_policy()),
        Err(X12BoundaryPolicyError::Schema {
            reason: "schema depth or node limit exceeded",
            ..
        })
    ));
}

#[test]
fn fixed_numeric_text_is_admitted_only_when_empty_or_representable() {
    for (ty, text, accepted) in [
        (ScalarType::Int, "", true),
        (ScalarType::Int, "002", true),
        (ScalarType::Int, "-9223372036854775808", true),
        (ScalarType::Int, "abc", false),
        (ScalarType::Int, "9223372036854775808", false),
        (ScalarType::Int, "-9223372036854775809", false),
        (ScalarType::Int, " 2", false),
        (ScalarType::Float, "", true),
        (ScalarType::Float, "2.00", true),
        (ScalarType::Float, "-2.00e2", true),
        (ScalarType::Float, "NaN", false),
        (ScalarType::Float, "Infinity", false),
        (ScalarType::Float, "-Infinity", false),
        (ScalarType::Float, "1e999", false),
        (ScalarType::Float, "2.00 ", false),
    ] {
        let mut candidate = program();
        let element = candidate
            .source
            .child_mut("Detail")
            .unwrap()
            .child_mut("W01")
            .unwrap()
            .child_mut("W0101")
            .unwrap();
        element.kind = SchemaKind::Scalar { ty };
        element.fixed = Some(text.into());
        let result = prepare_x12_boundary(&candidate, &source_policy());
        if accepted {
            assert!(result.is_ok(), "{ty:?} {text:?}: {result:?}");
        } else {
            assert!(
                matches!(result, Err(X12BoundaryPolicyError::Schema { .. })),
                "{ty:?} {text:?}: {result:?}"
            );
        }
    }
}

#[test]
fn descriptor_preserves_supported_metadata_and_optional_selection_does_not_mutate_program() {
    let mut program = program();
    program.source.child_mut("Detail").unwrap().item_count_range = ItemCountRange::new(0, Some(50));
    program
        .source
        .child_mut("Detail")
        .unwrap()
        .child_mut("W01")
        .unwrap()
        .child_mut("W0105")
        .unwrap()
        .string_length_range = StringLengthRange::new(1, Some(30));
    let before = program.clone();
    let constraint = EdiValueConstraint::new(
        vec!["Detail".into(), "W01".into(), "W0102".into()],
        2,
        2,
        vec!["EA".into()],
    )
    .unwrap();
    let policy = X12BoundaryPolicy {
        source: Some(X12BoundaryOptions {
            separators: None,
            constraints: vec![constraint],
            ..Default::default()
        }),
        target: None,
    };
    let first = prepare_x12_boundary(&program, &policy).unwrap();
    let second = prepare_x12_boundary(&program, &policy).unwrap();
    assert_eq!(first, second);
    assert_eq!(program, before);
    assert!(first.source_x12);
    assert!(!first.target_x12);
    let wrapper: serde_json::Value = serde_json::from_str(&first.source_descriptor).unwrap();
    assert_eq!(wrapper["version"], "004010");
    assert_eq!(wrapper["separators"], serde_json::Value::Null);
    assert_eq!(
        wrapper["constraints"],
        serde_json::json!([{
            "path": ["Detail", "W01", "W0102"], "min_chars": 2, "max_chars": 2, "allowed_values": ["EA"]
        }])
    );
    let restored = codegen_schema::decode(wrapper["schema"].as_str().unwrap(), 1_048_576).unwrap();
    assert_eq!(restored, program.source);
    assert_eq!(
        first.target_descriptor,
        codegen_schema::encode(&program.target, 1_048_576).unwrap()
    );
}

#[test]
fn policy_is_explicit_and_json_other_side_refuses_physical_options() {
    assert!(matches!(
        prepare_x12_boundary(&program(), &X12BoundaryPolicy::default()),
        Err(X12BoundaryPolicyError::NoX12Side)
    ));
    for json_document in [false, true] {
        assert!(
            validate_x12_json_format_options(&FormatOptions {
                json_document,
                ..Default::default()
            })
            .is_ok()
        );
    }
    assert!(
        validate_x12_json_format_options(&FormatOptions {
            json5: true,
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        validate_x12_json_format_options(&FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            ..Default::default()
        })
        .is_err()
    );
}

#[test]
fn supported_format_options_preserve_constraints_and_lf_syntax() {
    let separators = X12Separators {
        element: '*',
        component: '}',
        segment: '\n',
        release: None,
        repetition: None,
    };
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::X12),
        x12_interchange_version: Some("00401".into()),
        x12_separators: Some(separators),
        ..Default::default()
    };
    let captured = X12BoundaryOptions::from_format_options(&options).unwrap();
    assert_eq!(captured.separators, Some(separators));
    let profile = prepare_x12_boundary(
        &program(),
        &X12BoundaryPolicy {
            source: Some(captured),
            target: None,
        },
    )
    .unwrap();
    let wrapper: serde_json::Value = serde_json::from_str(&profile.source_descriptor).unwrap();
    assert_eq!(
        wrapper["separators"],
        serde_json::json!({"element":"*","component":"}","segment":"\n"})
    );
}

#[test]
fn unsupported_retained_format_fields_fail_before_emission() {
    let cases = [
        FormatOptions {
            edi_kind: Some(EdiBoundaryKind::Edifact),
            ..Default::default()
        },
        FormatOptions {
            edi_config_reference: Some(mapping::EdiConfigDependency::missing_configuration()),
            ..Default::default()
        },
        FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            edi_config_reference: Some(mapping::EdiConfigDependency::external_reference(
                "unresolved/example.config",
            )),
            ..Default::default()
        },
        FormatOptions {
            x12_interchange_version: Some("00502".into()),
            ..Default::default()
        },
        FormatOptions {
            json_document: true,
            ..Default::default()
        },
        FormatOptions {
            delimiter: Some(','),
            ..Default::default()
        },
        FormatOptions {
            xml_document: true,
            ..Default::default()
        },
    ];
    for options in cases {
        assert!(
            matches!(
                X12BoundaryOptions::from_format_options(&options),
                Err(X12BoundaryPolicyError::FormatOptions)
            ),
            "{options:?}"
        );
    }
    for separators in [
        X12Separators {
            element: '*',
            component: '*',
            segment: '~',
            release: None,
            repetition: None,
        },
        X12Separators {
            element: '*',
            component: ':',
            segment: '\r',
            release: None,
            repetition: None,
        },
        X12Separators {
            element: 'é',
            component: ':',
            segment: '~',
            release: None,
            repetition: None,
        },
        X12Separators {
            element: '*',
            component: ':',
            segment: '~',
            release: Some('?'),
            repetition: None,
        },
        X12Separators {
            element: '*',
            component: ':',
            segment: '~',
            release: None,
            repetition: Some('*'),
        },
    ] {
        assert!(matches!(
            X12BoundaryOptions::from_format_options(&FormatOptions {
                x12_separators: Some(separators),
                ..Default::default()
            }),
            Err(X12BoundaryPolicyError::Separators { .. })
        ));
    }
}

#[test]
fn rejects_unsupported_schema_shapes_and_metadata() {
    let cases: Vec<(&str, SchemaMutation)> = vec![
        ("repeating root", Box::new(|schema| schema.repeating = true)),
        (
            "wrong ISA version",
            Box::new(|schema| {
                schema
                    .child_mut("ISA")
                    .unwrap()
                    .child_mut("ISA12")
                    .unwrap()
                    .fixed = Some("00501".into())
            }),
        ),
        (
            "wrong GS version",
            Box::new(|schema| {
                schema
                    .child_mut("GS")
                    .unwrap()
                    .child_mut("GS08")
                    .unwrap()
                    .fixed = Some("005010".into())
            }),
        ),
        (
            "repeating envelope",
            Box::new(|schema| schema.child_mut("ST").unwrap().repeating = true),
        ),
        (
            "repeating element",
            Box::new(|schema| {
                schema
                    .child_mut("ISA")
                    .unwrap()
                    .child_mut("ISA01")
                    .unwrap()
                    .repeating = true
            }),
        ),
        (
            "Boolean element",
            Box::new(|schema| {
                schema
                    .child_mut("ISA")
                    .unwrap()
                    .child_mut("ISA01")
                    .unwrap()
                    .kind = SchemaKind::Scalar {
                    ty: ScalarType::Bool,
                }
            }),
        ),
        (
            "nullable element",
            Box::new(|schema| {
                schema
                    .child_mut("ISA")
                    .unwrap()
                    .child_mut("ISA01")
                    .unwrap()
                    .nullable = true
            }),
        ),
        (
            "XML element",
            Box::new(|schema| {
                schema
                    .child_mut("ISA")
                    .unwrap()
                    .child_mut("ISA01")
                    .unwrap()
                    .attribute = true
            }),
        ),
        (
            "duplicate name",
            Box::new(|schema| {
                let SchemaKind::Group { children, .. } = &mut schema.kind else {
                    unreachable!()
                };
                children.push(children[0].clone());
            }),
        ),
    ];
    for (label, mutate) in cases {
        let mut program = program();
        mutate(&mut program.source);
        assert!(
            prepare_x12_boundary(&program, &source_policy()).is_err(),
            "{label}"
        );
    }
}

#[test]
fn envelope_below_a_repeating_container_and_wrong_order_are_refused() {
    let mut candidate = program();
    let SchemaKind::Group { children, .. } = &mut candidate.source.kind else {
        unreachable!()
    };
    let st = children.remove(2);
    children.insert(2, SchemaNode::group("Transactions", vec![st]).repeating());
    assert!(matches!(
        prepare_x12_boundary(&candidate, &source_policy()),
        Err(X12BoundaryPolicyError::Schema {
            reason: "envelope segments cannot repeat or occur in repeating loops",
            ..
        })
    ));
    let mut candidate = program();
    let SchemaKind::Group { children, .. } = &mut candidate.source.kind else {
        unreachable!()
    };
    children.swap(1, 2);
    assert!(matches!(
        prepare_x12_boundary(&candidate, &source_policy()),
        Err(X12BoundaryPolicyError::Schema {
            reason: "envelope schemas must occur in ISA/GS/ST/SE/GE/IEA order",
            ..
        })
    ));
}

#[test]
fn admits_refitted_segment_names_and_one_composite_level() {
    let mut candidate = program();
    let detail = candidate.source.child_mut("Detail").unwrap();
    detail.child_mut("LX").unwrap().name = "MF_LX_1".into();
    let w01 = detail.child_mut("W01").unwrap();
    let SchemaKind::Group { children, .. } = &mut w01.kind else {
        unreachable!()
    };
    children.push(SchemaNode::group(
        "Product",
        vec![
            SchemaNode::scalar("Qualifier", ScalarType::String),
            SchemaNode::scalar("Identifier", ScalarType::String),
        ],
    ));
    assert!(prepare_x12_boundary(&candidate, &source_policy()).is_ok());
}

#[test]
fn constraint_paths_are_validated_and_not_silently_ignored() {
    for path in [vec!["Missing".into()], vec!["Detail".into()]] {
        let options = X12BoundaryOptions {
            separators: None,
            constraints: vec![EdiValueConstraint::new(path, 1, 2, vec![]).unwrap()],
            ..Default::default()
        };
        assert!(matches!(
            prepare_x12_boundary(
                &program(),
                &X12BoundaryPolicy {
                    source: Some(options),
                    target: None
                }
            ),
            Err(X12BoundaryPolicyError::Constraint { .. })
        ));
    }
    let constraint =
        EdiValueConstraint::new(vec!["ISA".into(), "ISA01".into()], 2, 2, vec![]).unwrap();
    let options = X12BoundaryOptions {
        separators: None,
        constraints: vec![constraint.clone(), constraint],
        ..Default::default()
    };
    assert!(matches!(
        prepare_x12_boundary(
            &program(),
            &X12BoundaryPolicy {
                source: Some(options),
                target: None
            }
        ),
        Err(X12BoundaryPolicyError::Constraint {
            reason: "duplicate constraint path",
            ..
        })
    ));
}

#[test]
fn schema_limits_refuse_deep_or_oversized_positional_profiles() {
    let mut candidate = program();
    let mut nested = segment("NTE", 2);
    for index in 0..=MAX_X12_SCHEMA_DEPTH {
        nested = SchemaNode::group(format!("Nested{index}"), vec![nested]);
    }
    let SchemaKind::Group { children, .. } = &mut candidate.source.kind else {
        unreachable!()
    };
    children.insert(3, nested);
    assert!(prepare_x12_boundary(&candidate, &source_policy()).is_err());

    let mut candidate = program();
    let SchemaKind::Group { children, .. } = &mut candidate.source.kind else {
        unreachable!()
    };
    children.insert(3, segment("NTE", 1_025));
    assert!(matches!(
        prepare_x12_boundary(&candidate, &source_policy()),
        Err(X12BoundaryPolicyError::Schema {
            reason: "a segment permits at most 1024 elements",
            ..
        })
    ));
}

#[test]
fn resolved_embedded_metadata_does_not_need_a_configuration_file() {
    // Successful import clears the typed unresolved dependency. The adapter
    // uses the complete embedded schema and constraints without filesystem I/O.
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::X12),
        edi_config_reference: None,
        edi_value_constraints: vec![
            EdiValueConstraint::new(vec!["ISA".into(), "ISA01".into()], 2, 2, vec!["00".into()])
                .unwrap(),
        ],
        ..Default::default()
    };
    let captured = X12BoundaryOptions::from_format_options(&options).unwrap();
    assert!(
        prepare_x12_boundary(
            &program(),
            &X12BoundaryPolicy {
                source: Some(captured),
                target: None
            }
        )
        .is_ok()
    );
}

#[test]
fn autocomplete_identity_uses_the_transaction_segment_not_body_element_names() {
    for name in ["ST", "MF_ST_1"] {
        for composite in [false, true] {
            let mut candidate = schema();
            candidate
                .child_mut("ST")
                .unwrap()
                .child_mut("ST01")
                .unwrap()
                .fixed = Some("940".into());
            let body = candidate
                .child_mut("Detail")
                .unwrap()
                .child_mut("W01")
                .unwrap();
            let SchemaKind::Group { children, .. } = &mut body.kind else {
                unreachable!()
            };
            let mut field = SchemaNode::scalar(name, ScalarType::String);
            if composite {
                field =
                    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)]);
                field.child_mut("Value").unwrap().fixed = Some("945".into());
            }
            children[0] = field;
            for transaction in [None, Some("940"), Some("945")] {
                for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
                    let options = X12BoundaryOptions {
                        autocomplete: Some(X12Autocomplete {
                            request_acknowledgement: false,
                            transaction_set: transaction.map(str::to_owned),
                        }),
                        ..Default::default()
                    };
                    let result = retained_profile(
                        &format!("body-name-{name}-{composite}-{transaction:?}-{side:?}"),
                        &candidate,
                        &options,
                        side,
                    );
                    if side == X12BoundarySide::Target && transaction == Some("945") {
                        let Err(X12BoundaryPolicyError::Constraint { side, path, reason }) = result
                        else {
                            panic!("expected transaction identity refusal: {result:#?}");
                        };
                        assert_eq!(side, X12BoundarySide::Target);
                        assert_eq!(path, ["autocomplete", "transaction_set"]);
                        assert_eq!(
                            reason,
                            "autocomplete transaction_set conflicts with fixed ST01"
                        );
                    } else {
                        let wrapper: serde_json::Value =
                            serde_json::from_str(&result.unwrap()).unwrap();
                        let completion = if let Some(transaction) = transaction {
                            serde_json::json!({"request_acknowledgement": false, "transaction_set": transaction})
                        } else {
                            serde_json::json!({"request_acknowledgement": false})
                        };
                        assert_eq!(wrapper["autocomplete"], completion);
                        let decoded = codegen_schema::decode(
                            wrapper["schema"].as_str().unwrap(),
                            MAX_EMBEDDED_X12_DESCRIPTOR_BYTES,
                        )
                        .unwrap();
                        assert_eq!(decoded, candidate);
                    }
                }
            }
        }
    }
}
