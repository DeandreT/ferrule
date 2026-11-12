use super::*;

#[test]
fn old_legacy_events_and_event_selective_lifecycle_dispatch_remain_exact() {
    let p = constant();
    let primary = row("root");
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let legacy = Collector::default();
    let expected_result = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&legacy),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(expected_result.clone())
    );
    let expected_events = vec![
        TraceEvent::ScopeStarted {
            scope: TraceScope::primary(),
            iteration: TraceIteration::Once,
            positions: Vec::new(),
        },
        TraceEvent::NodeValue {
            node: 0,
            positions: Vec::new(),
            value: TraceValue::new(&Value::Int(7)),
        },
        TraceEvent::TargetFieldWritten {
            scope: TraceScope::primary(),
            field: "Value".into(),
            binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
            positions: Vec::new(),
            kind: TraceOutputKind::Scalar,
            value: Some(TraceValue::new(&Value::Int(7))),
        },
        TraceEvent::TargetProduced {
            scope: TraceScope::primary(),
            positions: Vec::new(),
            output_path: None,
            kind: TraceOutputKind::Group,
        },
        TraceEvent::ScopeFinished {
            scope: TraceScope::primary(),
            candidates: 1,
            produced: 1,
            kind: TraceOutputKind::Group,
        },
    ];
    // The unchanged legacy route already records its Once candidate.
    // This records the preexisting legacy Once sequence independently.
    let mut legacy_expected_events = expected_events.clone();
    legacy_expected_events.insert(
        1,
        TraceEvent::IterationCandidate {
            scope: TraceScope::primary(),
            ordinal: 1,
            positions: Vec::new(),
            source_row: None,
        },
    );
    assert_eq!(*legacy.0.borrow(), legacy_expected_events);
    // Candidate observation is now complete in the Basic Once route too.
    // The unchanged legacy six-event oracle above remains exact.
    let expected_events = legacy_expected_events;
    assert!(registry.cache.get().is_none());
    state.owned_ancestry = true;
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            None,
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(expected_result.clone())
    );
    assert!(registry.cache.get().is_none());
    let declining = Lifecycle::new();
    declining.selected.set(0);
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&declining),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(expected_result.clone())
    );
    assert_eq!(*declining.events.borrow(), expected_events);
    assert!(declining.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    for (selected, count) in [(1, 1), (2, 1), (3, 2)] {
        let paired = Lifecycle::new();
        paired.selected.set(selected);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected_result.clone())
        );
        assert_eq!(*paired.events.borrow(), expected_events);
        assert_eq!(paired.contexts.borrow().len(), count);
    }
    let mut changing = Lifecycle::new();
    changing.stop_after_first = true;
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&changing),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(expected_result)
    );
    assert_eq!(*changing.events.borrow(), expected_events);
    assert_eq!(changing.contexts.borrow().len(), 1);
    assert!(matches!(
        changing.contexts.borrow()[0].0,
        TraceEvent::ScopeStarted { .. }
    ));
    struct OldBoolean {
        pairs: Cell<usize>,
    }
    impl TraceSink for OldBoolean {
        fn record(&self, _: TraceEvent) {}
        fn wants_owned_context(&self) -> bool {
            true
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(
                event,
                TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
            ));
            assert!(context.is_ok());
            self.pairs.set(self.pairs.get() + 1);
        }
    }
    let old = OldBoolean {
        pairs: Cell::new(0),
    };
    run(
        &p,
        &state,
        &registry,
        Some(&old),
        TraceOwnedPhase::Structural,
        &[],
    )
    .unwrap();
    assert_eq!(old.pairs.get(), 1);
    let mut scalar = constant();
    scalar.root.construction = mapping::ScopeConstruction::Scalar { value: 0 };
    scalar.root.bindings.clear();
    let roots = SourceRoots::new(&scalar, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&scalar, &roots);
    let paired = Lifecycle::new();
    assert_eq!(
        run(
            &scalar,
            &state,
            &registry,
            Some(&paired),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(Instance::Scalar(Value::Int(7)))
    );
    assert_eq!(
        boundaries(&paired.events.borrow()),
        vec![
            TraceEvent::ScopeStarted {
                scope: TraceScope::primary(),
                iteration: TraceIteration::Once,
                positions: Vec::new()
            },
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: 1,
                produced: 1,
                kind: TraceOutputKind::Scalar
            }
        ]
    );
}

#[test]
fn zero_singleton_and_mixed_candidates_keep_exact_owner_lifecycle_counts() {
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
    ] {
        for case in 0..3 {
            let p = repeated(owner.clone());
            let items = match case {
                0 => Vec::new(),
                1 => vec![row("one")],
                _ => vec![
                    Instance::Scalar(Value::Null),
                    row("one"),
                    Instance::Scalar(Value::JsonNull(ir::JsonNull)),
                    row("two"),
                ],
            };
            let count = if case == 0 {
                0
            } else if case == 1 {
                1
            } else {
                2
            };
            let primary = group(vec![("Rows", Instance::Repeated(items.clone()))]);
            let extras = group(vec![
                (
                    "Ref",
                    group(vec![("Rows", Instance::Repeated(items.clone()))]),
                ),
                (" Ref 雪 ", group(vec![("Rows", Instance::Repeated(items))])),
            ]);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let paired = Lifecycle::new();
            let plain = Collector::default();
            let expected = Instance::Repeated(if count == 0 {
                Vec::new()
            } else if count == 1 {
                vec![row("one")]
            } else {
                vec![row("one"), row("two")]
            });
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&paired),
                    TraceOwnedPhase::Structural,
                    &[]
                ),
                Ok(expected.clone())
            );
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&plain),
                    TraceOwnedPhase::Structural,
                    &[]
                ),
                Ok(expected)
            );
            assert_eq!(*paired.events.borrow(), *plain.0.borrow());
            assert_eq!(
                boundaries(&paired.events.borrow()),
                vec![
                    TraceEvent::ScopeStarted {
                        scope: TraceScope::primary(),
                        iteration: TraceIteration::OwnedSource {
                            source: OwnedSourceRef {
                                owner: owner.clone(),
                                path: path(&["Rows"])
                            }
                        },
                        positions: Vec::new()
                    },
                    TraceEvent::ScopeFinished {
                        scope: TraceScope::primary(),
                        candidates: count,
                        produced: count,
                        kind: TraceOutputKind::Repeated
                    },
                ]
            );
            assert_eq!(paired.contexts.borrow().len(), 2);
            assert!(paired.contexts.borrow().iter().all(|(_, outcome)| {
                let view = snapshot(outcome);
                view.phase == TraceOwnedPhase::Structural
                    && view.frames.is_empty()
                    && view.positions.is_empty()
            }));
            if case == 2 {
                let indexes = paired
                    .events
                    .borrow()
                    .iter()
                    .filter_map(|event| match event {
                        TraceEvent::TargetProduced { positions, .. } => Some(positions[0].index),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(indexes, vec![2, 4]);
            }
        }
    }
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
    ] {
        for case in 0..3 {
            let p = project(
                &["Ref", " Ref 雪 "],
                vec![(
                    0,
                    Node::Const {
                        value: Value::Int(7),
                    },
                )],
                Scope {
                    iteration: ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: owner.clone(),
                            path: Vec::new(),
                        },
                        output_path: None,
                    },
                    bindings: vec![Binding {
                        target_field: "Value".into(),
                        node: 0,
                    }],
                    ..Default::default()
                },
            );
            let input = match case {
                0 => Instance::Repeated(Vec::new()),
                1 => row("one"),
                _ => Instance::Repeated(vec![
                    Instance::Scalar(Value::Null),
                    row("one"),
                    Instance::Scalar(Value::JsonNull(ir::JsonNull)),
                    row("two"),
                ]),
            };
            let count = if case == 0 {
                0
            } else if case == 1 {
                1
            } else {
                2
            };
            let primary = input.clone();
            let extras = group(vec![("Ref", input.clone()), (" Ref 雪 ", input)]);
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
                Ok(Instance::Repeated(
                    (0..count)
                        .map(|_| group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
                        .collect()
                ))
            );
            assert_eq!(
                boundaries(&paired.events.borrow()),
                vec![
                    TraceEvent::ScopeStarted {
                        scope: TraceScope::primary(),
                        iteration: TraceIteration::OwnedSource {
                            source: OwnedSourceRef {
                                owner: owner.clone(),
                                path: Vec::new()
                            }
                        },
                        positions: Vec::new()
                    },
                    TraceEvent::ScopeFinished {
                        scope: TraceScope::primary(),
                        candidates: count,
                        produced: count,
                        kind: TraceOutputKind::Repeated
                    }
                ]
            );
        }
    }
    let mut p = constant();
    p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
    let primary = group(vec![("Rows", Instance::Repeated(vec![row("one")]))]);
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
        Ok(Instance::Repeated(vec![group(vec![(
            "Value",
            Instance::Scalar(Value::Int(7))
        )])]))
    );
    assert_eq!(
        boundaries(&paired.events.borrow()),
        vec![
            TraceEvent::ScopeStarted {
                scope: TraceScope::primary(),
                iteration: TraceIteration::Source {
                    path: path(&["Rows"])
                },
                positions: Vec::new()
            },
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: 1,
                produced: 1,
                kind: TraceOutputKind::Repeated
            }
        ]
    );
}

#[test]
fn receiving_context_references_and_supplied_phase_are_not_derived_from_boundary_names() {
    let p = constant();
    let primary = group(vec![("Rows", Instance::Repeated(vec![row("one")]))]);
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let item = &primary.field("Rows").unwrap().as_repeated().unwrap()[0];
    let state = OwnedContext {
        roots: &roots,
        frames: vec![SourceFrame {
            owner: OwnerKey::Primary,
            kind: crate::owned_context::OccurrenceKind::Item,
            collection: path(&["Rows"]),
            item,
            position_slot: 0,
        }],
        origins: vec![SourceOrigin {
            kind: crate::owned_context::SourceOriginKind::Ordinary,
            owner: OwnerKey::Primary,
            path: path(&["Rows"]),
            item,
        }],
        owned_ancestry: true,
    };
    let positions = [position(1)];
    let registry = TraceOwnedRegistry::new(&p, &roots);
    for phase in [
        TraceOwnedPhase::Structural,
        TraceOwnedPhase::Selection,
        TraceOwnedPhase::Output,
    ] {
        let check = |_: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            assert_eq!(view.phase, phase);
            assert!(std::ptr::eq(view.project, &p));
            assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
            assert!(std::ptr::eq(view.frames[0].item, item));
            assert!(std::ptr::eq(view.origins[0].item, item));
            assert_eq!(
                view.frames[0].collection.as_ptr(),
                state.frames[0].collection.as_ptr()
            );
            assert_eq!(
                view.origins[0].path.as_ptr(),
                state.origins[0].path.as_ptr()
            );
            assert_eq!(
                view.positions[0].collection.as_ptr(),
                positions[0].collection.as_ptr()
            );
            assert_eq!(view.positions[0].index, 1);
        };
        let mut paired = Lifecycle::new();
        paired.check = Some(&check);
        assert_eq!(
            run(&p, &state, &registry, Some(&paired), phase, &positions),
            Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![
                TraceEvent::ScopeStarted {
                    scope: TraceScope::primary(),
                    iteration: TraceIteration::Once,
                    positions: crate::trace::trace_positions(&positions)
                },
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: 1,
                    produced: 1,
                    kind: TraceOutputKind::Group
                }
            ]
        );
        assert_eq!(paired.contexts.borrow().len(), 2);
        assert!(
            paired
                .contexts
                .borrow()
                .iter()
                .all(|(_, outcome)| snapshot(outcome).phase == phase)
        );
        assert_eq!(
            paired.root_addresses.borrow()[0],
            paired.root_addresses.borrow()[1]
        );
    }
    let mut named = constant();
    named.extra_targets.push(mapping::NamedTarget {
        name: " Target 雪 ".into(),
        path: None,
        schema: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]),
        options: Default::default(),
        root: named.root.clone(),
    });
    let roots = SourceRoots::new(&named, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&named, &roots);
    let paired = Lifecycle::new();
    let failed = Cell::new(false);
    let program = EvalProgram::new(&named.graph, &named.user_functions, Some(&paired), &failed)
        .with_owned_context(&state)
        .with_owned_trace_registry(&registry)
        .with_owned_trace_phase(TraceOwnedPhase::Selection);
    let target = &named.extra_targets[0];
    let scope = TraceScope::named(&target.name, 0);
    let result = crate::eval_scope::eval_scope(
        program,
        &target.root,
        Some(&target.schema),
        &[&extras, &primary],
        &[],
        &named.extra_sources,
        None,
        &scope,
    );
    eprintln!("INPUT real_named_target={named:#?} RAW result={result:#?}");
    assert_eq!(
        result,
        Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
    );
    assert_eq!(
        boundaries(&paired.events.borrow()),
        vec![
            TraceEvent::ScopeStarted {
                scope: scope.clone(),
                iteration: TraceIteration::Once,
                positions: Vec::new()
            },
            TraceEvent::ScopeFinished {
                scope,
                candidates: 1,
                produced: 1,
                kind: TraceOutputKind::Group
            }
        ]
    );
    assert!(
        paired
            .contexts
            .borrow()
            .iter()
            .all(|(_, outcome)| snapshot(outcome).phase == TraceOwnedPhase::Selection)
    );
}

#[test]
fn nested_owned_scopes_start_and_finish_in_the_actual_parent_receiving_context() {
    let p = project(
        &["Ref"],
        vec![
            (0, owned_field(SourceOwner::Primary, &["Rows", "Value"])),
            (
                1,
                owned_field(
                    SourceOwner::Named { name: "Ref".into() },
                    &["Rows", "Value"],
                ),
            ),
        ],
        Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            bindings: vec![Binding {
                target_field: "Parent".into(),
                node: 0,
            }],
            children: vec![Scope {
                target_field: "Child".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Named { name: "Ref".into() },
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                bindings: vec![
                    Binding {
                        target_field: "Value".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "Outer".into(),
                        node: 0,
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let primary = group(vec![("Rows", Instance::Repeated(vec![row("parent")]))]);
    let extras = group(vec![(
        "Ref",
        group(vec![(
            "Rows",
            Instance::Repeated(vec![row("one"), row("two")]),
        )]),
    )]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let parent = &primary.field("Rows").unwrap().as_repeated().unwrap()[0];
    let parent_scope = TraceScope::primary();
    let child_scope = parent_scope.child("Child", 0);
    let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        let scope = match event {
            TraceEvent::ScopeStarted { scope, .. } | TraceEvent::ScopeFinished { scope, .. } => {
                scope
            }
            _ => unreachable!(),
        };
        assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
        assert!(std::ptr::eq(
            view.roots[1].input.unwrap(),
            roots.root(OwnerKey::Named(0)).unwrap()
        ));
        if scope == &child_scope {
            assert_eq!(view.phase, TraceOwnedPhase::Output);
            assert_eq!(view.frames.len(), 1);
            assert_eq!(view.origins.len(), 1);
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            assert!(std::ptr::eq(view.frames[0].item, parent));
            assert!(std::ptr::eq(view.origins[0].item, parent));
            assert_eq!(view.positions[view.frames[0].position_slot].index, 1);
        } else {
            assert_eq!(scope, &parent_scope);
            assert_eq!(view.phase, TraceOwnedPhase::Structural);
            assert!(view.frames.is_empty());
            assert!(view.origins.is_empty());
            assert!(view.positions.is_empty());
        }
    };
    let mut paired = Lifecycle::new();
    paired.check = Some(&check);
    let expected = Instance::Repeated(vec![group(vec![
        ("Parent", text("parent")),
        (
            "Child",
            Instance::Repeated(vec![
                group(vec![("Value", text("one")), ("Outer", text("parent"))]),
                group(vec![("Value", text("two")), ("Outer", text("parent"))]),
            ]),
        ),
    ])]);
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
    assert_eq!(
        boundaries(&paired.events.borrow()),
        vec![
            TraceEvent::ScopeStarted {
                scope: parent_scope.clone(),
                iteration: TraceIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Rows"])
                    }
                },
                positions: Vec::new()
            },
            TraceEvent::ScopeStarted {
                scope: child_scope.clone(),
                iteration: TraceIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Named { name: "Ref".into() },
                        path: path(&["Rows"])
                    }
                },
                positions: crate::trace::trace_positions(&[position(1)])
            },
            TraceEvent::ScopeFinished {
                scope: child_scope.clone(),
                candidates: 2,
                produced: 2,
                kind: TraceOutputKind::Repeated
            },
            TraceEvent::ScopeFinished {
                scope: parent_scope.clone(),
                candidates: 1,
                produced: 1,
                kind: TraceOutputKind::Repeated
            },
        ]
    );
    assert_eq!(paired.contexts.borrow().len(), 4);
}

#[test]
fn lifecycle_counts_preserve_control_compaction_and_complete_first_candidates() {
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(
            [(3, true), (1, false), (2, true)]
                .into_iter()
                .map(|(rank, keep)| {
                    group(vec![
                        ("Rank", Instance::Scalar(Value::Int(rank))),
                        ("Keep", Instance::Scalar(Value::Bool(keep))),
                    ])
                })
                .collect(),
        ),
    )]);
    for order in [
        SortFilterOrder::FilterThenSort,
        SortFilterOrder::SortThenFilter,
    ] {
        let p = project(
            &[],
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (1, owned_field(SourceOwner::Primary, &["Rows", "Rank"])),
                (2, owned_field(SourceOwner::Primary, &["Rows", "Keep"])),
                (
                    3,
                    Node::OwnedPosition(OwnedPosition {
                        owner: SourceOwner::Primary,
                        collection: Some(path(&["Rows"])),
                    }),
                ),
            ],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                filter: Some(2),
                sort_by: Some(1),
                sort_filter_order: order,
                windows: vec![SequenceWindow::SkipFirst { count: 0 }],
                bindings: vec![
                    Binding {
                        target_field: "Rank".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "Position".into(),
                        node: 3,
                    },
                ],
                ..Default::default()
            },
        );
        let extras = group(Vec::new());
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
            Ok(Instance::Repeated(vec![group(vec![
                ("Rank", Instance::Scalar(Value::Int(3))),
                ("Position", Instance::Scalar(Value::Int(1)))
            ])]))
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![
                TraceEvent::ScopeStarted {
                    scope: TraceScope::primary(),
                    iteration: TraceIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Primary,
                            path: path(&["Rows"])
                        }
                    },
                    positions: Vec::new()
                },
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: 3,
                    produced: 1,
                    kind: TraceOutputKind::Repeated
                }
            ]
        );
        assert!(paired.contexts.borrow().iter().all(|(_, outcome)| {
            let view = snapshot(outcome);
            view.phase == TraceOwnedPhase::Structural
                && view.frames.is_empty()
                && view.positions.is_empty()
        }));
    }
    for empty in [false, true] {
        let mut p = repeated(SourceOwner::Primary);
        p.root.iteration_output = mapping::IterationOutput::First;
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(if empty {
                Vec::new()
            } else {
                vec![row("one"), row("two"), row("three")]
            }),
        )]);
        let extras = group(Vec::new());
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
            Ok(if empty { group(Vec::new()) } else { row("one") })
        );
        assert_eq!(
            boundaries(&paired.events.borrow())[1],
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: if empty { 0 } else { 3 },
                produced: if empty { 0 } else { 1 },
                kind: TraceOutputKind::Group
            }
        );
    }
}
