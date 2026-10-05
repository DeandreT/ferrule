use super::*;
use crate::{DynamicSourceProgram, XmlInputProfile, XmlOutputMode};
use ir::Instance;

fn project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/static_named_inputs_static_primary_dynamic_named_xml_documents.json"
    ))
    .unwrap()
}

fn group(fields: Vec<(&str, Value)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), Instance::Scalar(value)))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn source(alpha: &[(&str, f64, Value, bool)], beta: &[(&str, i64, Value, bool)]) -> Instance {
    let alpha = alpha
        .iter()
        .map(|(path, amount, value, keep)| {
            group(vec![
                ("File", Value::String((*path).into())),
                ("Amount", Value::Float(*amount)),
                ("Value", value.clone()),
                ("Keep", Value::Bool(*keep)),
            ])
        })
        .collect();
    let beta = beta
        .iter()
        .map(|(path, amount, value, keep)| {
            group(vec![
                ("File", Value::String((*path).into())),
                ("Amount", Value::Int(*amount)),
                ("Value", value.clone()),
                ("Keep", Value::Bool(*keep)),
            ])
        })
        .collect();
    Instance::Group(
        vec![
            ("Alpha".into(), Instance::Repeated(alpha)),
            ("Beta".into(), Instance::Repeated(beta)),
        ]
        .into(),
    )
}

fn inputs(rate: f64) -> Vec<(String, Instance)> {
    // Complete supplied set in reverse declaration order, including unused input.
    vec![
        (
            "unused-padding".into(),
            group(vec![("Value", Value::String("unused".into()))]),
        ),
        (
            "a-count".into(),
            group(vec![("Count", Value::Int(9_007_199_254_740_993))]),
        ),
        (
            "z-rates".into(),
            group(vec![
                ("Rate", Value::Float(rate)),
                ("Prefix", Value::String("  summary  ".into())),
            ]),
        ),
    ]
}

fn mapped(
    project: &Project,
    source: &Instance,
    rate: f64,
) -> Result<engine::ExecutionOutputs, engine::EngineError> {
    let context = engine::ExecutionContext::new(std::path::Path::new("mixed.json"));
    engine::run_outputs_with_sources_and_context(project, source, inputs(rate), &context)
}

fn assert_float(value: Option<&Value>, expected: f64) {
    let Some(Value::Float(actual)) = value else {
        panic!("expected retained Float tag")
    };
    assert_eq!(actual.to_bits(), expected.to_bits());
}

fn assert_member(member: &ir::DocumentMember, amount: &Value, rate: f64, value: &Value) {
    assert_eq!(member.source_path(), member.path());
    assert_eq!(
        member.value().field("Amount").and_then(Instance::as_scalar),
        Some(amount)
    );
    if let Value::Float(amount) = amount {
        assert_float(
            member.value().field("Amount").and_then(Instance::as_scalar),
            *amount,
        );
    }
    assert_float(
        member.value().field("Rate").and_then(Instance::as_scalar),
        rate,
    );
    assert_eq!(
        member.value().field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(9_007_199_254_740_993))
    );
    assert_eq!(
        member.value().field("Value").and_then(Instance::as_scalar),
        Some(value)
    );
}

#[test]
fn complete_input_and_plural_output_policies_keep_independent_original_declarations() {
    let project = project();
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments
        ))
    );
    let boundary = program.xml_boundary.as_ref().unwrap();
    assert_eq!(boundary.input.profile(), Some(XmlInputProfile::Structured));
    assert_eq!(
        boundary
            .extra_inputs
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["z-rates", "a-count", "unused-padding"]
    );
    assert_eq!(
        boundary
            .extra_outputs
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["z-alpha", "a-beta"]
    );
    assert_eq!(
        boundary.output.schema_hints,
        project.target_options.xml_schema_hints
    );
    for (index, (source, policy)) in program
        .extra_sources
        .iter()
        .zip(&boundary.extra_inputs)
        .enumerate()
    {
        assert_eq!(source.source, project.extra_sources[index].schema);
        assert_eq!(source.name, policy.name);
        assert!(source.dynamic.is_none());
        assert_eq!(policy.input.profile(), Some(XmlInputProfile::Structured));
    }
    for (index, (target, policy)) in program
        .extra_targets
        .iter()
        .zip(&boundary.extra_outputs)
        .enumerate()
    {
        assert_eq!(target.target, project.extra_targets[index].schema);
        assert_eq!(target.name, policy.name);
        assert_eq!(
            policy.output.schema_hints,
            project.extra_targets[index].options.xml_schema_hints
        );
        assert_eq!(
            target
                .root
                .iteration
                .as_ref()
                .unwrap()
                .dynamic_document_iteration()
                .unwrap()
                .source()
                .path()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            if index == 0 {
                vec!["Alpha"]
            } else {
                vec!["Beta"]
            }
        );
    }
    assert_ne!(
        program.extra_targets[0].target.xml_namespace,
        program.extra_targets[1].target.xml_namespace
    );
    assert_ne!(
        boundary.extra_outputs[0].output.schema_hints,
        boundary.extra_outputs[1].output.schema_hints
    );
    let mut one = project;
    one.extra_targets.pop();
    assert_eq!(
        lower(&one).unwrap().xml_output_mode(),
        Ok(Some(
            XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments
        ))
    );
}

#[test]
fn complete_typed_native_outputs_preserve_tags_bits_nil_spaces_opaque_paths_and_both_orders() {
    let mut project = project();
    let alpha = [
        (
            "same.xml",
            9_007_199_254_740_992.0,
            Value::String("  alpha  ".into()),
            true,
        ),
        ("same.xml", -0.0, Value::xml_nil(), true),
        ("résult/雪😀.xml", 2.5, Value::String(String::new()), true),
    ];
    let beta = [
        (
            "same.xml",
            9_007_199_254_740_993,
            Value::String("雪😀".into()),
            true,
        ),
        ("/opaque.xml", 7, Value::String(String::new()), true),
        ("../last.xml", 8, Value::String("  beta  ".into()), true),
    ];
    for reversed in [false, true] {
        if reversed {
            project.extra_targets.swap(0, 1);
        }
        assert_eq!(
            lower(&project).unwrap().xml_output_mode(),
            Ok(Some(
                XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments
            ))
        );
        for rate in [9_007_199_254_740_992.0, -0.0] {
            let outputs = mapped(&project, &source(&alpha, &beta), rate).unwrap();
            assert_eq!(
                outputs
                    .primary
                    .field("Marker")
                    .and_then(Instance::as_scalar),
                Some(&Value::String("  summary  ".into()))
            );
            assert_float(
                outputs.primary.field("Rate").and_then(Instance::as_scalar),
                rate,
            );
            assert_eq!(
                outputs.primary.field("Count").and_then(Instance::as_scalar),
                Some(&Value::Int(9_007_199_254_740_993))
            );
            assert_eq!(outputs.extras.len(), 2);
            for (declaration, output) in outputs.extras.iter().enumerate() {
                assert_eq!(output.name, project.extra_targets[declaration].name);
                let Instance::DocumentSet(members) = &output.instance else {
                    panic!("expected independent named document set")
                };
                if output.name == "z-alpha" {
                    assert_eq!(members.len(), alpha.len());
                    for (member, (path, amount, value, _)) in members.iter().zip(&alpha) {
                        assert_eq!(member.path(), *path);
                        assert_member(member, &Value::Float(*amount), rate, value);
                    }
                } else {
                    assert_eq!(members.len(), beta.len());
                    for (member, (path, amount, value, _)) in members.iter().zip(&beta) {
                        assert_eq!(member.path(), *path);
                        assert_member(member, &Value::Int(*amount), rate, value);
                    }
                }
            }
        }
    }
}

#[test]
fn independent_empty_filtered_sets_retain_primary_envelopes_and_all_input_policies() {
    let project = project();
    let alpha = [("alpha.xml", 1.5, Value::Null, true)];
    let beta = [("beta.xml", 7, Value::Null, true)];
    for (a, b) in [
        (&alpha[..], &[][..]),
        (&[][..], &beta[..]),
        (&[][..], &[][..]),
    ] {
        let outputs = mapped(&project, &source(a, b), -0.0).unwrap();
        assert!(matches!(outputs.primary, Instance::Group(_)));
        assert_eq!(outputs.extras.len(), 2);
        for (output, expected) in outputs.extras.iter().zip([a.len(), b.len()]) {
            let Instance::DocumentSet(members) = &output.instance else {
                panic!("expected empty-capable envelope")
            };
            assert_eq!(members.len(), expected);
        }
    }
    let outputs = mapped(
        &project,
        &source(
            &[("alpha.xml", 1.0, Value::Null, false)],
            &[("beta.xml", 7, Value::Null, false)],
        ),
        1.0,
    )
    .unwrap();
    assert!(
        outputs
            .extras
            .iter()
            .all(|o| matches!(&o.instance, Instance::DocumentSet(m) if m.is_empty()))
    );
    let mut explicit = lower(&project).unwrap();
    explicit.xml_boundary.as_mut().unwrap().extra_inputs.pop();
    assert!(matches!(
        explicit.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
    // Generated APIs must still admit/parse the unused complete input; this native control does not assert their names/parser phases.
}

#[test]
fn every_input_and_output_policy_is_validated_without_partial_admission() {
    let valid = lower(&project()).unwrap();
    for index in 0..3 {
        for mutation in 0..3 {
            let mut bad = valid.clone();
            let policy = &mut bad.xml_boundary.as_mut().unwrap().extra_inputs[index];
            match mutation {
                0 => policy.name = "wrong".into(),
                1 => policy.input.root_view_policy = true,
                _ => policy.input.allow_inactive_root_type_members = true,
            }
            assert!(matches!(
                bad.xml_output_mode(),
                Err(ProgramValidationError::InvalidXmlBoundary { .. })
            ));
        }
    }
    for inputs in [true, false] {
        let mut bad = valid.clone();
        let boundary = bad.xml_boundary.as_mut().unwrap();
        if inputs {
            boundary.extra_inputs.swap(0, 1);
        } else {
            boundary.extra_outputs.swap(0, 1);
        }
        assert!(matches!(
            bad.xml_output_mode(),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        ));
    }
    let mut dynamic = valid;
    dynamic.extra_sources[2].dynamic = Some(DynamicSourceProgram {
        path: 0,
        driver: SourceIteration::new(vec!["Alpha".into()]),
    });
    assert_eq!(
        dynamic.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments
        ))
    );
    dynamic.extra_sources[0].dynamic = dynamic.extra_sources[2].dynamic.clone();
    assert_eq!(
        dynamic.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments
        ))
    );
}

#[test]
fn every_named_driver_remains_primary_repeating_group_and_static_named_roots_refuse() {
    let valid = lower(&project()).unwrap();
    let mut empty = valid.clone();
    empty.extra_targets[1].root.iteration = Some(IterationPlan::dynamic_documents(Vec::new(), 3));
    assert!(
        matches!(empty.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason })
        if reason.contains("a-beta") && reason.contains("repeating Group"))
    );
    let mut named = valid.clone();
    let row = named.source.child("Beta").unwrap().clone();
    let SchemaKind::Group { children, .. } = &mut named.extra_sources[2].source.kind else {
        unreachable!()
    };
    children.push(row);
    named.extra_targets[1].root.iteration = Some(IterationPlan::dynamic_documents(
        vec!["unused-padding".into(), "Beta".into()],
        3,
    ));
    for node in &mut named.expressions {
        if matches!(node.id, 3 | 4 | 5 | 10) {
            let Expression::SourceField { frame, .. } = &mut node.expression else {
                unreachable!()
            };
            *frame = Some(vec!["unused-padding".into(), "Beta".into()]);
        }
    }
    assert!(
        matches!(named.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason })
        if reason.contains("a-beta") && reason.contains("repeating Group"))
    );
    let mut static_root = valid;
    static_root.extra_targets[1].root.iteration = None;
    for node in &mut static_root.expressions {
        if matches!(node.id, 4 | 5) {
            node.expression = Expression::Const {
                value: if node.id == 4 {
                    Value::Int(7)
                } else {
                    Value::String("static".into())
                },
            };
        }
    }
    assert!(
        matches!(static_root.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason })
        if reason.contains("a-beta") && reason.contains("dynamic-document root"))
    );
}

#[test]
fn unsupported_ordinary_inputs_and_outputs_keep_core_and_observed_flags_remain_strict() {
    for declaration in 0..3 {
        let mut non_xml = project();
        non_xml.extra_sources[declaration].options.xml_document = false;
        let core = lower(&non_xml).unwrap();
        assert_eq!(core.xml_output_mode(), Ok(None));
        assert_eq!(core.extra_sources.len(), 3);
        assert_eq!(core.extra_targets.len(), 2);
        for flags in [(true, true), (true, false), (false, true)] {
            let mut observed = project();
            observed.extra_sources[declaration]
                .options
                .xml_allow_inactive_root_type_members = flags.0;
            observed.extra_sources[declaration]
                .options
                .xml_root_view_read_policy = flags.1;
            assert!(lower(&observed).is_err());
        }
    }
    let mut unsupported = project();
    let ir::SchemaKind::Group {
        children: fields, ..
    } = &mut unsupported.extra_sources[2].schema.kind
    else {
        panic!("expected unused-padding group");
    };
    assert_eq!(fields[0].name, "Value");
    fields[0].default = Some("unused".into());
    assert!(fields[0].metadata_is_valid());
    assert!(unsupported.extra_sources[2].schema.metadata_is_valid());
    assert_eq!(lower(&unsupported).unwrap().xml_output_mode(), Ok(None));
    let mut observed = project();
    observed.source_options.xml_allow_inactive_root_type_members = true;
    observed.source_options.xml_root_view_read_policy = true;
    assert!(lower(&observed).is_err());
    let mut stored_path = project();
    stored_path.extra_targets[1].path = Some("forbidden.xml".into());
    assert!(lower(&stored_path).is_err());
    let mut non_xml_output = project();
    non_xml_output.extra_targets[1].options.xml_document = false;
    non_xml_output.extra_targets[1].options.xml_schema_hints = None;
    assert_eq!(lower(&non_xml_output).unwrap().xml_output_mode(), Ok(None));
}

#[test]
fn input_declaration_count_includes_primary_and_does_not_share_output_member_ledger() {
    let mut project = project();
    let extra = project.extra_sources[2].clone();
    while project.extra_sources.len() < 4095 {
        let index = project.extra_sources.len();
        project.extra_sources.push(mapping::NamedSource {
            name: format!("unused-{index}"),
            ..extra.clone()
        });
    }
    assert_eq!(
        lower(&project).unwrap().xml_output_mode(),
        Ok(Some(
            XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments
        ))
    );
    project.extra_sources.push(mapping::NamedSource {
        name: "too-many".into(),
        ..extra
    });
    let core = lower(&project).unwrap();
    assert_eq!(core.xml_output_mode(), Ok(None));
    assert_eq!(core.extra_sources.len(), 4096);
    assert_eq!(core.extra_targets.len(), 2);
    // Neutral declaration admission only; no generated 4096-input success or real byte budget is executed here.
}

#[test]
fn later_second_target_mapping_failure_precedes_bad_primary_and_first_named_serialization() {
    let mut project = project();
    for id in [8, 2] {
        project.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::String("\u{1}".into()),
            },
        );
    }
    assert_eq!(
        lower(&project).unwrap().xml_output_mode(),
        Ok(Some(
            XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments
        ))
    );
    let alpha = [("alpha.xml", 1.5, Value::Null, true)];
    let complete = mapped(
        &project,
        &source(&alpha, &[("beta.xml", 7, Value::Null, true)]),
        1.0,
    )
    .unwrap();
    assert_eq!(
        complete
            .primary
            .field("Marker")
            .and_then(Instance::as_scalar),
        Some(&Value::String("\u{1}".into()))
    );
    let Instance::DocumentSet(members) = &complete.extras[0].instance else {
        unreachable!()
    };
    assert_eq!(
        members[0]
            .value()
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("\u{1}".into()))
    );
    assert!(matches!(
        mapped(
            &project,
            &source(
                &alpha,
                &[
                    ("beta.xml", 7, Value::Null, true),
                    (" \t", 8, Value::Null, true)
                ]
            ),
            1.0
        ),
        Err(engine::EngineError::EmptyDynamicTargetPath { node: 3 })
    ));
    // Public serialization order remains a matching runtime/emitter/actual-host obligation.
}
