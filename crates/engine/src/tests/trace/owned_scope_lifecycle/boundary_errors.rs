use super::*;

#[test]
fn failed_scopes_keep_successful_prefix_and_never_fabricate_a_finish() {
    let p = project(
        &[],
        vec![
            (
                0,
                Node::Const {
                    value: Value::Int(7),
                },
            ),
            (1, Node::Raise { message: None }),
        ],
        Scope {
            bindings: vec![
                Binding {
                    target_field: "Good".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Failed".into(),
                    node: 1,
                },
            ],
            ..Default::default()
        },
    );
    let primary = row("root");
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Selection,
            &[]
        ),
        Err(EngineError::MappingException {
            node: 1,
            message: None
        })
    );
    assert_eq!(
        boundaries(&paired.events.borrow()),
        vec![TraceEvent::ScopeStarted {
            scope: TraceScope::primary(),
            iteration: TraceIteration::Once,
            positions: Vec::new()
        }]
    );
    let writes = paired
        .events
        .borrow()
        .iter()
        .filter(|event| {
            matches!(
                event,
                TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        writes,
        vec![TraceEvent::TargetFieldWritten {
            scope: TraceScope::primary(),
            field: "Good".into(),
            binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
            positions: Vec::new(),
            kind: TraceOutputKind::Scalar,
            value: Some(TraceValue::new(&Value::Int(7)))
        }]
    );
    assert_eq!(
        snapshot(&paired.contexts.borrow()[0].1).phase,
        TraceOwnedPhase::Selection
    );
    let p = project(
        &["Missing"],
        Vec::new(),
        Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Named {
                        name: "Missing".into(),
                    },
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            ..Default::default()
        },
    );
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Err(EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(TraceScope::primary().source_scope)),
            source: OwnedSourceError::SourceUnavailable {
                owner: SourceOwner::Named {
                    name: "Missing".into()
                }
            }
        })
    );
    assert_eq!(
        boundaries(&paired.events.borrow()),
        vec![TraceEvent::ScopeStarted {
            scope: TraceScope::primary(),
            iteration: TraceIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Named {
                        name: "Missing".into()
                    },
                    path: path(&["Rows"])
                }
            },
            positions: Vec::new()
        }]
    );
}

#[test]
fn lifecycle_projection_and_snapshot_failures_remain_observer_metadata() {
    let p = constant();
    let mut other = constant();
    // Distinct empty Vecs do not establish declaration-slice identity mismatch.
    other.extra_sources = project(&["Unused"], Vec::new(), Scope::default()).extra_sources;
    let primary = row("root");
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    assert!(!std::ptr::eq(
        other.extra_sources.as_slice(),
        roots.declarations()
    ));
    let wrong = TraceOwnedRegistry::new(&other, &roots);
    let paired = Lifecycle::new();
    let expected = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
    assert_eq!(
        run(
            &p,
            &state,
            &wrong,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(expected.clone())
    );
    assert_eq!(paired.contexts.borrow().len(), 2);
    eprintln!(
        "RAW lifecycle actual foreign-registry outcomes={:#?}",
        paired.contexts.borrow()
    );
    assert!(
        paired.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Projection(TraceOwnedContextError::MismatchedRegistry))
    );
    let state = OwnedContext {
        roots: &roots,
        frames: vec![SourceFrame {
            owner: OwnerKey::Primary,
            kind: crate::owned_context::OccurrenceKind::Item,
            collection: Vec::new(),
            item: &primary,
            position_slot: 7,
        }],
        origins: Vec::new(),
        owned_ancestry: true,
    };
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(expected)
    );
    assert!(paired.contexts.borrow().iter().all(|(_, outcome)| *outcome
        == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
            at: TraceOwnedLocation::Frame(0),
            slot: 7
        }))));
    let p = project(
        &["Null", "Missing"],
        vec![(
            0,
            Node::Const {
                value: Value::Int(7),
            },
        )],
        Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    );
    let extras = group(vec![("Null", Instance::Scalar(Value::Null))]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
    );
    assert_eq!(paired.contexts.borrow().len(), 2);
    for (_, outcome) in paired.contexts.borrow().iter() {
        let view = snapshot(outcome);
        assert_eq!(
            view.roots[1]
                .input
                .as_ref()
                .unwrap()
                .scalar
                .as_ref()
                .unwrap()
                .value_type,
            "null"
        );
        assert!(view.roots[2].input.is_none());
    }
    let p = project(
        &["Unused"],
        vec![(
            0,
            Node::Const {
                value: Value::Int(7),
            },
        )],
        Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    );
    let extras = group(vec![("Unused", text("one")), ("Unused", text("two"))]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
    );
    assert_eq!(paired.contexts.borrow().len(), 2);
    assert!(paired.contexts.borrow().iter().all(|(_, outcome)| *outcome
        == Outcome::Projection(TraceOwnedContextError::Registry {
            declaration_index: 0,
            source: OwnedSourceError::AmbiguousOwner {
                name: "Unused".into()
            }
        })));
}

#[test]
fn legacy_iteration_refusal_and_public_basic_lifecycle_admission_stay_distinct() {
    let owned = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: SourceOwner::Named {
                name: "primary".into(),
            },
            path: Vec::new(),
        },
        output_path: None,
    };
    assert_eq!(scope_iteration(&owned), Err(mapping::OwnedSourceAccess));
    assert_eq!(
        basic_scope_iteration(&owned),
        Ok(TraceIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "primary".into()
                },
                path: Vec::new()
            }
        })
    );
    let documented = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: Vec::new(),
        },
        output_path: Some(0),
    };
    assert_eq!(
        basic_scope_iteration(&documented),
        Err(mapping::OwnedSourceAccess)
    );
    let p = project(
        &[],
        vec![(
            0,
            Node::OwnedSourceField(OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Value"]),
                },
                frame: None,
            }),
        )],
        Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    );
    let primary = row("root");
    let paired = Lifecycle::new();
    let execution =
        crate::ExecutionContext::new(std::path::Path::new("project.json")).with_trace_sink(&paired);
    assert_eq!(
        crate::run_with_context(&p, &primary, &execution),
        Ok(primary.clone())
    );
    assert_eq!(paired.contexts.borrow().len(), 2);
    assert!(paired.contexts.borrow().iter().all(|(_, outcome)| matches!(
        outcome,
        Outcome::Snapshot(Ok(snapshot)) if snapshot.phase == TraceOwnedPhase::Structural
    )));
    for observers in [
        crate::SourceOwnershipObservers {
            tracing: false,
            debugging: true,
        },
        crate::SourceOwnershipObservers {
            tracing: true,
            debugging: true,
        },
    ] {
        assert_eq!(
            crate::check_source_ownership_capability_with_observers(&p, None, observers),
            Ok(())
        );
    }
    let mut p = constant();
    p.root.iteration = ScopeIteration::DynamicDocuments {
        source: path(&["Rows"]),
        output_path: 0,
    };
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Err(EngineError::UnsupportedOwnedContext {
            node: None,
            function: None,
            scope: Some(TraceScope::primary().source_scope),
            context: crate::OwnedCapabilityContext::Document
        })
    );
    assert!(paired.events.borrow().is_empty());
    let p = project(
        &[],
        vec![
            (
                0,
                Node::Const {
                    value: Value::Int(7),
                },
            ),
            (
                1,
                Node::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
        ],
        Scope {
            iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Generate {
                from: None,
                to: 0,
                item: 1,
            }),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    );
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Err(EngineError::UnsupportedOwnedContext {
            node: None,
            function: None,
            scope: Some(TraceScope::primary().source_scope),
            context: crate::OwnedCapabilityContext::GeneratedItem
        })
    );
    assert!(paired.events.borrow().is_empty());
}
