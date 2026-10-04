use super::*;
use crate::{XmlInputProfile, XmlOutputMode};
use ir::Instance;

fn project() -> Project {
    serde_json::from_str(include_str!("fixtures/dynamic_primary_xml_documents.json")).unwrap()
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
fn dynamic_primary_xml_mode_preserves_existing_typed_member_order_and_paths() {
    let project = project();
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(XmlOutputMode::DynamicPrimaryDocuments))
    );
    assert_eq!(
        program.xml_boundary.as_ref().unwrap().input.profile(),
        Some(XmlInputProfile::Structured)
    );
    let mapped = engine::run(
        &project,
        &source(&[
            ("same.xml", "A"),
            ("same.xml", "B"),
            ("/opaque.xml", "C"),
            ("../opaque.xml", "D"),
        ]),
    )
    .unwrap();
    let Instance::DocumentSet(members) = mapped else {
        panic!("expected document set");
    };
    assert_eq!(
        members
            .iter()
            .map(|member| member.path())
            .collect::<Vec<_>>(),
        ["same.xml", "same.xml", "/opaque.xml", "../opaque.xml"]
    );
    for (member, value) in members.iter().zip(["A", "B", "C", "D"]) {
        assert_eq!(member.source_path(), member.path());
        assert_eq!(
            member.value().field("Value").and_then(Instance::as_scalar),
            Some(&Value::String(value.into()))
        );
    }
    assert_eq!(
        engine::run(&project, &source(&[])).unwrap(),
        Instance::DocumentSet(Vec::new())
    );
}

#[test]
fn dynamic_xml_driver_is_a_nonempty_path_to_a_repeating_group() {
    let valid = lower(&project()).unwrap();
    for mutation in 0..3 {
        let mut invalid = valid.clone();
        match mutation {
            0 => invalid.root.iteration = Some(IterationPlan::dynamic_documents(Vec::new(), 0)),
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
fn dynamic_xml_query_returns_typed_error_for_nested_and_nonrepeated_output() {
    let valid = lower(&project()).unwrap();
    let mut nested = valid.clone();
    let SchemaKind::Group { children, .. } = &mut nested.target.kind else {
        unreachable!()
    };
    children.push(SchemaNode::group("Nested", vec![scalar("Value")]));
    let mut child = nested.root.clone();
    child.target_field = "Nested".into();
    nested.root.children.push(child);
    assert!(matches!(
        nested.xml_output_mode(),
        Err(ProgramValidationError::DynamicDocumentsRequireRoot { .. })
    ));
    for mode in [
        crate::IterationOutput::First,
        crate::IterationOutput::MappedSequence,
    ] {
        let mut invalid = valid.clone();
        invalid.root.iteration = Some(IterationPlan::new(
            valid.root.iteration.as_ref().unwrap().input().clone(),
            None,
            None,
            Vec::new(),
            mode,
        ));
        assert!(matches!(
            invalid.xml_output_mode(),
            Err(ProgramValidationError::InvalidIterationOutput { .. })
        ));
    }
}

#[test]
fn named_boundaries_and_other_root_iteration_keep_optional_fallback() {
    let mut named = project();
    named.extra_sources.push(mapping::NamedSource {
        name: "unused".into(),
        path: "unused.xml".into(),
        schema: SchemaNode::group("Unused", vec![scalar("Label")]),
        options: named.source_options.clone(),
        dynamic_path: None,
    });
    let program = lower(&named).unwrap();
    assert_eq!(program.extra_sources.len(), 1);
    assert_eq!(program.xml_output_mode(), Ok(None));
    let mut target = project();
    target.extra_targets.push(NamedTarget {
        name: "unused".into(),
        path: None,
        schema: SchemaNode::group("Unused", vec![scalar("Label")]),
        options: target.target_options.clone(),
        root: Scope::default(),
    });
    let program = lower(&target).unwrap();
    assert_eq!(program.extra_targets.len(), 1);
    assert_eq!(program.xml_output_mode(), Ok(None));
    let mut ordinary = project();
    ordinary.root.iteration = ScopeIteration::Source(vec!["Row".into()]);
    assert_eq!(lower(&ordinary).unwrap().xml_output_mode(), Ok(None));
}

#[test]
fn unchanged_static_xml_fixture_still_has_single_document_mode() {
    let static_project: Project =
        serde_json::from_str(include_str!("fixtures/static_named_xml_inputs.json")).unwrap();
    assert_eq!(
        lower(&static_project).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::SingleDocument))
    );
    let mut invalid = lower(&project()).unwrap();
    invalid
        .xml_boundary
        .as_mut()
        .unwrap()
        .input
        .allow_inactive_root_type_members = true;
    assert!(matches!(
        invalid.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
}

#[test]
fn later_mapping_path_failure_precedes_a_first_member_xml_character_refusal() {
    let mut project = project();
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    let first = engine::run(&project, &source(&[("first.xml", "A")])).unwrap();
    let Instance::DocumentSet(members) = first else {
        panic!("expected one member");
    };
    assert_eq!(
        members[0]
            .value()
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("\u{1}".into()))
    );
    assert!(matches!(
        engine::run(&project, &source(&[("first.xml", "A"), (" ", "B")])),
        Err(engine::EngineError::EmptyDynamicTargetPath { node: 0 })
    ));
    // This proves the typed mapper fails before any XML adapter serialization.
    // The later emitter integration must assert the same priority through its public API.
}
