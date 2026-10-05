use super::*;
use crate::{Program, XmlInputProfile, XmlOutputMode};
use ir::Instance;
use std::{cell::RefCell, path::Path, sync::Arc};

fn project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/one_dynamic_named_input_static_primary_dynamic_named_xml_documents.json"
    ))
    .unwrap()
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn scalar_value(value: Value) -> Instance {
    Instance::Scalar(value)
}
fn source(paths: &[&str], beta_path: &str) -> Instance {
    group(vec![
        ("UseContext", scalar_value(Value::Bool(false))),
        (
            "LoadRow",
            Instance::Repeated(
                ["a.xml", "b.xml"]
                    .into_iter()
                    .map(|path| {
                        group(vec![(
                            "InputFile",
                            scalar_value(Value::String(path.into())),
                        )])
                    })
                    .collect(),
            ),
        ),
        (
            "OutputRow",
            Instance::Repeated(
                paths
                    .iter()
                    .enumerate()
                    .map(|(index, path)| {
                        group(vec![
                            ("Id", scalar_value(Value::Int(index as i64 + 1))),
                            ("OutputFile", scalar_value(Value::String((*path).into()))),
                            ("BetaFile", scalar_value(Value::String(beta_path.into()))),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}
fn inputs() -> Vec<(String, Instance)> {
    vec![
        (
            "unused".into(),
            group(vec![(
                "Stamp",
                scalar_value(Value::String("unused".into())),
            )]),
        ),
        (
            "rates".into(),
            group(vec![("Factor", scalar_value(Value::Float(2.0)))]),
        ),
    ]
}
struct Loader {
    calls: RefCell<Vec<(String, String)>>,
    fail_second: bool,
}
impl engine::DynamicSourceLoader for Loader {
    fn load(&self, name: &str, path: &str) -> Result<Arc<Instance>, String> {
        self.calls.borrow_mut().push((name.into(), path.into()));
        if self.fail_second && path == "b.xml" {
            return Err("original host marker".into());
        }
        let (code, amount) = if path == "a.xml" {
            (9_007_199_254_740_993, -0.0)
        } else {
            (7, 1.5)
        };
        Ok(Arc::new(group(vec![(
            "Entry",
            Instance::Repeated(vec![group(vec![
                ("Code", scalar_value(Value::Int(code))),
                ("Amount", scalar_value(Value::Float(amount))),
                ("Text", scalar_value(Value::String("  雪😀  ".into()))),
            ])]),
        )])))
    }
}
fn assert_new_mode(program: &Program) {
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments
        ))
    );
}

#[test]
fn dynamic_input_and_all_static_and_output_policies_keep_original_declarations() {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let program = lower(&project).unwrap();
    assert_new_mode(&program);
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(policy.input.profile(), Some(XmlInputProfile::Structured));
    assert_eq!(
        policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog", "unused"]
    );
    for (index, (source, input)) in program
        .extra_sources
        .iter()
        .zip(&policy.extra_inputs)
        .enumerate()
    {
        assert_eq!(source.name, project.extra_sources[index].name);
        assert_eq!(source.source, project.extra_sources[index].schema);
        assert_eq!(input.input.profile(), Some(XmlInputProfile::Structured));
        assert_eq!(source.dynamic.is_some(), index == 1);
    }
    assert_eq!(
        policy.output.schema_hints,
        project.target_options.xml_schema_hints
    );
    for (index, (target, output)) in program
        .extra_targets
        .iter()
        .zip(&policy.extra_outputs)
        .enumerate()
    {
        assert_eq!(target.name, output.name);
        assert_eq!(target.target, project.extra_targets[index].schema);
        assert_eq!(
            output.output.schema_hints,
            project.extra_targets[index].options.xml_schema_hints
        );
    }
    let mut reversed = project.clone();
    reversed.extra_targets.reverse();
    let reversed_program = lower(&reversed).unwrap();
    assert_new_mode(&reversed_program);
    for (target, policy) in reversed_program.extra_targets.iter().zip(
        &reversed_program
            .xml_boundary
            .as_ref()
            .unwrap()
            .extra_outputs,
    ) {
        let original = project
            .extra_targets
            .iter()
            .find(|candidate| candidate.name == target.name)
            .unwrap();
        assert_eq!(target.target, original.schema);
        assert_eq!(
            policy.output.schema_hints,
            original.options.xml_schema_hints
        );
    }
}

#[test]
fn optional_statics_and_single_named_target_do_not_change_the_dynamic_mode() {
    let mut project = project();
    project
        .extra_sources
        .retain(|source| source.dynamic_path.is_some());
    project.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::Float(2.0),
        },
    );
    project.extra_targets.truncate(1);
    assert!(engine::validate(&project).is_empty());
    assert_new_mode(&lower(&project).unwrap());
}

#[test]
fn second_dynamic_input_is_refused_in_this_static_primary_branch() {
    let mut project = project();
    let mut second = project.extra_sources[1].clone();
    second.name = "second_catalog".into();
    project.extra_sources.push(second);
    assert!(engine::validate(&project).is_empty());
    assert_eq!(lower(&project).unwrap().xml_output_mode(), Ok(None));
    let mut program = lower(&self::project()).unwrap();
    program.extra_sources[2].dynamic = program.extra_sources[1].dynamic.clone();
    assert!(
        matches!(program.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason })
        if reason.contains("at most one dynamic named input"))
    );
}

#[test]
fn every_named_target_requires_a_primary_repeating_group_document_driver() {
    let valid = lower(&project()).unwrap();
    for path in [
        Vec::new(),
        vec!["catalog".into(), "Entry".into()],
        vec!["OutputRow".into(), "Id".into()],
    ] {
        let mut bad = valid.clone();
        bad.extra_targets[1].root.iteration = Some(IterationPlan::dynamic_documents(path, 9));
        for node in &mut bad.expressions {
            if matches!(node.id, 1 | 9) {
                node.expression = Expression::Const {
                    value: if node.id == 1 {
                        Value::Int(1)
                    } else {
                        Value::String("beta.xml".into())
                    },
                };
            }
        }
        assert!(
            matches!(bad.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason })
            if reason.contains("beta") && reason.contains("repeating Group"))
        );
    }
    let mut static_named = valid.clone();
    static_named.extra_targets[1].root.iteration = None;
    for node in &mut static_named.expressions {
        if matches!(node.id, 1 | 9) {
            node.expression = Expression::Const {
                value: if node.id == 1 {
                    Value::Int(1)
                } else {
                    Value::String("beta.xml".into())
                },
            };
        }
    }
    assert!(
        matches!(static_named.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason })
        if reason.contains("beta") && reason.contains("dynamic-document root"))
    );
    let mut iterating_primary = valid;
    iterating_primary.root.iteration = Some(IterationPlan::dynamic_documents(
        vec!["OutputRow".into()],
        0,
    ));
    assert!(matches!(
        iterating_primary.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
}

#[test]
fn complete_policy_alignment_and_observed_and_advanced_inputs_remain_strict() {
    let valid = lower(&project()).unwrap();
    for input in 0..3 {
        let mut bad = valid.clone();
        bad.xml_boundary.as_mut().unwrap().extra_inputs[input].name = "wrong".into();
        assert!(matches!(
            bad.xml_output_mode(),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        ));
    }
    let mut bad = valid.clone();
    bad.xml_boundary.as_mut().unwrap().extra_outputs.swap(0, 1);
    assert!(matches!(
        bad.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
    let mut unsupported = project();
    let SchemaKind::Group { children, .. } = &mut unsupported.extra_sources[2].schema.kind else {
        unreachable!()
    };
    children[0].default = Some("default".into());
    assert!(children[0].metadata_is_valid());
    assert!(unsupported.extra_sources[2].schema.metadata_is_valid());
    assert_eq!(lower(&unsupported).unwrap().xml_output_mode(), Ok(None));
    let mut observed = project();
    observed.source_options.xml_allow_inactive_root_type_members = true;
    observed.source_options.xml_root_view_read_policy = true;
    assert!(lower(&observed).is_err());
}

#[test]
fn native_mapping_preserves_order_numeric_tags_and_full_repeated_loader_drivers() {
    for reverse in [false, true] {
        let mut project = project();
        if reverse {
            project.extra_targets.reverse();
        }
        let loader = Loader {
            calls: RefCell::new(Vec::new()),
            fail_second: false,
        };
        let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
            .with_dynamic_source_loader(&loader);
        let outputs = engine::run_outputs_with_sources_and_context(
            &project,
            &source(&["../雪.xml", "../雪.xml"], "/absolute.xml"),
            inputs(),
            &execution,
        )
        .unwrap();
        assert_eq!(
            outputs.primary,
            group(vec![(
                "Marker",
                scalar_value(Value::String("complete".into()))
            )])
        );
        assert_eq!(
            outputs
                .extras
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            if reverse {
                vec!["beta", "alpha"]
            } else {
                vec!["alpha", "beta"]
            }
        );
        for output in &outputs.extras {
            let Instance::DocumentSet(members) = &output.instance else {
                panic!("named documents")
            };
            assert_eq!(members.len(), 2);
            for (index, member) in members.iter().enumerate() {
                assert_eq!(
                    member.path(),
                    if output.name == "alpha" {
                        "../雪.xml"
                    } else {
                        "/absolute.xml"
                    }
                );
                assert_eq!(
                    member.value().field("Id").and_then(Instance::as_scalar),
                    Some(&Value::Int(index as i64 + 1))
                );
                let Some(Instance::Repeated(lines)) = member.value().field("Line") else {
                    panic!("line repetition")
                };
                assert_eq!(lines.len(), 2);
                for (line, (code, amount)) in lines
                    .iter()
                    .zip([(9_007_199_254_740_993, -0.0f64), (7, 1.5f64)])
                {
                    assert_eq!(
                        line.field("Text").and_then(Instance::as_scalar),
                        Some(&Value::String("  雪😀  ".into()))
                    );
                    if output.name == "beta" {
                        assert_eq!(
                            line.field("Code").and_then(Instance::as_scalar),
                            Some(&Value::Int(code))
                        );
                    } else {
                        let Some(Value::Float(actual)) =
                            line.field("Amount").and_then(Instance::as_scalar)
                        else {
                            panic!("Float amount")
                        };
                        let Some(Value::Float(scaled)) =
                            line.field("Scaled").and_then(Instance::as_scalar)
                        else {
                            panic!("Float scaled")
                        };
                        assert_eq!(actual.to_bits(), amount.to_bits());
                        assert_eq!(scaled.to_bits(), (amount * 2.0).to_bits());
                    }
                }
            }
        }
        assert_eq!(
            loader.calls.borrow().as_slice(),
            ["a.xml", "b.xml"]
                .repeat(4)
                .into_iter()
                .map(|path| ("catalog".into(), path.into()))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn native_empty_named_lists_keep_primary_and_do_not_reach_loader_scopes() {
    let loader = Loader {
        calls: RefCell::new(Vec::new()),
        fail_second: true,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&loader);
    let outputs = engine::run_outputs_with_sources_and_context(
        &project(),
        &source(&[], "unused.xml"),
        inputs(),
        &execution,
    )
    .unwrap();
    assert_eq!(
        outputs.primary,
        group(vec![(
            "Marker",
            scalar_value(Value::String("complete".into()))
        )])
    );
    assert_eq!(outputs.extras.len(), 2);
    assert!(
        outputs
            .extras
            .iter()
            .all(|output| output.instance == Instance::DocumentSet(Vec::new()))
    );
    assert!(loader.calls.borrow().is_empty());
}

#[test]
fn native_host_failure_and_late_mapping_error_return_no_partial_outputs() {
    let loader = Loader {
        calls: RefCell::new(Vec::new()),
        fail_second: true,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&loader);
    assert!(
        matches!(engine::run_outputs_with_sources_and_context(&project(), &source(&["one.xml"], "beta.xml"), inputs(), &execution),
        Err(engine::EngineError::DynamicSourceLoad { source_name, path, message })
        if source_name == "catalog" && path == "b.xml" && message == "original host marker")
    );
    assert_eq!(loader.calls.borrow().len(), 2);
    let mut project = project();
    // Invalid writer text is valid graph data; whole mapping must fail on the later path first.
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    project.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    let loader = Loader {
        calls: RefCell::new(Vec::new()),
        fail_second: false,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&loader);
    assert!(matches!(
        engine::run_outputs_with_sources_and_context(
            &project,
            &source(&["one.xml", "two.xml"], ""),
            inputs(),
            &execution
        ),
        Err(engine::EngineError::EmptyDynamicTargetPath { node: 9 })
    ));
    assert_eq!(loader.calls.borrow().len(), 6);
}

#[test]
fn native_context_reads_are_lazy_and_use_real_timestamp_and_typed_parameter_values() {
    let mut project = project();
    project.graph.nodes.insert(
        10,
        Node::SourceField {
            path: vec!["UseContext".into()],
            frame: None,
        },
    );
    project.graph.nodes.insert(
        11,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.graph.nodes.insert(
        12,
        Node::RuntimeParameter {
            name: "label".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    project.graph.nodes.insert(
        13,
        Node::Call {
            function: "concat".into(),
            args: vec![11, 12],
        },
    );
    project.graph.nodes.insert(
        14,
        Node::Const {
            value: Value::String("complete".into()),
        },
    );
    project.graph.nodes.insert(
        4,
        Node::If {
            condition: 10,
            then: 13,
            else_: 14,
        },
    );
    assert!(engine::validate(&project).is_empty());
    assert_new_mode(&lower(&project).unwrap());
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"));
    let lazy = engine::run_outputs_with_sources_and_context(
        &project,
        &source(&[], "unused.xml"),
        inputs(),
        &execution,
    )
    .unwrap();
    assert_eq!(
        lazy.primary.field("Marker").and_then(Instance::as_scalar),
        Some(&Value::String("complete".into()))
    );
    let selected = group(vec![
        ("UseContext", scalar_value(Value::Bool(true))),
        ("LoadRow", Instance::Repeated(Vec::new())),
        ("OutputRow", Instance::Repeated(Vec::new())),
    ]);
    assert!(matches!(
        engine::run_outputs_with_sources_and_context(&project, &selected, inputs(), &execution),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    ));
    let mut parameters = engine::RuntimeParameters::default();
    parameters
        .insert("label", Value::String(" 雪".into()))
        .unwrap();
    let supplied = execution
        .with_current_datetime("2026-01-02T03:04:05Z")
        .with_parameters(&parameters);
    let outputs =
        engine::run_outputs_with_sources_and_context(&project, &selected, inputs(), &supplied)
            .unwrap();
    assert_eq!(
        outputs
            .primary
            .field("Marker")
            .and_then(Instance::as_scalar),
        Some(&Value::String("2026-01-02T03:04:05Z 雪".into()))
    );
}
