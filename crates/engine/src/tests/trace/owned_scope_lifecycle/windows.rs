use super::candidates::candidate_events;
use super::filtering::{decision_events, filter_project, rank_output, rank_row};
use super::*;

struct WindowObservations<'a> {
    events: RefCell<Vec<TraceEvent>>,
    contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
    root_addresses: RefCell<Vec<usize>>,
    selected: Cell<bool>,
    stop_after_first: bool,
    check: Option<&'a Check<'a>>,
}
impl WindowObservations<'_> {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            contexts: RefCell::new(Vec::new()),
            root_addresses: RefCell::new(Vec::new()),
            selected: Cell::new(true),
            stop_after_first: false,
            check: None,
        }
    }
}
impl TraceSink for WindowObservations<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
    fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
        self.selected.get() && matches!(event, TraceEvent::WindowApplied { .. })
    }
    fn record_with_owned_context(
        &self,
        event: TraceEvent,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) {
        assert!(matches!(event, TraceEvent::WindowApplied { .. }));
        let outcome = match context {
            Ok(view) => {
                if let Some(check) = self.check {
                    check(&event, &view);
                }
                self.root_addresses
                    .borrow_mut()
                    .push(view.roots.as_ptr() as usize);
                Outcome::Snapshot(view.snapshot())
            }
            Err(error) => Outcome::Projection(error.clone()),
        };
        self.contexts.borrow_mut().push((event.clone(), outcome));
        if self.stop_after_first {
            self.selected.set(false);
        }
        self.record(event);
    }
}
fn window_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .filter(|event| matches!(event, TraceEvent::WindowApplied { .. }))
        .cloned()
        .collect()
}
fn window_event(
    scope: TraceScope,
    index: usize,
    window: crate::TraceWindow,
    before: usize,
    after: usize,
) -> TraceEvent {
    TraceEvent::WindowApplied {
        scope,
        window_index: index,
        window,
        before,
        after,
    }
}
pub(super) fn value_rows(values: &[&str]) -> Instance {
    group(vec![(
        "Rows",
        Instance::Repeated(values.iter().map(|value| row(value)).collect()),
    )])
}

#[test]
fn chained_windows_keep_all_transform_counts_and_original_count_value_policy() {
    let primary = value_rows(&["a", "b", "c", "d", "e", "f"]);
    let extras = group(Vec::new());
    for first in [false, true] {
        let mut p = repeated(SourceOwner::Primary);
        for (node, value) in [
            (10, Value::String(" 1 ".into())),
            (11, Value::Float(4.9)),
            (12, Value::String("2".into())),
            (13, Value::Int(2)),
            (14, Value::Float(3.8)),
            (15, Value::String("1".into())),
        ] {
            p.graph.nodes.insert(node, Node::Const { value });
        }
        p.root.windows = vec![
            SequenceWindow::SkipFirst { count: 10 },
            SequenceWindow::First { count: 11 },
            SequenceWindow::From { position: 12 },
            SequenceWindow::FromTo {
                first: 13,
                last: 14,
            },
            SequenceWindow::Last { count: 15 },
        ];
        if first {
            p.root.iteration_output = mapping::IterationOutput::First;
        }
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let plain = Collector::default();
        let sink = WindowObservations::new();
        let expected = if first {
            row("e")
        } else {
            Instance::Repeated(vec![row("e")])
        };
        assert_eq!(
            run(&p, &state, &registry, None, TraceOwnedPhase::Selection, &[]),
            Ok(expected.clone())
        );
        assert!(registry.cache.get().is_none());
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Ok(expected.clone())
        );
        assert!(registry.cache.get().is_none());
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
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        let scope = TraceScope::primary();
        let mut expected_events = vec![
            window_event(scope.clone(), 1, crate::TraceWindow::SkipFirst(1), 6, 5),
            window_event(scope.clone(), 2, crate::TraceWindow::First(4), 5, 4),
            window_event(scope.clone(), 3, crate::TraceWindow::From(2), 4, 3),
            window_event(
                scope.clone(),
                4,
                crate::TraceWindow::FromTo { first: 2, last: 3 },
                3,
                2,
            ),
            window_event(scope.clone(), 5, crate::TraceWindow::Last(1), 2, 1),
        ];
        if first {
            expected_events.push(window_event(scope, 6, crate::TraceWindow::First(1), 1, 1));
        }
        assert_eq!(window_events(&sink.events.borrow()), expected_events);
        assert_eq!(sink.contexts.borrow().len(), if first { 6 } else { 5 });
        for (_, outcome) in sink.contexts.borrow().iter() {
            let Outcome::Snapshot(Ok(view)) = outcome else {
                panic!("window witness")
            };
            assert_eq!(view.phase, TraceOwnedPhase::Selection);
            assert!(view.frames.is_empty() && view.origins.is_empty() && view.positions.is_empty());
            assert!(!view.owned_ancestry);
        }
        assert!(
            sink.root_addresses
                .borrow()
                .windows(2)
                .all(|pair| pair[0] == pair[1])
        );
        for node in 10..=15 {
            assert_eq!(sink.events.borrow().iter().filter(|event| matches!(event, TraceEvent::NodeValue { node: actual, .. } if *actual == node)).count(), 1);
        }
    }
    for (count, evaluated, kept) in [(Value::Int(-4), 0, 0), (Value::String("99".into()), 99, 6)] {
        let mut p = repeated(SourceOwner::Primary);
        p.graph.nodes.insert(10, Node::Const { value: count });
        p.root.windows = vec![SequenceWindow::First { count: 10 }];
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(Instance::Repeated(
                ["a", "b", "c", "d", "e", "f"]
                    .into_iter()
                    .take(kept)
                    .map(row)
                    .collect()
            ))
        );
        assert_eq!(
            window_events(&sink.events.borrow()),
            vec![window_event(
                TraceScope::primary(),
                1,
                crate::TraceWindow::First(evaluated),
                6,
                kept
            )]
        );
    }
}

#[test]
fn window_lengths_measure_sorted_filter_survivors_before_first_truncation() {
    for order in [
        SortFilterOrder::FilterThenSort,
        SortFilterOrder::SortThenFilter,
    ] {
        for first in [false, true] {
            let mut p = filter_project(SourceOwner::Primary, Some(order));
            p.graph.nodes.insert(
                10,
                Node::Const {
                    value: Value::Int(1),
                },
            );
            p.root.windows.push(SequenceWindow::First { count: 10 });
            if first {
                p.root.iteration_output = mapping::IterationOutput::First;
            }
            let primary = group(vec![(
                "Rows",
                Instance::Repeated(vec![
                    rank_row(3, true),
                    rank_row(1, false),
                    rank_row(2, true),
                ]),
            )]);
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = WindowObservations::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(if first {
                    rank_output(2, 1)
                } else {
                    Instance::Repeated(vec![rank_output(2, 1)])
                })
            );
            let mut expected = vec![window_event(
                TraceScope::primary(),
                1,
                crate::TraceWindow::First(1),
                2,
                1,
            )];
            if first {
                expected.push(window_event(
                    TraceScope::primary(),
                    2,
                    crate::TraceWindow::First(1),
                    1,
                    1,
                ));
            }
            assert_eq!(window_events(&sink.events.borrow()), expected);
            assert_eq!(decision_events(&sink.events.borrow()).len(), 3);
            let events = sink.events.borrow();
            let last_filter = events
                .iter()
                .rposition(|e| matches!(e, TraceEvent::FilterDecision { .. }))
                .unwrap();
            let first_window = events
                .iter()
                .position(|e| matches!(e, TraceEvent::WindowApplied { .. }))
                .unwrap();
            let first_target = events
                .iter()
                .position(|e| matches!(e, TraceEvent::TargetProduced { .. }))
                .unwrap();
            assert!(last_filter < first_window && first_window < first_target);
            for (_, outcome) in sink.contexts.borrow().iter() {
                let Outcome::Snapshot(Ok(view)) = outcome else {
                    panic!("window witness")
                };
                assert_eq!(view.phase, TraceOwnedPhase::Output);
                assert!(view.frames.is_empty() && view.positions.is_empty());
            }
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e, TraceEvent::TargetProduced { .. }))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn bounded_first_window_reports_complete_logical_cohort_without_retaining_tail() {
    struct Counting<'a> {
        expected: usize,
        root: &'a Instance,
        candidates: Cell<usize>,
        produced: Cell<usize>,
        callbacks: Cell<usize>,
        window: RefCell<Option<TraceEvent>>,
    }
    impl TraceSink for Counting<'_> {
        fn wants_source_row(&self) -> bool {
            false
        }
        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            matches!(event, TraceEvent::WindowApplied { .. })
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
                TraceEvent::WindowApplied { .. } => {
                    assert!(self.window.borrow().is_none());
                    *self.window.borrow_mut() = Some(event);
                }
                TraceEvent::TargetProduced { .. } => self.produced.set(self.produced.get() + 1),
                TraceEvent::ScopeFinished {
                    candidates,
                    produced,
                    ..
                } => {
                    assert_eq!(candidates, self.expected);
                    assert_eq!(produced, usize::from(self.expected != 0));
                }
                _ => {}
            }
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            let view = context.unwrap();
            assert_eq!(view.phase, TraceOwnedPhase::Selection);
            assert!(view.frames.is_empty() && view.origins.is_empty() && view.positions.is_empty());
            assert!(std::ptr::eq(view.roots[0].input.unwrap(), self.root));
            assert_eq!(view.snapshot().unwrap().phase, TraceOwnedPhase::Selection);
            self.callbacks.set(self.callbacks.get() + 1);
            self.record(event);
        }
    }
    for count in [0, 1, 3, 4096] {
        let mut p = repeated(SourceOwner::Primary);
        p.root.iteration_output = mapping::IterationOutput::First;
        assert!(crate::eval_scope::retain_first_source_candidate(&p.root));
        let primary = group(vec![(
            "Rows",
            Instance::Repeated((0..count).map(|i| row(&i.to_string())).collect()),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Counting {
            expected: count,
            root: &primary,
            candidates: Cell::new(0),
            produced: Cell::new(0),
            callbacks: Cell::new(0),
            window: RefCell::new(None),
        };
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
        assert_eq!(sink.candidates.get(), count);
        assert_eq!(sink.produced.get(), usize::from(count != 0));
        assert_eq!(sink.callbacks.get(), 1);
        assert_eq!(
            *sink.window.borrow(),
            Some(window_event(
                TraceScope::primary(),
                1,
                crate::TraceWindow::First(1),
                count,
                usize::from(count != 0)
            ))
        );
        assert!(registry.cache.get().is_some());
        let source = OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        };
        let (owner, base, prefix, tail, budget) = state.select(&source).unwrap();
        let selected = crate::source_iteration::select_observed(
            crate::source_iteration::ObservedWalkRequest {
                base,
                path: tail,
                raw_prefix: prefix,
                canonical_prefix: prefix,
                owner: Some(owner),
                context: &state,
                position_offset: 0,
                strict_collection: true,
            },
            &budget,
            true,
        )
        .unwrap();
        assert_eq!(selected.candidates, count);
        assert_eq!(selected.extensions.len(), usize::from(count != 0));
    }
    let mut p = repeated(SourceOwner::Primary);
    p.root.iteration_output = mapping::IterationOutput::First;
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            Instance::Scalar(Value::Null),
            row("one"),
            Instance::Scalar(Value::JsonNull(ir::JsonNull)),
            row("two"),
        ]),
    )]);
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = WindowObservations::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(row("one"))
    );
    assert_eq!(candidate_events(&sink.events.borrow()).len(), 2);
    assert_eq!(
        window_events(&sink.events.borrow()),
        vec![window_event(
            TraceScope::primary(),
            1,
            crate::TraceWindow::First(1),
            2,
            1
        )]
    );
}

#[test]
fn window_witness_borrows_actual_receiving_parent_phase_and_named_identity() {
    for key in [OwnerKey::Primary, OwnerKey::Named(1)] {
        for phase in [
            TraceOwnedPhase::Structural,
            TraceOwnedPhase::Selection,
            TraceOwnedPhase::Output,
        ] {
            let mut p = constant();
            p.extra_sources =
                project(&["Ref", " Ref 雪 "], Vec::new(), Scope::default()).extra_sources;
            p.graph.nodes.insert(
                10,
                Node::Const {
                    value: Value::Int(1),
                },
            );
            p.root.windows.push(SequenceWindow::First { count: 10 });
            let primary = value_rows(&["P.one", "P.two"]);
            let extras = group(vec![
                ("Ref", value_rows(&["N.one", "N.two"])),
                (" Ref 雪 ", value_rows(&["U.one", "U.two"])),
            ]);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let original = roots.root(key).unwrap();
            let item = &original.field("Rows").unwrap().as_repeated().unwrap()[1];
            let state = OwnedContext {
                roots: &roots,
                frames: vec![SourceFrame {
                    owner: key,
                    kind: crate::owned_context::OccurrenceKind::Item,
                    collection: path(&["Rows"]),
                    item,
                    position_slot: 0,
                }],
                origins: vec![SourceOrigin {
                    kind: crate::owned_context::SourceOriginKind::Ordinary,
                    owner: key,
                    path: path(&["Rows"]),
                    item,
                }],
                owned_ancestry: true,
            };
            let positions = [position(2)];
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let check = |_: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                assert_eq!(view.phase, phase);
                assert!(view.owned_ancestry);
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.origins.len(), 1);
                assert_eq!(view.positions.len(), 1);
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
                assert_eq!(view.positions[0].index, 2);
                match key {
                    OwnerKey::Primary => {
                        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary)
                    }
                    OwnerKey::Named(index) => {
                        let TraceOwnedOwnerRef::Named {
                            declaration_index,
                            name,
                        } = view.frames[0].owner
                        else {
                            panic!("named receiving owner")
                        };
                        assert_eq!(declaration_index, index);
                        assert_eq!(name, " Ref 雪 ");
                        assert_eq!(name.as_ptr(), p.extra_sources[index].name.as_ptr());
                        assert!(std::ptr::eq(view.roots[index + 1].input.unwrap(), original));
                    }
                }
            };
            let mut sink = WindowObservations::new();
            sink.check = Some(&check);
            assert_eq!(
                run(&p, &state, &registry, Some(&sink), phase, &positions),
                Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
            );
            assert_eq!(
                window_events(&sink.events.borrow()),
                vec![window_event(
                    TraceScope::primary(),
                    1,
                    crate::TraceWindow::First(1),
                    1,
                    1
                )]
            );
            assert_eq!(sink.contexts.borrow().len(), 1);
        }
    }
    let mut p = repeated(SourceOwner::Primary);
    let mut child = repeated(SourceOwner::Named { name: "Ref".into() }).root;
    child.target_field = "Child".into();
    child.bindings[0].node = 1;
    child.windows.push(SequenceWindow::First { count: 10 });
    p.graph.nodes.insert(
        1,
        owned_field(
            SourceOwner::Named { name: "Ref".into() },
            &["Rows", "Value"],
        ),
    );
    p.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(1),
        },
    );
    p.root.children.push(child);
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            Instance::Scalar(Value::Null),
            row("parent-one"),
            row("parent-two"),
        ]),
    )]);
    let extras = group(vec![("Ref", value_rows(&["child-one", "child-two"]))]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let parents = primary.field("Rows").unwrap().as_repeated().unwrap();
    let scope = TraceScope::primary().child("Child", 0);
    let seen = Cell::new(0usize);
    let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        let TraceEvent::WindowApplied { scope: actual, .. } = event else {
            unreachable!()
        };
        assert_eq!(actual, &scope);
        assert_eq!(view.phase, TraceOwnedPhase::Output);
        assert_eq!(view.frames.len(), 1);
        assert_eq!(view.origins.len(), 1);
        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
        let raw = seen.get() + 2;
        assert!(std::ptr::eq(view.frames[0].item, &parents[raw - 1]));
        assert!(std::ptr::eq(view.origins[0].item, &parents[raw - 1]));
        assert_eq!(view.positions[0].index, raw);
        seen.set(seen.get() + 1);
    };
    let mut sink = WindowObservations::new();
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
        Ok(Instance::Repeated(
            ["parent-one", "parent-two"]
                .into_iter()
                .map(|value| group(vec![
                    ("Value", text(value)),
                    ("Child", Instance::Repeated(vec![row("child-one")]))
                ]))
                .collect()
        ))
    );
    assert_eq!(
        window_events(&sink.events.borrow()),
        vec![
            window_event(scope.clone(), 1, crate::TraceWindow::First(1), 2, 1),
            window_event(scope.clone(), 1, crate::TraceWindow::First(1), 2, 1)
        ]
    );
    assert_eq!(seen.get(), 2);
}

#[test]
fn window_observer_preferences_legacy_events_and_metadata_errors_stay_isolated() {
    let mut p = repeated(SourceOwner::Primary);
    p.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(1),
        },
    );
    p.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::Int(1),
        },
    );
    p.root.windows = vec![
        SequenceWindow::SkipFirst { count: 10 },
        SequenceWindow::First { count: 11 },
    ];
    let primary = value_rows(&["a", "b", "c"]);
    let extras = group(Vec::new());
    let expected = Instance::Repeated(vec![row("b")]);
    for selected in [false, true] {
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        sink.selected.set(selected);
        let plain = Collector::default();
        assert_eq!(
            run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
            Ok(expected.clone())
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert!(registry.cache.get().is_none());
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert_eq!(sink.contexts.borrow().len(), if selected { 2 } else { 0 });
        assert_eq!(registry.cache.get().is_some(), selected);
        assert!(
            sink.root_addresses
                .borrow()
                .windows(2)
                .all(|pair| pair[0] == pair[1])
        );
    }
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let mut changing = WindowObservations::new();
    changing.stop_after_first = true;
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&changing),
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(expected.clone())
    );
    assert_eq!(changing.contexts.borrow().len(), 1);
    assert_eq!(window_events(&changing.events.borrow()).len(), 2);
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
    let old = BooleanOnly(Cell::new(0));
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&old),
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(expected.clone())
    );
    assert_eq!(old.0.get(), 3);
    let foreign = constant();
    let wrong = TraceOwnedRegistry::new(&foreign, &roots);
    let sink = WindowObservations::new();
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
    assert_eq!(sink.contexts.borrow().len(), 2);
    assert!(
        sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
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
    let sink = WindowObservations::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(expected.clone())
    );
    assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
        == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
            at: TraceOwnedLocation::Frame(0),
            slot: 7
        }))));
    assert_eq!(sink.contexts.borrow().len(), 2);
    p.root.windows.clear();
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = WindowObservations::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(Instance::Repeated(vec![row("a"), row("b"), row("c")]))
    );
    assert!(window_events(&sink.events.borrow()).is_empty());
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    p.root.windows = vec![
        SequenceWindow::SkipFirst { count: 10 },
        SequenceWindow::First { count: 11 },
    ];
    p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
    p.graph.nodes.insert(
        0,
        Node::SourceField {
            path: path(&["Value"]),
            frame: Some(path(&["Rows"])),
        },
    );
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = WindowObservations::new();
    let plain = Collector::default();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
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
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(expected)
    );
    assert_eq!(*sink.events.borrow(), *plain.0.borrow());
    assert_eq!(
        window_events(&sink.events.borrow()),
        vec![
            window_event(
                TraceScope::primary(),
                1,
                crate::TraceWindow::SkipFirst(1),
                3,
                2
            ),
            window_event(TraceScope::primary(), 2, crate::TraceWindow::First(1), 2, 1)
        ]
    );
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
}

#[test]
fn failed_window_inputs_and_preapplication_work_preserve_error_order_and_event_prefixes() {
    let primary = value_rows(&["a", "b", "c"]);
    let extras = group(Vec::new());
    for (node, expected) in [
        (
            Node::Const {
                value: Value::Bool(true),
            },
            EngineError::NotAnItemCount {
                node: 11,
                found: "bool",
            },
        ),
        (
            Node::Raise { message: None },
            EngineError::MappingException {
                node: 11,
                message: None,
            },
        ),
        (
            owned_field(
                SourceOwner::Named { name: "Ref".into() },
                &["Rows", "Value"],
            ),
            EngineError::OwnedSourceAccess {
                node: Some(11),
                scope: None,
                source: OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named { name: "Ref".into() },
                },
            },
        ),
    ] {
        let mut p = repeated(SourceOwner::Primary);
        p.graph.nodes.insert(
            10,
            Node::Const {
                value: Value::Int(1),
            },
        );
        p.graph.nodes.insert(11, node);
        p.root.windows = vec![
            SequenceWindow::First { count: 10 },
            SequenceWindow::Last { count: 11 },
        ];
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        let plain = Collector::default();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            )
            .as_ref(),
            Err(&expected)
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Err(expected)
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert!(window_events(&sink.events.borrow()).is_empty());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|event| matches!(event, TraceEvent::NodeValue { node: 10, .. }))
                .count(),
            1
        );
        assert!(!sink.events.borrow().iter().any(|e| matches!(
            e,
            TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
        )));
    }
    let mut p = repeated(SourceOwner::Primary);
    p.root.iteration_output = mapping::IterationOutput::First;
    let roots = SourceRoots::new(
        &p,
        &primary,
        &extras,
        OwnedLimits {
            walk_contexts: 2,
            ..OwnedLimits::default()
        },
    )
    .unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = WindowObservations::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Err(EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(TraceScope::primary().source_scope)),
            source: OwnedSourceError::ResourceLimit {
                resource: "walk contexts",
                limit: 2
            }
        })
    );
    assert!(window_events(&sink.events.borrow()).is_empty());
    assert_eq!(candidate_events(&sink.events.borrow()).len(), 2);
    p.root.iteration = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Buckets", "Rows"]),
        },
        output_path: None,
    };
    let malformed = group(vec![(
        "Buckets",
        Instance::Repeated(vec![
            group(vec![("Rows", Instance::Repeated(vec![row("good")]))]),
            text("bad"),
        ]),
    )]);
    let roots = SourceRoots::new(&p, &malformed, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = WindowObservations::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Err(EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(TraceScope::primary().source_scope)),
            source: OwnedSourceError::MalformedOwnedTraversal {
                path: path(&["Buckets"]),
                found: "a non-null scalar"
            }
        })
    );
    assert!(window_events(&sink.events.borrow()).is_empty());
    assert_eq!(candidate_events(&sink.events.borrow()).len(), 1);
    let mut p = repeated(SourceOwner::Primary);
    p.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(1),
        },
    );
    p.graph.nodes.insert(20, Node::Raise { message: None });
    p.root.windows = vec![SequenceWindow::First { count: 10 }];
    p.root.filter = Some(20);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = WindowObservations::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Err(EngineError::MappingException {
            node: 20,
            message: None
        })
    );
    assert!(window_events(&sink.events.borrow()).is_empty());
    assert!(decision_events(&sink.events.borrow()).is_empty());
    p.root.filter = None;
    p.root.bindings.push(Binding {
        target_field: "Failed".into(),
        node: 20,
    });
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = WindowObservations::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Err(EngineError::MappingException {
            node: 20,
            message: None
        })
    );
    assert_eq!(
        window_events(&sink.events.borrow()),
        vec![window_event(
            TraceScope::primary(),
            1,
            crate::TraceWindow::First(1),
            3,
            1
        )]
    );
    assert_eq!(
        sink.events
            .borrow()
            .iter()
            .filter(|e| matches!(e, TraceEvent::TargetFieldWritten { .. }))
            .count(),
        1
    );
    assert!(!sink.events.borrow().iter().any(|e| matches!(
        e,
        TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
    )));
}
