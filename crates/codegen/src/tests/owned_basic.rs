use super::*;
use crate::{IterationOutput, OwnedSourceIteration};
use mapping::{OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};

fn ref_at(owner: SourceOwner, path: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path.iter().map(|segment| (*segment).into()).collect(),
    }
}
#[test]
fn owned_basic_exact_primary_and_literal_named_identities_lower() {
    let mut project = supported_project();
    for name in ["Ref", " Ref 雪 ", "ref", "primary"] {
        project.extra_sources.push(mapping::NamedSource {
            name: name.into(),
            path: "unused.json".into(),
            schema: project.source.clone(),
            options: Default::default(),
            dynamic_path: None,
        });
    }
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
        SourceOwner::Named { name: "ref".into() },
        SourceOwner::Named {
            name: "primary".into(),
        },
    ] {
        let field = OwnedSourceField {
            source: ref_at(owner, &["First"]),
            frame: None,
        };
        project
            .graph
            .nodes
            .insert(20, Node::OwnedSourceField(field.clone()));
        let result = lower(&project);
        eprintln!("A-V01 project={project:#?} lower={result:#?}");
        let program = result.unwrap();
        assert_eq!(
            program
                .expressions
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            [10, 20, 30]
        );
        assert_eq!(
            program.expressions[1].expression,
            Expression::OwnedSourceField(field)
        );
        assert_eq!(program.source, project.source);
        assert_eq!(program.extra_sources[0].name, "Ref");
        assert_eq!(
            program.expressions[2].expression,
            Expression::SourceField {
                path: vec!["NestedValue".into()],
                frame: None
            }
        );
    }
}
#[test]
fn owned_basic_none_and_explicit_empty_pins_and_positions_stay_distinct() {
    let mut project = supported_project();
    for frame in [None, Some(vec![])] {
        let field = OwnedSourceField {
            source: ref_at(SourceOwner::Primary, &["First"]),
            frame: frame.clone(),
        };
        project
            .graph
            .nodes
            .insert(20, Node::OwnedSourceField(field.clone()));
        let result = lower(&project);
        eprintln!("A-V02 field={field:#?} result={result:#?}");
        assert_eq!(
            result.unwrap().expressions[1].expression,
            Expression::OwnedSourceField(field)
        );
        let position = OwnedPosition {
            owner: SourceOwner::Primary,
            collection: frame,
        };
        project
            .graph
            .nodes
            .insert(20, Node::OwnedPosition(position.clone()));
        let result = lower(&project);
        eprintln!("A-V02 position={position:#?} result={result:#?}");
        assert_eq!(
            result.unwrap().expressions[1].expression,
            Expression::OwnedPosition(position)
        );
    }
}
#[test]
fn owned_basic_iteration_getters_retain_wire_discriminants_and_old_constructors() {
    let source = ref_at(SourceOwner::Named { name: "Ref".into() }, &["Values"]);
    let owned = IterationPlan::owned_source(source.clone());
    let clone = owned.clone();
    eprintln!("A-V03 original={owned:#?} clone={clone:#?}");
    assert_eq!(owned, clone);
    assert_eq!(
        owned.owned_source_iteration(),
        Some(&OwnedSourceIteration::new(source.clone()))
    );
    assert_eq!(
        owned.source_selector(),
        Some(mapping::SourceSelectorRef::Owned(&source))
    );
    assert_eq!(owned.source_iteration(), None);
    assert_eq!(owned.dynamic_document_iteration(), None);
    assert_eq!(owned.generated_sequence(), None);
    assert_eq!(owned.inner_join(), None);
    assert_eq!(owned.concatenated(), None);
    assert_eq!(owned.output(), IterationOutput::Repeated);
    assert_eq!(owned.filter(), None);
    assert_eq!(owned.sort(), None);
    assert_eq!(owned.grouping(), None);
    assert!(owned.windows().is_empty());
    let legacy = IterationPlan::source(vec!["Ref".into(), "Values".into()]);
    assert_eq!(legacy.source_iteration().unwrap().path(), ["Ref", "Values"]);
    assert_eq!(legacy.owned_source_iteration(), None);
}
#[test]
fn owned_basic_valid_dead_nodes_are_omitted_and_both_if_branches_are_retained() {
    let baseline_project = supported_project();
    let baseline_result = lower(&baseline_project);
    eprintln!(
        "A-V05 complete baseline Project={baseline_project:#?} lower Result={baseline_result:#?}"
    );
    let original = baseline_result.unwrap();
    let mut project = baseline_project;
    project.graph.nodes.insert(
        99,
        Node::OwnedSourceField(OwnedSourceField {
            source: ref_at(SourceOwner::Primary, &["First"]),
            frame: None,
        }),
    );
    project.user_functions.insert(
        mapping::FunctionId::new(7),
        mapping::UserFunction {
            library: "owned-omission".into(),
            name: "unused".into(),
            description: None,
            parameters: vec![],
            output_name: "value".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: std::collections::BTreeMap::from([(
                    0,
                    Node::OwnedSourceField(OwnedSourceField {
                        source: ref_at(SourceOwner::Primary, &["First"]),
                        frame: None,
                    }),
                )]),
            },
            output: 0,
        },
    );
    let dead = lower(&project);
    eprintln!("A-V05 original={original:#?} candidate={project:#?} result={dead:#?}");
    assert_eq!(dead.unwrap(), original);
    project.graph.nodes.insert(
        40,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    project.graph.nodes.insert(
        50,
        Node::If {
            condition: 40,
            then: 99,
            else_: 20,
        },
    );
    project.root.bindings[1].node = 50;
    let result = lower(&project);
    eprintln!("A-V06 potential If project={project:#?} lower={result:#?}");
    let program = result.unwrap();
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [10, 20, 30, 40, 50, 99]
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .find(|node| node.id == 50)
            .unwrap()
            .expression,
        Expression::If {
            condition: 40,
            then: 99,
            else_: 20
        }
    );
    assert!(matches!(
        program.expressions.last().unwrap().expression,
        Expression::OwnedSourceField(_)
    ));
}
#[test]
fn ordinary_legacy_raise_under_an_unrelated_named_target_stays_valid() {
    let mut project = supported_project();
    project
        .graph
        .nodes
        .insert(99, Node::Raise { message: None });
    let mut root = project.root.clone();
    root.bindings[1].node = 99;
    project.extra_targets.push(NamedTarget {
        name: "RaiseTarget".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root,
    });
    let result = lower(&project);
    eprintln!("P19 ordinary legacy Raise={result:#?}");
    let program = result.unwrap();
    assert_eq!(
        program
            .expressions
            .iter()
            .find(|node| node.id == 99)
            .unwrap()
            .expression,
        Expression::Raise { message: None }
    );
    let gate = crate::validate_owned_emission(&program);
    eprintln!("P19 complete Program={program:#?} validate_owned_emission Result={gate:#?}");
    assert_eq!(gate, Ok(()));
}

#[test]
fn named_repeated_scalar_selector_lowers_without_losing_its_owner_or_explicit_pin() {
    let mut project = supported_project();
    project.extra_sources.push(mapping::NamedSource {
        name: "Ref".into(),
        path: "unused.json".into(),
        schema: SchemaNode::group("RefRoot", vec![scalar("Values").repeating()]),
        options: Default::default(),
        dynamic_path: None,
    });
    let source = ref_at(SourceOwner::Named { name: "Ref".into() }, &["Values"]);
    project.root.iteration = ScopeIteration::OwnedSource {
        source: source.clone(),
        output_path: None,
    };
    let field = OwnedSourceField {
        source: source.clone(),
        frame: Some(vec!["Values".into()]),
    };
    project
        .graph
        .nodes
        .insert(20, Node::OwnedSourceField(field.clone()));
    let result = lower(&project);
    eprintln!("A-V03 complete named scalar project={project:#?} lower={result:#?}");
    let program = result.unwrap();
    assert_eq!(
        program
            .root
            .iteration
            .as_ref()
            .unwrap()
            .owned_source_iteration()
            .unwrap()
            .source(),
        &source
    );
    assert_eq!(
        program.root.iteration.as_ref().unwrap().source_iteration(),
        None
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .find(|node| node.id == 20)
            .unwrap()
            .expression,
        Expression::OwnedSourceField(field)
    );
}

#[test]
fn owned_runtime_input_keeps_flextext_metadata_omission_exact() {
    let layout = mapping::FlexTextLayout::new(
        "Root",
        mapping::FlexCommand::DelimitedRecords {
            name: "Row".into(),
            dialect: mapping::DelimitedDialect::new(',', "\n", '"', '"').unwrap(),
            fields: vec![
                mapping::DelimitedRecordField::new("owned_source_field", ScalarType::String)
                    .unwrap(),
            ],
        },
        mapping::FlexLineEnding::Lf,
        false,
    )
    .unwrap();
    let layout_json = serde_json::to_string(&layout).unwrap();
    let path_json = r#"["Row","owned_source_field"]"#;
    let mut project = supported_project();
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Out", ScalarType::String)],
    );
    let owned = OwnedSourceField {
        source: ref_at(SourceOwner::Primary, &["First"]),
        frame: None,
    };
    project.graph.nodes = std::collections::BTreeMap::from([
        (1, Node::OwnedSourceField(owned.clone())),
        (
            2,
            Node::Const {
                value: Value::String(layout_json.clone()),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String(path_json.into()),
            },
        ),
        (
            4,
            Node::Call {
                function: "flextext_parse_field".into(),
                args: vec![1, 2, 3],
            },
        ),
    ]);
    project.root = Scope {
        bindings: vec![MappingBinding {
            target_field: "Out".into(),
            node: 4,
        }],
        ..Scope::default()
    };
    let result = lower(&project);
    eprintln!("A-V05 flextext complete project={project:#?} lower={result:#?}");
    let program = result.unwrap();
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [1, 4]
    );
    assert_eq!(
        program.expressions[0].expression,
        Expression::OwnedSourceField(owned)
    );
    let Expression::DelimitedTextField { input, parser } = &program.expressions[1].expression
    else {
        panic!("validated literal projection")
    };
    assert_eq!(*input, 1);
    assert_eq!(parser.layout_descriptor(), layout_json);
    assert_eq!(parser.path_descriptor(), path_json);
}

#[test]
fn unused_stored_driver_conservatively_rechecks_a_primary_owned_node_after_its_basic_visit() {
    let mut baseline = supported_project();
    baseline.extra_sources.push(mapping::NamedSource {
        name: "UnusedDynamic".into(),
        path: "unused.json".into(),
        schema: SchemaNode::group("Other", vec![scalar("Value")]),
        options: Default::default(),
        dynamic_path: Some(mapping::DynamicSourcePath {
            node: 20,
            iteration: vec![],
        }),
    });
    let baseline_result = lower(&baseline);
    eprintln!(
        "A516 stored-driver no-owned complete Project={baseline:#?} lower Result={baseline_result:#?}"
    );
    let expected = Diagnostic::OwnedSource {
        diagnostic: crate::OwnedSourceDiagnostic::Capability {
            location: crate::OwnedSourceDiagnosticLocation::Expression {
                namespace: crate::OwnedSourceNamespace::Main,
                node: 20,
                scope: None,
            },
            request: Some(crate::OwnedSourceDiagnosticRequest::Field {
                owner: crate::OwnedSourceDiagnosticOwner::Primary,
                path: vec!["First".into()],
                frame: None,
            }),
            reason: crate::OwnedSourceCapabilityReason::DynamicLoader,
            origin: crate::OwnedSourceCapabilityOrigin::DynamicSource {
                index: 0,
                name: "UnusedDynamic".into(),
            },
        }
        .into(),
    };
    let mut reused = baseline.clone();
    reused.graph.nodes.insert(
        20,
        Node::OwnedSourceField(OwnedSourceField {
            source: ref_at(SourceOwner::Primary, &["First"]),
            frame: None,
        }),
    );
    let reused_result = lower(&reused);
    eprintln!(
        "A516 stored-driver reused-owned complete Project={reused:#?} lower Result={reused_result:#?} expected literal Diagnostic={expected:#?}"
    );
    // Compiler validation includes the retained unused driver. This makes no runtime demand or loader-call claim.
    assert!(baseline_result.is_ok());
    let baseline_program = baseline_result.unwrap();
    assert_eq!(
        baseline_program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [10, 20, 30]
    );
    assert_eq!(
        baseline_program
            .expressions
            .iter()
            .find(|node| node.id == 20)
            .unwrap()
            .expression,
        Expression::SourceField {
            path: vec!["First".into()],
            frame: None
        }
    );
    assert_eq!(reused_result.unwrap_err().diagnostics(), [expected]);
}
