use super::*;
use crate::{XmlInputProfile, XmlOutputMode};
use ir::Instance;

fn project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/static_primary_multiple_dynamic_named_xml_documents.json"
    ))
    .unwrap()
}

fn source(alpha: &[(&str, f64)], beta: &[(&str, i64)]) -> Instance {
    let alpha = alpha
        .iter()
        .map(|(path, amount)| {
            Instance::Group(
                vec![
                    (
                        "File".into(),
                        Instance::Scalar(Value::String((*path).into())),
                    ),
                    ("Amount".into(), Instance::Scalar(Value::Float(*amount))),
                ]
                .into(),
            )
        })
        .collect();
    let beta = beta
        .iter()
        .map(|(path, amount)| {
            Instance::Group(
                vec![
                    (
                        "File".into(),
                        Instance::Scalar(Value::String((*path).into())),
                    ),
                    ("Amount".into(), Instance::Scalar(Value::Int(*amount))),
                ]
                .into(),
            )
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

fn assert_target(target: &engine::NamedOutput, name: &str, rows: &[(&str, Value)]) {
    assert_eq!(target.name, name);
    let Instance::DocumentSet(members) = &target.instance else {
        panic!("expected independently mapped named document set")
    };
    assert_eq!(members.len(), rows.len());
    for (member, (path, amount)) in members.iter().zip(rows) {
        assert_eq!(member.path(), *path);
        assert_eq!(member.source_path(), *path);
        let mut fields = vec![("Amount".into(), Instance::Scalar(amount.clone()))];
        if name == "z-audit" {
            fields.push((
                "Label".into(),
                Instance::Scalar(Value::String("audit".into())),
            ));
        }
        assert_eq!(member.value(), &Instance::Group(fields.into()));
        if let Value::Float(expected) = amount {
            let Some(Value::Float(actual)) =
                member.value().field("Amount").and_then(Instance::as_scalar)
            else {
                panic!("the first target must retain its Float tag")
            };
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
    }
}

#[test]
fn plural_mixed_mode_proves_each_target_schema_driver_and_literal_policy() {
    let project = project();
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(XmlOutputMode::StaticPrimaryDynamicNamedDocuments))
    );
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(policy.input.profile(), Some(XmlInputProfile::Structured));
    assert!(policy.extra_inputs.is_empty());
    assert!(program.extra_sources.is_empty());
    assert_eq!(
        program
            .extra_targets
            .iter()
            .map(|target| target.name.as_str())
            .collect::<Vec<_>>(),
        ["z-audit", "a-receipt"]
    );
    for (index, (target, output)) in program
        .extra_targets
        .iter()
        .zip(&policy.extra_outputs)
        .enumerate()
    {
        assert_eq!(output.name, target.name);
        assert_eq!(target.target, project.extra_targets[index].schema);
        assert_eq!(
            output.output.schema_hints,
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
            if index == 0 { ["Alpha"] } else { ["Beta"] }
        );
    }
    assert_ne!(
        policy.extra_outputs[0].output.schema_hints,
        policy.extra_outputs[1].output.schema_hints
    );
    assert_ne!(
        program.extra_targets[0].target.xml_namespace,
        program.extra_targets[1].target.xml_namespace
    );
    assert!(matches!(
        program.extra_targets[0]
            .target
            .child("Amount")
            .unwrap()
            .kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
    assert!(matches!(
        program.extra_targets[1]
            .target
            .child("Amount")
            .unwrap()
            .kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
}

#[test]
fn declaration_order_and_independent_empty_sets_preserve_complete_typed_results() {
    let mut project = project();
    let samples = [
        (
            vec![
                ("same.xml", 9_007_199_254_740_992.0),
                ("same.xml", -0.0),
                ("résult/雪😀.xml", 2.5),
            ],
            vec![
                ("same.xml", 9_007_199_254_740_993),
                ("/opaque.xml", 7),
                ("../opaque.xml", 8),
            ],
        ),
        (Vec::new(), vec![("beta.xml", 11)]),
        (vec![("alpha.xml", 1.5)], Vec::new()),
        (Vec::new(), Vec::new()),
    ];
    for reverse in [false, true] {
        if reverse {
            project.extra_targets.swap(0, 1);
        }
        let program = lower(&project).unwrap();
        assert_eq!(
            program
                .xml_boundary
                .as_ref()
                .unwrap()
                .extra_outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            project
                .extra_targets
                .iter()
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>()
        );
        for (alpha, beta) in &samples {
            let mapped = engine::run_outputs(&project, &source(alpha, beta)).unwrap();
            assert_eq!(
                mapped.primary,
                Instance::Group(
                    vec![(
                        "Marker".into(),
                        Instance::Scalar(Value::String("ok".into()))
                    )]
                    .into()
                )
            );
            assert_eq!(mapped.extras.len(), 2);
            let alpha_rows: Vec<_> = alpha
                .iter()
                .map(|(path, value)| (*path, Value::Float(*value)))
                .collect();
            let beta_rows: Vec<_> = beta
                .iter()
                .map(|(path, value)| (*path, Value::Int(*value)))
                .collect();
            let (alpha_index, beta_index) = if reverse { (1, 0) } else { (0, 1) };
            assert_target(&mapped.extras[alpha_index], "z-audit", &alpha_rows);
            assert_target(&mapped.extras[beta_index], "a-receipt", &beta_rows);
        }
    }
}

#[test]
fn second_target_driver_must_independently_end_in_a_repeating_primary_group() {
    let valid = lower(&project()).unwrap();
    for mutation in 0..3 {
        let mut invalid = valid.clone();
        match mutation {
            0 => {
                invalid.extra_targets[1].root.iteration =
                    Some(IterationPlan::dynamic_documents(Vec::new(), 3))
            }
            1 => {
                let SchemaKind::Group { children, .. } = &mut invalid.source.kind else {
                    unreachable!()
                };
                children[1].repeating = false;
            }
            _ => {
                let SchemaKind::Group { children, .. } = &mut invalid.source.kind else {
                    unreachable!()
                };
                children[1].kind = SchemaKind::Scalar {
                    ty: ScalarType::Int,
                };
            }
        }
        assert!(matches!(invalid.xml_output_mode(),
            Err(ProgramValidationError::InvalidXmlBoundary { reason })
                if reason.contains("a-receipt") && reason.contains("repeating Group")));
    }
}

#[test]
fn unsupported_second_driver_keeps_valid_core_targets_and_omits_the_whole_adapter() {
    let mut project = project();
    let SchemaKind::Group { children, .. } = &mut project.source.kind else {
        unreachable!()
    };
    children[1] = SchemaNode::scalar("Beta", ScalarType::Int).repeating();
    project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::String("beta.xml".into()),
        },
    );
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::Int(17),
        },
    );
    assert!(engine::validate(&project).is_empty());
    let program = lower(&project).unwrap();
    assert_eq!(program.xml_output_mode(), Ok(None));
    assert_eq!(program.extra_targets.len(), 2);
    for target in &program.extra_targets {
        assert!(
            target
                .root
                .iteration
                .as_ref()
                .unwrap()
                .dynamic_document_iteration()
                .is_some()
        );
    }
    assert!(matches!(
        program.source.child("Beta").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
}

#[test]
fn a_static_named_root_among_dynamic_roots_refuses_only_the_optional_xml_adapter() {
    let mut project = project();
    project.extra_targets[1].root.iteration = ScopeIteration::None;
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::Int(23),
        },
    );
    let program = lower(&project).unwrap();
    assert_eq!(program.xml_output_mode(), Ok(None));
    assert_eq!(program.extra_targets.len(), 2);
    assert!(
        program.extra_targets[0]
            .root
            .iteration
            .as_ref()
            .unwrap()
            .dynamic_document_iteration()
            .is_some()
    );
    assert!(program.extra_targets[1].root.iteration.is_none());
    let mapped = engine::run_outputs(&project, &source(&[], &[])).unwrap();
    assert!(matches!(
        &mapped.extras[0].instance,
        Instance::DocumentSet(_)
    ));
    assert_eq!(
        mapped.extras[1]
            .instance
            .field("Amount")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(23))
    );

    let mut explicit = lower(&self::project()).unwrap();
    explicit.extra_targets[1].root.iteration = None;
    assert!(matches!(explicit.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { reason })
            if reason.contains("a-receipt") && reason.contains("dynamic-document root")));
}

#[test]
fn every_target_schema_and_policy_is_checked_without_reordering_or_partial_admission() {
    let valid = lower(&project()).unwrap();
    for mutation in 0..4 {
        let mut invalid = valid.clone();
        let policies = &mut invalid.xml_boundary.as_mut().unwrap().extra_outputs;
        match mutation {
            0 => policies.swap(0, 1),
            1 => {
                policies.pop();
            }
            2 => policies[1].name = policies[0].name.clone(),
            _ => policies.push(policies[0].clone()),
        }
        assert!(matches!(invalid.xml_output_mode(),
            Err(ProgramValidationError::InvalidXmlBoundary { reason })
                if reason.contains("exact declaration order")));
    }
    let mut invalid_member = valid.clone();
    invalid_member.extra_targets[1].target.repeating = true;
    assert!(matches!(invalid_member.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { reason })
            if reason.contains("a-receipt") && reason.contains("nonrepeating group")));
    let mut invalid_project = project();
    invalid_project.extra_targets[1].schema.name = "bad root".into();
    let core = lower(&invalid_project).unwrap();
    assert_eq!(core.xml_output_mode(), Ok(None));
    assert_eq!(core.extra_targets.len(), 2);
    assert_eq!(core.extra_targets[1].target.name, "bad root");
    assert!(
        invalid_project.extra_targets[0]
            .options
            .xml_schema_hints
            .is_some()
    );

    let mut non_xml = project();
    non_xml.extra_targets[1].options.xml_document = false;
    non_xml.extra_targets[1].options.xml_schema_hints = None;
    assert!(non_xml.extra_targets[0].options.xml_schema_hints.is_some());
    assert!(non_xml.target_options.xml_document);
    assert_eq!(lower(&non_xml).unwrap().xml_output_mode(), Ok(None));
}

#[test]
fn later_named_mapping_failure_precedes_bad_xml_in_primary_and_an_earlier_named_target() {
    let mut project = project();
    for node in [2, 5] {
        project.graph.nodes.insert(
            node,
            Node::Const {
                value: Value::String("\u{1}".into()),
            },
        );
    }
    assert_eq!(
        lower(&project).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::StaticPrimaryDynamicNamedDocuments))
    );
    let alpha = [("first.xml", 1.5)];
    let complete = engine::run_outputs(&project, &source(&alpha, &[("beta.xml", 7)])).unwrap();
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
            .field("Label")
            .and_then(Instance::as_scalar),
        Some(&Value::String("\u{1}".into()))
    );
    assert!(matches!(
        engine::run_outputs(&project, &source(&alpha, &[("beta.xml", 7), (" \t", 8)])),
        Err(engine::EngineError::EmptyDynamicTargetPath { node: 3 })
    ));
    // Generated XML APIs must finish all mappings before any serializer is called.
}

#[test]
fn existing_single_named_primary_list_static_and_observed_contracts_stay_separate() {
    let one: Project = serde_json::from_str(include_str!(
        "fixtures/static_primary_dynamic_named_xml_documents.json"
    ))
    .unwrap();
    let primary: Project =
        serde_json::from_str(include_str!("fixtures/dynamic_primary_xml_documents.json")).unwrap();
    let static_outputs: Project =
        serde_json::from_str(include_str!("fixtures/static_named_xml_inputs.json")).unwrap();
    assert_eq!(
        lower(&one).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::StaticPrimaryDynamicNamedDocuments))
    );
    assert_eq!(
        lower(&primary).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::DynamicPrimaryDocuments))
    );
    assert_eq!(
        lower(&static_outputs).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::SingleDocument))
    );
    let mut observed = project();
    observed.source_options.xml_root_view_read_policy = true;
    assert!(lower(&observed).is_err());
    let mut named_input = project();
    named_input.extra_sources.push(mapping::NamedSource {
        name: "unused".into(),
        path: "unused.xml".into(),
        schema: SchemaNode::group("Unused", vec![scalar("Value")]),
        options: named_input.source_options.clone(),
        dynamic_path: None,
    });
    assert_eq!(lower(&named_input).unwrap().xml_output_mode(), Ok(None));
}
