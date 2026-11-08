use super::*;

struct Candidates<'a> {
    events: RefCell<Vec<TraceEvent>>,
    contexts: RefCell<Vec<Outcome>>,
    selected: Cell<bool>,
    stop_after_first: bool,
    source_rows: bool,
    check: Option<&'a Check<'a>>,
}
impl Candidates<'_> {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            contexts: RefCell::new(Vec::new()),
            selected: Cell::new(true),
            stop_after_first: false,
            source_rows: true,
            check: None,
        }
    }
}
impl TraceSink for Candidates<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
    fn wants_source_row(&self) -> bool {
        self.source_rows
    }
    fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
        self.selected.get() && matches!(event, TraceEvent::IterationCandidate { .. })
    }
    fn record_with_owned_context(
        &self,
        event: TraceEvent,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) {
        assert!(matches!(event, TraceEvent::IterationCandidate { .. }));
        let outcome = match context {
            Ok(view) => {
                if let Some(check) = self.check {
                    check(&event, &view);
                }
                Outcome::Snapshot(view.snapshot())
            }
            Err(error) => Outcome::Projection(error.clone()),
        };
        self.contexts.borrow_mut().push(outcome);
        if self.stop_after_first {
            self.selected.set(false);
        }
        self.record(event);
    }
}
pub(super) fn candidate_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .filter(|event| matches!(event, TraceEvent::IterationCandidate { .. }))
        .cloned()
        .collect()
}
fn candidate_event(
    scope: TraceScope,
    ordinal: usize,
    positions: &[PositionFrame],
    item: Option<&Instance>,
) -> TraceEvent {
    TraceEvent::IterationCandidate {
        scope,
        ordinal,
        positions: crate::trace::trace_positions(positions),
        source_row: item.map(crate::TraceSourceRow::new),
    }
}

#[test]
fn candidates_keep_exact_primary_named_owners_and_raw_null_gap_positions() {
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
    ] {
        for empty in [false, true] {
            let p = repeated(owner.clone());
            let rows = || {
                Instance::Repeated(if empty {
                    Vec::new()
                } else {
                    vec![
                        Instance::Scalar(Value::Null),
                        row("one"),
                        Instance::Scalar(Value::JsonNull(ir::JsonNull)),
                        row("two"),
                    ]
                })
            };
            let primary = group(vec![("Rows", rows())]);
            let extras = group(vec![
                ("Ref", group(vec![("Rows", rows())])),
                (" Ref 雪 ", group(vec![("Rows", rows())])),
            ]);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let key = match &owner {
                SourceOwner::Primary => OwnerKey::Primary,
                SourceOwner::Named { name } if name == "Ref" => OwnerKey::Named(0),
                SourceOwner::Named { .. } => OwnerKey::Named(1),
            };
            let original = roots.root(key).unwrap();
            let items = original.field("Rows").unwrap().as_repeated().unwrap();
            let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                let TraceEvent::IterationCandidate { ordinal, .. } = event else {
                    unreachable!()
                };
                let raw = if *ordinal == 1 { 2 } else { 4 };
                assert_eq!(view.phase, TraceOwnedPhase::Structural);
                assert!(view.owned_ancestry);
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.origins.len(), 1);
                assert_eq!(view.positions.len(), 1);
                let expected_owner = match &owner {
                    SourceOwner::Primary => TraceOwnedOwnerRef::Primary,
                    SourceOwner::Named { name } => TraceOwnedOwnerRef::Named {
                        declaration_index: if name == "Ref" { 0 } else { 1 },
                        name,
                    },
                };
                assert_eq!(view.frames[0].owner, expected_owner);
                assert_eq!(view.origins[0].owner, expected_owner);
                assert_eq!(view.frames[0].collection, path(&["Rows"]));
                assert_eq!(view.origins[0].path, path(&["Rows"]));
                assert!(std::ptr::eq(view.frames[0].item, &items[raw - 1]));
                assert!(std::ptr::eq(view.origins[0].item, &items[raw - 1]));
                assert_eq!(view.positions[view.frames[0].position_slot].index, raw);
                assert!(std::ptr::eq(
                    view.roots[match key {
                        OwnerKey::Primary => 0,
                        OwnerKey::Named(i) => i + 1,
                    }]
                    .input
                    .unwrap(),
                    original
                ));
            };
            let mut sink = Candidates::new();
            sink.check = Some(&check);
            let expected = Instance::Repeated(if empty {
                Vec::new()
            } else {
                vec![row("one"), row("two")]
            });
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(expected)
            );
            let expected_events = if empty {
                Vec::new()
            } else {
                vec![
                    candidate_event(TraceScope::primary(), 1, &[position(2)], Some(&items[1])),
                    candidate_event(TraceScope::primary(), 2, &[position(4)], Some(&items[3])),
                ]
            };
            assert_eq!(candidate_events(&sink.events.borrow()), expected_events);
            assert_eq!(sink.contexts.borrow().len(), if empty { 0 } else { 2 });
            assert_eq!(registry.cache.get().is_some(), !empty);
            assert!(state.frames.is_empty() && state.origins.is_empty());
        }
    }
}

#[test]
fn singleton_root_and_once_candidates_keep_identity_and_target_scope() {
    let primary = row("root");
    let extras = group(Vec::new());
    for once in [false, true] {
        let mut p = constant();
        if !once {
            p.root.iteration = ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: Vec::new(),
                },
                output_path: None,
            };
        }
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Candidates::new();
        let expected = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Ok(if once {
                expected.clone()
            } else {
                Instance::Repeated(vec![expected.clone()])
            })
        );
        assert_eq!(
            candidate_events(&sink.events.borrow()),
            vec![candidate_event(TraceScope::primary(), 1, &[], None)]
        );
        let contexts = sink.contexts.borrow();
        let Outcome::Snapshot(Ok(view)) = &contexts[0] else {
            panic!("complete candidate witness")
        };
        assert_eq!(view.phase, TraceOwnedPhase::Structural);
        assert!(view.frames.is_empty() && view.positions.is_empty());
        assert!(state.frames.is_empty() && state.origins.is_empty());
        let named_scope = TraceScope::named(" Target 雪 ", 0);
        let sink = Candidates::new();
        let failed = Cell::new(false);
        let program = EvalProgram::new(&p.graph, &p.user_functions, Some(&sink), &failed)
            .with_owned_context(&state)
            .with_owned_trace_registry(&registry)
            .with_owned_trace_phase(TraceOwnedPhase::Output);
        let result = crate::eval_scope::eval_scope(
            program,
            &p.root,
            None,
            &[&extras, &primary],
            &[],
            &p.extra_sources,
            None,
            &named_scope,
        );
        assert_eq!(
            result,
            Ok(if once {
                expected
            } else {
                Instance::Repeated(vec![expected])
            })
        );
        assert_eq!(
            candidate_events(&sink.events.borrow()),
            vec![candidate_event(named_scope, 1, &[], None)]
        );
    }
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
    ] {
        for empty in [false, true] {
            let mut p = constant();
            p.source.repeating = true;
            p.extra_sources = project(&["Ref"], Vec::new(), Scope::default()).extra_sources;
            p.extra_sources[0].schema.repeating = true;
            p.root.iteration = ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: owner.clone(),
                    path: Vec::new(),
                },
                output_path: None,
            };
            let items = || {
                Instance::Repeated(if empty {
                    Vec::new()
                } else {
                    vec![row("one"), row("two")]
                })
            };
            let primary = items();
            let extras = group(vec![("Ref", items())]);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let key = if owner == SourceOwner::Primary {
                OwnerKey::Primary
            } else {
                OwnerKey::Named(0)
            };
            let original = roots.root(key).unwrap().as_repeated().unwrap();
            let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                let TraceEvent::IterationCandidate { ordinal, .. } = event else {
                    unreachable!()
                };
                assert_eq!(view.phase, TraceOwnedPhase::Structural);
                assert_eq!(view.frames.len(), 1);
                assert!(view.frames[0].collection.is_empty());
                assert!(view.positions[0].collection.is_empty());
                assert_eq!(view.positions[0].index, *ordinal);
                assert!(std::ptr::eq(view.frames[0].item, &original[*ordinal - 1]));
            };
            let mut sink = Candidates::new();
            sink.check = Some(&check);
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(Instance::Repeated(if empty {
                    Vec::new()
                } else {
                    vec![
                        group(vec![("Value", Instance::Scalar(Value::Int(7)))]),
                        group(vec![("Value", Instance::Scalar(Value::Int(7)))]),
                    ]
                }))
            );
            let expected = original
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let mut raw = position(index + 1);
                    raw.collection.clear();
                    candidate_event(TraceScope::primary(), index + 1, &[raw], Some(item))
                })
                .collect::<Vec<_>>();
            assert_eq!(candidate_events(&sink.events.borrow()), expected);
        }
    }
}

#[test]
fn nested_candidates_borrow_outer_and_inner_exact_owners_in_structural_phase() {
    let mut p = repeated(SourceOwner::Primary);
    let mut child = repeated(SourceOwner::Named { name: "Ref".into() }).root;
    child.target_field = "Child".into();
    child.bindings[0].node = 1;
    child.bindings.push(Binding {
        target_field: "Outer".into(),
        node: 0,
    });
    p.graph.nodes.insert(
        1,
        owned_field(
            SourceOwner::Named { name: "Ref".into() },
            &["Rows", "Value"],
        ),
    );
    p.root.children.push(child);
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
    let named = roots
        .root(OwnerKey::Named(0))
        .unwrap()
        .field("Rows")
        .unwrap()
        .as_repeated()
        .unwrap();
    let child_scope = TraceScope::primary().child("Child", 0);
    let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        let TraceEvent::IterationCandidate { scope, ordinal, .. } = event else {
            unreachable!()
        };
        assert_eq!(view.phase, TraceOwnedPhase::Structural);
        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
        assert!(std::ptr::eq(view.frames[0].item, parent));
        assert!(std::ptr::eq(view.origins[0].item, parent));
        assert_eq!(view.positions[view.frames[0].position_slot].index, 1);
        if scope == &child_scope {
            assert_eq!(view.frames.len(), 2);
            assert_eq!(view.origins.len(), 2);
            assert_eq!(view.positions.len(), 2);
            assert_eq!(
                view.frames[1].owner,
                TraceOwnedOwnerRef::Named {
                    declaration_index: 0,
                    name: "Ref"
                }
            );
            assert!(std::ptr::eq(view.frames[1].item, &named[*ordinal - 1]));
            assert!(std::ptr::eq(view.origins[1].item, &named[*ordinal - 1]));
            assert_eq!(view.positions[view.frames[1].position_slot].index, *ordinal);
        } else {
            assert_eq!(scope, &TraceScope::primary());
            assert_eq!(view.frames.len(), 1);
            assert_eq!(view.positions.len(), 1);
        }
    };
    let mut sink = Candidates::new();
    sink.check = Some(&check);
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Selection,
            &[]
        ),
        Ok(Instance::Repeated(vec![group(vec![
            ("Value", text("parent")),
            (
                "Child",
                Instance::Repeated(vec![
                    group(vec![("Value", text("one")), ("Outer", text("parent"))]),
                    group(vec![("Value", text("two")), ("Outer", text("parent"))])
                ])
            )
        ])]))
    );
    assert_eq!(
        candidate_events(&sink.events.borrow()),
        vec![
            candidate_event(TraceScope::primary(), 1, &[position(1)], Some(parent)),
            candidate_event(
                child_scope.clone(),
                1,
                &[position(1), position(1)],
                Some(&named[0])
            ),
            candidate_event(
                child_scope.clone(),
                2,
                &[position(1), position(2)],
                Some(&named[1])
            )
        ]
    );
    assert_eq!(sink.contexts.borrow().len(), 3);
    assert!(state.frames.is_empty() && state.origins.is_empty());
    // An already-produced outer context retains its genuine index greater than one.
    let p = repeated(SourceOwner::Named { name: "Ref".into() });
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![row("outer-one"), row("outer-two")]),
    )]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let outer = &primary.field("Rows").unwrap().as_repeated().unwrap()[1];
    let state = OwnedContext {
        roots: &roots,
        frames: vec![SourceFrame {
            owner: OwnerKey::Primary,
            kind: crate::owned_context::OccurrenceKind::Item,
            collection: path(&["Rows"]),
            item: outer,
            position_slot: 0,
        }],
        origins: vec![SourceOrigin {
            kind: crate::owned_context::SourceOriginKind::Ordinary,
            owner: OwnerKey::Primary,
            path: path(&["Rows"]),
            item: outer,
        }],
        owned_ancestry: true,
    };
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let positions = [position(2)];
    let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        let TraceEvent::IterationCandidate { ordinal, .. } = event else {
            unreachable!()
        };
        assert_eq!(view.phase, TraceOwnedPhase::Structural);
        assert_eq!(view.frames.len(), 2);
        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
        assert!(std::ptr::eq(view.frames[0].item, outer));
        assert!(std::ptr::eq(view.origins[0].item, outer));
        assert_eq!(view.frames[0].position_slot, 0);
        assert_eq!(view.positions[0].index, 2);
        assert_eq!(view.frames[1].position_slot, 1);
        assert_eq!(view.positions[1].index, *ordinal);
    };
    let mut sink = Candidates::new();
    sink.check = Some(&check);
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &positions
        ),
        Ok(Instance::Repeated(vec![row("one"), row("two")]))
    );
    let named = roots
        .root(OwnerKey::Named(0))
        .unwrap()
        .field("Rows")
        .unwrap()
        .as_repeated()
        .unwrap();
    assert_eq!(
        candidate_events(&sink.events.borrow()),
        vec![
            candidate_event(
                TraceScope::primary(),
                1,
                &[position(2), position(1)],
                Some(&named[0])
            ),
            candidate_event(
                TraceScope::primary(),
                2,
                &[position(2), position(2)],
                Some(&named[1])
            )
        ]
    );
    assert_eq!(state.frames[0].position_slot, 0);
    assert!(std::ptr::eq(state.frames[0].item, outer));
}

#[test]
fn first_observes_complete_candidates_without_retaining_tail_or_replaying_events() {
    use crate::source_iteration::{
        ObservedWalkRequest, select_observed, select_observed_with_candidate_observer,
    };
    const COUNT: usize = 4096;
    let mut p = repeated(SourceOwner::Primary);
    p.root.iteration_output = mapping::IterationOutput::First;
    let primary = group(vec![(
        "Rows",
        Instance::Repeated((0..COUNT).map(|i| row(&i.to_string())).collect()),
    )]);
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let source = OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    };
    let (owner, base, prefix, tail, budget) = state.select(&source).unwrap();
    let request = ObservedWalkRequest {
        base,
        path: tail,
        raw_prefix: prefix,
        canonical_prefix: prefix,
        owner: Some(owner),
        context: &state,
        position_offset: 0,
        strict_collection: true,
    };
    let observed = Cell::new(0usize);
    let mut observe = |extension: &mut crate::source_iteration::ObservedExtension<'_>, ordinal| {
        assert_eq!(ordinal, observed.get() + 1);
        assert_eq!(extension.extension.positions[0].index, ordinal);
        observed.set(ordinal);
    };
    let selected =
        select_observed_with_candidate_observer(request, &budget, true, &mut observe).unwrap();
    assert_eq!(observed.get(), COUNT);
    assert_eq!(selected.candidates, COUNT);
    assert_eq!(selected.extensions.len(), 1);
    assert!(std::ptr::eq(
        selected.extensions[0].frames[0].item,
        &primary.field("Rows").unwrap().as_repeated().unwrap()[0]
    ));
    let replay = selected.replay.unwrap();
    let mut replay_count = 0;
    replay
        .request
        .visit(&replay.budget, &mut |_| {
            replay_count += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(replay_count, COUNT);
    assert_eq!(observed.get(), COUNT);
    let (_, _, _, _, silent_budget) = state.select(&source).unwrap();
    let silent = select_observed(request, &silent_budget, true).unwrap();
    assert_eq!(silent.candidates, COUNT);
    assert_eq!(silent.extensions.len(), 1);
    struct Counting {
        candidates: Cell<usize>,
        produced: Cell<usize>,
        finishes: Cell<usize>,
    }
    impl TraceSink for Counting {
        fn wants_source_row(&self) -> bool {
            false
        }
        fn record(&self, event: TraceEvent) {
            match event {
                TraceEvent::IterationCandidate {
                    ordinal,
                    source_row,
                    ..
                } => {
                    assert_eq!(ordinal, self.candidates.get() + 1);
                    assert!(source_row.is_none());
                    self.candidates.set(ordinal);
                }
                TraceEvent::TargetProduced { .. } => self.produced.set(self.produced.get() + 1),
                TraceEvent::ScopeFinished {
                    candidates,
                    produced,
                    ..
                } => {
                    assert_eq!(candidates, COUNT);
                    assert_eq!(produced, 1);
                    self.finishes.set(self.finishes.get() + 1);
                }
                _ => {}
            }
        }
    }
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = Counting {
        candidates: Cell::new(0),
        produced: Cell::new(0),
        finishes: Cell::new(0),
    };
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(row("0"))
    );
    assert_eq!(
        (
            sink.candidates.get(),
            sink.produced.get(),
            sink.finishes.get()
        ),
        (COUNT, 1, 1)
    );
    assert!(registry.cache.get().is_none());
    for count in [0, 3] {
        let primary = group(vec![(
            "Rows",
            Instance::Repeated((0..count).map(|i| row(&i.to_string())).collect()),
        )]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Candidates::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Ok(if count == 0 {
                group(Vec::new())
            } else {
                row("0")
            })
        );
        assert_eq!(candidate_events(&sink.events.borrow()).len(), count);
        assert_eq!(sink.contexts.borrow().len(), count);
        assert_eq!(registry.cache.get().is_some(), count != 0);
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
                .count(),
            usize::from(count != 0)
        );
        assert_eq!(
            boundaries(&sink.events.borrow())[1],
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: count,
                produced: usize::from(count != 0),
                kind: TraceOutputKind::Group
            }
        );
        if count == 3 {
            let items = primary.field("Rows").unwrap().as_repeated().unwrap();
            assert_eq!(
                candidate_events(&sink.events.borrow()),
                (1..=3)
                    .map(|index| candidate_event(
                        TraceScope::primary(),
                        index,
                        &[position(index)],
                        Some(&items[index - 1])
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn malformed_first_tail_keeps_candidate_prefix_and_original_error_before_output() {
    let p = project(
        &[],
        vec![(
            0,
            Node::Const {
                value: Value::Int(7),
            },
        )],
        Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Buckets", "Rows"]),
                },
                output_path: None,
            },
            iteration_output: mapping::IterationOutput::First,
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    );
    let good = group(vec![("Rows", Instance::Repeated(vec![row("one")]))]);
    let primary = group(vec![(
        "Buckets",
        Instance::Repeated(vec![good, text("malformed")]),
    )]);
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let silent = run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]);
    let sink = Candidates::new();
    let traced = run(
        &p,
        &state,
        &registry,
        Some(&sink),
        TraceOwnedPhase::Output,
        &[],
    );
    assert_eq!(traced, silent);
    assert_eq!(
        traced,
        Err(EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(TraceScope::primary().source_scope)),
            source: OwnedSourceError::MalformedOwnedTraversal {
                path: path(&["Buckets"]),
                found: "a non-null scalar"
            }
        })
    );
    let buckets = primary.field("Buckets").unwrap().as_repeated().unwrap();
    let item = &buckets[0].field("Rows").unwrap().as_repeated().unwrap()[0];
    let mut outer = position(1);
    outer.collection = path(&["Buckets"]);
    let mut inner = position(1);
    inner.collection = path(&["Buckets", "Rows"]);
    assert_eq!(
        candidate_events(&sink.events.borrow()),
        vec![candidate_event(
            TraceScope::primary(),
            1,
            &[outer, inner],
            Some(item)
        )]
    );
    assert!(!sink.events.borrow().iter().any(|event| matches!(
        event,
        TraceEvent::TargetProduced { .. }
            | TraceEvent::ScopeFinished { .. }
            | TraceEvent::NodeValue { .. }
    )));
    assert_eq!(sink.contexts.borrow().len(), 1);
}

#[test]
fn candidate_event_preferences_keep_registry_lazy_and_previews_bounded() {
    let p = repeated(SourceOwner::Primary);
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![row(&"é".repeat(400)), row("two")]),
    )]);
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let silent = run(
        &p,
        &state,
        &registry,
        None,
        TraceOwnedPhase::Structural,
        &[],
    );
    assert!(registry.cache.get().is_none());
    let mut declining = Candidates::new();
    declining.selected.set(false);
    declining.source_rows = false;
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&declining),
            TraceOwnedPhase::Structural,
            &[]
        ),
        silent
    );
    assert!(registry.cache.get().is_none());
    assert!(declining.contexts.borrow().is_empty());
    assert!(
        candidate_events(&declining.events.borrow())
            .iter()
            .all(|event| matches!(
                event,
                TraceEvent::IterationCandidate {
                    source_row: None,
                    ..
                }
            ))
    );
    let mut changing = Candidates::new();
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
        silent
    );
    assert_eq!(changing.contexts.borrow().len(), 1);
    assert_eq!(candidate_events(&changing.events.borrow()).len(), 2);
    let events = changing.events.borrow();
    let candidates = candidate_events(&events);
    let TraceEvent::IterationCandidate {
        source_row: Some(preview),
        ..
    } = &candidates[0]
    else {
        panic!("requested row preview")
    };
    assert_eq!(preview.fields.len(), 1);
    let value = preview.fields[0].value.as_ref().unwrap();
    assert!(value.truncated);
    assert!(value.preview.chars().count() <= 160);
    struct BooleanOnly(Cell<usize>);
    impl TraceSink for BooleanOnly {
        fn record(&self, _: TraceEvent) {}
        fn wants_owned_context(&self) -> bool {
            true
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            _: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(
                event,
                TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
            ));
            self.0.set(self.0.get() + 1);
        }
    }
    let boolean = BooleanOnly(Cell::new(0));
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&boolean),
            TraceOwnedPhase::Structural,
            &[]
        ),
        silent
    );
    assert_eq!(boolean.0.get(), 2);
}

#[test]
fn candidate_projection_and_snapshot_errors_remain_observer_only() {
    let p = constant();
    let mut foreign = constant();
    foreign.extra_sources = project(&["Unused"], Vec::new(), Scope::default()).extra_sources;
    let primary = row("root");
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut state = OwnedContext::new(&roots);
    state.owned_ancestry = true;
    let wrong = TraceOwnedRegistry::new(&foreign, &roots);
    let sink = Candidates::new();
    let expected = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
    assert_eq!(
        run(
            &p,
            &state,
            &wrong,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(expected.clone())
    );
    assert_eq!(
        *sink.contexts.borrow(),
        vec![Outcome::Projection(
            TraceOwnedContextError::MismatchedRegistry
        )]
    );
    assert!(
        sink.events
            .borrow()
            .iter()
            .any(|event| matches!(event, TraceEvent::ScopeFinished { .. }))
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
    let sink = Candidates::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Selection,
            &[]
        ),
        Ok(expected)
    );
    assert_eq!(
        *sink.contexts.borrow(),
        vec![Outcome::Snapshot(Err(
            TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 7
            }
        ))]
    );
    assert_eq!(state.frames.len(), 1);
    assert!(std::ptr::eq(state.frames[0].item, &primary));
}

#[test]
fn candidates_precede_control_compaction_and_keep_original_structural_positions() {
    for order in [
        SortFilterOrder::FilterThenSort,
        SortFilterOrder::SortThenFilter,
    ] {
        let p = project(
            &[],
            vec![
                (0, owned_field(SourceOwner::Primary, &["Rows", "Rank"])),
                (1, owned_field(SourceOwner::Primary, &["Rows", "Keep"])),
            ],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                filter: Some(1),
                sort_by: Some(0),
                sort_filter_order: order,
                bindings: vec![Binding {
                    target_field: "Rank".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        );
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
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Candidates::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(Instance::Repeated(vec![
                group(vec![("Rank", Instance::Scalar(Value::Int(2)))]),
                group(vec![("Rank", Instance::Scalar(Value::Int(3)))])
            ]))
        );
        let items = primary.field("Rows").unwrap().as_repeated().unwrap();
        assert_eq!(
            candidate_events(&sink.events.borrow()),
            (1..=3)
                .map(|index| candidate_event(
                    TraceScope::primary(),
                    index,
                    &[position(index)],
                    Some(&items[index - 1])
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(sink.contexts.borrow().len(), 3);
        for (index, outcome) in sink.contexts.borrow().iter().enumerate() {
            let Outcome::Snapshot(Ok(view)) = outcome else {
                panic!("candidate snapshot")
            };
            assert_eq!(view.phase, TraceOwnedPhase::Structural);
            assert_eq!(view.positions[0].index, index + 1);
            assert_eq!(view.frames[0].index, index + 1);
        }
        let events = sink.events.borrow();
        let last_candidate = events
            .iter()
            .rposition(|event| matches!(event, TraceEvent::IterationCandidate { .. }))
            .unwrap();
        let first_value = events
            .iter()
            .position(|event| matches!(event, TraceEvent::NodeValue { .. }))
            .unwrap();
        assert!(last_candidate < first_value);
    }
}
