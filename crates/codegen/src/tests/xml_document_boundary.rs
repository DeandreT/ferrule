use super::*;

fn document_project() -> Project {
    let mut project = supported_project();
    project.source = serde_json::from_str(r#"{"name":"Root","xml_namespace":{"kind":"unqualified"},"xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code"]},{"name":"Derived","members":["Code","Extra"]}]}}"#).unwrap();
    project.source_options = mapping::FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: true,
        ..Default::default()
    };
    project.target = SchemaNode::group("Output", vec![scalar("Value").attribute()]);
    project.graph.nodes = BTreeMap::from([(
        0,
        Node::SourceRootField {
            path: vec!["Code".into()],
            required: false,
        },
    )]);
    project.root = Scope {
        bindings: vec![MappingBinding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Default::default()
    };
    project
}

#[test]
fn public_observed_xml_document_lowering_carries_literal_output_policy() {
    let mut project = document_project();
    project.target_options = mapping::FormatOptions {
        xml_document: true,
        xml_schema_hints: Some(ir::XmlSchemaHints {
            no_namespace_location: Some("../literal.xsd".into()),
            locations: vec![ir::XmlSchemaLocation {
                namespace: "urn:literal".into(),
                location: "https://literal.invalid/a.xsd".into(),
            }],
        }),
        ..Default::default()
    };
    let program = lower(&project).unwrap();
    let boundary = program.xml_boundary.unwrap();
    assert_eq!(
        boundary.input,
        crate::XmlInputPolicy {
            allow_inactive_root_type_members: true,
            root_view_policy: true,
        }
    );
    assert_eq!(
        boundary.output.schema_hints,
        project.target_options.xml_schema_hints
    );
    assert!(boundary.output.declaration && boundary.output.indent);
    assert!(boundary.output.default_namespace.is_none());
}

#[test]
fn public_xml_document_lowering_keeps_ordinary_hint_guard_and_named_boundaries_closed() {
    let mut ordinary = document_project();
    ordinary.source_options = Default::default();
    assert!(lower(&ordinary).unwrap().xml_boundary.is_none());
    ordinary.target_options.xml_document = true;
    ordinary.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some("literal.xsd".into()),
        locations: vec![],
    });
    assert!(
        lower(&ordinary)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("XML schema hints"))
    );
    let mut named = document_project();
    named.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("constant".into()),
        },
    );
    named.extra_targets.push(mapping::NamedTarget {
        name: "another".into(),
        schema: named.target.clone(),
        root: named.root.clone(),
        path: None,
        options: Default::default(),
    });
    assert!(
        lower(&named)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("one primary input and output"))
    );
}

#[test]
fn public_xml_document_lowering_refuses_root_sequences_before_emission() {
    let mut project = document_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("constant".into()),
        },
    );
    project.root.set_source(Some(vec![]));
    let error = lower(&project).unwrap_err();
    assert!(
        error
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("non-iterating primary root")),
        "{error:?}"
    );
}

#[test]
fn public_xml_document_profile_refuses_names_unsupported_by_the_actual_reader_or_hint_parser() {
    let valid = document_project();
    let mut supplementary_input = valid.clone();
    supplementary_input.source.name = "𐀀".into();
    assert!(
        lower(&supplementary_input)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("supplementary XML names"))
    );
    let mut supplementary_output = valid.clone();
    supplementary_output.target.name = "𐀀".into();
    assert!(lower(&supplementary_output).is_ok());
    supplementary_output.target_options.xml_document = true;
    supplementary_output.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some("literal.xsd".into()),
        locations: vec![],
    });
    assert!(
        lower(&supplementary_output)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("supplementary XML names"))
    );
    let mut colon = valid;
    colon.target.name = "p:Output".into();
    assert!(
        lower(&colon)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("local NCNames"))
    );
}

#[test]
fn public_xml_document_profile_refuses_namespace_declaration_roles_before_artifacts() {
    let mut project = document_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("ordinary".into()),
        },
    );
    project.root.bindings[0].target_field = "xmlns".into();
    if let SchemaKind::Group { children, .. } = &mut project.target.kind {
        children[0].name = "xmlns".into();
    }
    assert!(
        lower(&project)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("namespace declaration metadata"))
    );
    let mut source_uri = document_project();
    source_uri.source.xml_namespace = Some(ir::XmlNamespace::Qualified(
        ir::XmlNamespaceUri::new("urn:has space").unwrap(),
    ));
    assert!(
        lower(&source_uri)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("namespace is invalid or too large"))
    );
}

fn root_view_static_inputs_project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/root_view_static_named_xml_inputs.json"
    ))
    .unwrap()
}

#[test]
fn observed_primary_composes_with_exact_static_structured_named_inputs() {
    let project = root_view_static_inputs_project();
    assert!(engine::validate(&project).is_empty());
    let program = lower(&project).expect("observed primary plus ordinary static inputs");
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(crate::XmlOutputMode::SingleDocument))
    );
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(
        policy.input.profile(),
        Some(crate::XmlInputProfile::RootView)
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
        assert!(source.dynamic.is_none());
        assert_eq!(
            input.input.profile(),
            Some(crate::XmlInputProfile::Structured)
        );
        assert_eq!(
            source.source,
            project
                .extra_sources
                .iter()
                .find(|s| s.name == source.name)
                .unwrap()
                .schema
        );
    }
    assert!(program.extra_targets.is_empty());
    assert!(policy.extra_outputs.is_empty());
    assert!(program.root.iteration.is_none());
    for mutation in 0..5 {
        let mut invalid = program.clone();
        let inputs = &mut invalid.xml_boundary.as_mut().unwrap().extra_inputs;
        match mutation {
            0 => {
                inputs.pop();
            }
            1 => inputs.swap(0, 1),
            2 => inputs[0].name = "wrong".into(),
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
fn observed_compound_stays_strict_without_ordinary_schema_fallback() {
    let mut unsupported = root_view_static_inputs_project();
    let SchemaKind::Group { children, .. } = &mut unsupported.extra_sources[0].schema.kind else {
        unreachable!()
    };
    children[0].default = Some("0".into());
    assert!(
        lower(&unsupported).is_err(),
        "mandatory observed boundary cannot silently omit its adapter"
    );
    let mut mixed = root_view_static_inputs_project();
    mixed.extra_sources[1]
        .options
        .xml_allow_inactive_root_type_members = true;
    assert!(lower(&mixed).is_err());
    let mut observed_named = document_project();
    observed_named.extra_sources.push(mapping::NamedSource {
        name: "other".into(),
        path: "other.xml".into(),
        schema: observed_named.source.clone(),
        options: observed_named.source_options.clone(),
        dynamic_path: None,
    });
    assert!(
        lower(&observed_named)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|error| {
                error
                    .to_string()
                    .contains("named observed XML root-view input adapters")
            })
    );
    let mut extra_output = root_view_static_inputs_project();
    extra_output.extra_targets.push(mapping::NamedTarget {
        name: "another".into(),
        path: None,
        schema: extra_output.target.clone(),
        root: Scope::default(),
        options: extra_output.target_options.clone(),
    });
    assert!(
        lower(&extra_output)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|error| { error.to_string().contains("one primary input and output") })
    );
    let mut dynamic = lower(&root_view_static_inputs_project()).unwrap();
    dynamic.extra_sources[0].dynamic = Some(crate::DynamicSourceProgram {
        path: 0,
        driver: SourceIteration::new(Vec::new()),
    });
    assert!(
        validate_program(&dynamic).is_err(),
        "dynamic sources remain outside the observed route"
    );
}

#[test]
fn observed_compound_input_count_includes_primary() {
    let mut program = lower(&root_view_static_inputs_project()).unwrap();
    let source = program.extra_sources[0].clone();
    let input = program.xml_boundary.as_ref().unwrap().extra_inputs[0].clone();
    for index in 2..4095 {
        let name = format!("unused{index}");
        let mut source = source.clone();
        source.name = name.clone();
        let mut input = input.clone();
        input.name = name;
        program.extra_sources.push(source);
        program
            .xml_boundary
            .as_mut()
            .unwrap()
            .extra_inputs
            .push(input);
    }
    assert_eq!(validate_program(&program), Ok(()));
    let mut source = source;
    source.name = "one-too-many".into();
    let mut input = input;
    input.name = "one-too-many".into();
    program.extra_sources.push(source);
    program
        .xml_boundary
        .as_mut()
        .unwrap()
        .extra_inputs
        .push(input);
    assert!(
        matches!(validate_program(&program), Err(ProgramValidationError::InvalidXmlBoundary { reason })
        if reason == "XML document input sets permit at most 4096 artifacts including primary")
    );
}
