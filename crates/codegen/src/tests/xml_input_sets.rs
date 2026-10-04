use super::*;

fn project() -> Project {
    serde_json::from_str(include_str!("fixtures/static_named_xml_inputs.json")).unwrap()
}

#[test]
fn static_xml_inputs_retain_every_schema_and_exact_declaration_policy() {
    let project = project();
    let program = lower(&project).unwrap();
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(
        policy.input.profile(),
        Some(crate::XmlInputProfile::Structured)
    );
    assert_eq!(
        policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "labels"]
    );
    for (source, input) in program.extra_sources.iter().zip(&policy.extra_inputs) {
        assert_eq!(
            source.source,
            project
                .extra_sources
                .iter()
                .find(|expected| expected.name == source.name)
                .unwrap()
                .schema
        );
        assert!(source.dynamic.is_none());
        assert_eq!(
            input.input.profile(),
            Some(crate::XmlInputProfile::Structured)
        );
    }
    for mutation in 0..5 {
        let mut invalid = program.clone();
        let inputs = &mut invalid.xml_boundary.as_mut().unwrap().extra_inputs;
        match mutation {
            0 => {
                inputs.pop();
            }
            1 => inputs.swap(0, 1),
            2 => inputs[0].name = "not-rates".into(),
            3 => inputs[0].input.root_view_policy = true,
            _ => {
                inputs[0].input = crate::XmlInputPolicy {
                    allow_inactive_root_type_members: true,
                    root_view_policy: true,
                }
            }
        }
        assert!(matches!(
            validate_program(&invalid),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        ));
    }
}

#[test]
fn ordinary_unproved_named_source_omits_whole_adapter_and_retains_hinted_core() {
    for mutation in 0..2 {
        let mut project = project();
        project.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
            no_namespace_location: Some("literal-primary.xsd".into()),
            locations: vec![],
        });
        if mutation == 0 {
            project.extra_sources[0].options.xml_document = false;
        } else {
            let SchemaKind::Group { children, .. } = &mut project.extra_sources[0].schema.kind
            else {
                unreachable!()
            };
            children[0].default = Some("unused default".into());
        }
        let program = lower(&project).unwrap();
        assert!(program.xml_boundary.is_none());
        assert_eq!(program.extra_sources.len(), 2);
        assert_eq!(program.extra_targets.len(), project.extra_targets.len());
        assert_eq!(
            program.extra_sources[0].source,
            project.extra_sources[0].schema
        );
    }
}

#[test]
fn named_observed_or_mixed_flags_never_select_structured_fallback() {
    for (allow, root) in [(true, true), (true, false), (false, true)] {
        let mut project = project();
        project.extra_sources[1]
            .options
            .xml_allow_inactive_root_type_members = allow;
        project.extra_sources[1].options.xml_root_view_read_policy = root;
        assert!(lower(&project).is_err());
    }
}

#[test]
fn input_artifact_count_includes_primary_and_never_drops_a_core_source() {
    let mut project = project();
    let source = project.extra_sources[0].clone();
    project.extra_sources = (0..4095)
        .map(|index| mapping::NamedSource {
            name: if index == 0 {
                "rates".into()
            } else if index == 1 {
                "labels".into()
            } else {
                format!("input{index}")
            },
            schema: if index == 1 {
                project.extra_sources[1].schema.clone()
            } else {
                source.schema.clone()
            },
            ..source.clone()
        })
        .collect();
    assert!(lower(&project).unwrap().xml_boundary.is_some());
    project.extra_sources.push(mapping::NamedSource {
        name: "one-too-many".into(),
        ..source
    });
    let program = lower(&project).unwrap();
    assert!(program.xml_boundary.is_none());
    assert_eq!(program.extra_sources.len(), 4096);
}
