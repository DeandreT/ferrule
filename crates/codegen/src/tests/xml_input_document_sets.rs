use super::*;
use crate::{DynamicSourceProgram, XmlInputProfile, XmlOutputMode};
use ir::Instance;

fn project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/static_named_inputs_dynamic_primary_xml_documents.json"
    ))
    .unwrap()
}

fn source(rows: &[(&str, Value)]) -> Instance {
    Instance::Group(
        vec![(
            "Row".into(),
            Instance::Repeated(
                rows.iter()
                    .map(|(path, value)| {
                        Instance::Group(
                            vec![
                                (
                                    "File".into(),
                                    Instance::Scalar(Value::String((*path).into())),
                                ),
                                ("Value".into(), Instance::Scalar(value.clone())),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn inputs(rate: f64, keep: bool) -> Vec<(String, Instance)> {
    // Transport order deliberately differs from declaration order.
    vec![
        (
            "beta".into(),
            Instance::Group(
                vec![
                    (
                        "Count".into(),
                        Instance::Scalar(Value::Int(9_007_199_254_740_993)),
                    ),
                    ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
                ]
                .into(),
            ),
        ),
        (
            "alpha".into(),
            Instance::Group(
                vec![
                    ("Rate".into(), Instance::Scalar(Value::Float(rate))),
                    (
                        "Prefix".into(),
                        Instance::Scalar(Value::String(String::new())),
                    ),
                ]
                .into(),
            ),
        ),
    ]
}

#[test]
fn every_static_named_schema_and_policy_is_retained_in_original_order() {
    let project = project();
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::StaticNamedInputsDynamicPrimaryDocuments
        ))
    );
    let boundary = program.xml_boundary.as_ref().unwrap();
    assert_eq!(boundary.input.profile(), Some(XmlInputProfile::Structured));
    assert!(boundary.extra_outputs.is_empty());
    assert_eq!(
        boundary
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
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
    assert_eq!(
        program.extra_sources[0].source.child("Rate").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    );
    assert_eq!(
        program.extra_sources[1].source.child("Count").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    );
    assert_eq!(
        boundary.output.schema_hints,
        project.target_options.xml_schema_hints
    );
}

#[test]
fn typed_mapping_uses_both_named_sources_without_losing_precision_or_opaque_paths() {
    let project = project();
    let paths = ["same.xml", "same.xml", "/opaque.xml", "../résult/雪😀.xml"];
    let values = [
        Value::String("  space  ".into()),
        Value::String(String::new()),
        Value::xml_nil(),
        Value::String("雪😀".into()),
    ];
    let rows = paths.into_iter().zip(values.clone()).collect::<Vec<_>>();
    for rate in [9_007_199_254_740_992.0, -0.0] {
        let mapped =
            engine::run_with_sources(&project, &source(&rows), inputs(rate, true)).unwrap();
        let Instance::DocumentSet(members) = mapped else {
            panic!("expected documents");
        };
        assert_eq!(
            members
                .iter()
                .map(|member| member.path())
                .collect::<Vec<_>>(),
            paths
        );
        for (member, value) in members.iter().zip(&values) {
            assert_eq!(member.source_path(), member.path());
            assert_eq!(
                member.value().field("Value").and_then(Instance::as_scalar),
                Some(value)
            );
            assert_eq!(
                member.value().field("Count").and_then(Instance::as_scalar),
                Some(&Value::Int(9_007_199_254_740_993))
            );
            let Some(Value::Float(actual)) =
                member.value().field("Rate").and_then(Instance::as_scalar)
            else {
                panic!("expected Float");
            };
            assert_eq!(actual.to_bits(), rate.to_bits());
        }
    }
}

#[test]
fn named_filter_can_produce_zero_documents_while_all_input_policies_remain_required() {
    let project = project();
    let rows = [("one.xml", Value::String("A".into()))];
    assert_eq!(
        engine::run_with_sources(&project, &source(&rows), inputs(1.0, false)).unwrap(),
        Instance::DocumentSet(Vec::new())
    );
    assert_eq!(
        engine::run_with_sources(&project, &source(&[]), inputs(1.0, true)).unwrap(),
        Instance::DocumentSet(Vec::new())
    );
    let mut program = lower(&project).unwrap();
    program.xml_boundary.as_mut().unwrap().extra_inputs.pop();
    assert!(matches!(
        program.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
}

#[test]
fn invalid_policy_at_either_declaration_never_admits_a_partial_input_set() {
    let valid = lower(&project()).unwrap();
    for declaration in 0..2 {
        for mutation in 0..3 {
            let mut invalid = valid.clone();
            let policy = &mut invalid.xml_boundary.as_mut().unwrap().extra_inputs[declaration];
            match mutation {
                0 => policy.name = "wrong".into(),
                1 => policy.input.root_view_policy = true,
                _ => policy.input.allow_inactive_root_type_members = true,
            }
            assert!(matches!(
                invalid.xml_output_mode(),
                Err(ProgramValidationError::InvalidXmlBoundary { .. })
            ));
        }
    }
    let mut invalid = valid.clone();
    invalid
        .xml_boundary
        .as_mut()
        .unwrap()
        .extra_inputs
        .swap(0, 1);
    assert!(matches!(
        invalid.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
}

#[test]
fn unsupported_ordinary_named_input_keeps_core_but_observed_flags_stay_strict() {
    let mut observed_primary = project();
    observed_primary
        .source_options
        .xml_allow_inactive_root_type_members = true;
    observed_primary.source_options.xml_root_view_read_policy = true;
    assert!(lower(&observed_primary).is_err());
    for declaration in 0..2 {
        for mutation in 0..2 {
            let mut project = project();
            if mutation == 0 {
                project.extra_sources[declaration].options.xml_document = false;
            } else {
                let SchemaKind::Group { children, .. } =
                    &mut project.extra_sources[declaration].schema.kind
                else {
                    unreachable!()
                };
                children[0].default = Some("unused".into());
            }
            let program = lower(&project).unwrap();
            assert_eq!(program.xml_output_mode(), Ok(None));
            assert_eq!(program.extra_sources.len(), 2);
            assert!(
                program
                    .root
                    .iteration
                    .as_ref()
                    .unwrap()
                    .dynamic_document_iteration()
                    .is_some()
            );
        }
        for (allow, root) in [(true, true), (true, false), (false, true)] {
            let mut project = project();
            project.extra_sources[declaration]
                .options
                .xml_allow_inactive_root_type_members = allow;
            project.extra_sources[declaration]
                .options
                .xml_root_view_read_policy = root;
            assert!(lower(&project).is_err());
        }
    }
}

#[test]
fn dynamic_named_sources_and_named_drivers_remain_outside_this_adapter() {
    let mut named_output = project();
    named_output.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: None,
        schema: SchemaNode::group("Audit", vec![scalar("Label")]),
        options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        root: Scope::default(),
    });
    let lowered = lower(&named_output).unwrap();
    assert_eq!(lowered.extra_sources.len(), 2);
    assert_eq!(lowered.extra_targets.len(), 1);
    assert_eq!(lowered.xml_output_mode(), Ok(None));
    let valid = lower(&project()).unwrap();
    let mut dynamic = valid.clone();
    let mut unused = dynamic.extra_sources[0].clone();
    unused.name = "unused_dynamic".into();
    unused.dynamic = Some(DynamicSourceProgram {
        path: 0,
        driver: SourceIteration::new(vec!["Row".into()]),
    });
    dynamic.extra_sources.push(unused);
    dynamic
        .xml_boundary
        .as_mut()
        .unwrap()
        .extra_inputs
        .push(crate::NamedXmlInputPolicy {
            name: "unused_dynamic".into(),
            input: crate::XmlInputPolicy {
                allow_inactive_root_type_members: false,
                root_view_policy: false,
            },
        });
    assert!(
        matches!(dynamic.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("only static named inputs"))
    );
    let mut named_driver = valid;
    let row = named_driver.source.child("Row").unwrap().clone();
    let SchemaKind::Group { children, .. } = &mut named_driver.extra_sources[0].source.kind else {
        unreachable!()
    };
    children.push(row);
    named_driver.root.iteration = Some(IterationPlan::dynamic_documents(
        vec!["alpha".into(), "Row".into()],
        5,
    ));
    for node in &mut named_driver.expressions {
        if matches!(node.id, 0 | 1) {
            let Expression::SourceField { frame, .. } = &mut node.expression else {
                unreachable!()
            };
            *frame = Some(vec!["alpha".into(), "Row".into()]);
        }
    }
    assert!(
        matches!(named_driver.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("repeating Group"))
    );
}

#[test]
fn input_artifact_count_includes_primary_without_limiting_output_member_count() {
    let mut project = project();
    let alpha = project.extra_sources[0].clone();
    while project.extra_sources.len() < 4095 {
        let index = project.extra_sources.len();
        project.extra_sources.push(mapping::NamedSource {
            name: format!("unused{index}"),
            ..alpha.clone()
        });
    }
    assert_eq!(
        lower(&project).unwrap().xml_output_mode(),
        Ok(Some(
            XmlOutputMode::StaticNamedInputsDynamicPrimaryDocuments
        ))
    );
    project.extra_sources.push(mapping::NamedSource {
        name: "too-many".into(),
        ..alpha
    });
    let program = lower(&project).unwrap();
    assert_eq!(program.extra_sources.len(), 4096);
    assert_eq!(program.xml_output_mode(), Ok(None));
}

#[test]
fn complete_mapping_path_failure_precedes_an_earlier_member_xml_refusal() {
    let mut project = project();
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    let mapped = engine::run_with_sources(
        &project,
        &source(&[("first.xml", Value::Null)]),
        inputs(1.0, true),
    )
    .unwrap();
    let Instance::DocumentSet(members) = mapped else {
        panic!("expected documents");
    };
    assert_eq!(
        members[0]
            .value()
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("\u{1}".into()))
    );
    assert!(matches!(
        engine::run_with_sources(
            &project,
            &source(&[("first.xml", Value::Null), (" ", Value::Null)]),
            inputs(1.0, true)
        ),
        Err(engine::EngineError::EmptyDynamicTargetPath { node: 5 })
    ));
    // Public XML API precedence still requires matching emitter/runtime and actual host qualification.
}
