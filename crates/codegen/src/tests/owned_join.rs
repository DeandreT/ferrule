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
