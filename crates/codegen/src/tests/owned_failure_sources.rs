//! #471 owned failure walks: IR, profile admission and closed refusals.
use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Node, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::collections::BTreeMap;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn owned(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: None,
    })
}
fn walk(owner: SourceOwner) -> mapping::FailureIteration {
    mapping::FailureIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(&["Rows"]),
        },
    }
}
fn rule(
    iteration: mapping::FailureIteration,
    predicate: u32,
    message: u32,
) -> mapping::FailureRule {
    mapping::FailureRule {
        iteration,
        selection: mapping::FailureSelection::WhenTrue { predicate },
        message: Some(message),
    }
}
/// Rule 1 is a legacy primary-row rule; rule 2 walks the named endpoint.
fn project() -> mapping::Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Message", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                    SchemaNode::scalar("Message", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    mapping::Project {
        source: source.clone(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: source,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..Default::default()
        },
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (1, owned(SourceOwner::Primary, &["Rows", "Flag"])),
                (2, owned(SourceOwner::Primary, &["Rows", "Message"])),
                (3, owned(named(), &["Rows", "Flag"])),
                (4, owned(named(), &["Rows", "Message"])),
                (
                    10,
                    Node::Const {
                        value: Value::String("ok".into()),
                    },
                ),
            ]),
        },
        failure_rules: vec![
            rule(
                mapping::FailureIteration::Source {
                    collection: path(&["Rows"]),
                },
                1,
                2,
            ),
            rule(walk(named()), 3, 4),
        ],
    }
}
fn closed(index: usize, reason: OwnedSourceCapabilityReason) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::FailureIteration { index },
            request: Some(OwnedSourceDiagnosticRequest::Iteration {
                owner: OwnedSourceDiagnosticOwner::Named { name: "Ref".into() },
                path: path(&["Rows"]),
            }),
            reason,
            origin: OwnedSourceCapabilityOrigin::Failure { index },
        }),
    }
}

#[test]
fn owned_failure_walk_lowers_exactly_and_composes_with_primary_rows() {
    let p = project();
    let program = lower_with_basic_owned_failure_sources(&p).unwrap();
    assert_eq!(
        program.failure_rules[1],
        FailureRule {
            iteration: FailureIteration::OwnedSource(OwnedSourceIteration::new(OwnedSourceRef {
                owner: named(),
                path: path(&["Rows"]),
            })),
            selection: FailureSelection::WhenTrue(3),
            message: Some(4),
        }
    );
    assert!(is_basic_owned_primary_row_failure(
        &program,
        &program.failure_rules[0]
    ));
    assert!(!is_basic_owned_primary_row_failure(
        &program,
        &program.failure_rules[1]
    ));
    assert_eq!(
        validate_program_with_basic_owned_failure_sources(&program),
        Ok(())
    );
    assert_eq!(
        validate_basic_owned_failure_source_emission(&program),
        Ok(())
    );
    let plan =
        legacy_named_input_admission_plan_with_basic_owned_failure_sources(&program).unwrap();
    assert_eq!(
        plan.all_outputs.entries[0].availability,
        NamedInputAvailability::Optional
    );
}

#[test]
fn every_earlier_profile_and_direct_validator_refuses_the_owned_walk() {
    let mut p = project();
    p.failure_rules.remove(0);
    let program = lower_with_basic_owned_failure_sources(&p).unwrap();
    let expected = closed(0, OwnedSourceCapabilityReason::FailureRule);
    for result in [
        validate_program(&program),
        validate_owned_emission(&program),
        validate_program_with_basic_owned_row_failures(&program),
        validate_basic_owned_row_failure_emission(&program),
        validate_program_with_basic_owned_group_starting_with(&program),
        validate_basic_owned_group_starting_with_emission(&program),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    for result in [
        lower(&p),
        lower_with_basic_owned_row_failures(&p),
        lower_with_basic_owned_group_starting_with(&p),
    ] {
        let Err(error) = result else {
            panic!("older profile admitted an owned failure walk");
        };
        let ProgramValidationError::OwnedSource { diagnostic } = expected.clone() else {
            unreachable!();
        };
        assert_eq!(
            error.diagnostics(),
            &[Diagnostic::OwnedSource { diagnostic }]
        );
    }
}

#[test]
fn owned_failure_walk_keeps_its_scalar_subset_closed() {
    let mut p = project();
    p.graph.nodes.insert(
        5,
        Node::Aggregate {
            function: mapping::AggregateOp::Count,
            collection: path(&["Rows"]),
            value: vec![],
            expression: None,
            arg: None,
        },
    );
    p.failure_rules[1].message = Some(5);
    let Err(error) = lower_with_basic_owned_failure_sources(&p) else {
        panic!("aggregate message admitted");
    };
    let ProgramValidationError::OwnedSource { diagnostic } =
        closed(1, OwnedSourceCapabilityReason::FailureRule)
    else {
        unreachable!();
    };
    assert_eq!(
        error.diagnostics(),
        &[Diagnostic::OwnedSource { diagnostic }]
    );

    // A legacy-only rule beside it keeps the ordinary failure subset and route.
    let mut p = project();
    p.failure_rules.truncate(1);
    p.failure_rules[0].iteration = walk(SourceOwner::Primary);
    let program = lower_with_basic_owned_failure_sources(&p).unwrap();
    let mut direct = program.clone();
    direct.failure_rules[0].iteration =
        FailureIteration::OwnedSource(OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Message"]),
        }));
    assert_eq!(
        validate_program_with_basic_owned_failure_sources(&direct),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
                location: OwnedSourceDiagnosticLocation::FailureIteration { index: 0 },
                request: OwnedSourceDiagnosticRequest::Iteration {
                    owner: OwnedSourceDiagnosticOwner::Primary,
                    path: path(&["Message"]),
                },
                reason: OwnedSourceStructureReason::InvalidOwnedCollection {
                    found: OwnedSourceSchemaShape::Scalar { repeating: false },
                },
            }),
        })
    );
}

#[test]
fn owned_failure_profile_keeps_filter_map_failure_refusal() {
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
    let sequence: mapping::SequenceExpr =
        serde_json::from_value(case["sequence_descriptor"].clone()).unwrap();
    let item = sequence.item();
    let mut p = project();
    p.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Sequence { sequence },
        selection: mapping::FailureSelection::All,
        message: None,
    }];
    p.graph = serde_json::from_value(case["parent_graph"].clone()).unwrap();
    p.graph.nodes.insert(
        900,
        Node::Const {
            value: Value::String("ok".into()),
        },
    );
    p.root.bindings[0].node = 900;
    p.user_functions = serde_json::from_value(case["user_functions"].clone()).unwrap();
    let refusal = |result: Result<Program, LowerError>| match result {
        Ok(_) => panic!("filter_map failure admitted"),
        Err(error) => error.diagnostics().to_vec(),
    };
    let ordinary = refusal(lower(&p));
    assert_eq!(
        ordinary,
        vec![Diagnostic::Validation {
            location: "failure rule 1".into(),
            message: format!(
                "filter_map_v1 item {item} at failure rule 1: UnsupportedConsumer {{ site: \"failure rule\" }}"
            ),
        }]
    );
    assert_eq!(
        refusal(lower_with_basic_owned_failure_sources(&p)),
        ordinary
    );
}
