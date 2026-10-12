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
    assert_eq!(
        lower(&p).unwrap_err().diagnostics(),
        [Diagnostic::UnsupportedSourceIteration {
            scope: SourceScopeLocation::root(SourceScopeTarget::Primary),
        }]
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
    assert_eq!(
        lower(&p).unwrap_err().diagnostics(),
        [Diagnostic::UnsupportedSourceIteration {
            scope: SourceScopeLocation {
                target: SourceScopeTarget::Named {
                    index: 0,
                    name: "primary".into()
                },
                path: vec![SourceScopeStep::Child(0)]
            },
        }]
    );
}
#[test]
fn legacy_source_and_existing_expression_omission_contract_remain_available() {
    let mut p = supported_project();
    p.root.iteration = ScopeIteration::Source(vec![]);
    p.graph
        .nodes
        .insert(99, super::owned_source_test_node(false));
    assert!(lower(&p).is_ok());
    p.root.iteration = iteration();
    assert!(
        lower(&p)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic, Diagnostic::UnsupportedSourceIteration { .. }))
    );
}
