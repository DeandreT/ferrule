//! #475 owned dynamic drivers: lowering, structure and closed generation gates.
use crate::*;
use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, Node, OwnedSourceField, OwnedSourceRef, Scope, ScopeIteration,
    SourceOwner,
};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
fn owned(owner: SourceOwner, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path(parts),
    }
}
fn docs() -> SourceOwner {
    SourceOwner::Named {
        name: "Docs".into(),
    }
}
fn batch(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::group(
                "Files",
                vec![
                    SchemaNode::scalar("Path", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}
/// `Docs` loads per Primary `Files` row; `loading` adds a scope that loads it.
pub(crate) fn project(loading: bool) -> mapping::Project {
    let mut graph = mapping::Graph::default();
    graph.nodes.insert(
        1,
        Node::OwnedSourceField(OwnedSourceField {
            source: owned(SourceOwner::Primary, &["Files", "Path"]),
            frame: Some(path(&["Files"])),
        }),
    );
    graph.nodes.insert(
        2,
        Node::OwnedSourceField(OwnedSourceField {
            source: owned(docs(), &["Title"]),
            frame: None,
        }),
    );
    mapping::Project {
        source: batch("Batch"),
        target: SchemaNode::group(
            "Out",
            vec![
                SchemaNode::group(
                    "Docs",
                    vec![SchemaNode::scalar("Title", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![
            mapping::NamedSource {
                name: "Catalog".into(),
                path: "catalog.json".into(),
                schema: batch("Catalog"),
                options: Default::default(),
                dynamic_path: None,
            },
            mapping::NamedSource {
                name: "Docs".into(),
                path: String::new(),
                schema: SchemaNode::group(
                    "Doc",
                    vec![SchemaNode::scalar("Title", ScalarType::String)],
                ),
                options: Default::default(),
                dynamic_path: Some(DynamicSourcePath::owned(
                    1,
                    owned(SourceOwner::Primary, &["Files"]),
                )),
            },
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            children: loading
                .then(|| Scope {
                    target_field: "Docs".into(),
                    iteration: ScopeIteration::OwnedSource {
                        source: owned(docs(), &[]),
                        output_path: None,
                    },
                    bindings: vec![Binding {
                        target_field: "Title".into(),
                        node: 2,
                    }],
                    ..Scope::default()
                })
                .into_iter()
                .collect(),
            ..Scope::default()
        },
    }
}

fn driver_refusal() -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::DynamicDriver {
                index: 1,
                name: "Docs".into(),
            },
            request: Some(OwnedSourceDiagnosticRequest::Iteration {
                owner: OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Files"]),
            }),
            reason: OwnedSourceCapabilityReason::DynamicLoader,
            origin: OwnedSourceCapabilityOrigin::DynamicSource {
                index: 1,
                name: "Docs".into(),
            },
        }),
    }
}

/// Lowers without capability admission so closed gates can be checked directly.
fn program(loading: bool) -> Program {
    crate::lower::lower_with_validator(&project(loading), |_| Ok(())).expect("lowers")
}

#[test]
fn lowering_retains_the_exact_owned_driver_and_refuses_it_by_default() {
    let error = lower(&project(false)).expect_err("owned drivers stay closed");
    assert_eq!(
        error.diagnostics(),
        [Diagnostic::OwnedSource {
            diagnostic: match driver_refusal() {
                ProgramValidationError::OwnedSource { diagnostic } => diagnostic,
                _ => unreachable!(),
            }
        }]
    );
    let program = program(false);
    let dynamic = program.extra_sources[1].dynamic.as_ref().expect("dynamic");
    assert_eq!(
        dynamic.driver.owned(),
        Some(&owned(SourceOwner::Primary, &["Files"]))
    );
    assert!(dynamic.driver.legacy().is_none());
    assert_eq!(validate_program(&program), Err(driver_refusal()));
    assert_eq!(
        validate_program_with_basic_owned_dynamic_field(&program),
        Err(driver_refusal())
    );
    assert_eq!(
        validate_program_with_basic_owned_joins(&program),
        Err(driver_refusal())
    );
    assert_eq!(validate_owned_emission(&program), Err(driver_refusal()));
}

#[test]
fn owned_driver_structure_is_checked_before_capability() {
    let mut program = program(false);
    program.extra_sources[1]
        .dynamic
        .as_mut()
        .expect("dynamic")
        .driver = OwnedSourceIteration::new(owned(
        SourceOwner::Named {
            name: "Missing".into(),
        },
        &["Files"],
    ))
    .into();
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
                location: OwnedSourceDiagnosticLocation::DynamicDriver {
                    index: 1,
                    name: "Docs".into(),
                },
                request: OwnedSourceDiagnosticRequest::Iteration {
                    owner: OwnedSourceDiagnosticOwner::Named {
                        name: "Missing".into()
                    },
                    path: path(&["Files"]),
                },
                reason: OwnedSourceStructureReason::UnknownOwner,
            }),
        })
    );
    program.extra_sources[1]
        .dynamic
        .as_mut()
        .expect("dynamic")
        .driver = OwnedSourceIteration::new(owned(SourceOwner::Primary, &["Files", "Path"])).into();
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::OwnedSource { diagnostic })
            if matches!(*diagnostic, OwnedSourceDiagnostic::Structure {
                reason: OwnedSourceStructureReason::InvalidOwnedCollection { .. },
                ..
            })
    ));
}

#[test]
fn loading_scopes_stay_closed_as_dynamic_owners() {
    let error = lower(&project(true)).expect_err("owned loading stays closed");
    assert!(
        matches!(
            error.diagnostics(),
            [Diagnostic::OwnedSource { diagnostic }]
                if matches!(**diagnostic, OwnedSourceDiagnostic::Capability {
                    reason: OwnedSourceCapabilityReason::DynamicOwner,
                    location: OwnedSourceDiagnosticLocation::Iteration { .. },
                    ..
                })
        ),
        "{error:?}"
    );
}
