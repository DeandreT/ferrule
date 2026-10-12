use super::*;
use mapping::{
    OwnedSourceRef, SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget,
};
fn iteration() -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec![],
        },
        output_path: None,
    }
}
#[test]
fn valid_owned_scope_is_typed_refusal_before_any_backend_program_exists() {
    let mut p = supported_project();
    p.root.iteration = iteration();
    assert!(
        engine::validate(&p).is_empty(),
        "{:?}",
        engine::validate(&p)
    );
    let result = lower(&p);
    eprintln!("A-V03 exact input={p:#?} lower={result:#?}");
    let program = result.unwrap();
    let input = program.root.iteration.as_ref().unwrap();
    assert_eq!(
        input.owned_source_iteration().unwrap().source(),
        &OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec![]
        }
    );
    assert_eq!(input.source_iteration(), None);
    let gate = crate::validate_owned_emission(&program);
    eprintln!("A-V03 gate={gate:#?}");
    assert_eq!(
        gate,
        Err(ProgramValidationError::OwnedSource {
            diagnostic: crate::OwnedSourceDiagnostic::Capability {
                location: crate::OwnedSourceDiagnosticLocation::Iteration {
                    scope: SourceScopeLocation::root(SourceScopeTarget::Primary)
                },
                request: Some(crate::OwnedSourceDiagnosticRequest::Iteration {
                    owner: crate::OwnedSourceDiagnosticOwner::Primary,
                    path: vec![]
                }),
                reason: crate::OwnedSourceCapabilityReason::UnsupportedBasicContext,
                origin: crate::OwnedSourceCapabilityOrigin::Scope {
                    scope: SourceScopeLocation::root(SourceScopeTarget::Primary)
                },
            }
            .into()
        })
    );
}
#[test]
fn named_and_nested_iteration_refusals_retain_target_slot_and_route() {
    let mut p = supported_project();
    let mut root = p.root.clone();
    root.children[0].iteration = iteration();
    p.extra_targets.push(NamedTarget {
        name: "primary".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root,
    });
    assert!(
        engine::validate(&p).is_empty(),
        "{:?}",
        engine::validate(&p)
    );
    let result = lower(&p);
    eprintln!("A-V04 all target input={p:#?} lower={result:#?}");
    let program = result.unwrap();
    let actual = program.extra_targets[0].root.children[0]
        .iteration
        .as_ref()
        .unwrap();
    assert_eq!(
        actual.owned_source_iteration().unwrap().source(),
        &OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec![]
        }
    );
    let gate = crate::validate_owned_emission(&program);
    eprintln!("A-V04 named route gate={gate:#?}");
    let route = SourceScopeLocation {
        target: SourceScopeTarget::Named {
            index: 0,
            name: "primary".into(),
        },
        path: vec![SourceScopeStep::Child(0)],
    };
    assert_eq!(
        gate,
        Err(ProgramValidationError::OwnedSource {
            diagnostic: crate::OwnedSourceDiagnostic::Capability {
                location: crate::OwnedSourceDiagnosticLocation::Iteration {
                    scope: route.clone()
                },
                request: Some(crate::OwnedSourceDiagnosticRequest::Iteration {
                    owner: crate::OwnedSourceDiagnosticOwner::Primary,
                    path: vec![]
                }),
                reason: crate::OwnedSourceCapabilityReason::UnsupportedBasicContext,
                origin: crate::OwnedSourceCapabilityOrigin::Scope { scope: route },
            }
            .into()
        })
    );
}
#[test]
fn legacy_source_and_existing_expression_omission_contract_remain_available() {
    let mut p = supported_project();
    p.root.iteration = ScopeIteration::Source(vec![]);
    p.graph
        .nodes
        .insert(99, super::owned_source_test_node(false));
    let legacy_result = lower(&p);
    eprintln!("A-V05 complete legacy Project={p:#?} lower Result={legacy_result:#?}");
    assert!(legacy_result.is_ok());
    p.root.iteration = iteration();
    let result = lower(&p);
    eprintln!("A-V05 exact legacy/owned omission={result:#?}");
    let program = result.unwrap();
    assert!(!program.expressions.iter().any(|node| node.id == 99));
    assert_eq!(
        program
            .root
            .iteration
            .as_ref()
            .unwrap()
            .owned_source_iteration()
            .unwrap()
            .source()
            .owner,
        SourceOwner::Primary
    );
}

#[test]
fn owned_document_selector_keeps_its_output_expression_and_real_primary_route_closed() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        77,
        Node::Const {
            value: Value::String("output.json".into()),
        },
    );
    project.root.iteration = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec![],
        },
        output_path: Some(77),
    };
    let result = lower(&project);
    eprintln!("A-S07 complete document project={project:#?} lower={result:#?}");
    assert_eq!(
        result.unwrap_err().diagnostics(),
        [Diagnostic::OwnedSource {
            diagnostic: crate::OwnedSourceDiagnostic::Capability {
                location: crate::OwnedSourceDiagnosticLocation::Iteration {
                    scope: SourceScopeLocation::root(SourceScopeTarget::Primary)
                },
                request: Some(crate::OwnedSourceDiagnosticRequest::Iteration {
                    owner: crate::OwnedSourceDiagnosticOwner::Primary,
                    path: vec![]
                }),
                reason: crate::OwnedSourceCapabilityReason::Document,
                origin: crate::OwnedSourceCapabilityOrigin::Expression {
                    namespace: crate::OwnedSourceNamespace::Main,
                    node: 77
                },
            }
            .into()
        }]
    );
}
