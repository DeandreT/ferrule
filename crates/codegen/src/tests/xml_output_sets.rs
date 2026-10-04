use super::*;

fn project() -> Project {
    let mut project = supported_project();
    project.source = SchemaNode::group("Root", vec![scalar("Input")]);
    project.target = SchemaNode::group("Primary", vec![scalar("Value")]);
    project.graph.nodes = BTreeMap::from([(
        1,
        Node::Const {
            value: Value::String("ok".into()),
        },
    )]);
    project.root = Scope {
        bindings: vec![MappingBinding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Default::default()
    };
    project.source_options = mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    project.target_options = project.source_options.clone();
    for name in ["z-audit", "a-receipt"] {
        project.extra_targets.push(NamedTarget {
            name: name.into(),
            schema: project.target.clone(),
            root: project.root.clone(),
            path: None,
            options: mapping::FormatOptions {
                xml_document: true,
                xml_schema_hints: Some(ir::XmlSchemaHints {
                    no_namespace_location: Some(format!("{name}.xsd")),
                    locations: vec![],
                }),
                ..Default::default()
            },
        });
    }
    project
}

#[test]
fn xml_named_output_policies_own_exact_declaration_order_and_literal_hints() {
    let project = project();
    let program = lower(&project).unwrap();
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(
        policy.input.profile(),
        Some(crate::XmlInputProfile::Structured)
    );
    assert_eq!(
        policy
            .extra_outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        ["z-audit", "a-receipt"]
    );
    assert!(policy.output.schema_hints.is_none());
    for (target, output) in project.extra_targets.iter().zip(&policy.extra_outputs) {
        assert_eq!(output.output.schema_hints, target.options.xml_schema_hints);
    }
    for mutation in 0..4 {
        let mut invalid = program.clone();
        let extra = &mut invalid.xml_boundary.as_mut().unwrap().extra_outputs;
        match mutation {
            0 => {
                extra.pop();
            }
            1 => extra.swap(0, 1),
            2 => extra[1].name = extra[0].name.clone(),
            _ => extra.push(extra[0].clone()),
        }
        assert!(matches!(
            validate_program(&invalid),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        ));
    }
}

#[test]
fn one_unsupported_ordinary_xml_target_omits_whole_adapter_without_dropping_core_targets() {
    let mut invalid_project = project();
    invalid_project.extra_targets[1].schema.name = "bad root".into();
    let program = lower(&invalid_project).unwrap();
    assert!(program.xml_boundary.is_none());
    assert_eq!(program.extra_targets.len(), 2);
    assert_eq!(program.extra_targets[1].target.name, "bad root");
    let mut mixed = project();
    mixed.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some("primary.xsd".into()),
        locations: vec![],
    });
    mixed.extra_targets[1].options.xml_document = false;
    mixed.extra_targets[1].options.xml_schema_hints = None;
    // Valid primary and first named hints remain optional XML metadata even
    // though a non-XML second target omits the complete adapter.
    assert!(mixed.extra_targets[0].options.xml_schema_hints.is_some());
    let fallback = lower(&mixed).unwrap();
    assert!(fallback.xml_boundary.is_none());
    assert_eq!(fallback.extra_targets.len(), 2);
    assert_eq!(fallback.extra_targets[0].name, "z-audit");
    assert_eq!(fallback.extra_targets[1].name, "a-receipt");
}

#[test]
fn explicit_xml_named_policy_refuses_nonclosed_target_and_count_including_primary() {
    let mut program = lower(&project()).unwrap();
    program.extra_targets[1].target.name = "bad root".into();
    assert!(
        matches!(validate_program(&program), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("a-receipt") && reason.contains("NCNames"))
    );
    let project = project();
    let mut program = lower(&project).unwrap();
    let named = program.extra_targets[0].clone();
    let policy = program.xml_boundary.as_ref().unwrap().extra_outputs[0].clone();
    program.extra_targets = (0..4095)
        .map(|i| crate::NamedTargetProgram {
            name: format!("target{i}"),
            ..named.clone()
        })
        .collect();
    program.xml_boundary.as_mut().unwrap().extra_outputs = (0..4095)
        .map(|i| crate::NamedXmlOutputPolicy {
            name: format!("target{i}"),
            ..policy.clone()
        })
        .collect();
    assert_eq!(validate_program(&program), Ok(()));
    program.extra_targets.push(crate::NamedTargetProgram {
        name: "one-too-many".into(),
        ..named
    });
    program
        .xml_boundary
        .as_mut()
        .unwrap()
        .extra_outputs
        .push(crate::NamedXmlOutputPolicy {
            name: "one-too-many".into(),
            ..policy
        });
    assert!(
        matches!(validate_program(&program), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("4096"))
    );
}
