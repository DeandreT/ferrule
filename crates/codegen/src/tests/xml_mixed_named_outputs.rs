use super::*;
use crate::{XmlInputProfile, XmlOutputMode};
use ir::Instance;

fn project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/static_primary_mixed_named_xml_documents.json"
    ))
    .unwrap()
}
fn source(
    primary_bad: bool,
    static_bad: bool,
    static_context: bool,
    rows: &[(&str, &str, f64, bool)],
) -> Instance {
    Instance::Group(
        vec![
            (
                "PrimaryText".into(),
                Instance::Scalar(Value::String("雪 & 😀".into())),
            ),
            (
                "PrimaryBad".into(),
                Instance::Scalar(Value::Bool(primary_bad)),
            ),
            (
                "StaticBad".into(),
                Instance::Scalar(Value::Bool(static_bad)),
            ),
            (
                "StaticContext".into(),
                Instance::Scalar(Value::Bool(static_context)),
            ),
            (
                "Row".into(),
                Instance::Repeated(
                    rows.iter()
                        .map(|(path, value, amount, context)| {
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
                                    ("Amount".into(), Instance::Scalar(Value::Float(*amount))),
                                    (
                                        "NeedContext".into(),
                                        Instance::Scalar(Value::Bool(*context)),
                                    ),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            ),
        ]
        .into(),
    )
}
fn value<'a>(instance: &'a Instance, field: &str) -> &'a Value {
    instance.field(field).and_then(Instance::as_scalar).unwrap()
}
fn ordinary_constants(project: &mut Project) {
    for (id, node) in &mut project.graph.nodes {
        *node = Node::Const {
            value: match *id {
                2 => Value::Float(-0.0),
                9 => Value::Int(9_007_199_254_740_993),
                _ => Value::String("constant.xml".into()),
            },
        };
    }
}

#[test]
fn exact_mixed_mode_owns_both_independent_policies_in_original_order() {
    for reverse in [false, true] {
        let mut project = project();
        if reverse {
            project.extra_targets.swap(0, 1);
        }
        assert!(engine::validate(&project).is_empty());
        let lowered = lower(&project).unwrap();
        assert_eq!(
            lowered.xml_output_mode(),
            Ok(Some(XmlOutputMode::StaticPrimaryMixedNamedXmlOutputs))
        );
        let policy = lowered.xml_boundary.as_ref().unwrap();
        assert_eq!(policy.input.profile(), Some(XmlInputProfile::Structured));
        assert!(policy.extra_inputs.is_empty());
        assert!(lowered.extra_sources.is_empty());
        for (index, (target, output)) in lowered
            .extra_targets
            .iter()
            .zip(&policy.extra_outputs)
            .enumerate()
        {
            assert_eq!(target.name, project.extra_targets[index].name);
            assert_eq!(output.name, target.name);
            assert_eq!(target.target, project.extra_targets[index].schema);
            assert_eq!(
                output.output.schema_hints,
                project.extra_targets[index].options.xml_schema_hints
            );
            assert_eq!(target.root.iteration.is_none(), target.name == "z-static");
        }
        assert_ne!(
            lowered.extra_targets[0].target.xml_namespace,
            lowered.extra_targets[1].target.xml_namespace
        );
        assert_ne!(
            policy.extra_outputs[0].output.schema_hints,
            policy.extra_outputs[1].output.schema_hints
        );
    }
}

#[test]
fn complete_native_outputs_keep_static_int_and_dynamic_paths_float_bits_and_empty_lists() {
    let samples = [
        Vec::new(),
        vec![
            ("  same.xml  ", "雪 & 😀", -0.0, false),
            ("  same.xml  ", "second", 2.5, false),
        ],
    ];
    for reverse in [false, true] {
        let mut project = project();
        if reverse {
            project.extra_targets.swap(0, 1);
        }
        for rows in &samples {
            let input = source(false, false, false, rows);
            let mapped = engine::run_outputs(&project, &input).unwrap();
            assert_eq!(
                value(&mapped.primary, "Marker"),
                &Value::String("雪 & 😀".into())
            );
            assert_eq!(
                value(&mapped.primary, "Echo"),
                value(&mapped.primary, "Marker")
            );
            for (index, output) in mapped.extras.iter().enumerate() {
                assert_eq!(output.name, project.extra_targets[index].name);
                if output.name == "z-static" {
                    assert!(matches!(output.instance, Instance::Group(_)));
                    assert_eq!(
                        value(&output.instance, "Label"),
                        &Value::String("receipt".into())
                    );
                    assert_eq!(
                        value(&output.instance, "Echo"),
                        value(&output.instance, "Label")
                    );
                    assert_eq!(
                        value(&output.instance, "Exact"),
                        &Value::Int(9_007_199_254_740_993)
                    );
                } else {
                    let Instance::DocumentSet(members) = &output.instance else {
                        panic!("dynamic declaration retains its list")
                    };
                    assert_eq!(members.len(), rows.len());
                    for (member, (path, text, amount, _)) in members.iter().zip(rows) {
                        assert_eq!(member.path(), *path);
                        assert_eq!(member.source_path(), *path);
                        assert_eq!(
                            value(member.value(), "Value"),
                            &Value::String((*text).into())
                        );
                        assert_eq!(
                            value(member.value(), "Echo"),
                            value(member.value(), "Value")
                        );
                        let Value::Float(actual) = value(member.value(), "Amount") else {
                            panic!("Float tag")
                        };
                        assert_eq!(actual.to_bits(), amount.to_bits());
                    }
                }
            }
        }
    }
}

#[test]
fn empty_members_and_unselected_branches_are_lazy_but_selected_context_is_exact() {
    let mut project = project();
    project.extra_targets.swap(0, 1);
    let empty = source(false, false, false, &[]);
    assert!(engine::run_outputs(&project, &empty).is_ok());
    let input = source(false, false, false, &[("opaque.xml", "ok", 1.0, true)]);
    assert!(matches!(
        engine::run_outputs(&project, &input),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let path = std::path::Path::new("context.xml");
    let missing = engine::ExecutionContext::new(path);
    assert!(matches!(
        engine::run_outputs_with_sources_and_context(&project, &input, Vec::new(), &missing),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let supplied = missing.with_current_datetime("2026-01-01T00:00:00Z");
    let output =
        engine::run_outputs_with_sources_and_context(&project, &input, Vec::new(), &supplied)
            .unwrap();
    let Instance::DocumentSet(members) = &output.extras[0].instance else {
        panic!("original reversed list")
    };
    assert_eq!(
        value(members[0].value(), "Value"),
        &Value::String("2026-01-01T00:00:00Z".into())
    );
}

#[test]
fn late_mapping_failure_wins_before_an_invalid_primary_or_named_writer() {
    for reverse in [false, true] {
        let mut project = project();
        if reverse {
            project.extra_targets.swap(0, 1);
        }
        let late_path = source(
            true,
            true,
            false,
            &[("first.xml", "bad", 1.0, false), ("", "ok", 2.0, false)],
        );
        assert!(matches!(
            engine::run_outputs(&project, &late_path),
            Err(engine::EngineError::EmptyDynamicTargetPath { node: 0 })
        ));
        let late_context = source(true, false, true, &[("first.xml", "bad", 1.0, false)]);
        assert!(matches!(
            engine::run_outputs(&project, &late_context),
            Err(engine::EngineError::MissingRuntimeValue(
                mapping::RuntimeValue::CurrentDateTime
            ))
        ));
        // The primary's invalid XML value is independently real, not an expected footer.
        let mut primary_only = project.clone();
        primary_only.extra_targets.clear();
        let primary = engine::run(&primary_only, &late_path).unwrap();
        assert_eq!(value(&primary, "Marker"), &Value::String("\u{1}".into()));
        assert!(format_xml::to_string(&project.target, &primary).is_err());
    }
}

#[test]
fn complete_policy_alignment_cannot_admit_a_partial_or_reordered_set() {
    let valid = lower(&project()).unwrap();
    for mutation in 0..4 {
        let mut invalid = valid.clone();
        let outputs = &mut invalid.xml_boundary.as_mut().unwrap().extra_outputs;
        match mutation {
            0 => outputs.swap(0, 1),
            1 => {
                outputs.pop();
            }
            2 => outputs[1].name = outputs[0].name.clone(),
            _ => outputs.push(outputs[0].clone()),
        }
        assert!(
            matches!(invalid.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason })
            if reason.contains("exact declaration order"))
        );
    }
}

#[test]
fn schema_valid_children_and_mismatched_labels_refuse_even_without_root_primitives() {
    for selected in 0..3 {
        let mut project = project();
        ordinary_constants(&mut project);
        let (schema, root) = if selected == 0 {
            (&mut project.target, &mut project.root)
        } else {
            let target = &mut project.extra_targets[selected - 1];
            (&mut target.schema, &mut target.root)
        };
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            panic!("group")
        };
        children.push(SchemaNode::group("Child", Vec::new()));
        root.children.push(Scope {
            target_field: "Child".into(),
            ..Default::default()
        });
        assert!(
            project
                .graph
                .nodes
                .values()
                .all(|node| matches!(node, Node::Const { .. }))
        );
        assert!(engine::validate(&project).is_empty());
        let mut lowered = lower(&project).unwrap();
        assert_eq!(lowered.xml_output_mode(), Ok(None));
        lowered.xml_boundary = lower(&self::project()).unwrap().xml_boundary;
        assert!(
            matches!(lowered.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("flat"))
        );
    }
    let valid = lower(&project()).unwrap();
    for selected in 0..3 {
        let mut empty_label = valid.clone();
        let root = if selected == 0 {
            &mut empty_label.root
        } else {
            &mut empty_label.extra_targets[selected - 1].root
        };
        root.target_field.clear();
        assert_eq!(
            empty_label.xml_output_mode(),
            Ok(Some(XmlOutputMode::StaticPrimaryMixedNamedXmlOutputs))
        );
        let mut mismatch = valid.clone();
        let root = if selected == 0 {
            &mut mismatch.root
        } else {
            &mut mismatch.extra_targets[selected - 1].root
        };
        root.target_field = "wrong-root".into();
        assert!(mismatch.xml_output_mode().is_err());
    }
}

#[test]
fn old_routes_and_out_of_scope_mixed_shapes_keep_their_modes_or_refusals() {
    let mut all_lists = project();
    all_lists.extra_targets[0].root.iteration = ScopeIteration::DynamicDocuments {
        source: vec!["Row".into()],
        output_path: 0,
    };
    assert_eq!(
        lower(&all_lists).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::StaticPrimaryDynamicNamedDocuments))
    );
    let mut all_static = project();
    all_static.extra_targets[1].root.iteration = ScopeIteration::None;
    ordinary_constants(&mut all_static);
    assert_eq!(
        lower(&all_static).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::SingleDocument))
    );
    let mut three = project();
    let mut third = three.extra_targets[0].clone();
    third.name = "third".into();
    three.extra_targets.push(third);
    assert_eq!(lower(&three).unwrap().xml_output_mode(), Ok(None));
    let mut named_input = project();
    named_input.extra_sources.push(mapping::NamedSource {
        name: "meta".into(),
        path: "meta.xml".into(),
        schema: SchemaNode::group("Meta", vec![SchemaNode::scalar("Tag", ScalarType::String)]),
        options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        dynamic_path: None,
    });
    assert_eq!(lower(&named_input).unwrap().xml_output_mode(), Ok(None));
    named_input.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 0,
        iteration: vec!["Row".into()],
    });
    assert_eq!(lower(&named_input).unwrap().xml_output_mode(), Ok(None));
    let mut scalar_driver = project();
    let SchemaKind::Group { children, .. } = &mut scalar_driver.source.kind else {
        panic!("input group")
    };
    children[4] = SchemaNode::scalar("Row", ScalarType::String).repeating();
    scalar_driver.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("item.xml".into()),
        },
    );
    scalar_driver.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("leaf".into()),
        },
    );
    scalar_driver.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Float(-0.0),
        },
    );
    scalar_driver.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    assert!(engine::validate(&scalar_driver).is_empty());
    assert_eq!(lower(&scalar_driver).unwrap().xml_output_mode(), Ok(None));
}

#[test]
fn actual_member_count_includes_both_static_artifacts_without_counting_envelopes_twice() {
    let project = project();
    for count in [4094, 4095] {
        let rows = vec![("same.xml", "tiny", 1.0, false); count];
        let mapped = engine::run_outputs(&project, &source(false, false, false, &rows)).unwrap();
        assert!(matches!(mapped.primary, Instance::Group(_)));
        assert!(matches!(mapped.extras[0].instance, Instance::Group(_)));
        let Instance::DocumentSet(members) = &mapped.extras[1].instance else {
            panic!("actual list")
        };
        assert_eq!(members.len(), count);
        assert_eq!(2 + members.len(), if count == 4094 { 4096 } else { 4097 });
    }
}

fn multiple_list_project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/static_primary_mixed_multiple_named_xml_documents.json"
    ))
    .unwrap()
}

fn multiple_list_source(
    primary_bad: bool,
    first: &[(&str, &str, f64, bool)],
    second: &[(&str, &str, f64, bool)],
) -> Instance {
    let first = source(primary_bad, false, false, first);
    let second = source(false, false, false, second);
    let Instance::Group(fields) = &first else {
        panic!("primary group")
    };
    let mut fields = fields
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<Vec<_>>();
    fields.push(("SecondRow".into(), second.field("Row").unwrap().clone()));
    Instance::Group(fields.into())
}

#[test]
fn multiple_mixed_lists_keep_independent_drivers_schemas_and_policy_order() {
    for reverse in [false, true] {
        let mut project = multiple_list_project();
        if reverse {
            project.extra_targets.reverse();
        }
        assert!(engine::validate(&project).is_empty());
        let lowered = lower(&project).unwrap();
        assert_eq!(
            lowered.xml_output_mode(),
            Ok(Some(XmlOutputMode::StaticPrimaryMixedNamedXmlOutputs))
        );
        let policy = lowered.xml_boundary.as_ref().unwrap();
        assert!(policy.extra_inputs.is_empty() && lowered.extra_sources.is_empty());
        for ((target, output), original) in lowered
            .extra_targets
            .iter()
            .zip(&policy.extra_outputs)
            .zip(&project.extra_targets)
        {
            assert_eq!(target.name, original.name);
            assert_eq!(target.target, original.schema);
            assert_eq!(output.name, original.name);
            assert_eq!(
                output.output.schema_hints,
                original.options.xml_schema_hints
            );
        }
        assert_eq!(
            lowered
                .extra_targets
                .iter()
                .filter(|target| target.root.iteration.is_none())
                .count(),
            1
        );
        let lists = project
            .extra_targets
            .iter()
            .filter(|target| {
                matches!(
                    target.root.iteration,
                    ScopeIteration::DynamicDocuments { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(lists.len(), 2);
        assert_ne!(lists[0].schema, lists[1].schema);
        assert_ne!(
            lists[0].options.xml_schema_hints,
            lists[1].options.xml_schema_hints
        );
    }
}

#[test]
fn multiple_native_lists_preserve_independent_empty_envelopes_paths_and_exact_values() {
    let first_rows = [("  same.xml  ", "雪 & 😀", -0.0, false)];
    let second_rows = [
        ("  same.xml  ", "second", -0.0, false),
        ("別/next.xml", "third", 2.5, false),
    ];
    for reverse in [false, true] {
        let mut project = multiple_list_project();
        if reverse {
            project.extra_targets.reverse();
        }
        for empty in 0..4 {
            let first = if empty & 1 == 0 {
                first_rows.as_slice()
            } else {
                &[]
            };
            let second = if empty & 2 == 0 {
                second_rows.as_slice()
            } else {
                &[]
            };
            let mapped =
                engine::run_outputs(&project, &multiple_list_source(false, first, second)).unwrap();
            assert_eq!(mapped.extras.len(), 3);
            let mut count = 1;
            for (output, target) in mapped.extras.iter().zip(&project.extra_targets) {
                assert_eq!(output.name, target.name);
                if matches!(target.root.iteration, ScopeIteration::None) {
                    count += 1;
                    assert_eq!(
                        value(&output.instance, "Exact"),
                        &Value::Int(9_007_199_254_740_993)
                    );
                } else {
                    let Instance::DocumentSet(members) = &output.instance else {
                        panic!("list envelope")
                    };
                    let (rows, field) = if target.name == "a-dynamic" {
                        (first, "Value")
                    } else {
                        (second, "Payload")
                    };
                    count += members.len();
                    assert_eq!(members.len(), rows.len());
                    for (member, (path, text, amount, _)) in members.iter().zip(rows) {
                        assert_eq!(member.path(), *path);
                        assert_eq!(member.source_path(), *path);
                        assert_eq!(value(member.value(), field), &Value::String((*text).into()));
                        let Value::Float(actual) = value(member.value(), "Amount") else {
                            panic!("Float tag")
                        };
                        assert_eq!(actual.to_bits(), amount.to_bits());
                        let xml = format_xml::to_string_with_options(
                            &target.schema,
                            member.value(),
                            &format_xml::XmlWriteOptions {
                                schema_hints: target.options.xml_schema_hints.clone(),
                                ..Default::default()
                            },
                        )
                        .unwrap();
                        let parsed = format_xml::from_str(&xml, &target.schema).unwrap();
                        assert_eq!(value(&parsed, field), value(member.value(), field));
                        assert!(xml.contains(if target.name == "a-dynamic" {
                            "literal-dynamic.xsd"
                        } else {
                            "literal-secondary.xsd"
                        }));
                    }
                }
            }
            assert_eq!(count, 2 + first.len() + second.len());
        }
    }
}

#[test]
fn late_second_list_mapping_refuses_before_earlier_invalid_xml_and_keeps_lazy_context() {
    for reverse in [false, true] {
        let mut project = multiple_list_project();
        if reverse {
            project.extra_targets.reverse();
        }
        let input = multiple_list_source(
            true,
            &[("first.xml", "bad", -0.0, false)],
            &[("second.xml", "ok", -0.0, false), ("", "late", 2.5, false)],
        );
        assert!(matches!(
            engine::run_outputs(&project, &input),
            Err(engine::EngineError::EmptyDynamicTargetPath { node: 19 })
        ));
        let lazy = multiple_list_source(false, &[], &[]);
        assert!(engine::run_outputs(&project, &lazy).is_ok());
        let selected = multiple_list_source(
            true,
            &[("first.xml", "bad", -0.0, false)],
            &[("second.xml", "ok", -0.0, true)],
        );
        assert!(matches!(
            engine::run_outputs(&project, &selected),
            Err(engine::EngineError::MissingRuntimeValue(
                mapping::RuntimeValue::CurrentDateTime
            ))
        ));
        let path = std::path::Path::new("context.xml");
        let execution =
            engine::ExecutionContext::new(path).with_current_datetime("2026-01-01T00:00:00Z");
        let mapped = engine::run_outputs_with_sources_and_context(
            &project,
            &selected,
            Vec::new(),
            &execution,
        )
        .unwrap();
        let output = mapped
            .extras
            .iter()
            .find(|output| output.name == "b-secondary")
            .unwrap();
        let Instance::DocumentSet(members) = &output.instance else {
            panic!("second list")
        };
        assert_eq!(
            value(members[0].value(), "Payload"),
            &Value::String("2026-01-01T00:00:00Z".into())
        );
        assert_eq!(
            value(&mapped.primary, "Marker"),
            &Value::String("\u{1}".into())
        );
        assert!(format_xml::to_string(&project.target, &mapped.primary).is_err());
    }
}
