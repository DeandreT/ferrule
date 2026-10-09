use super::*;
use crate::{Binding, Expression, ExpressionNode, TargetScope};
use ir::{ItemCountRange, StringLengthRange, Value};

type SchemaMutation = Box<dyn Fn(&mut SchemaNode)>;

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
            lenient_segments: true,
            ..Default::default()
        },
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
            x12_interchange_version: Some("00501".into()),
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
            repetition: Some('^'),
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
