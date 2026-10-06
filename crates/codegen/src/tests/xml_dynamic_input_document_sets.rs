use super::*;
use crate::{XmlInputProfile, XmlOutputMode};
use ir::Instance;
use std::{cell::RefCell, path::Path, sync::Arc};

fn project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/one_dynamic_named_input_dynamic_primary_xml_documents.json"
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
fn value(value: Value) -> Instance {
    Instance::Scalar(value)
}
fn source(paths: &[&str], nil: bool) -> Instance {
    group(vec![
        (
            "OutputRow",
            Instance::Repeated(
                paths
                    .iter()
                    .enumerate()
                    .map(|(index, path)| {
                        group(vec![
                            ("Id", value(Value::Int(index as i64 + 1))),
                            ("File", value(Value::String((*path).into()))),
                            ("Keep", value(Value::Bool(true))),
                            ("Rank", value(Value::Int(index as i64 + 1))),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "LoadRow",
            Instance::Repeated(
                [
                    Value::String("a.xml".into()),
                    Value::String("b.xml".into()),
                    if nil { Value::xml_nil() } else { Value::Null },
                ]
                .into_iter()
                .enumerate()
                .map(|(index, path)| {
                    group(vec![
                        ("Id", value(Value::Int(91 + index as i64))),
                        ("Path", value(path)),
                    ])
                })
                .collect(),
            ),
        ),
    ])
}
fn inputs() -> Vec<(String, Instance)> {
    // Host transport order differs from the complete declaration order rates/catalog/labels.
    vec![
        (
            "labels".into(),
            group(vec![("Marker", value(Value::String("雪😀".into())))]),
        ),
        (
            "rates".into(),
            group(vec![("Factor", value(Value::Float(2.0)))]),
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
        let amount = if path == "a.xml" {
            9_007_199_254_740_992.0
        } else {
            -0.0
        };
        Ok(Arc::new(group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![
                ("Name", value(Value::String(path.into()))),
                ("Amount", value(Value::Float(amount))),
                ("Adjustment", value(Value::xml_nil())),
                ("Text", value(Value::String("  text 雪😀  ".into()))),
            ])]),
        )])))
    }
}

#[test]
fn one_dynamic_policy_and_every_static_schema_keep_original_declaration_ownership() {
    let project = project();
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
        ))
    );
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(policy.input.profile(), Some(XmlInputProfile::Structured));
    assert_eq!(
        policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog", "labels"]
    );
    assert!(policy.extra_outputs.is_empty());
    for (index, (input, declared)) in program
        .extra_sources
        .iter()
        .zip(&policy.extra_inputs)
        .enumerate()
    {
        assert_eq!(input.name, declared.name);
        assert_eq!(input.source, project.extra_sources[index].schema);
        assert_eq!(declared.input.profile(), Some(XmlInputProfile::Structured));
        assert_eq!(input.dynamic.is_some(), index == 1);
    }
    assert_eq!(
        policy.output.schema_hints,
        project.target_options.xml_schema_hints
    );
}

#[test]
fn static_inputs_are_optional_and_a_second_dynamic_declaration_keeps_the_xml_adapter() {
    let mut only_dynamic = project();
    only_dynamic
        .extra_sources
        .retain(|source| source.dynamic_path.is_some());
    only_dynamic.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::Float(2.0),
        },
    );
    only_dynamic.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::String("literal".into()),
        },
    );
    assert_eq!(
        lower(&only_dynamic).unwrap().xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
        ))
    );
    let mut multiple = project();
    let mut second = multiple.extra_sources[1].clone();
    second.name = "second_catalog".into();
    multiple.extra_sources.push(second);
    let lowered = lower(&multiple).unwrap();
    assert_eq!(lowered.extra_sources.len(), 4);
    assert_eq!(
        lowered.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
        ))
    );
}

#[test]
fn existing_single_document_mode_still_admits_multiple_dynamic_declarations() {
    let mut project = project();
    project.root.iteration = ScopeIteration::None;
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(1),
        },
    );
    let mut second = project.extra_sources[1].clone();
    second.name = "second_catalog".into();
    project.extra_sources.push(second);
    assert_eq!(
        lower(&project).unwrap().xml_output_mode(),
        Ok(Some(XmlOutputMode::SingleDocument))
    );
}

#[test]
fn named_outputs_and_nonprimary_document_drivers_do_not_gain_this_adapter() {
    let mut project = project();
    project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: None,
        schema: SchemaNode::group("Audit", vec![scalar("Label")]),
        options: project.target_options.clone(),
        root: Scope::default(),
    });
    assert_eq!(lower(&project).unwrap().xml_output_mode(), Ok(None));
    let mut program = lower(&self::project()).unwrap();
    program.root.iteration = Some(IterationPlan::dynamic_documents(
        vec!["catalog".into(), "Row".into()],
        0,
    ));
    assert!(
        matches!(program.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("repeating Group"))
    );
    for path in [Vec::new(), vec!["OutputRow".into(), "File".into()]] {
        let mut program = lower(&self::project()).unwrap();
        program.root.iteration = Some(IterationPlan::dynamic_documents(path, 0));
        assert!(
            matches!(program.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason.contains("repeating Group"))
        );
    }
}

#[test]
fn every_schema_and_policy_is_checked_while_observed_profiles_remain_strict() {
    for declaration in 0..3 {
        let mut program = lower(&project()).unwrap();
        program.xml_boundary.as_mut().unwrap().extra_inputs[declaration].name = "wrong".into();
        assert!(matches!(
            program.xml_output_mode(),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        ));
        let mut unsupported = project();
        let SchemaKind::Group { children, .. } =
            &mut unsupported.extra_sources[declaration].schema.kind
        else {
            unreachable!()
        };
        let scalar = if declaration == 1 {
            let SchemaKind::Group { children, .. } = &mut children[0].kind else {
                unreachable!()
            };
            &mut children[0]
        } else {
            &mut children[0]
        };
        scalar.default = Some("1".into());
        assert!(scalar.metadata_is_valid());
        assert_eq!(lower(&unsupported).unwrap().xml_output_mode(), Ok(None));
    }
    let mut observed = project();
    observed.source_options.xml_allow_inactive_root_type_members = true;
    observed.source_options.xml_root_view_read_policy = true;
    assert!(lower(&observed).is_err());
}

#[test]
fn native_complete_mapping_repeats_full_loader_driver_and_retains_member_data_and_paths() {
    let loader = Loader {
        calls: RefCell::new(Vec::new()),
        fail_second: false,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&loader);
    let paths = ["same.xml", "same.xml", "../雪😀.xml"];
    let mapped = engine::run_outputs_with_sources_and_context(
        &project(),
        &source(&paths, false),
        inputs(),
        &execution,
    )
    .unwrap();
    assert!(mapped.extras.is_empty());
    let Instance::DocumentSet(members) = mapped.primary else {
        panic!("expected documents")
    };
    assert_eq!(
        members
            .iter()
            .map(|member| member.path())
            .collect::<Vec<_>>(),
        paths
    );
    assert_eq!(
        loader.calls.borrow().as_slice(),
        &[
            ("catalog".into(), "a.xml".into()),
            ("catalog".into(), "b.xml".into()),
            ("catalog".into(), "a.xml".into()),
            ("catalog".into(), "b.xml".into()),
            ("catalog".into(), "a.xml".into()),
            ("catalog".into(), "b.xml".into())
        ]
    );
    for (member_index, member) in members.into_iter().enumerate() {
        let Instance::Group(fields) = member.value() else {
            panic!("member group")
        };
        assert_eq!(
            fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Id", "Marker", "Line"]
        );
        assert_eq!(
            member.value().field("Id").and_then(Instance::as_scalar),
            Some(&Value::Int(member_index as i64 + 1))
        );
        assert_eq!(
            member.value().field("Marker").and_then(Instance::as_scalar),
            Some(&Value::String("雪😀".into()))
        );
        let Some(Instance::Repeated(lines)) = member.value().field("Line") else {
            panic!("expected lines")
        };
        assert_eq!(lines.len(), 2);
        for (line, (id, amount, name)) in lines.iter().zip([
            (91, 9_007_199_254_740_992.0f64, "a.xml"),
            (92, -0.0f64, "b.xml"),
        ]) {
            let Instance::Group(fields) = line else {
                panic!("line group")
            };
            assert_eq!(
                fields
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
                ["DriverId", "Name", "Amount", "Scaled", "Adjustment", "Text"]
            );
            assert_eq!(
                line.field("Name").and_then(Instance::as_scalar),
                Some(&Value::String(name.into()))
            );
            assert_eq!(
                line.field("DriverId").and_then(Instance::as_scalar),
                Some(&Value::Int(id))
            );
            let Some(Value::Float(actual)) = line.field("Amount").and_then(Instance::as_scalar)
            else {
                panic!("Float amount")
            };
            assert_eq!(actual.to_bits(), amount.to_bits());
            let Some(Value::Float(scaled)) = line.field("Scaled").and_then(Instance::as_scalar)
            else {
                panic!("Float scaled")
            };
            assert_eq!(scaled.to_bits(), (amount * 2.0).to_bits());
            assert_eq!(
                line.field("Adjustment").and_then(Instance::as_scalar),
                Some(&Value::xml_nil())
            );
            assert_eq!(
                line.field("Text").and_then(Instance::as_scalar),
                Some(&Value::String("  text 雪😀  ".into()))
            );
        }
    }
}

#[test]
fn native_nil_path_and_host_error_abort_before_public_partial_results() {
    let nil_loader = Loader {
        calls: RefCell::new(Vec::new()),
        fail_second: false,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&nil_loader);
    assert!(
        matches!(engine::run_outputs_with_sources_and_context(&project(), &source(&["one.xml"], true), inputs(), &execution),
        Err(engine::EngineError::DynamicSourcePath { source_name, .. }) if source_name == "catalog")
    );
    assert_eq!(nil_loader.calls.borrow().len(), 2);
    let loader = Loader {
        calls: RefCell::new(Vec::new()),
        fail_second: true,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&loader);
    assert!(
        matches!(engine::run_outputs_with_sources_and_context(&project(), &source(&["one.xml", "later.xml"], false), inputs(), &execution),
        Err(engine::EngineError::DynamicSourceLoad { source_name, path, message }) if source_name == "catalog" && path == "b.xml" && message == "original host marker")
    );
    assert_eq!(loader.calls.borrow().len(), 2);
}

#[test]
fn empty_primary_document_set_does_not_evaluate_unreached_loader_scopes() {
    let loader = Loader {
        calls: RefCell::new(Vec::new()),
        fail_second: true,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&loader);
    let mapped = engine::run_outputs_with_sources_and_context(
        &project(),
        &source(&[], false),
        inputs(),
        &execution,
    )
    .unwrap();
    assert_eq!(mapped.primary, Instance::DocumentSet(Vec::new()));
    assert!(mapped.extras.is_empty() && loader.calls.borrow().is_empty());
}

fn multiple_project() -> Project {
    serde_json::from_str(include_str!(
        "fixtures/multiple_dynamic_named_inputs_dynamic_primary_xml_documents.json"
    ))
    .unwrap()
}

#[test]
fn multiple_dynamic_primary_inputs_keep_each_original_index_and_schema() {
    let mut project = multiple_project();
    assert!(engine::validate(&project).is_empty());
    let program = lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
        ))
    );
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(
        policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog", "labels", "codes"]
    );
    for (index, (source, input)) in program
        .extra_sources
        .iter()
        .zip(&policy.extra_inputs)
        .enumerate()
    {
        assert_eq!(source.name, input.name);
        assert_eq!(source.source, project.extra_sources[index].schema);
        assert_eq!(source.dynamic.is_some(), index == 1 || index == 3);
        assert_eq!(input.input.profile(), Some(XmlInputProfile::Structured));
    }
    assert_ne!(
        program.extra_sources[1].source,
        program.extra_sources[3].source
    );
    let mut misaligned = program;
    misaligned
        .xml_boundary
        .as_mut()
        .unwrap()
        .extra_inputs
        .swap(1, 3);
    assert!(matches!(
        misaligned.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
    let mut unproved = project.clone();
    let SchemaKind::Group { children, .. } = &mut unproved.extra_sources[3].schema.kind else {
        unreachable!()
    };
    let SchemaKind::Group { children, .. } = &mut children[0].kind else {
        unreachable!()
    };
    children[0].default = Some("1".into());
    assert!(children[0].metadata_is_valid());
    assert_eq!(lower(&unproved).unwrap().xml_output_mode(), Ok(None));

    project
        .extra_sources
        .retain(|source| source.dynamic_path.is_some());
    project.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::Float(2.0),
        },
    );
    project.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::String("literal".into()),
        },
    );
    assert_eq!(
        lower(&project).unwrap().xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
        ))
    );
}

struct MultiplePrimaryLoader {
    calls: RefCell<Vec<(String, String)>>,
    fail_source: Option<&'static str>,
}
impl engine::DynamicSourceLoader for MultiplePrimaryLoader {
    fn load(&self, name: &str, path: &str) -> Result<Arc<Instance>, String> {
        self.calls.borrow_mut().push((name.into(), path.into()));
        if self.fail_source == Some(name) {
            return Err("original source host marker".into());
        }
        let (collection, fields) = match name {
            "catalog" => (
                "Row",
                vec![
                    ("Name", value(Value::String("catalog name".into()))),
                    ("Amount", value(Value::Float(-0.0))),
                    ("Adjustment", value(Value::xml_nil())),
                    ("Text", value(Value::String("catalog text".into()))),
                ],
            ),
            "codes" => (
                "Entry",
                vec![
                    ("Code", value(Value::Int(9_007_199_254_740_993))),
                    ("Text", value(Value::String("codes text".into()))),
                ],
            ),
            _ => return Err(format!("unexpected source {name}")),
        };
        Ok(Arc::new(group(vec![(
            collection,
            Instance::Repeated(vec![group(fields)]),
        )])))
    }
}
fn multiple_source(paths: &[&str]) -> Instance {
    let mut input = source(paths, false);
    let Instance::Group(fields) = &mut input else {
        unreachable!()
    };
    fields
        .iter_mut()
        .find(|(name, _)| name == "LoadRow")
        .unwrap()
        .1 = Instance::Repeated(vec![group(vec![
        ("Id", value(Value::Int(91))),
        ("Path", value(Value::String("same.xml".into()))),
    ])]);
    input
}

#[test]
fn native_primary_members_use_both_source_schemas_at_the_same_path() {
    let loader = MultiplePrimaryLoader {
        calls: RefCell::default(),
        fail_source: None,
    };
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
        .with_dynamic_source_loader(&loader);
    let paths = ["same-output.xml", "same-output.xml"];
    let mapped = engine::run_outputs_with_sources_and_context(
        &multiple_project(),
        &multiple_source(&paths),
        inputs(),
        &execution,
    )
    .unwrap();
    assert!(mapped.extras.is_empty());
    let Instance::DocumentSet(members) = mapped.primary else {
        panic!("primary members")
    };
    assert_eq!(
        members
            .iter()
            .map(|member| member.path())
            .collect::<Vec<_>>(),
        paths
    );
    for (index, member) in members.iter().enumerate() {
        assert_eq!(
            member.value().field("Id").and_then(Instance::as_scalar),
            Some(&Value::Int(index as i64 + 1))
        );
        let Some(Instance::Repeated(lines)) = member.value().field("Line") else {
            panic!("catalog lines")
        };
        assert_eq!(lines.len(), 1);
        for field in ["Amount", "Scaled"] {
            let Some(Value::Float(actual)) = lines[0].field(field).and_then(Instance::as_scalar)
            else {
                panic!("Float {field}")
            };
            assert_eq!(actual.to_bits(), (-0.0f64).to_bits());
        }
        assert_eq!(
            lines[0].field("Text").and_then(Instance::as_scalar),
            Some(&Value::String("catalog text".into()))
        );
        let Some(Instance::Repeated(codes)) = member.value().field("CodeLine") else {
            panic!("codes lines")
        };
        assert_eq!(codes.len(), 1);
        assert_eq!(
            codes[0].field("Code").and_then(Instance::as_scalar),
            Some(&Value::Int(9_007_199_254_740_993))
        );
        assert_eq!(
            codes[0].field("Text").and_then(Instance::as_scalar),
            Some(&Value::String("codes text".into()))
        );
    }
    assert_eq!(
        loader.calls.borrow().as_slice(),
        [
            ("catalog".to_owned(), "same.xml".to_owned()),
            ("codes".to_owned(), "same.xml".to_owned()),
            ("catalog".to_owned(), "same.xml".to_owned()),
            ("codes".to_owned(), "same.xml".to_owned()),
        ]
    );
}

#[test]
fn unreached_second_source_and_empty_members_leave_missing_context_paths_lazy() {
    let mut project = multiple_project();
    project.graph.nodes.insert(
        100,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.extra_sources[3].dynamic_path.as_mut().unwrap().node = 100;
    project.root.children.pop();
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        lower(&project).unwrap().xml_output_mode(),
        Ok(Some(
            XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
        ))
    );
    for paths in [vec!["one.xml"], Vec::new()] {
        let loader = MultiplePrimaryLoader {
            calls: RefCell::default(),
            fail_source: Some("codes"),
        };
        let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
            .with_dynamic_source_loader(&loader);
        let mapped = engine::run_outputs_with_sources_and_context(
            &project,
            &multiple_source(&paths),
            inputs(),
            &execution,
        )
        .unwrap();
        let Instance::DocumentSet(members) = mapped.primary else {
            panic!("primary members")
        };
        assert_eq!(members.len(), paths.len());
        assert_eq!(loader.calls.borrow().len(), paths.len());
        assert!(
            loader
                .calls
                .borrow()
                .iter()
                .all(|(name, _)| name == "catalog")
        );
    }
}

#[test]
fn first_source_failure_skips_later_context_and_second_source_failure_returns_no_outputs() {
    for first in [true, false] {
        let mut project = multiple_project();
        project.graph.nodes.insert(
            10,
            Node::Const {
                value: Value::String("\u{1}".into()),
            },
        );
        project.graph.nodes.insert(
            100,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::CurrentDateTime,
            },
        );
        if first {
            project.extra_sources[3].dynamic_path.as_mut().unwrap().node = 100;
        } else {
            project.root.children[1].bindings[1].node = 100;
        }
        assert!(engine::validate(&project).is_empty());
        assert_eq!(
            lower(&project).unwrap().xml_output_mode(),
            Ok(Some(
                XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
            ))
        );
        let failed = if first { "catalog" } else { "codes" };
        let loader = MultiplePrimaryLoader {
            calls: RefCell::default(),
            fail_source: Some(failed),
        };
        let execution = engine::ExecutionContext::new(Path::new("mapping.json"))
            .with_dynamic_source_loader(&loader);
        assert!(matches!(engine::run_outputs_with_sources_and_context(
            &project, &multiple_source(&["one.xml", "later.xml"]), inputs(), &execution,
        ), Err(engine::EngineError::DynamicSourceLoad { source_name, path, message })
            if source_name == failed && path == "same.xml" && message == "original source host marker"));
        assert_eq!(
            loader.calls.borrow().as_slice(),
            if first {
                vec![("catalog".to_owned(), "same.xml".to_owned())]
            } else {
                vec![
                    ("catalog".to_owned(), "same.xml".to_owned()),
                    ("codes".to_owned(), "same.xml".to_owned()),
                ]
            }
        );
    }
}
