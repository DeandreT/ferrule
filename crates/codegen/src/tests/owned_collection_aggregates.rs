//! #447 owned collection aggregates: IR, profile admission and refusals.
use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Node, OwnedAggregate, OwnedAggregateValue, OwnedSourceField, OwnedSourceRef,
    SourceOwner,
};
use std::collections::BTreeMap;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn rows(owner: SourceOwner) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path(&["Rows"]),
    }
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
fn sum(owner: SourceOwner) -> OwnedAggregate {
    OwnedAggregate {
        function: AggregateOp::Sum,
        collection: rows(owner),
        value: OwnedAggregateValue::Path(path(&["Rank"])),
        arg: None,
    }
}
fn join(owner: SourceOwner) -> OwnedAggregate {
    OwnedAggregate {
        function: AggregateOp::Join,
        collection: rows(owner),
        value: OwnedAggregateValue::Expression(5),
        arg: Some(4),
    }
}
/// `Value` sums Primary ranks and `Other` joins computed named names.
fn project() -> mapping::Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::group("Lines", vec![SchemaNode::scalar("Qty", ScalarType::Int)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Other", ScalarType::String),
        ],
    );
    let bind = |target_field: &str, node| mapping::Binding {
        target_field: target_field.into(),
        node,
    };
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
            bindings: vec![bind("Value", 2), bind("Other", 3)],
            ..Default::default()
        },
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (2, Node::OwnedAggregate(sum(SourceOwner::Primary))),
                (3, Node::OwnedAggregate(join(named()))),
                (
                    4,
                    Node::Const {
                        value: Value::String(",".into()),
                    },
                ),
                (5, owned(named(), &["Rows", "Name"])),
            ]),
        },
        failure_rules: Vec::new(),
    }
}
fn retained(node: u32, owner: SourceOwner) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
                scope: None,
            },
            request: Some(OwnedSourceDiagnosticRequest::Iteration {
                owner: (&owner).into(),
                path: path(&["Rows"]),
            }),
            reason: OwnedSourceCapabilityReason::AggregateLookup,
            origin: OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}
fn reason(error: &LowerError) -> Option<OwnedSourceCapabilityReason> {
    match error.diagnostics() {
        [Diagnostic::OwnedSource { diagnostic }] => match diagnostic.as_ref() {
            OwnedSourceDiagnostic::Capability { reason, .. } => Some(*reason),
            OwnedSourceDiagnostic::Structure { .. } => None,
        },
        _ => None,
    }
}

#[test]
fn owned_aggregates_lower_exactly_and_stay_demand_driven() {
    let program = lower_with_basic_owned_collection_aggregates(&project()).unwrap();
    let expression = |id| {
        program
            .expressions
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.expression.clone())
    };
    assert_eq!(
        expression(2),
        Some(Expression::OwnedAggregate(sum(SourceOwner::Primary)))
    );
    assert_eq!(
        expression(3),
        Some(Expression::OwnedAggregate(join(named())))
    );
    assert_eq!(
        validate_program_with_basic_owned_collection_aggregates(&program),
        Ok(())
    );
    assert_eq!(
        validate_basic_owned_collection_aggregates_emission(&program),
        Ok(())
    );
    let plan =
        legacy_named_input_admission_plan_with_basic_owned_collection_aggregates(&program).unwrap();
    assert_eq!(
        plan.all_outputs.entries[0].availability,
        NamedInputAvailability::Optional
    );
    // The ordinary emission gate never opens owned aggregates.
    assert!(validate_owned_emission(&program).is_err());
}

#[test]
fn every_earlier_profile_refuses_owned_aggregates() {
    let p = project();
    let program = lower_with_basic_owned_collection_aggregates(&p).unwrap();
    let expected = retained(2, SourceOwner::Primary);
    for result in [
        validate_basic_owned_emission(&program),
        validate_program_with_basic_owned_sort(&program),
        validate_program_with_basic_owned_group_by(&program),
        validate_program_with_basic_owned_failure_sources(&program),
        validate_program_with_basic_owned_path_hierarchy(&program),
        validate_program_with_basic_owned_adjacency_tree(&program),
        validate_program_with_basic_owned_group_into_blocks(&program),
        validate_program_with_basic_owned_recursive_collect(&program),
        validate_program_with_basic_owned_sequence_exists(&program),
        validate_program_with_basic_owned_recursive_filter(&program),
        validate_program_with_basic_owned_lookup_find(&program),
        validate_basic_owned_lookup_find_emission(&program),
        // The sibling dynamic-field companion also composes owned search only.
        validate_program_with_basic_owned_dynamic_field(&program),
        validate_basic_owned_dynamic_field_emission(&program),
        validate_program_with_basic_owned_xml_serialize(&program),
        validate_basic_owned_xml_serialize_emission(&program),
        validate_program_with_basic_owned_sequence_aggregate(&program),
        validate_basic_owned_sequence_aggregate_emission(&program),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    for result in [
        lower_with_basic_owned_windows(&p),
        lower_with_basic_owned_failure_sources(&p),
        lower_with_basic_owned_group_into_blocks(&p),
        lower_with_basic_owned_recursive_filter(&p),
        lower_with_basic_owned_lookup_find(&p),
        lower_with_basic_owned_dynamic_field(&p),
        lower_with_basic_owned_xml_serialize(&p),
        lower_with_basic_owned_sequence_aggregate(&p),
    ] {
        let Err(error) = result else {
            panic!("older profile admitted an owned aggregate");
        };
        assert_eq!(
            reason(&error),
            Some(OwnedSourceCapabilityReason::AggregateLookup)
        );
    }
}

#[test]
fn owned_aggregates_stay_closed_in_specialized_and_isolated_contexts() {
    // A legacy aggregate item expression is an AggregateLookup context.
    let mut p = project();
    p.graph.nodes.insert(
        7,
        Node::Aggregate {
            function: AggregateOp::Sum,
            collection: path(&["Rows"]),
            value: Vec::new(),
            expression: Some(2),
            arg: None,
        },
    );
    p.root.bindings[0].node = 7;
    let error = lower_with_basic_owned_collection_aggregates(&p).unwrap_err();
    assert_eq!(
        reason(&error),
        Some(OwnedSourceCapabilityReason::AggregateLookup)
    );
    // A generated-item predicate keeps its closed context.
    let mut p = project();
    p.graph.nodes.extend([
        (
            7,
            Node::SequenceExists {
                sequence: mapping::SequenceExpr::Generate {
                    from: None,
                    to: 8,
                    item: 9,
                },
                predicate: 10,
            },
        ),
        (
            8,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            9,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            10,
            Node::Call {
                function: "exists".into(),
                args: vec![2],
            },
        ),
    ]);
    p.root.bindings[0].node = 7;
    let error = lower_with_basic_owned_collection_aggregates(&p).unwrap_err();
    assert_eq!(
        reason(&error),
        Some(OwnedSourceCapabilityReason::GeneratedItem)
    );
    // Owned failure messages keep their scalar subset.
    let mut p = project();
    p.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::OwnedSource {
            source: rows(named()),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 7 },
        message: Some(3),
    });
    let error = lower_with_basic_owned_collection_aggregates(&p).unwrap_err();
    assert_eq!(
        reason(&error),
        Some(OwnedSourceCapabilityReason::FailureRule)
    );
}

fn structure(
    node: u32,
    request_path: &[&str],
    frame: Option<&[&str]>,
    reason: OwnedSourceStructureReason,
) -> Result<(), ProgramValidationError> {
    Err(ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
                scope: None,
            },
            request: match frame {
                Some(frame) => OwnedSourceDiagnosticRequest::Field {
                    owner: OwnedSourceDiagnosticOwner::Primary,
                    path: path(request_path),
                    frame: Some(path(frame)),
                },
                None => OwnedSourceDiagnosticRequest::Iteration {
                    owner: OwnedSourceDiagnosticOwner::Primary,
                    path: path(request_path),
                },
            },
            reason,
        }),
    })
}

#[test]
fn direct_programs_check_owned_aggregate_structure() {
    let program = lower_with_basic_owned_collection_aggregates(&project()).unwrap();
    let with = |aggregate: OwnedAggregate| {
        let mut direct = program.clone();
        for node in &mut direct.expressions {
            if node.id == 2 {
                node.expression = Expression::OwnedAggregate(aggregate.clone());
            }
        }
        direct
    };
    let crossing = with(OwnedAggregate {
        value: OwnedAggregateValue::Path(path(&["Lines", "Qty"])),
        ..sum(SourceOwner::Primary)
    });
    let expected = structure(
        2,
        &["Rows", "Lines", "Qty"],
        Some(&["Rows"]),
        OwnedSourceStructureReason::InvalidOwnedField {
            found: OwnedSourceSchemaShape::Group { repeating: true },
        },
    );
    assert_eq!(
        validate_program_with_basic_owned_collection_aggregates(&crossing),
        expected
    );
    assert_eq!(validate_owned_emission(&crossing), expected);
    // Only Count accepts the empty value path over group items.
    let whole_item = |function| {
        with(OwnedAggregate {
            function,
            value: OwnedAggregateValue::Path(Vec::new()),
            ..sum(SourceOwner::Primary)
        })
    };
    assert_eq!(
        validate_program_with_basic_owned_collection_aggregates(&whole_item(AggregateOp::Count)),
        Ok(())
    );
    assert_eq!(
        validate_program_with_basic_owned_collection_aggregates(&whole_item(AggregateOp::Max)),
        structure(
            2,
            &["Rows"],
            Some(&["Rows"]),
            OwnedSourceStructureReason::InvalidOwnedField {
                found: OwnedSourceSchemaShape::Group { repeating: true },
            },
        )
    );
    let scalar_collection = with(OwnedAggregate {
        collection: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Note"]),
        },
        ..sum(SourceOwner::Primary)
    });
    assert_eq!(
        validate_program_with_basic_owned_collection_aggregates(&scalar_collection),
        structure(
            2,
            &["Note"],
            None,
            OwnedSourceStructureReason::InvalidOwnedCollection {
                found: OwnedSourceSchemaShape::Scalar { repeating: false },
            },
        )
    );
}
