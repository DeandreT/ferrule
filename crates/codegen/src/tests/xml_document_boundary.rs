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
    // The flat combination is now proved; retain this refusal for a root label
    // that denotes neither the document root nor its own schema name.
    extra_output.extra_targets[0].root.target_field = "descendant".into();
    assert!(
        lower(&extra_output).is_err(),
        "a mismatched root label remains unsupported"
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

fn root_view_static_output_project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/root_view_primary_static_named_xml_output.json"
    ))
    .unwrap()
}

#[test]
fn observed_primary_and_one_static_output_keep_independent_schema_and_hint_policies() {
    let project = root_view_static_output_project();
    assert!(engine::validate(&project).is_empty());
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(crate::XmlOutputMode::SingleDocument))
    );
    assert!(program.extra_sources.is_empty());
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(
        policy.input.profile(),
        Some(crate::XmlInputProfile::RootView)
    );
    assert!(policy.extra_inputs.is_empty());
    assert_eq!(policy.extra_outputs.len(), 1);
    assert_eq!(policy.extra_outputs[0].name, "audit");
    assert_eq!(
        policy.output.schema_hints,
        project.target_options.xml_schema_hints
    );
    assert_eq!(
        policy.extra_outputs[0].output.schema_hints,
        project.extra_targets[0].options.xml_schema_hints
    );
    assert_ne!(
        policy.output.schema_hints,
        policy.extra_outputs[0].output.schema_hints
    );
    assert_eq!(
        program.extra_targets[0].target,
        project.extra_targets[0].schema
    );
    assert_ne!(
        program.target.xml_namespace,
        program.extra_targets[0].target.xml_namespace
    );
    for mutation in 0..3 {
        let mut invalid = program.clone();
        match mutation {
            0 => {
                invalid.xml_boundary.as_mut().unwrap().extra_outputs.clear();
            }
            1 => {
                invalid.xml_boundary.as_mut().unwrap().extra_outputs[0].name = "wrong".into();
            }
            _ => {
                let extra = invalid.xml_boundary.as_mut().unwrap().extra_outputs[0].clone();
                invalid
                    .xml_boundary
                    .as_mut()
                    .unwrap()
                    .extra_outputs
                    .push(extra);
            }
        }
        assert!(matches!(
            validate_program(&invalid),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        ));
    }
}

#[test]
fn observed_static_output_exception_keeps_other_compound_and_root_shapes_closed() {
    let valid = root_view_static_output_project();
    for mutation in 0..7 {
        let mut project = valid.clone();
        match mutation {
            0 => {
                let mut second = project.extra_targets[0].clone();
                second.name = "second".into();
                second.root.target_field = "descendant".into();
                project.extra_targets.push(second);
            }
            1 => {
                project.extra_sources = root_view_static_inputs_project().extra_sources;
                // Only Structured named inputs compose with these output roots.
                project.extra_sources[0]
                    .options
                    .xml_allow_inactive_root_type_members = true;
                project.extra_sources[0].options.xml_root_view_read_policy = true;
            }
            2 => {
                project.extra_targets[0].root.set_source(Some(vec![]));
            }
            3 => {
                project.root.set_source(Some(vec![]));
            }
            4 => {
                project.extra_targets[0].schema.repeating = true;
            }
            5 => {
                project.extra_targets[0].options = Default::default();
            }
            _ => {
                project.extra_targets[0].schema.name = "bad root".into();
            }
        }
        assert!(lower(&project).is_err(), "mutation {mutation}");
    }
    let mut explicit = lower(&valid).unwrap();
    explicit.extra_targets[0].root.repeating = true;
    assert!(matches!(
        validate_program(&explicit),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
    assert!(lower(&root_view_static_inputs_project()).is_ok());
    assert!(lower(&document_project()).is_ok());
}

fn root_view_static_output_source(derived: bool, extra: Option<&str>) -> ir::Instance {
    // Trusted host-owned typed input: this test does not execute an XML reader.
    let mut fields = vec![(
        "Code".to_owned(),
        ir::Instance::Scalar(Value::String("  code 雪😀  ".into())),
    )];
    if let Some(extra) = extra {
        fields.push((
            "Extra".into(),
            ir::Instance::Scalar(Value::String(extra.into())),
        ));
    }
    let origin = if derived {
        ir::XmlTypeOrigin::Explicit("{urn:types}Derived")
    } else {
        ir::XmlTypeOrigin::Absent
    };
    ir::Instance::Group(
        ir::InstanceGroup::from(fields)
            .with_xml_type_origin(origin)
            .unwrap(),
    )
}

#[test]
fn native_observed_static_outputs_keep_type_selection_and_required_read_laziness() {
    for derived in [false, true] {
        let source = root_view_static_output_source(derived, derived.then_some("  extra 雪😀  "));
        let mapped = engine::run_outputs(&root_view_static_output_project(), &source).unwrap();
        assert_eq!(
            mapped
                .primary
                .field("Code")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::String("  code 雪😀  ".into()))
        );
        assert_eq!(
            mapped
                .primary
                .field("Derived")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::Bool(derived))
        );
        assert_eq!(
            mapped
                .primary
                .field("Marker")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::String("primary".into()))
        );
        assert_eq!(mapped.extras.len(), 1);
        assert_eq!(mapped.extras[0].name, "audit");
        assert_eq!(
            mapped.extras[0]
                .instance
                .field("Code")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::String("  code 雪😀  ".into()))
        );
        assert_eq!(
            mapped.extras[0]
                .instance
                .field("Extra")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::String(
                if derived {
                    "  extra 雪😀  "
                } else {
                    "skipped"
                }
                .into()
            ))
        );
        assert_eq!(
            mapped.extras[0]
                .instance
                .field("Marker")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::String("audit".into()))
        );
        assert_eq!(
            source.xml_type_origin(),
            Ok(if derived {
                ir::XmlTypeOrigin::Explicit("{urn:types}Derived")
            } else {
                ir::XmlTypeOrigin::Absent
            })
        );
    }
}

#[test]
fn late_named_required_read_stops_mapping_before_any_primary_writer() {
    let mut project = root_view_static_output_project();
    project.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    assert!(engine::validate(&project).is_empty());
    assert!(lower(&project).is_ok());
    assert!(
        matches!(engine::run_outputs(&project, &root_view_static_output_source(true, None)),
        Err(engine::EngineError::PrimaryRoot { node: 1, source: ir::PrimaryRootError::MissingRequiredField { path } })
        if path == vec!["Extra".to_owned()])
    );
}

#[test]
fn named_output_context_read_is_lazy_and_preserves_missing_value_failure() {
    let mut project = root_view_static_output_project();
    project.graph.nodes.insert(
        6,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.graph.nodes.insert(
        7,
        Node::If {
            condition: 2,
            then: 6,
            else_: 3,
        },
    );
    project.extra_targets[0].root.bindings[2].node = 7;
    assert!(lower(&project).is_ok());
    let base = engine::run_outputs(&project, &root_view_static_output_source(false, None)).unwrap();
    assert_eq!(
        base.extras[0]
            .instance
            .field("Marker")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("skipped".into()))
    );
    let derived = root_view_static_output_source(true, Some("present"));
    assert!(matches!(
        engine::run_outputs(&project, &derived),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let context = engine::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_current_datetime("2026-10-05T12:00:00Z");
    let mapped =
        engine::run_outputs_with_sources_and_context(&project, &derived, Vec::new(), &context)
            .unwrap();
    assert_eq!(
        mapped.extras[0]
            .instance
            .field("Marker")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("2026-10-05T12:00:00Z".into()))
    );
}

fn root_view_plural_static_output_project(count: usize, reversed: bool) -> Project {
    let wire = match (count, reversed) {
        (2, false) => include_str!("fixtures/root_view_two_static_named_xml_outputs.json"),
        (2, true) => include_str!("fixtures/root_view_two_static_named_xml_outputs_reversed.json"),
        (3, false) => include_str!("fixtures/root_view_three_static_named_xml_outputs.json"),
        _ => unreachable!(),
    };
    serde_json::from_str(wire).unwrap()
}

#[test]
fn observed_plural_output_lowering_preserves_every_declared_schema_hint_and_order() {
    for (count, reversed) in [(2, false), (3, false), (2, true)] {
        let project = root_view_plural_static_output_project(count, reversed);
        assert!(engine::validate(&project).is_empty());
        let program = lower(&project).unwrap();
        assert_eq!(
            program.xml_output_mode(),
            Ok(Some(crate::XmlOutputMode::SingleDocument))
        );
        let policy = program.xml_boundary.as_ref().unwrap();
        assert_eq!(policy.extra_outputs.len(), count);
        assert_eq!(program.extra_targets.len(), count);
        for ((target, lowered), output) in project
            .extra_targets
            .iter()
            .zip(&program.extra_targets)
            .zip(&policy.extra_outputs)
        {
            assert_eq!(lowered.name, target.name);
            assert_eq!(lowered.target, target.schema);
            assert_eq!(lowered.root.target_field, target.schema.name);
            assert_eq!(output.name, target.name);
            assert_eq!(output.output.schema_hints, target.options.xml_schema_hints);
        }
        assert_ne!(
            policy.extra_outputs[0].output.schema_hints,
            policy.extra_outputs[1].output.schema_hints
        );
        for derived in [false, true] {
            let source = root_view_static_output_source(derived, derived.then_some("later extra"));
            let outputs = engine::run_outputs(&project, &source).unwrap();
            assert_eq!(outputs.extras.len(), count);
            for (output, target) in outputs.extras.iter().zip(&project.extra_targets) {
                assert_eq!(output.name, target.name);
                assert_eq!(
                    output
                        .instance
                        .field("Code")
                        .and_then(ir::Instance::as_scalar),
                    Some(&Value::String("  code 雪😀  ".into()))
                );
                let expected = if target.name == "z-audit" || !derived {
                    "skipped"
                } else {
                    "later extra"
                };
                assert_eq!(
                    output
                        .instance
                        .field("Extra")
                        .and_then(ir::Instance::as_scalar),
                    Some(&Value::String(expected.into()))
                );
            }
        }
    }
}

#[test]
fn observed_plural_last_mapping_failure_precedes_primary_writer_and_context_stays_lazy() {
    let mut project = root_view_plural_static_output_project(2, false);
    project.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    assert!(lower(&project).is_ok());
    assert!(
        matches!(engine::run_outputs(&project, &root_view_static_output_source(true, None)),
        Err(engine::EngineError::PrimaryRoot { node: 1, source: ir::PrimaryRootError::MissingRequiredField { path } })
        if path == vec!["Extra".to_owned()])
    );
    project.graph.nodes.insert(
        9,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.graph.nodes.insert(
        10,
        Node::If {
            condition: 2,
            then: 9,
            else_: 3,
        },
    );
    project.extra_targets[1]
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Marker")
        .unwrap()
        .node = 10;
    assert!(lower(&project).is_ok());
    let base = engine::run_outputs(&project, &root_view_static_output_source(false, None)).unwrap();
    assert_eq!(
        base.extras[1]
            .instance
            .field("Marker")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("skipped".into()))
    );
    let source = root_view_static_output_source(true, Some("present"));
    assert!(matches!(
        engine::run_outputs(&project, &source),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let context = engine::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_current_datetime("2026-10-05T12:00:00Z");
    let outputs =
        engine::run_outputs_with_sources_and_context(&project, &source, Vec::new(), &context)
            .unwrap();
    assert_eq!(
        outputs.extras[1]
            .instance
            .field("Marker")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("2026-10-05T12:00:00Z".into()))
    );
}

#[test]
fn observed_plural_flatness_is_required_without_any_primary_root_primitive() {
    let mut project = root_view_plural_static_output_project(2, false);
    for (&id, node) in &mut project.graph.nodes {
        *node = Node::Const {
            value: if id == 2 {
                Value::Bool(false)
            } else {
                Value::String("constant".into())
            },
        };
    }
    assert!(engine::validate(&project).is_empty());
    let program = lower(&project).unwrap();
    for primary in [false, true] {
        let mut nested = project.clone();
        let (schema, scope) = if primary {
            (&mut nested.target, &mut nested.root)
        } else {
            let target = &mut nested.extra_targets[1];
            (&mut target.schema, &mut target.root)
        };
        if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
            children.push(SchemaNode::group("Child", Vec::new()));
        }
        scope.children.push(Scope {
            target_field: "Child".into(),
            ..Scope::default()
        });
        assert!(
            engine::validate(&nested).is_empty(),
            "the nested shape must otherwise be valid"
        );
        assert!(lower(&nested).is_err());
        let mut mismatched = project.clone();
        if primary {
            mismatched.root.target_field = "descendant".into();
        } else {
            mismatched.extra_targets[1].root.target_field = "Audit".into();
        }
        assert!(lower(&mismatched).is_err());
        let mut neutral = program.clone();
        let (schema, scope) = if primary {
            (&mut neutral.target, &mut neutral.root)
        } else {
            let target = &mut neutral.extra_targets[1];
            (&mut target.target, &mut target.root)
        };
        if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
            children.push(SchemaNode::group("Child", Vec::new()));
        }
        scope.children.push(crate::TargetScope {
            target_field: "Child".into(),
            repeating: false,
            iteration: None,
            construction: crate::TargetConstruction::Group,
            bindings: Vec::new(),
            children: Vec::new(),
        });
        assert!(matches!(
            validate_program(&neutral),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        ));
    }
    for mutation in 0..11 {
        let mut invalid = project.clone();
        match mutation {
            0 => invalid.root.filter = Some(2),
            1 => invalid.extra_targets[1].root.filter = Some(2),
            2 => invalid.extra_targets[1].root.set_source(Some(vec![])),
            3 => invalid.extra_targets[1].schema.repeating = true,
            4 => invalid.extra_targets[1].root.construction = ScopeConstruction::CopyCurrentSource,
            5 => invalid.target.repeating = true,
            6 => invalid
                .root
                .dynamic_bindings
                .push(mapping::DynamicBinding { key: 0, value: 1 }),
            7 => invalid.extra_targets[1]
                .root
                .dynamic_bindings
                .push(mapping::DynamicBinding { key: 0, value: 1 }),
            8 => invalid.root.set_source(Some(vec![])),
            9 => {
                invalid.extra_targets[1].root.set_source(Some(vec![]));
                assert!(invalid.extra_targets[1].root.set_output_path(Some(0)));
            }
            10 => {
                invalid.root.set_source(Some(vec![]));
                assert!(invalid.root.set_output_path(Some(0)));
            }
            _ => unreachable!(),
        }
        assert!(lower(&invalid).is_err(), "no-reader mutation {mutation}");
    }
}

fn root_view_combined_static_project(count: usize) -> Project {
    serde_json::from_str(match count {
        1 => include_str!(
            "fixtures/root_view_two_static_structured_inputs_1_static_named_outputs.json"
        ),
        2 => include_str!(
            "fixtures/root_view_two_static_structured_inputs_2_static_named_outputs.json"
        ),
        _ => unreachable!(),
    })
    .unwrap()
}

fn root_view_combined_static_sources(
    read_extra: bool,
    need_context: bool,
) -> Vec<(String, ir::Instance)> {
    vec![
        (
            "rates".into(),
            ir::Instance::Group(
                vec![("Revision".into(), ir::Instance::Scalar(Value::Int(9)))].into(),
            ),
        ),
        (
            "labels".into(),
            ir::Instance::Group(
                vec![
                    (
                        "Prefix".into(),
                        ir::Instance::Scalar(Value::String("side:".into())),
                    ),
                    (
                        "ReadExtra".into(),
                        ir::Instance::Scalar(Value::Bool(read_extra)),
                    ),
                    (
                        "NeedContext".into(),
                        ir::Instance::Scalar(Value::Bool(need_context)),
                    ),
                    (
                        "Code".into(),
                        ir::Instance::Scalar(Value::String("secondary-code".into())),
                    ),
                    (
                        "Extra".into(),
                        ir::Instance::Scalar(Value::String("secondary-extra".into())),
                    ),
                ]
                .into(),
            ),
        ),
    ]
}

#[test]
fn observed_combined_static_documents_keep_policies_order_and_colliding_fields_separate() {
    let context = engine::ExecutionContext::new(std::path::Path::new("combined.json"));
    for count in [1, 2] {
        for reversed in [false, true] {
            let mut project = root_view_combined_static_project(count);
            if reversed {
                project.extra_targets.reverse();
            }
            assert!(engine::validate(&project).is_empty());
            let program = lower(&project).unwrap();
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
                assert_eq!(source.name, input.name);
            }
            for ((native, target), output) in project
                .extra_targets
                .iter()
                .zip(&program.extra_targets)
                .zip(&policy.extra_outputs)
            {
                assert_eq!(native.name, target.name);
                assert_eq!(native.schema, target.target);
                assert_eq!(target.name, output.name);
                assert_eq!(native.options.xml_schema_hints, output.output.schema_hints);
            }
            let primary = root_view_static_output_source(true, Some("primary-extra"));
            let mut extras = root_view_combined_static_sources(true, false);
            if reversed {
                extras.reverse();
            }
            let mapped =
                engine::run_outputs_with_sources_and_context(&project, &primary, extras, &context)
                    .unwrap();
            assert_eq!(
                mapped
                    .primary
                    .field("Code")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("  code 雪😀  ".into()))
            );
            assert_eq!(
                mapped
                    .primary
                    .field("NamedCode")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("secondary-code".into()))
            );
            assert_eq!(
                mapped
                    .primary
                    .field("NamedExtra")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("secondary-extra".into()))
            );
            assert_eq!(
                mapped
                    .extras
                    .iter()
                    .map(|output| output.name.as_str())
                    .collect::<Vec<_>>(),
                project
                    .extra_targets
                    .iter()
                    .map(|target| target.name.as_str())
                    .collect::<Vec<_>>()
            );
            let later = mapped
                .extras
                .iter()
                .find(|output| output.name == "a-summary")
                .unwrap();
            assert_eq!(
                later
                    .instance
                    .field("Extra")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("primary-extra".into()))
            );
            assert_eq!(
                later
                    .instance
                    .field("NamedExtra")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("secondary-extra".into()))
            );
            assert_eq!(
                primary.xml_type_origin(),
                Ok(ir::XmlTypeOrigin::Explicit("{urn:types}Derived"))
            );
        }
    }
}

#[test]
fn observed_combined_late_required_and_context_reads_are_lazy_without_named_fallback() {
    let context = engine::ExecutionContext::new(std::path::Path::new("combined.json"));
    let mut project = root_view_combined_static_project(2);
    let primary = root_view_static_output_source(true, None);
    let lazy = engine::run_outputs_with_sources_and_context(
        &project,
        &primary,
        root_view_combined_static_sources(false, false),
        &context,
    )
    .unwrap();
    assert_eq!(
        lazy.extras[1]
            .instance
            .field("Extra")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("skipped".into()))
    );
    assert_eq!(
        lazy.extras[1]
            .instance
            .field("NamedExtra")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("secondary-extra".into()))
    );
    // The earlier primary writer would reject this value, but mapping the later
    // root must expose the genuine primary required-read failure first.
    project.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    assert!(lower(&project).is_ok());
    assert!(
        matches!(engine::run_outputs_with_sources_and_context(&project, &primary, root_view_combined_static_sources(true, false), &context),
        Err(engine::EngineError::PrimaryRoot { node: 1, source: ir::PrimaryRootError::MissingRequiredField { path } }) if path == vec!["Extra".to_owned()])
    );
    project.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::String("unused".into()),
        },
    );
    assert!(matches!(
        engine::run_outputs_with_sources_and_context(
            &project,
            &primary,
            root_view_combined_static_sources(false, true),
            &context
        ),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let supplied = context.with_current_datetime("2026-10-05T12:00:00Z");
    let mapped = engine::run_outputs_with_sources_and_context(
        &project,
        &primary,
        root_view_combined_static_sources(false, true),
        &supplied,
    )
    .unwrap();
    assert_eq!(
        mapped.extras[1]
            .instance
            .field("Timestamp")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("2026-10-05T12:00:00Z".into()))
    );
}

#[test]
fn observed_combined_flatness_is_unconditional_with_one_output_and_no_root_readers() {
    for count in [1, 2] {
        let mut project = root_view_combined_static_project(count);
        for (&id, node) in &mut project.graph.nodes {
            *node = Node::Const {
                value: match id {
                    2 | 5 | 9 => Value::Bool(false),
                    3 => Value::Int(9),
                    _ => Value::String("constant".into()),
                },
            };
        }
        assert!(engine::validate(&project).is_empty());
        let program = lower(&project).unwrap();
        for primary in [false, true] {
            let mut nested = project.clone();
            let (schema, scope) = if primary {
                (&mut nested.target, &mut nested.root)
            } else {
                let target = nested.extra_targets.last_mut().unwrap();
                (&mut target.schema, &mut target.root)
            };
            if let SchemaKind::Group { children, .. } = &mut schema.kind {
                children.push(SchemaNode::group("Child", Vec::new()));
            }
            scope.children.push(Scope {
                target_field: "Child".into(),
                ..Scope::default()
            });
            assert!(
                engine::validate(&nested).is_empty(),
                "the nested control must otherwise be valid"
            );
            assert!(lower(&nested).is_err());
            let mut neutral = program.clone();
            let (schema, scope) = if primary {
                (&mut neutral.target, &mut neutral.root)
            } else {
                let target = neutral.extra_targets.last_mut().unwrap();
                (&mut target.target, &mut target.root)
            };
            if let SchemaKind::Group { children, .. } = &mut schema.kind {
                children.push(SchemaNode::group("Child", Vec::new()));
            }
            scope.children.push(crate::TargetScope {
                target_field: "Child".into(),
                repeating: false,
                iteration: None,
                construction: crate::TargetConstruction::Group,
                bindings: Vec::new(),
                children: Vec::new(),
            });
            assert!(matches!(
                validate_program(&neutral),
                Err(ProgramValidationError::InvalidXmlBoundary { .. })
            ));
            for mutation in 0..12 {
                let mut invalid = project.clone();
                let scope = if primary {
                    &mut invalid.root
                } else {
                    &mut invalid.extra_targets.last_mut().unwrap().root
                };
                match mutation {
                    0 => scope.target_field = "descendant".into(),
                    1 => scope.set_source(Some(vec![])),
                    2 => scope.filter = Some(5),
                    3 => scope.post_group_filter = Some(5),
                    4 => scope.merge_dynamic_fields = true,
                    5 => scope.construction = ScopeConstruction::CopyCurrentSource,
                    6 => scope
                        .dynamic_bindings
                        .push(mapping::DynamicBinding { key: 0, value: 1 }),
                    7 => {
                        scope.set_source(Some(vec![]));
                        assert!(scope.set_output_path(Some(0)));
                    }
                    8 => scope
                        .windows
                        .push(mapping::SequenceWindow::First { count: 3 }),
                    9 => {
                        scope.set_source(Some(vec![]));
                        scope.sort_by = Some(3);
                    }
                    10 => {
                        scope.set_source(Some(vec![]));
                        scope.group_by = Some(3);
                    }
                    11 => scope.dynamic_children.push(mapping::DynamicChild {
                        key: 0,
                        scope: Scope::default(),
                    }),
                    _ => unreachable!(),
                }
                assert!(
                    lower(&invalid).is_err(),
                    "count {count}, primary {primary}, mutation {mutation}"
                );
            }
            let mut repeated = project.clone();
            if primary {
                repeated.target.repeating = true;
            } else {
                repeated.extra_targets.last_mut().unwrap().schema.repeating = true;
            }
            assert!(lower(&repeated).is_err());
        }
    }
}

#[test]
fn observed_combined_preserves_static_input_options_and_complete_policy_refusals() {
    let project = root_view_combined_static_project(2);
    let program = lower(&project).unwrap();
    for mutation in 0..7 {
        let mut invalid = project.clone();
        match mutation {
            0 => invalid.extra_sources[0].options.xml_document = false,
            1 => {
                invalid.extra_sources[0]
                    .options
                    .xml_allow_inactive_root_type_members = true
            }
            2 => {
                invalid.extra_sources[0]
                    .options
                    .xml_allow_inactive_root_type_members = true;
                invalid.extra_sources[0].options.xml_root_view_read_policy = true;
            }
            3 => invalid.extra_sources[0].schema = project.source.clone(),
            4 => {
                invalid.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
                    node: 6,
                    iteration: Vec::new(),
                })
            }
            5 => {
                if let SchemaKind::Group { children, .. } =
                    &mut invalid.extra_sources[0].schema.kind
                {
                    children[0].default = Some("unused".into());
                }
            }
            6 => invalid.extra_targets[1].root.target_field = "Audit".into(),
            _ => unreachable!(),
        }
        assert!(lower(&invalid).is_err(), "mutation {mutation}");
    }
    for mutation in 0..7 {
        let mut invalid = program.clone();
        let policy = invalid.xml_boundary.as_mut().unwrap();
        match mutation {
            0 => {
                policy.extra_inputs.pop();
            }
            1 => policy.extra_inputs.reverse(),
            2 => policy.extra_inputs[0].name = "missing-rates".into(),
            3 => policy.extra_inputs[0].input.root_view_policy = true,
            4 => policy.extra_outputs.reverse(),
            5 => {
                policy.extra_outputs.pop();
            }
            6 => policy.extra_inputs.push(policy.extra_inputs[0].clone()),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                validate_program(&invalid),
                Err(ProgramValidationError::InvalidXmlBoundary { .. })
            ),
            "mutation {mutation}"
        );
    }
}
