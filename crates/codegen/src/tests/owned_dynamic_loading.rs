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
                    vec![
                        SchemaNode::scalar("Title", ScalarType::String),
                        SchemaNode::group(
                            "Lines",
                            vec![SchemaNode::scalar("Code", ScalarType::String)],
                        )
                        .repeating(),
                    ],
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
    // A driver never selects its own endpoint, under every companion.
    program.extra_sources[1]
        .dynamic
        .as_mut()
        .expect("dynamic")
        .driver = OwnedSourceIteration::new(owned(docs(), &["Lines"])).into();
    let self_driven = Err(ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
            location: OwnedSourceDiagnosticLocation::DynamicDriver {
                index: 1,
                name: "Docs".into(),
            },
            request: OwnedSourceDiagnosticRequest::Iteration {
                owner: OwnedSourceDiagnosticOwner::Named {
                    name: "Docs".into(),
                },
                path: path(&["Lines"]),
            },
            reason: OwnedSourceStructureReason::SelfDrivenDynamicSource,
        }),
    });
    assert_eq!(validate_program(&program), self_driven);
    assert_eq!(
        validate_program_with_basic_owned_dynamic_loading(&program),
        self_driven
    );
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

#[test]
fn the_loading_companion_admits_exact_drivers_paths_and_document_reads() {
    let program = lower_with_basic_owned_dynamic_loading(&project(true)).expect("admitted");
    assert_eq!(
        validate_basic_owned_dynamic_loading_emission(&program),
        Ok(())
    );
    // An unused declaration requests nothing.
    let unused = lower_with_basic_owned_dynamic_loading(&project(false)).expect("admitted");
    assert_eq!(
        validate_basic_owned_dynamic_loading_emission(&unused),
        Ok(())
    );
    // Every older companion keeps refusing the loading scope and the unused
    // owned driver declaration.
    for validate in [
        validate_program,
        validate_program_with_basic_owned_dynamic_field,
        validate_program_with_basic_owned_joins,
        validate_program_with_basic_owned_xml_serialize,
        validate_program_with_basic_owned_collection_aggregates,
    ] {
        assert!(matches!(
            validate(&program),
            Err(ProgramValidationError::OwnedSource { diagnostic })
                if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
                    reason: OwnedSourceCapabilityReason::DynamicOwner,
                    location: OwnedSourceDiagnosticLocation::Iteration { .. },
                    ..
                })
        ));
        assert_eq!(validate(&unused), Err(driver_refusal()));
    }

    let scope = &program.root.children[0];
    let source = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::owned_source_iteration)
        .expect("owned loading scope")
        .source();
    let loading = owned_dynamic_loading(&program, [], source).expect("loading scope");
    assert_eq!(
        (loading.declaration_index, loading.source, loading.path),
        (1, "Docs", 1)
    );
    assert_eq!(loading.driver, &owned(SourceOwner::Primary, &["Files"]));
    // An enclosing iteration of the same owner already roots it.
    assert!(owned_dynamic_loading(&program, [source], source).is_none());
}

#[test]
fn the_loading_plan_keeps_loaded_owners_reserved_and_drivers_demand_driven() {
    let mut project = project(true);
    project.extra_sources[1].dynamic_path = Some(DynamicSourcePath::owned(
        1,
        owned(
            SourceOwner::Named {
                name: "Catalog".into(),
            },
            &["Files"],
        ),
    ));
    let Node::OwnedSourceField(field) = project.graph.nodes.get_mut(&1).expect("path") else {
        unreachable!()
    };
    field.source.owner = SourceOwner::Named {
        name: "Catalog".into(),
    };
    let program = lower_with_basic_owned_dynamic_loading(&project).expect("admitted");
    let plan =
        legacy_named_input_admission_plan_with_basic_owned_dynamic_loading(&program).expect("plan");
    assert_eq!(
        plan.all_outputs
            .entries
            .iter()
            .map(|entry| (entry.name, entry.availability))
            .collect::<Vec<_>>(),
        [
            ("Catalog", NamedInputAvailability::Optional),
            ("Docs", NamedInputAvailability::ReservedUnavailable),
        ]
    );
    assert!(legacy_named_input_admission_plan_with_basic_owned_dynamic_field(&program).is_err());
}

#[test]
fn loading_reads_stay_exact_field_and_position_reads() {
    // A loaded owner is admitted only for exact field and position reads; an
    // owned Lookup over it keeps the dynamic-owner refusal.
    let mut project = project(true);
    project.graph.nodes.insert(
        3,
        Node::OwnedLookup(mapping::OwnedLookup {
            collection: owned(docs(), &["Lines"]),
            key: path(&["Code"]),
            matches: 2,
            value: path(&["Code"]),
        }),
    );
    project.root.children[0].bindings[0].node = 3;
    let error = lower_with_basic_owned_dynamic_loading(&project).expect_err("closed");
    assert!(
        matches!(
            error.diagnostics(),
            [Diagnostic::OwnedSource { diagnostic }]
                if matches!(**diagnostic, OwnedSourceDiagnostic::Capability {
                    reason: OwnedSourceCapabilityReason::DynamicOwner,
                    ..
                })
        ),
        "{error:?}"
    );
}

#[test]
fn loading_scopes_never_group_loaded_documents() {
    // Engine validation refuses the Project; a directly lowered Program keeps
    // the capability refusal.
    let mut project = project(true);
    project.root.children[0].group_by = Some(2);
    assert!(lower_with_basic_owned_dynamic_loading(&project).is_err());
    // Lower with a static Docs, then attach the owned driver declaration and
    // its path expression.
    let loading = program(true);
    project.extra_sources[1].dynamic_path = None;
    let mut program = crate::lower::lower_with_validator(&project, |_| Ok(())).expect("lowers");
    program.extra_sources[1].dynamic = loading.extra_sources[1].dynamic.clone();
    program.expressions.extend(
        loading
            .expressions
            .iter()
            .filter(|node| node.id == 1)
            .cloned(),
    );
    program.expressions.sort_by_key(|node| node.id);
    assert!(
        matches!(
            validate_program_with_basic_owned_dynamic_loading(&program),
            Err(ProgramValidationError::OwnedSource { diagnostic })
                if matches!(*diagnostic, OwnedSourceDiagnostic::Capability {
                    reason: OwnedSourceCapabilityReason::Grouping,
                    location: OwnedSourceDiagnosticLocation::Iteration { .. },
                    request: Some(OwnedSourceDiagnosticRequest::Iteration { .. }),
                    ..
                })
        ),
        "{:?}",
        validate_program_with_basic_owned_dynamic_loading(&program)
    );
}
