//! #463 owned recursive collection walks: IR, profile admission and refusals.
use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Node, NodeId, OwnedRecursiveCollect, OwnedSourceRef, SourceOwner};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn walk(owner: SourceOwner, item: u32) -> mapping::SequenceExpr {
    mapping::SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
        collection: OwnedSourceRef {
            owner,
            path: path(&["Dir"]),
        },
        children: path(&["Dir"]),
        descent_value: path(&["Note"]),
        values: path(&["file"]),
        value: path(&["Note"]),
        prefix: 1,
        separator: 2,
        item,
    })
}
fn endpoint(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            SchemaNode::group(
                "Dir",
                vec![
                    SchemaNode::scalar("Note", ScalarType::String),
                    SchemaNode::group("file", vec![SchemaNode::scalar("Note", ScalarType::String)])
                        .repeating(),
                    SchemaNode::recursive_group("Dir", "Dir").repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
/// `Value` is ItemAt over the named endpoint's owned walk (node 10, item 11).
fn project() -> mapping::Project {
    let mut graph = mapping::Graph::default();
    for (id, node) in [
        (
            1,
            Node::Const {
                value: Value::String(String::new()),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("/".into()),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            10,
            Node::SequenceItemAt {
                sequence: walk(named(), 11),
                index: 3,
            },
        ),
        (11, item()),
    ] {
        graph.nodes.insert(id, node);
    }
    mapping::Project {
        source: endpoint("Input"),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: endpoint("Ref"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..mapping::Scope::default()
        },
    }
}
fn closed(node: NodeId) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
                scope: Some(mapping::SourceScopeLocation::root(
                    mapping::SourceScopeTarget::Primary,
                )),
            },
            request: Some(OwnedSourceDiagnosticRequest::Iteration {
                owner: OwnedSourceDiagnosticOwner::Named { name: "Ref".into() },
                path: path(&["Dir"]),
            }),
            reason: OwnedSourceCapabilityReason::GeneratedItem,
            origin: OwnedSourceCapabilityOrigin::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}

#[test]
fn owned_walk_lowers_with_its_exact_owner_and_lazy_named_input() {
    let program = lower_with_basic_owned_recursive_collect(&project()).unwrap();
    let Some(Expression::SequenceItemAt {
        sequence:
            GeneratedSequence::OwnedRecursiveCollect {
                collection, item, ..
            },
        index: 3,
    }) = program
        .expressions
        .iter()
        .find(|node| node.id == 10)
        .map(|node| &node.expression)
    else {
        panic!("lowered owned ItemAt");
    };
    assert_eq!(collection.owner, named());
    assert_eq!(*item, 11);
    validate_program_with_basic_owned_recursive_collect(&program).unwrap();
    validate_basic_owned_recursive_collect_emission(&program).unwrap();
    // The owned root collection never makes its endpoint an eager input.
    let plan =
        legacy_named_input_admission_plan_with_basic_owned_recursive_collect(&program).unwrap();
    assert_eq!(
        plan.primary.entries[0].availability,
        NamedInputAvailability::Optional
    );
}

#[test]
fn every_earlier_profile_and_direct_validation_refuses_the_owned_producer() {
    let program = lower_with_basic_owned_recursive_collect(&project()).unwrap();
    for result in [
        validate_program(&program),
        validate_owned_emission(&program),
        validate_basic_owned_emission(&program),
        validate_program_with_basic_owned_sequence_item_at(&program),
        validate_program_with_basic_owned_group_ending_with(&program),
        validate_program_with_basic_owned_failure_sources(&program),
        validate_basic_owned_failure_source_emission(&program),
        legacy_named_input_admission_plan_with_basic_owned_failure_sources(&program).map(drop),
        validate_program_with_basic_owned_path_hierarchy(&program),
        validate_program_with_basic_owned_adjacency_tree(&program),
        validate_basic_owned_adjacency_tree_emission(&program),
        legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(&program).map(drop),
        validate_program_with_basic_owned_group_into_blocks(&program),
        validate_basic_owned_group_into_blocks_emission(&program),
    ] {
        assert_eq!(result, Err(closed(10)));
    }
    assert!(lower(&project()).is_err());
    assert!(lower_with_basic_owned_failure_sources(&project()).is_err());
    assert!(lower_with_basic_owned_adjacency_tree(&project()).is_err());
    assert!(lower_with_basic_owned_group_into_blocks(&project()).is_err());
}

#[test]
fn private_item_reducers_and_generated_iteration_stay_closed() {
    let mut exists = project();
    exists.graph.nodes.insert(
        10,
        Node::SequenceExists {
            sequence: walk(named(), 11),
            predicate: 11,
        },
    );
    let error = lower_with_basic_owned_recursive_collect(&exists).unwrap_err();
    assert_eq!(
        error.diagnostics(),
        [Diagnostic::OwnedSource {
            diagnostic: match closed(10) {
                ProgramValidationError::OwnedSource { diagnostic } => diagnostic,
                _ => unreachable!("authored owned refusal"),
            },
        }]
    );
    let mut scope = project();
    scope.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Files", ScalarType::String).repeating()],
    );
    scope.root = mapping::Scope {
        children: vec![mapping::Scope {
            target_field: "Files".into(),
            iteration: mapping::ScopeIteration::Sequence(walk(SourceOwner::Primary, 11)),
            construction: mapping::ScopeConstruction::Scalar { value: 11 },
            ..mapping::Scope::default()
        }],
        ..mapping::Scope::default()
    };
    scope.graph.nodes.remove(&10);
    let files = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0));
    assert!(matches!(
        lower_with_basic_owned_recursive_collect(&scope).unwrap_err().diagnostics(),
        [Diagnostic::OwnedSource { diagnostic }]
            if **diagnostic == OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Iteration { scope: files.clone() },
                request: Some(OwnedSourceDiagnosticRequest::Iteration {
                    owner: OwnedSourceDiagnosticOwner::Primary,
                    path: path(&["Dir"]),
                }),
                reason: OwnedSourceCapabilityReason::GeneratedItem,
                origin: OwnedSourceCapabilityOrigin::Scope { scope: files.clone() },
            }
    ));
}

#[test]
fn renderers_without_a_paired_consumer_report_the_private_item() {
    let source = OwnedSourceRef {
        owner: named(),
        path: path(&["Dir"]),
    };
    assert_eq!(
        owned_sequence_emission_error(11, &source),
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 11,
                    scope: None,
                },
                request: Some(OwnedSourceDiagnosticRequest::Iteration {
                    owner: OwnedSourceDiagnosticOwner::Named { name: "Ref".into() },
                    path: path(&["Dir"]),
                }),
                reason: OwnedSourceCapabilityReason::UnsupportedBasicContext,
                origin: OwnedSourceCapabilityOrigin::RetainedExpression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 11,
                },
            }),
        }
    );
}
