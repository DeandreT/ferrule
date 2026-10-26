//! #450 owned joined aggregates: lowering, the owned join profile and the
//! closed contexts and refusals of every other route.
use super::*;
use mapping::{
    AggregateOp, JoinId as MappingJoinId, NamedSource, NodeId, OwnedJoinAggregate,
    OwnedJoinConditions, OwnedJoinKey, OwnedJoinPlan, OwnedJoinSource, OwnedSourceRef, SourceOwner,
    SourceScopeLocation, SourceScopeTarget,
};

fn rows() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![SchemaNode::group("Rows", vec![typed_scalar("id", ScalarType::Int)]).repeating()],
    )
}

fn rows_of(owner: SourceOwner) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: vec!["Rows".into()],
    }
}

fn named_rows() -> OwnedSourceRef {
    rows_of(SourceOwner::Named {
        name: "lookup".into(),
    })
}

fn plan() -> OwnedJoinPlan {
    OwnedJoinPlan::new(
        OwnedJoinSource::new(rows_of(SourceOwner::Primary)),
        OwnedJoinSource::new(named_rows()),
        OwnedJoinConditions::new(OwnedJoinKey::new(
            rows_of(SourceOwner::Primary),
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap()
}

fn joined(expression: Option<NodeId>, arg: Option<NodeId>) -> Node {
    Node::OwnedJoinAggregate(OwnedJoinAggregate {
        function: AggregateOp::Join,
        join: MappingJoinId::new(5),
        plan: plan(),
        expression,
        arg,
    })
}

fn join_field(join: u64, collection: OwnedSourceRef) -> Node {
    Node::OwnedJoinField(mapping::OwnedJoinField {
        join: MappingJoinId::new(join),
        collection,
        path: vec!["id".into()],
    })
}

/// Root binding `Value` (node 1) joins the Lookup ids of every tuple (node 2)
/// with the parent separator (node 3); `tuple` (node 4) counts tuples.
fn aggregate_project() -> Project {
    let mut graph = Graph::default();
    for (id, node) in [
        (1, joined(Some(2), Some(3))),
        (2, join_field(5, named_rows())),
        (
            3,
            Node::Const {
                value: Value::String("-".into()),
            },
        ),
        (
            4,
            Node::OwnedJoinAggregate(OwnedJoinAggregate {
                function: AggregateOp::Count,
                join: MappingJoinId::new(6),
                plan: plan(),
                expression: None,
                arg: None,
            }),
        ),
    ] {
        graph.nodes.insert(id, node);
    }
    Project {
        source: rows(),
        target: SchemaNode::group(
            "Target",
            vec![
                typed_scalar("Value", ScalarType::String),
                typed_scalar("Count", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "lookup".into(),
            path: "lookup.json".into(),
            schema: rows(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![
                mapping::Binding {
                    target_field: "Value".into(),
                    node: 1,
                },
                mapping::Binding {
                    target_field: "Count".into(),
                    node: 4,
                },
            ],
            ..Scope::default()
        },
    }
}

fn root() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary)
}

fn capability(
    node: NodeId,
    request: Option<crate::OwnedSourceDiagnosticRequest>,
    reason: crate::OwnedSourceCapabilityReason,
    origin: NodeId,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: crate::OwnedSourceDiagnostic::Capability {
            location: crate::OwnedSourceDiagnosticLocation::Expression {
                namespace: crate::OwnedSourceNamespace::Main,
                node,
                scope: Some(root()),
            },
            request,
            reason,
            origin: crate::OwnedSourceCapabilityOrigin::Expression {
                namespace: crate::OwnedSourceNamespace::Main,
                node: origin,
            },
        }
        .into(),
    }
}

fn first_request() -> Option<crate::OwnedSourceDiagnosticRequest> {
    Some(crate::OwnedSourceDiagnosticRequest::Iteration {
        owner: crate::OwnedSourceDiagnosticOwner::Primary,
        path: vec!["Rows".into()],
    })
}

fn as_diagnostic(error: ProgramValidationError) -> Diagnostic {
    match error {
        ProgramValidationError::OwnedSource { diagnostic } => {
            Diagnostic::OwnedSource { diagnostic }
        }
        _ => unreachable!("authored owned refusal"),
    }
}

#[test]
fn the_owned_join_profile_lowers_joined_aggregates_and_older_routes_refuse() {
    let project = aggregate_project();
    assert_eq!(engine::validate(&project), vec![]);
    let program = crate::lower_with_basic_owned_joins(&project).unwrap();
    let lowered = program
        .expressions
        .iter()
        .find(|node| node.id == 1)
        .map(|node| &node.expression);
    assert!(
        matches!(lowered, Some(Expression::OwnedJoinAggregate(aggregate))
        if aggregate.join == MappingJoinId::new(5) && aggregate.plan == plan()
            && aggregate.expression == Some(2) && aggregate.arg == Some(3))
    );
    crate::validate_program_with_basic_owned_joins(&program).unwrap();
    crate::validate_basic_owned_joins_emission(&program).unwrap();
    let plan = crate::legacy_named_input_admission_plan_with_basic_owned_joins(&program).unwrap();
    // Joined aggregate sources are demand-driven; the endpoint stays optional.
    assert_eq!(
        plan.all_outputs
            .entries
            .iter()
            .map(|entry| format!("{:?}", entry.availability))
            .collect::<Vec<_>>(),
        ["Optional"]
    );
    let expected = capability(
        1,
        first_request(),
        crate::OwnedSourceCapabilityReason::Join,
        1,
    );
    for result in [
        validate_program(&program),
        crate::validate_owned_emission(&program),
        crate::validate_basic_owned_emission(&program),
        crate::validate_program_with_basic_owned_lookup_find(&program),
        crate::validate_basic_owned_lookup_find_emission(&program),
        crate::validate_program_with_basic_owned_collection_aggregates(&program),
        crate::validate_basic_owned_collection_aggregates_emission(&program),
        crate::validate_program_with_basic_owned_xml_serialize(&program),
        crate::validate_program_with_basic_owned_xml_mixed_content(&program),
        crate::validate_basic_owned_xml_mixed_content_emission(&program),
        crate::validate_program_with_basic_owned_xml_mixed_construction(&program),
        crate::validate_basic_owned_xml_mixed_construction_emission(&program),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    for result in [
        lower(&project),
        crate::lower_with_basic_owned_lookup_find(&project),
        crate::lower_with_basic_owned_collection_aggregates(&project),
        crate::lower_with_basic_owned_xml_mixed_content(&project),
        crate::lower_with_basic_owned_xml_mixed_construction(&project),
    ] {
        assert_eq!(
            result.unwrap_err().diagnostics(),
            [as_diagnostic(expected.clone())]
        );
    }
}

#[test]
fn ordered_text_owned_joined_aggregates_remain_closed() {
    for function in [AggregateOp::LastPresentText, AggregateOp::LastDistinctText] {
        for reached in [true, false] {
            let mut program = crate::lower_with_basic_owned_joins(&aggregate_project()).unwrap();
            let expression = &mut program
                .expressions
                .iter_mut()
                .find(|node| node.id == 1)
                .unwrap()
                .expression;
            let Expression::OwnedJoinAggregate(aggregate) = expression else {
                panic!("authored joined aggregate");
            };
            aggregate.function = function;
            if !reached {
                program
                    .root
                    .bindings
                    .retain(|binding| binding.expression != 1);
            }
            let error = crate::validate_basic_owned_joins_emission(&program).unwrap_err();
            assert!(
                matches!(error, ProgramValidationError::OwnedSource { diagnostic }
                if matches!(diagnostic.as_ref(), crate::OwnedSourceDiagnostic::Capability {
                    location: crate::OwnedSourceDiagnosticLocation::Expression { node: 1, .. },
                    reason: crate::OwnedSourceCapabilityReason::Join, ..
                }))
            );
        }
        let mut project = aggregate_project();
        let Node::OwnedJoinAggregate(aggregate) = project.graph.nodes.get_mut(&1).unwrap() else {
            panic!("authored joined aggregate");
        };
        aggregate.function = function;
        assert!(crate::lower_with_basic_owned_joins(&project).is_err());
    }
}

#[test]
fn decimal_sum_is_admitted_through_owned_joined_aggregates() {
    for reached in [true, false] {
        let mut program = crate::lower_with_basic_owned_joins(&aggregate_project()).unwrap();
        let expression = &mut program
            .expressions
            .iter_mut()
            .find(|node| node.id == 1)
            .unwrap()
            .expression;
        let Expression::OwnedJoinAggregate(aggregate) = expression else {
            panic!("authored joined aggregate");
        };
        aggregate.function = AggregateOp::DecimalSum;
        aggregate.arg = None;
        if !reached {
            program
                .root
                .bindings
                .retain(|binding| binding.expression != 1);
        }
        assert!(crate::validate_basic_owned_joins_emission(&program).is_ok());
    }
    let mut project = aggregate_project();
    let Node::OwnedJoinAggregate(aggregate) = project.graph.nodes.get_mut(&1).unwrap() else {
        panic!("authored joined aggregate");
    };
    aggregate.function = AggregateOp::DecimalSum;
    aggregate.arg = None;
    assert!(crate::lower_with_basic_owned_joins(&project).is_ok());
}

#[test]
fn tuple_expressions_admit_only_their_join_reads_and_pure_scalars() {
    let tuple_refusal =
        |request| capability(2, request, crate::OwnedSourceCapabilityReason::Join, 1);
    for (node, request) in [
        (
            Node::SourceField {
                path: vec!["Rows".into(), "id".into()],
                frame: None,
            },
            None,
        ),
        (
            Node::OwnedSourceField(mapping::OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: vec!["Rows".into(), "id".into()],
                },
                frame: None,
            }),
            Some(crate::OwnedSourceDiagnosticRequest::Field {
                owner: crate::OwnedSourceDiagnosticOwner::Primary,
                path: vec!["Rows".into(), "id".into()],
                frame: None,
            }),
        ),
        // A joined aggregate never nests inside another tuple.
        (joined(None, None), first_request()),
    ] {
        let mut project = aggregate_project();
        project.graph.nodes.insert(2, node);
        assert_eq!(
            crate::lower_with_basic_owned_joins(&project)
                .unwrap_err()
                .diagnostics(),
            [as_diagnostic(tuple_refusal(request))]
        );
    }
    // Pure scalar nodes and the flattened position are admitted.
    let mut project = aggregate_project();
    project.graph.nodes.insert(
        2,
        Node::Call {
            function: "concat".into(),
            args: vec![7, 8],
        },
    );
    project.graph.nodes.insert(7, join_field(5, named_rows()));
    project.graph.nodes.insert(
        8,
        Node::JoinPosition {
            join: MappingJoinId::new(5),
        },
    );
    let program = crate::lower_with_basic_owned_joins(&project).unwrap();
    crate::validate_basic_owned_joins_emission(&program).unwrap();
}

#[test]
fn direct_programs_bind_tuple_reads_to_the_aggregate_join_only() {
    let program = crate::lower_with_basic_owned_joins(&aggregate_project()).unwrap();
    let replace = |id, expression| {
        let mut program = program.clone();
        program
            .expressions
            .iter_mut()
            .find(|node| node.id == id)
            .expect("authored node")
            .expression = expression;
        program
    };
    let read = |join: u64, collection: OwnedSourceRef| {
        Expression::OwnedJoinField(mapping::OwnedJoinField {
            join: MappingJoinId::new(join),
            collection,
            path: vec!["id".into()],
        })
    };
    let other = OwnedSourceRef {
        owner: SourceOwner::Named {
            name: "lookup".into(),
        },
        path: vec!["Other".into()],
    };
    for (program, expected) in [
        // The other aggregate's join is not active in this tuple.
        (
            replace(2, read(6, named_rows())),
            ProgramValidationError::InactiveJoinExpression {
                node: 2,
                join: crate::JoinId::new(6),
            },
        ),
        (
            replace(2, read(5, other.clone())),
            ProgramValidationError::InvalidOwnedJoinMember {
                node: 2,
                join: crate::JoinId::new(5),
                collection: other,
            },
        ),
        // The argument evaluates in the parent, where the tuple is inactive.
        (
            replace(3, read(5, named_rows())),
            ProgramValidationError::InactiveJoinExpression {
                node: 3,
                join: crate::JoinId::new(5),
            },
        ),
    ] {
        for result in [
            crate::validate_program_with_basic_owned_joins(&program),
            crate::validate_basic_owned_joins_emission(&program),
            validate_program(&program),
        ] {
            assert_eq!(result, Err(expected.clone()));
        }
    }
}

#[test]
fn closed_contexts_refuse_joined_aggregates_in_the_owned_join_profile() {
    // A failure predicate keeps its closed scalar subset.
    let mut project = aggregate_project();
    project.graph.nodes.insert(
        9,
        Node::Call {
            function: "equal".into(),
            args: vec![4, 10],
        },
    );
    project.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(0),
        },
    );
    project.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: Vec::new(),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 9 },
        message: None,
    });
    let error = crate::lower_with_basic_owned_joins(&project).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if matches!(diagnostic.as_ref(), crate::OwnedSourceDiagnostic::Capability {
            reason: crate::OwnedSourceCapabilityReason::FailureRule, ..
        })),
        "{error:?}"
    );
    // A dynamic named owner is never a static endpoint.
    let mut project = aggregate_project();
    project.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::String("lookup.json".into()),
        },
    );
    project.extra_sources[0].dynamic_path =
        Some(mapping::DynamicSourcePath::legacy(11, Vec::new()));
    let error = crate::lower_with_basic_owned_joins(&project).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if matches!(diagnostic.as_ref(), crate::OwnedSourceDiagnostic::Capability {
            reason: crate::OwnedSourceCapabilityReason::DynamicOwner, ..
        })),
        "{error:?}"
    );
}
