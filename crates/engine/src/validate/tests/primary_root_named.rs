use super::*;

fn observed_one_static_named_root_project() -> Project {
    let mut project = valid_project();
    project.source = serde_json::from_str(r#"{"name":"Root","xml_namespace":{"kind":"unqualified"},"xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code"]},{"name":"Derived","members":["Code","Extra"]}]}}"#).unwrap();
    project.source_options = mapping::FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: true,
        ..Default::default()
    };
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    project.target_options = mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    project.graph = Graph {
        nodes: [
            (
                0,
                Node::SourceRootField {
                    path: vec!["Code".into()],
                    required: true,
                },
            ),
            (
                1,
                Node::SourceRootField {
                    path: vec!["Extra".into()],
                    required: true,
                },
            ),
            (
                2,
                Node::SourceRootXmlTypeEquals {
                    canonical_expanded_type: "Derived".into(),
                },
            ),
            (
                3,
                Node::Const {
                    value: Value::String("skipped".into()),
                },
            ),
            (
                4,
                Node::If {
                    condition: 2,
                    then: 1,
                    else_: 3,
                },
            ),
        ]
        .into(),
    };
    project.extra_targets = vec![mapping::NamedTarget {
        name: "audit".into(),
        path: None,
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 4,
            }],
            ..Scope::default()
        },
    }];
    project
}

fn observed_one_static_named_root_source(derived: bool, extra: Option<&str>) -> ir::Instance {
    let mut fields = vec![(
        "Code".into(),
        ir::Instance::Scalar(Value::String("primary owner 雪".into())),
    )];
    if let Some(extra) = extra {
        fields.push((
            "Extra".into(),
            ir::Instance::Scalar(Value::String(extra.into())),
        ));
    }
    ir::Instance::Group(
        ir::InstanceGroup::from(fields)
            .with_xml_type_origin(if derived {
                ir::XmlTypeOrigin::Explicit("Derived")
            } else {
                ir::XmlTypeOrigin::Absent
            })
            .unwrap(),
    )
}

#[test]
fn observed_one_static_named_root_readers_share_primary_owner_and_stay_lazy() {
    let project = observed_one_static_named_root_project();
    assert!(validate(&project).is_empty());
    for derived in [false, true] {
        let source =
            observed_one_static_named_root_source(derived, derived.then_some("derived extra"));
        let outputs = crate::run_outputs(&project, &source).unwrap();
        assert_eq!(
            outputs
                .primary
                .field("Value")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::String("primary owner 雪".into()))
        );
        assert_eq!(outputs.extras.len(), 1);
        assert_eq!(outputs.extras[0].name, "audit");
        assert_eq!(
            outputs.extras[0]
                .instance
                .field("Value")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::String(
                if derived { "derived extra" } else { "skipped" }.into()
            ))
        );
    }
    assert!(matches!(
        crate::run_outputs(&project, &observed_one_static_named_root_source(true, None)),
        Err(crate::EngineError::PrimaryRoot {
            node: 1,
            source: ir::PrimaryRootError::MissingRequiredField { path },
        }) if path == vec!["Extra".to_owned()]
    ));
}

#[test]
fn observed_static_named_roots_accept_own_schema_names_and_reject_other_names() {
    let mut project = observed_one_static_named_root_project();
    project.root.target_field = project.target.name.clone();
    project.extra_targets[0].schema.name = "Audit".into();
    project.extra_targets[0].root.target_field = "Audit".into();
    assert!(validate(&project).is_empty());
    let source = observed_one_static_named_root_source(true, Some("derived extra"));
    let outputs = crate::run_outputs(&project, &source).unwrap();
    assert_eq!(
        outputs
            .primary
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("primary owner 雪".into()))
    );
    assert_eq!(
        outputs.extras[0]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("derived extra".into()))
    );
    for (primary, name) in [
        (true, "descendant"),
        (false, "descendant"),
        (true, "Audit"),
        (false, "Target"),
    ] {
        let mut invalid = project.clone();
        if primary {
            invalid.root.target_field = name.into();
        } else {
            invalid.extra_targets[0].root.target_field = name.into();
        }
        assert!(validate(&invalid).iter().any(|issue| {
            issue
                .message
                .contains("primary-root primitive is unavailable")
        }));
    }
}

#[test]
fn observed_one_static_named_root_context_is_lazy_with_original_typed_failure() {
    let mut project = observed_one_static_named_root_project();
    project.graph.nodes.insert(
        5,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.graph.nodes.insert(
        4,
        Node::If {
            condition: 2,
            then: 5,
            else_: 3,
        },
    );
    assert!(validate(&project).is_empty());
    let base = crate::run_outputs(
        &project,
        &observed_one_static_named_root_source(false, None),
    )
    .unwrap();
    assert_eq!(
        base.extras[0]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("skipped".into()))
    );
    let source = observed_one_static_named_root_source(true, Some("present"));
    assert!(matches!(
        crate::run_outputs(&project, &source),
        Err(crate::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let context = crate::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_current_datetime("2026-10-05T12:00:00Z");
    let outputs =
        crate::run_outputs_with_sources_and_context(&project, &source, Vec::new(), &context)
            .unwrap();
    assert_eq!(
        outputs.extras[0]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("2026-10-05T12:00:00Z".into()))
    );
}

#[test]
fn observed_one_static_named_root_admission_preserves_other_ownership_refusals() {
    let valid = observed_one_static_named_root_project();
    for mutation in 0..19 {
        let mut project = valid.clone();
        match mutation {
            0 => project.source_options.xml_root_view_read_policy = false,
            1 | 18 => project.extra_sources.push(NamedSource {
                name: "reference".into(),
                path: "reference.xml".into(),
                schema: SchemaNode::group(
                    "Reference",
                    vec![SchemaNode::scalar("Code", ScalarType::String)],
                ),
                options: Default::default(),
                dynamic_path: (mutation == 18).then_some(DynamicSourcePath {
                    node: 0,
                    iteration: vec![],
                }),
            }),
            2 => {
                let mut other = project.extra_targets[0].clone();
                other.name = "other".into();
                other.root.target_field = "descendant".into();
                project.extra_targets.push(other);
            }
            3 => project.extra_targets[0].root.set_source(Some(vec![])),
            4 => project.root.set_source(Some(vec![])),
            5 => project.extra_targets[0].schema.repeating = true,
            6 => project.target.repeating = true,
            7 => project.extra_targets[0].root.children.push(Scope {
                target_field: "Value".into(),
                bindings: project.root.bindings.clone(),
                ..Scope::default()
            }),
            8 => project.root.children.push(Scope {
                target_field: "Value".into(),
                bindings: project.root.bindings.clone(),
                ..Scope::default()
            }),
            9 => project.extra_targets[0].root.filter = Some(2),
            10 => project.extra_targets[0]
                .root
                .dynamic_bindings
                .push(DynamicBinding { key: 0, value: 1 }),
            11 => project.extra_targets[0].options = Default::default(),
            12 => project.target_options = Default::default(),
            13 => {
                project.extra_targets[0].schema = SchemaNode::scalar("Target", ScalarType::String)
            }
            14 => project.extra_targets[0].root.construction = ScopeConstruction::CopyCurrentSource,
            15 => project.source_options.xml_allow_inactive_root_type_members = false,
            16 => project
                .root
                .dynamic_bindings
                .push(DynamicBinding { key: 0, value: 1 }),
            17 => project.extra_targets[0].root.target_field = "descendant".into(),
            _ => unreachable!(),
        }
        assert!(
            validate(&project).iter().any(|issue| issue
                .message
                .contains("primary-root primitive is unavailable")),
            "mutation {mutation} granted named reader ownership"
        );
    }
    // The former static-input route remains valid when no named output consumes
    // a root primitive; the new exception never combines these routes.
    let mut old = valid;
    old.extra_targets.clear();
    old.extra_sources.push(NamedSource {
        name: "reference".into(),
        path: "reference.xml".into(),
        schema: SchemaNode::group(
            "Reference",
            vec![SchemaNode::scalar("Code", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    assert!(validate(&old).is_empty());
}

fn observed_plural_static_named_root_project(count: usize) -> Project {
    let mut project = observed_one_static_named_root_project();
    project.root.target_field = project.target.name.clone();
    let template = project.extra_targets[0].clone();
    project.extra_targets = ["z-audit", "a-summary", "m-receipt"]
        .into_iter()
        .zip(["Audit", "Summary", "Receipt"])
        .take(count)
        .enumerate()
        .map(|(index, (name, schema_name))| {
            let mut target = template.clone();
            target.name = name.into();
            target.schema.name = schema_name.into();
            target.root.target_field = schema_name.into();
            // Only the later roots own the lazy required Extra read.
            target.root.bindings[0].node = if index == 0 { 0 } else { 4 };
            target
        })
        .collect();
    project
}

#[test]
fn observed_plural_static_named_roots_keep_order_primary_owner_and_late_laziness() {
    for count in [2, 3] {
        for reverse in [false, true] {
            let mut project = observed_plural_static_named_root_project(count);
            if reverse {
                project.extra_targets.reverse();
            }
            assert!(validate(&project).is_empty());
            for derived in [false, true] {
                let source = observed_one_static_named_root_source(
                    derived,
                    derived.then_some("derived extra"),
                );
                let outputs = crate::run_outputs(&project, &source).unwrap();
                assert_eq!(outputs.extras.len(), count);
                assert_eq!(
                    outputs
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
                for (output, target) in outputs.extras.iter().zip(&project.extra_targets) {
                    let expected = if target.root.bindings[0].node == 0 {
                        "primary owner 雪"
                    } else if derived {
                        "derived extra"
                    } else {
                        "skipped"
                    };
                    assert_eq!(
                        output
                            .instance
                            .field("Value")
                            .and_then(ir::Instance::as_scalar),
                        Some(&Value::String(expected.into()))
                    );
                }
            }
            assert!(matches!(
                crate::run_outputs(&project, &observed_one_static_named_root_source(true, None)),
                Err(crate::EngineError::PrimaryRoot {
                    node: 1,
                    source: ir::PrimaryRootError::MissingRequiredField { path },
                }) if path == vec!["Extra".to_owned()]
            ));
            project.root.target_field.clear();
            for target in &mut project.extra_targets {
                target.root.target_field.clear();
            }
            assert!(validate(&project).is_empty());
        }
    }
}

#[test]
fn observed_plural_last_named_context_is_lazy_and_uses_the_supplied_context() {
    let mut project = observed_plural_static_named_root_project(2);
    project.graph.nodes.insert(
        5,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.graph.nodes.insert(
        4,
        Node::If {
            condition: 2,
            then: 5,
            else_: 3,
        },
    );
    assert!(validate(&project).is_empty());
    let base = crate::run_outputs(
        &project,
        &observed_one_static_named_root_source(false, None),
    )
    .unwrap();
    assert_eq!(
        base.extras[1]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("skipped".into()))
    );
    let derived = observed_one_static_named_root_source(true, Some("present"));
    assert!(matches!(
        crate::run_outputs(&project, &derived),
        Err(crate::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let context = crate::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_current_datetime("2026-10-05T12:00:00Z");
    let outputs =
        crate::run_outputs_with_sources_and_context(&project, &derived, Vec::new(), &context)
            .unwrap();
    assert_eq!(
        outputs.extras[1]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("2026-10-05T12:00:00Z".into()))
    );
}

#[test]
fn observed_plural_named_reader_permission_rejects_mismatch_and_invalid_consumers() {
    let valid = observed_plural_static_named_root_project(3);
    for mutation in 0..9 {
        let mut invalid = valid.clone();
        match mutation {
            0 => invalid.extra_targets[2].root.target_field = "Audit".into(),
            1 => invalid.root.target_field = "descendant".into(),
            2 => invalid.extra_targets[2].root.set_source(Some(vec![])),
            3 => invalid.extra_targets[2].schema.repeating = true,
            4 => {
                let bindings = invalid.extra_targets[2].root.bindings.clone();
                invalid.extra_targets[2].root.children.push(Scope {
                    target_field: "Value".into(),
                    bindings,
                    ..Scope::default()
                });
            }
            5 => invalid.extra_targets[2].root.filter = Some(2),
            6 => invalid.extra_targets[2]
                .root
                .dynamic_bindings
                .push(DynamicBinding { key: 0, value: 1 }),
            7 => invalid.extra_targets[2].root.construction = ScopeConstruction::CopyCurrentSource,
            8 => invalid.extra_sources.push(NamedSource {
                name: "secondary".into(),
                path: "secondary.xml".into(),
                schema: valid.source.clone(),
                options: Default::default(),
                dynamic_path: None,
            }),
            _ => unreachable!(),
        }
        assert!(
            validate(&invalid).iter().any(|issue| issue
                .message
                .contains("primary-root primitive is unavailable")),
            "mutation {mutation}"
        );
    }
}

fn observed_combined_static_named_root_project(count: usize) -> Project {
    let mut project = observed_plural_static_named_root_project(count);
    project.extra_sources.push(NamedSource {
        name: "labels".into(), path: "labels.xml".into(), dynamic_path: None,
        schema: serde_json::from_str(r#"{"name":"Labels","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"ReadExtra","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"bool"}},{"name":"NeedContext","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"bool"}}]}}"#).unwrap(),
        options: mapping::FormatOptions { xml_document: true, ..Default::default() },
    });
    for (id, field) in [
        (5, "Code"),
        (6, "Extra"),
        (7, "ReadExtra"),
        (11, "NeedContext"),
    ] {
        project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec!["labels".into(), field.into()],
                frame: None,
            },
        );
    }
    project.graph.nodes.insert(
        8,
        Node::If {
            condition: 7,
            then: 1,
            else_: 3,
        },
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
            condition: 11,
            then: 9,
            else_: 3,
        },
    );
    if let ir::SchemaKind::Group { children, .. } = &mut project.target.kind {
        children.extend([
            SchemaNode::scalar("NamedCode", ScalarType::String),
            SchemaNode::scalar("NamedExtra", ScalarType::String),
        ]);
    }
    project.root.bindings.extend([
        Binding {
            target_field: "NamedCode".into(),
            node: 5,
        },
        Binding {
            target_field: "NamedExtra".into(),
            node: 6,
        },
    ]);
    project.extra_targets.last_mut().unwrap().root.bindings[0].node = 8;
    project
}

fn observed_combined_secondary(
    read_extra: bool,
    need_context: bool,
) -> Vec<(String, ir::Instance)> {
    vec![(
        "labels".into(),
        ir::Instance::Group(
            vec![
                (
                    "Code".into(),
                    ir::Instance::Scalar(Value::String("secondary-code".into())),
                ),
                (
                    "Extra".into(),
                    ir::Instance::Scalar(Value::String("secondary-extra".into())),
                ),
                (
                    "ReadExtra".into(),
                    ir::Instance::Scalar(Value::Bool(read_extra)),
                ),
                (
                    "NeedContext".into(),
                    ir::Instance::Scalar(Value::Bool(need_context)),
                ),
            ]
            .into(),
        ),
    )]
}

#[test]
fn observed_combined_named_roots_preserve_primary_owner_beside_colliding_named_fields() {
    let context = crate::ExecutionContext::new(std::path::Path::new("combined.json"));
    for count in [1, 2] {
        for reversed in [false, true] {
            let mut project = observed_combined_static_named_root_project(count);
            if reversed {
                project.extra_targets.reverse();
            }
            assert!(validate(&project).is_empty());
            let source = observed_one_static_named_root_source(true, Some("primary-extra"));
            let outputs = crate::run_outputs_with_sources_and_context(
                &project,
                &source,
                observed_combined_secondary(true, false),
                &context,
            )
            .unwrap();
            assert_eq!(
                outputs
                    .primary
                    .field("Value")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("primary owner 雪".into()))
            );
            assert_eq!(
                outputs
                    .primary
                    .field("NamedCode")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("secondary-code".into()))
            );
            assert_eq!(
                outputs
                    .primary
                    .field("NamedExtra")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("secondary-extra".into()))
            );
            assert_eq!(
                outputs
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
            let read = outputs
                .extras
                .iter()
                .zip(&project.extra_targets)
                .find(|(_, target)| target.root.bindings[0].node == 8)
                .unwrap()
                .0;
            assert_eq!(
                read.instance
                    .field("Value")
                    .and_then(ir::Instance::as_scalar),
                Some(&Value::String("primary-extra".into()))
            );
            assert_eq!(
                source.xml_type_origin(),
                Ok(ir::XmlTypeOrigin::Explicit("Derived"))
            );
        }
    }
}

#[test]
fn observed_combined_named_condition_keeps_required_reads_and_context_lazy() {
    let mut project = observed_combined_static_named_root_project(2);
    let source = observed_one_static_named_root_source(true, None);
    let context = crate::ExecutionContext::new(std::path::Path::new("combined.json"));
    let lazy = crate::run_outputs_with_sources_and_context(
        &project,
        &source,
        observed_combined_secondary(false, false),
        &context,
    )
    .unwrap();
    assert_eq!(
        lazy.extras[1]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("skipped".into()))
    );
    assert!(
        matches!(crate::run_outputs_with_sources_and_context(&project, &source, observed_combined_secondary(true, false), &context),
        Err(crate::EngineError::PrimaryRoot { node: 1, source: ir::PrimaryRootError::MissingRequiredField { path } }) if path == vec!["Extra".to_owned()])
    );
    project.extra_targets[1].root.bindings[0].node = 10;
    assert!(validate(&project).is_empty());
    let lazy = crate::run_outputs_with_sources_and_context(
        &project,
        &source,
        observed_combined_secondary(false, false),
        &context,
    )
    .unwrap();
    assert_eq!(
        lazy.extras[1]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("skipped".into()))
    );
    assert!(matches!(
        crate::run_outputs_with_sources_and_context(
            &project,
            &source,
            observed_combined_secondary(false, true),
            &context
        ),
        Err(crate::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let supplied = context.with_current_datetime("2026-10-05T12:00:00Z");
    let outputs = crate::run_outputs_with_sources_and_context(
        &project,
        &source,
        observed_combined_secondary(false, true),
        &supplied,
    )
    .unwrap();
    assert_eq!(
        outputs.extras[1]
            .instance
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("2026-10-05T12:00:00Z".into()))
    );
}

#[test]
fn observed_combined_named_roots_do_not_grant_descendant_control_or_private_ownership() {
    let valid = observed_combined_static_named_root_project(2);
    for mutation in 0..10 {
        let mut invalid = valid.clone();
        match mutation {
            0 => invalid.extra_sources[0].options.xml_document = false,
            1 => invalid.extra_sources[0].schema = valid.source.clone(),
            2 => {
                invalid.extra_sources[0].dynamic_path = Some(DynamicSourcePath {
                    node: 3,
                    iteration: Vec::new(),
                })
            }
            3 => invalid.extra_targets[1].root.target_field = "Audit".into(),
            4 => invalid.extra_targets[1].root.filter = Some(2),
            5 => invalid.extra_targets[1].root.children.push(Scope {
                target_field: "Value".into(),
                bindings: invalid.root.bindings.clone(),
                ..Scope::default()
            }),
            6 => invalid
                .root
                .dynamic_bindings
                .push(DynamicBinding { key: 0, value: 1 }),
            7 => invalid.extra_targets[1].schema.repeating = true,
            8 => invalid.extra_targets[1].root.set_source(Some(vec![])),
            9 => {
                invalid.graph.nodes.insert(
                    12,
                    Node::Aggregate {
                        function: mapping::AggregateOp::Count,
                        collection: Vec::new(),
                        value: Vec::new(),
                        expression: Some(1),
                        arg: None,
                    },
                );
            }
            _ => unreachable!(),
        }
        assert!(
            validate(&invalid).iter().any(|issue| issue
                .message
                .contains("primary-root primitive is unavailable")),
            "mutation {mutation}"
        );
    }
}
