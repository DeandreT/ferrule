use super::*;
use crate::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    TargetConstruction,
};
use mapping::{
    NamedSource, OwnedPosition, OwnedSourceField, OwnedSourceRef, RecursiveFilterPlan, SourceOwner,
    SourceScopeLocation, SourceScopeStep, SourceScopeTarget,
};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn directory() -> SchemaNode {
    SchemaNode::group(
        "Dir",
        vec![
            scalar("name"),
            SchemaNode::group("file", vec![scalar("name")]).repeating(),
            SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}

fn input() -> SchemaNode {
    SchemaNode::group("Input", vec![scalar("Suffix"), directory().repeating()])
}

fn lookup() -> SourceOwner {
    SourceOwner::Named {
        name: "Lookup".into(),
    }
}

fn owned_field(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: None,
    })
}

/// The Lookup filter keeps files ending with the Primary suffix whose item
/// position is one; the predicate reads two owners and an owned position.
fn owned_project() -> Project {
    let mut project = supported_project();
    project.source = input();
    project.target = SchemaNode::group("Out", vec![directory().repeating()]);
    project.extra_sources = vec![NamedSource {
        name: "Lookup".into(),
        path: "lookup.json".into(),
        schema: input(),
        options: Default::default(),
        dynamic_path: None,
    }];
    project.graph = Graph {
        nodes: BTreeMap::from([
            (1, owned_field(lookup(), &["Dir", "file", "name"])),
            (2, owned_field(SourceOwner::Primary, &["Suffix"])),
            (
                3,
                Node::Call {
                    function: "ends_with".into(),
                    args: vec![1, 2],
                },
            ),
            (
                4,
                Node::OwnedPosition(OwnedPosition {
                    owner: lookup(),
                    collection: Some(path(&["Dir", "file"])),
                }),
            ),
            (
                5,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                6,
                Node::Call {
                    function: "equal".into(),
                    args: vec![4, 5],
                },
            ),
            (
                7,
                Node::Call {
                    function: "and".into(),
                    args: vec![3, 6],
                },
            ),
        ]),
    };
    project.root = Scope {
        children: vec![Scope {
            target_field: "Dir".into(),
            iteration: mapping::ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: lookup(),
                    path: path(&["Dir"]),
                },
                output_path: None,
            },
            construction: ScopeConstruction::RecursiveFilter {
                plan: RecursiveFilterPlan::new("Dir".into(), "file".into(), 7).unwrap(),
            },
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

fn child() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary).descendant(SourceScopeStep::Child(0))
}

fn closed(reason: Capability) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: Location::Iteration { scope: child() },
            request: Some(Request::Iteration {
                owner: Owner::Named {
                    name: "Lookup".into(),
                },
                path: path(&["Dir"]),
            }),
            reason,
            origin: Origin::Scope { scope: child() },
        }),
    }
}

#[test]
fn owned_filter_lowers_unchanged_and_only_its_companion_admits_it() {
    let project = owned_project();
    let program = crate::lower_with_basic_owned_recursive_filter(&project).unwrap();
    assert_eq!(
        program.root.children[0].construction,
        TargetConstruction::RecursiveFilter {
            children: "Dir".into(),
            items: "file".into(),
            predicate: 7,
        }
    );
    let Some(iteration) = &program.root.children[0].iteration else {
        panic!("owned iteration lowers");
    };
    assert_eq!(
        iteration
            .owned_source_iteration()
            .map(|source| source.source()),
        Some(&OwnedSourceRef {
            owner: lookup(),
            path: path(&["Dir"]),
        })
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7]
    );
    crate::validate_basic_owned_recursive_filter_emission(&program).unwrap();
    let plan = crate::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(&program)
        .unwrap();
    assert_eq!(
        plan.all_outputs
            .entries
            .iter()
            .map(|entry| (entry.name, entry.availability))
            .collect::<Vec<_>>(),
        [("Lookup", crate::NamedInputAvailability::Optional)]
    );

    let expected = closed(Capability::TreeOperator);
    for result in [
        validate_program(&program),
        crate::validate_program_with_basic_owned_recursive_collect(&program),
        crate::validate_basic_owned_recursive_collect_emission(&program),
        crate::validate_program_with_basic_owned_adjacency_tree(&program),
        crate::validate_basic_owned_adjacency_tree_emission(&program),
        crate::validate_program_with_basic_owned_path_hierarchy(&program),
        crate::validate_program_with_basic_owned_group_into_blocks(&program),
        crate::validate_program_with_basic_owned_failure_sources(&program),
        crate::validate_program_with_basic_owned_group_ending_with(&program),
        crate::validate_program_with_basic_owned_windows(&program),
        crate::validate_program_with_basic_owned_sequence_exists(&program),
        crate::validate_basic_owned_sequence_exists_emission(&program),
        crate::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(&program)
            .map(drop),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    for lowered in [
        lower(&project),
        crate::lower_with_basic_owned_recursive_collect(&project),
        crate::lower_with_basic_owned_adjacency_tree(&project),
        crate::lower_with_basic_owned_group_into_blocks(&project),
        crate::lower_with_basic_owned_sequence_exists(&project),
    ] {
        let Err(error) = lowered else {
            panic!("older lowering keeps owned filters closed");
        };
        let ProgramValidationError::OwnedSource { diagnostic } = expected.clone() else {
            unreachable!();
        };
        assert_eq!(
            error.diagnostics(),
            [Diagnostic::OwnedSource { diagnostic }]
        );
    }
}

#[test]
fn owned_filter_structure_uses_the_endpoint_schema_and_legacy_checks() {
    let program = crate::lower_with_basic_owned_recursive_filter(&owned_project()).unwrap();
    let validate = crate::validate_program_with_basic_owned_recursive_filter;
    let mut missing = program.clone();
    missing.root.children[0].iteration = Some(crate::IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Named {
            name: "missing".into(),
        },
        path: path(&["Dir"]),
    }));
    // Ordinary structural precedence runs first: the unknown owner supplies
    // no group schema, so the legacy plan diagnostic is reported.
    assert_eq!(
        validate(&missing),
        Err(
            ProgramValidationError::RecursiveFilterConstructionRequiresGroupSource {
                target_path: path(&["Dir"]),
            }
        )
    );

    // The legacy plan checks keep their diagnostics on the owned schema.
    let mut narrowed = program.clone();
    narrowed.extra_sources[0].source = SchemaNode::group(
        "Input",
        vec![
            scalar("Suffix"),
            SchemaNode::group(
                "Dir",
                vec![
                    scalar("name"),
                    SchemaNode::group("file", vec![scalar("name")]).repeating(),
                    SchemaNode::group("Dir", vec![scalar("name")]).repeating(),
                ],
            )
            .repeating(),
        ],
    );
    assert!(matches!(
        validate(&narrowed),
        Err(
            ProgramValidationError::RecursiveFilterConstructionRequiresMatchingGroups { .. }
                | ProgramValidationError::InvalidRecursiveFilterChildren { .. }
        )
    ));
}

#[test]
fn owned_filter_closed_contexts_and_legacy_filters_keep_refusing() {
    let program = crate::lower_with_basic_owned_recursive_filter(&owned_project()).unwrap();
    let validate = crate::validate_basic_owned_recursive_filter_emission;

    // First output is not an admitted owned-filter control.
    let mut first = program.clone();
    first.target = SchemaNode::group("Out", vec![directory()]);
    first.root.children[0].repeating = false;
    first.root.children[0].iteration = Some(crate::IterationPlan::new(
        crate::OwnedSourceIteration::new(OwnedSourceRef {
            owner: lookup(),
            path: path(&["Dir"]),
        }),
        None,
        None,
        Vec::new(),
        crate::IterationOutput::First,
    ));
    let result = validate(&first);
    assert!(
        matches!(
            &result,
            Err(ProgramValidationError::OwnedSource { diagnostic })
                if matches!(**diagnostic, OwnedSourceDiagnostic::Capability { .. })
        ),
        "{result:?}"
    );

    // An aggregate in the owned predicate keeps its closed capability.
    let mut aggregate = program.clone();
    aggregate.expressions.push(crate::ExpressionNode {
        id: 8,
        expression: Expression::Aggregate {
            function: crate::AggregateFunction::Count,
            collection: path(&["Dir", "file"]),
            value: crate::AggregateValue::Path(Vec::new()),
            arg: None,
        },
    });
    if let Some(node) = aggregate.expressions.iter_mut().find(|node| node.id == 6) {
        node.expression = Expression::Call {
            function: crate::ScalarFunction::Equal,
            args: vec![8, 5],
        };
    }
    assert!(matches!(
        validate(&aggregate),
        Err(ProgramValidationError::OwnedSource { diagnostic })
            if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
                reason: Capability::AggregateLookup,
                ..
            })
    ));

    // A legacy filter beside owned reads stays a closed tree operator.
    let mut legacy = program.clone();
    legacy.root.children[0].iteration = Some(crate::IterationPlan::source(path(&["Dir"])));
    assert!(matches!(
        validate(&legacy),
        Err(ProgramValidationError::OwnedSource { diagnostic })
            if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
                reason: Capability::TreeOperator,
                ..
            })
    ));
}
