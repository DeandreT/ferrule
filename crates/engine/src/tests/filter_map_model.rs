use super::*;
use ir::SchemaNode;
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, FilterMapAdmissionKind, FunctionId,
    NamedTarget, ScopeIteration,
};

fn project() -> Project {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "identity-all")
        .unwrap();
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: serde_json::from_value(case["user_functions"].clone()).unwrap(),
        graph: serde_json::from_value(case["parent_graph"].clone()).unwrap(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(
                    serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
                ),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 11,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn input() -> Instance {
    Instance::Group(Vec::new().into())
}

fn retain_origins(value: &Instance, path: &str) {
    match value {
        Instance::Scalar(Value::Float(value)) => {
            eprintln!("float path={path:?} actual_bits={:016x}", value.to_bits())
        }
        Instance::Scalar(_) => {}
        Instance::Group(group) => {
            eprintln!("origin path={path:?} actual={:?}", group.xml_type_origin());
            for (index, (_, child)) in group.iter().enumerate() {
                retain_origins(child, &format!("{path}/field-{index}"));
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for (index, item) in items.iter().enumerate() {
                retain_origins(item, &format!("{path}/item-{index}"));
            }
        }
        Instance::DocumentSet(documents) => {
            for (index, document) in documents.iter().enumerate() {
                retain_origins(document.value(), &format!("{path}/document-{index}"));
            }
        }
    }
}

fn record(project: &Project) -> Result<Instance, EngineError> {
    let actual = run(project, &input());
    eprintln!(
        "input={}\nactual={actual:#?}",
        serde_json::to_string(project).unwrap()
    );
    if let Ok(instance) = &actual {
        retain_origins(instance, "root");
    }
    actual
}

#[test]
fn structurally_admitted_composition_has_a_typed_execution_guard() {
    let mut project = project();
    let issues = validate(&project);
    eprintln!("validation={issues:#?}");
    assert!(issues.is_empty());
    assert!(matches!(
        record(&project),
        Err(EngineError::UnsupportedSequenceComposition { item: 11 })
    ));
    project.graph.nodes.insert(1, Node::Raise { message: None });
    // The unavailable capability is refused before even a valid bound is read.
    assert!(matches!(
        record(&project),
        Err(EngineError::UnsupportedSequenceComposition { item: 11 })
    ));
}

#[test]
fn unselected_composition_still_has_static_admission_without_eager_execution() {
    let mut project = project();
    let sequence = project.root.children[0].sequence().unwrap().clone();
    project.target =
        SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 40,
        }],
        ..Scope::default()
    };
    project.graph.nodes.extend([
        (30, Node::SequenceItemAt { sequence, index: 1 }),
        (
            40,
            Node::If {
                condition: 42,
                then: 30,
                else_: 41,
            },
        ),
        (
            41,
            Node::Const {
                value: Value::Int(7),
            },
        ),
        (
            42,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
    ]);
    let actual = record(&project);
    assert_eq!(
        actual.unwrap(),
        Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(7)))].into())
    );
    project
        .user_functions
        .get_mut(&FunctionId::new(100))
        .unwrap()
        .output_type = ScalarType::Int;
    let actual = record(&project);
    let Err(EngineError::FilterMapAdmission(error)) = actual else {
        panic!("expected static typed refusal");
    };
    assert_eq!(error.item, 11);
    assert_eq!(error.location, "graph node 30");
    assert!(matches!(
        error.kind,
        FilterMapAdmissionKind::StageSignature { .. }
    ));
}

#[test]
fn source_owner_is_not_a_downstream_binding_permission() {
    let mut project = project();
    project.root.children[0].bindings[0].node = 10;
    let issues = validate(&project);
    eprintln!(
        "project={}\nvalidation={issues:#?}",
        serde_json::to_string(&project).unwrap()
    );
    assert!(issues.iter().any(|issue| issue.message
        == "expression 10 references generated sequence item node 10 outside its owning context"));
}

#[test]
fn duplicate_named_owners_and_failure_consumers_are_refused_before_runtime() {
    let mut project = project();
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    let actual = record(&project);
    assert!(matches!(
        actual,
        Err(EngineError::FilterMapAdmission(error)) if matches!(
            error.kind,
            FilterMapAdmissionKind::DuplicatePrivateOwner { item: 10, count: 2 }
        )
    ));
    let mut project = self::project();
    let sequence = project.root.children[0].sequence().unwrap().clone();
    project.root.children.clear();
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Sequence { sequence },
        selection: FailureSelection::All,
        message: None,
    });
    let actual = record(&project);
    assert!(matches!(
        actual,
        Err(EngineError::FilterMapAdmission(error)) if matches!(
            error.kind,
            FilterMapAdmissionKind::UnsupportedConsumer { site: "failure rule" }
        )
    ));
}

#[test]
fn both_complete_legacy_literal_sequences_keep_their_runtime_behavior() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap();
    for case in corpus["legacy_controls"].as_array().unwrap() {
        let mut project = project();
        project.user_functions.clear();
        project.graph = serde_json::from_value(case["parent_graph"].clone()).unwrap();
        project.root.children[0].iteration =
            ScopeIteration::Sequence(serde_json::from_value(case["descriptor"].clone()).unwrap());
        project.root.children[0].bindings[0].node = 10;
        let actual = record(&project);
        let expected_items = if case["id"] == "legacy-generate-omitted-from" {
            vec![1, 2, 3]
                .into_iter()
                .map(|value| {
                    Instance::Group(
                        vec![("Value".into(), Instance::Scalar(Value::Int(value)))].into(),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        assert_eq!(
            actual.unwrap(),
            Instance::Group(vec![("Rows".into(), Instance::Repeated(expected_items))].into())
        );
    }
}
