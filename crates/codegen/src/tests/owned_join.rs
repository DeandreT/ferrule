use super::*;
use mapping::{
    JoinId as MappingJoinId, NamedSource, OwnedJoinConditions, OwnedJoinKey, OwnedJoinPlan,
    OwnedJoinSource, OwnedSourceRef, SourceOwner, SourceScopeLocation, SourceScopeStep,
    SourceScopeTarget,
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

fn owned_join_project() -> Project {
    let named = || SourceOwner::Named {
        name: "lookup".into(),
    };
    let plan = OwnedJoinPlan::new(
        OwnedJoinSource::new(rows_of(SourceOwner::Primary)),
        OwnedJoinSource::new(rows_of(named())),
        OwnedJoinConditions::new(OwnedJoinKey::new(
            rows_of(SourceOwner::Primary),
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap();
    Project {
        source: rows(),
        target: SchemaNode::group("Target", vec![SchemaNode::group("Row", vec![]).repeating()]),
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
        graph: Graph::default(),
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::OwnedInnerJoin {
                    id: MappingJoinId::new(5),
                    plan,
                },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn owned_joins_are_refused_before_lowering_in_every_profile() {
    let project = owned_join_project();
    assert!(engine::validate(&project).is_empty());
    let scope =
        SourceScopeLocation::root(SourceScopeTarget::Primary).descendant(SourceScopeStep::Child(0));
    let expected = [Diagnostic::OwnedSource {
        diagnostic: crate::OwnedSourceDiagnostic::Capability {
            location: crate::OwnedSourceDiagnosticLocation::Iteration {
                scope: scope.clone(),
            },
            request: Some(crate::OwnedSourceDiagnosticRequest::Iteration {
                owner: crate::OwnedSourceDiagnosticOwner::Primary,
                path: vec!["Rows".into()],
            }),
            reason: crate::OwnedSourceCapabilityReason::Join,
            origin: crate::OwnedSourceCapabilityOrigin::Scope { scope },
        }
        .into(),
    }];
    for result in [
        lower(&project),
        crate::lower_with_basic_owned_lookup_find(&project),
        crate::lower_with_basic_owned_recursive_filter(&project),
    ] {
        assert_eq!(result.unwrap_err().diagnostics(), expected);
    }
}

fn named_rows() -> OwnedSourceRef {
    rows_of(SourceOwner::Named {
        name: "lookup".into(),
    })
}

fn join_field(join: u64, collection: OwnedSourceRef, path: &[&str]) -> Node {
    Node::OwnedJoinField(mapping::OwnedJoinField {
        join: MappingJoinId::new(join),
        collection,
        path: path.iter().map(|segment| (*segment).into()).collect(),
    })
}

/// The owned join binds both same-path members, a raw position and the
/// flattened position, with a filter, a sort and a parent window bound.
fn tuple_project() -> Project {
    let mut project = owned_join_project();
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    typed_scalar("left", ScalarType::Int),
                    typed_scalar("right", ScalarType::Int),
                    typed_scalar("at", ScalarType::Int),
                    typed_scalar("tuple", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    for (id, node) in [
        (1, join_field(5, rows_of(SourceOwner::Primary), &["id"])),
        (2, join_field(5, named_rows(), &["id"])),
        (
            3,
            Node::OwnedJoinSourcePosition(mapping::OwnedJoinSourcePosition {
                join: MappingJoinId::new(5),
                collection: named_rows(),
            }),
        ),
        (
            4,
            Node::JoinPosition {
                join: MappingJoinId::new(5),
            },
        ),
        (
            5,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(2),
            },
        ),
    ] {
        project.graph.nodes.insert(id, node);
    }
    let row = &mut project.root.children[0];
    row.bindings = ["left", "right", "at", "tuple"]
        .into_iter()
        .zip(1..)
        .map(|(target_field, node)| mapping::Binding {
            target_field: target_field.into(),
            node,
        })
        .collect();
    row.filter = Some(5);
    row.sort_by = Some(3);
    row.windows = vec![mapping::SequenceWindow::First { count: 6 }];
    project
}

fn join_scope() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary).descendant(SourceScopeStep::Child(0))
}

fn join_capability(
    location: crate::OwnedSourceDiagnosticLocation,
    request: Option<crate::OwnedSourceDiagnosticRequest>,
    reason: crate::OwnedSourceCapabilityReason,
    origin: crate::OwnedSourceCapabilityOrigin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: crate::OwnedSourceDiagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }
        .into(),
    }
}

fn as_diagnostic(error: ProgramValidationError) -> Diagnostic {
    match error {
        ProgramValidationError::OwnedSource { diagnostic } => {
            Diagnostic::OwnedSource { diagnostic }
        }
        _ => unreachable!("authored owned refusal"),
    }
}

fn first_request() -> Option<crate::OwnedSourceDiagnosticRequest> {
    Some(crate::OwnedSourceDiagnosticRequest::Iteration {
        owner: crate::OwnedSourceDiagnosticOwner::Primary,
        path: vec!["Rows".into()],
    })
}

#[test]
fn the_owned_join_profile_lowers_full_identities_and_older_validators_refuse() {
    let project = tuple_project();
    assert_eq!(engine::validate(&project), vec![]);
    let program = crate::lower_with_basic_owned_joins(&project).unwrap();
    let join = program.root.children[0]
        .iteration
        .as_ref()
        .and_then(IterationPlan::owned_inner_join)
        .expect("owned join iteration");
    assert_eq!(join.id(), crate::JoinId::new(5));
    assert_eq!(
        join.plan()
            .sources()
            .map(|source| source.collection().clone())
            .collect::<Vec<_>>(),
        [rows_of(SourceOwner::Primary), named_rows()]
    );
    crate::validate_program_with_basic_owned_joins(&program).unwrap();
    crate::validate_basic_owned_joins_emission(&program).unwrap();
    let plan = crate::legacy_named_input_admission_plan_with_basic_owned_joins(&program).unwrap();
    // An owned join source is demand-driven; the endpoint stays optional.
    assert_eq!(
        plan.all_outputs
            .entries
            .iter()
            .map(|entry| format!("{:?}", entry.availability))
            .collect::<Vec<_>>(),
        ["Optional"]
    );
    let expected = join_capability(
        crate::OwnedSourceDiagnosticLocation::Iteration {
            scope: join_scope(),
        },
        first_request(),
        crate::OwnedSourceCapabilityReason::Join,
        crate::OwnedSourceCapabilityOrigin::Scope {
            scope: join_scope(),
        },
    );
    for result in [
        validate_program(&program),
        crate::validate_owned_emission(&program),
        crate::validate_program_with_basic_owned_lookup_find(&program),
        crate::validate_basic_owned_lookup_find_emission(&program),
        crate::validate_program_with_basic_owned_recursive_filter(&program),
        crate::validate_program_with_basic_owned_windows(&program),
        crate::validate_basic_owned_emission(&program),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    assert_eq!(
        lower(&project).unwrap_err().diagnostics(),
        [as_diagnostic(expected)]
    );
}

#[test]
fn tuple_contexts_and_descendants_stay_closed_outside_join_reads() {
    let tuple_refusal = |node| {
        join_capability(
            crate::OwnedSourceDiagnosticLocation::Expression {
                namespace: crate::OwnedSourceNamespace::Main,
                node,
                scope: Some(join_scope()),
            },
            None,
            crate::OwnedSourceCapabilityReason::Join,
            crate::OwnedSourceCapabilityOrigin::Scope {
                scope: join_scope(),
            },
        )
    };
    // A legacy read of the tuple and an owned read of another occurrence.
    for (node, expression) in [
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
    ] {
        let mut project = tuple_project();
        project.graph.nodes.insert(1, node);
        let program = crate::lower_with_basic_owned_joins(&project);
        let mut expected = tuple_refusal(1);
        if let ProgramValidationError::OwnedSource { diagnostic } = &mut expected
            && let crate::OwnedSourceDiagnostic::Capability { request, .. } = diagnostic.as_mut()
        {
            *request = expression;
        }
        assert_eq!(
            program.unwrap_err().diagnostics(),
            [as_diagnostic(expected)]
        );
    }
    // A nested iteration below the tuple is refused at that child.
    let mut project = tuple_project();
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::group("Nested", vec![]).repeating()])
                .repeating(),
        ],
    );
    let row = &mut project.root.children[0];
    row.bindings.clear();
    row.children = vec![Scope {
        target_field: "Nested".into(),
        iteration: ScopeIteration::OwnedSource {
            source: rows_of(SourceOwner::Primary),
            output_path: None,
        },
        ..Scope::default()
    }];
    let child = join_scope().descendant(SourceScopeStep::Child(0));
    assert_eq!(
        crate::lower_with_basic_owned_joins(&project)
            .unwrap_err()
            .diagnostics(),
        [as_diagnostic(join_capability(
            crate::OwnedSourceDiagnosticLocation::Iteration { scope: child },
            None,
            crate::OwnedSourceCapabilityReason::Join,
            crate::OwnedSourceCapabilityOrigin::Scope {
                scope: join_scope(),
            },
        ))]
    );
}

#[test]
fn direct_programs_bind_each_read_to_an_active_owned_member() {
    let program = crate::lower_with_basic_owned_joins(&tuple_project()).unwrap();
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
    let other = OwnedSourceRef {
        owner: SourceOwner::Named {
            name: "lookup".into(),
        },
        path: vec!["Other".into()],
    };
    for (program, expected) in [
        (
            replace(
                1,
                Expression::OwnedJoinField(mapping::OwnedJoinField {
                    join: MappingJoinId::new(9),
                    collection: rows_of(SourceOwner::Primary),
                    path: vec!["id".into()],
                }),
            ),
            ProgramValidationError::InactiveJoinExpression {
                node: 1,
                join: crate::JoinId::new(9),
            },
        ),
        (
            replace(
                1,
                Expression::OwnedJoinField(mapping::OwnedJoinField {
                    join: MappingJoinId::new(5),
                    collection: other.clone(),
                    path: vec!["id".into()],
                }),
            ),
            ProgramValidationError::InvalidOwnedJoinMember {
                node: 1,
                join: crate::JoinId::new(5),
                collection: other,
            },
        ),
        (
            replace(
                1,
                Expression::OwnedJoinField(mapping::OwnedJoinField {
                    join: MappingJoinId::new(5),
                    collection: named_rows(),
                    path: vec!["missing".into()],
                }),
            ),
            ProgramValidationError::InvalidOwnedJoinFieldPath {
                node: 1,
                join: crate::JoinId::new(5),
                collection: named_rows(),
                path: vec!["missing".into()],
            },
        ),
        (
            replace(
                1,
                Expression::JoinField {
                    join: crate::JoinId::new(5),
                    collection: vec!["Rows".into()],
                    path: vec!["id".into()],
                },
            ),
            ProgramValidationError::InactiveJoinExpression {
                node: 1,
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
