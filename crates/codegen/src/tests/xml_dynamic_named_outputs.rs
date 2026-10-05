use super::*;
use crate::{XmlInputProfile, XmlOutputMode};
use ir::Instance;

fn project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/static_primary_dynamic_named_xml_documents.json"
    ))
    .unwrap()
}

fn source(rows: &[(&str, &str)]) -> Instance {
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
                                (
                                    "Value".into(),
                                    Instance::Scalar(Value::String((*value).into())),
                                ),
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

#[test]
fn static_primary_and_one_dynamic_named_target_preserve_complete_typed_outputs() {
    let project = project();
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(XmlOutputMode::StaticPrimaryDynamicNamedDocuments))
    );
    assert_eq!(
        program.xml_boundary.as_ref().unwrap().input.profile(),
        Some(XmlInputProfile::Structured)
    );
    for rows in [
        vec![
            ("same.xml", "A"),
            ("same.xml", "B"),
            ("/opaque.xml", "C"),
            ("../opaque.xml", "D"),
        ],
        Vec::new(),
    ] {
        let mapped = engine::run_outputs(&project, &source(&rows)).unwrap();
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
        assert_eq!(mapped.extras.len(), 1);
        assert_eq!(mapped.extras[0].name, "audit");
        let Instance::DocumentSet(members) = &mapped.extras[0].instance else {
            panic!("expected named document set")
        };
        assert_eq!(members.len(), rows.len());
        for (member, (path, value)) in members.iter().zip(rows) {
            assert_eq!(member.path(), path);
            assert_eq!(member.source_path(), path);
            assert_eq!(
                member.value(),
                &Instance::Group(
                    vec![(
                        "Value".into(),
                        Instance::Scalar(Value::String(value.into()))
                    )]
                    .into()
                )
            );
        }
    }
}

#[test]
fn named_dynamic_driver_must_be_a_nonempty_path_to_a_repeating_group() {
    let valid = lower(&project()).unwrap();
    for mutation in 0..3 {
        let mut invalid = valid.clone();
        match mutation {
            0 => {
                invalid.extra_targets[0].root.iteration =
                    Some(IterationPlan::dynamic_documents(Vec::new(), 0))
            }
            1 => {
                let SchemaKind::Group { children, .. } = &mut invalid.source.kind else {
                    unreachable!()
                };
                children[0].repeating = false;
            }
            _ => {
                let SchemaKind::Group { children, .. } = &mut invalid.source.kind else {
                    unreachable!()
                };
                children[0].kind = SchemaKind::Scalar {
                    ty: ScalarType::String,
                };
            }
        }
        assert!(
            matches!(invalid.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("repeating Group"))
        );
    }
}

#[test]
fn static_named_inputs_gain_xml_adapter_while_other_boundaries_keep_optional_fallback() {
    let mut named_input = project();
    named_input.extra_sources.push(mapping::NamedSource {
        name: "unused".into(),
        path: "unused.xml".into(),
        schema: SchemaNode::group("Unused", vec![scalar("Label")]),
        options: named_input.source_options.clone(),
        dynamic_path: None,
    });
    let mut second_target = project();
    second_target.extra_targets.push(NamedTarget {
        name: "other".into(),
        path: None,
        schema: SchemaNode::group("Other", vec![scalar("Label")]),
        options: second_target.target_options.clone(),
        root: Scope::default(),
    });
    let mut iterating_primary = project();
    iterating_primary.root.iteration = ScopeIteration::Source(vec!["Row".into()]);
    for (candidate, expected_mode) in [
        (
            named_input,
            Some(XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments),
        ),
        (second_target, None),
        (iterating_primary, None),
    ] {
        let typed = lower(&candidate).unwrap();
        assert!(
            typed.extra_targets[0]
                .root
                .iteration
                .as_ref()
                .unwrap()
                .dynamic_document_iteration()
                .is_some()
        );
        assert_eq!(typed.xml_output_mode(), Ok(expected_mode));
    }
}

#[test]
fn mixed_boundary_alignment_and_member_shape_are_checked_without_mutating_old_modes() {
    let valid = lower(&project()).unwrap();
    let mut wrong_name = valid.clone();
    wrong_name.xml_boundary.as_mut().unwrap().extra_outputs[0].name = "different".into();
    assert!(
        matches!(wrong_name.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("exact declaration order"))
    );
    let mut missing_policy = valid.clone();
    missing_policy
        .xml_boundary
        .as_mut()
        .unwrap()
        .extra_outputs
        .clear();
    assert!(matches!(
        missing_policy.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
    let mut repeated_member = valid.clone();
    repeated_member.extra_targets[0].target.repeating = true;
    assert!(
        matches!(repeated_member.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("nonrepeating group"))
    );
    let mut repeated_primary = valid.clone();
    repeated_primary.target.repeating = true;
    assert!(
        matches!(repeated_primary.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("static noniterating group"))
    );
    let old_primary: Project =
        serde_json::from_str(include_str!("fixtures/dynamic_primary_xml_documents.json")).unwrap();
    assert_eq!(
        lower(&old_primary).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::DynamicPrimaryDocuments))
    );
    let old_static: Project =
        serde_json::from_str(include_str!("fixtures/static_named_xml_inputs.json")).unwrap();
    assert_eq!(
        lower(&old_static).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::SingleDocument))
    );
}

#[test]
fn stored_path_nested_dynamic_and_nonrepeated_output_refusals_remain_typed() {
    let mut stored = project();
    stored.extra_targets[0].path = Some("stored.xml".into());
    assert!(lower(&stored).is_err());
    let valid = lower(&project()).unwrap();
    let mut nested = valid.clone();
    let SchemaKind::Group { children, .. } = &mut nested.extra_targets[0].target.kind else {
        unreachable!()
    };
    children.push(SchemaNode::group("Nested", vec![scalar("Value")]));
    let mut child = nested.extra_targets[0].root.clone();
    child.target_field = "Nested".into();
    nested.extra_targets[0].root.children.push(child);
    assert!(
        matches!(nested.xml_output_mode(), Err(ProgramValidationError::NamedTarget { error, .. }) if matches!(*error, ProgramValidationError::DynamicDocumentsRequireRoot { .. }))
    );
    for mode in [
        crate::IterationOutput::First,
        crate::IterationOutput::MappedSequence,
    ] {
        let mut invalid = valid.clone();
        invalid.extra_targets[0].root.iteration = Some(IterationPlan::new(
            valid.extra_targets[0]
                .root
                .iteration
                .as_ref()
                .unwrap()
                .input()
                .clone(),
            None,
            None,
            Vec::new(),
            mode,
        ));
        assert!(invalid.xml_output_mode().is_err());
    }
    let mut observed = project();
    observed.source_options.xml_root_view_read_policy = true;
    assert!(lower(&observed).is_err());
}

#[test]
fn later_named_mapping_failure_precedes_bad_xml_in_the_already_mapped_primary() {
    let mut project = project();
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    let mapped = engine::run_outputs(&project, &source(&[("first.xml", "A")])).unwrap();
    assert_eq!(
        mapped.primary.field("Marker").and_then(Instance::as_scalar),
        Some(&Value::String("\u{1}".into()))
    );
    assert!(matches!(
        engine::run_outputs(&project, &source(&[("first.xml", "A"), (" ", "B")])),
        Err(engine::EngineError::EmptyDynamicTargetPath { node: 0 })
    ));
    // Public emitted XML calls must preserve this eager mapping-before-serialization priority.
}
