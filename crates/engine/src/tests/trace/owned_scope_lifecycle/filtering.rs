use super::candidates::candidate_events;
use super::*;

struct FilterDecisions<'a> {
    events: RefCell<Vec<TraceEvent>>,
    contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
    root_addresses: RefCell<Vec<usize>>,
    selected: Cell<bool>,
    stop_after_first: bool,
    check: Option<&'a Check<'a>>,
}
impl FilterDecisions<'_> {
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
impl TraceSink for FilterDecisions<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
    fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
        self.selected.get() && matches!(event, TraceEvent::FilterDecision { .. })
    }
    fn record_with_owned_context(
        &self,
        event: TraceEvent,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) {
        assert!(matches!(event, TraceEvent::FilterDecision { .. }));
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
pub(super) fn decision_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .filter(|event| matches!(event, TraceEvent::FilterDecision { .. }))
        .cloned()
        .collect()
}
pub(super) fn decision_event(
    scope: TraceScope,
    node: mapping::NodeId,
    phase: crate::TraceFilterPhase,
    positions: &[PositionFrame],
    passed: bool,
) -> TraceEvent {
    TraceEvent::FilterDecision {
        scope,
        node,
        phase,
        positions: crate::trace::trace_positions(positions),
        passed,
    }
}
pub(super) fn rank_row(rank: i64, keep: bool) -> Instance {
    group(vec![
        ("Rank", Instance::Scalar(Value::Int(rank))),
        ("Keep", Instance::Scalar(Value::Bool(keep))),
    ])
}
pub(super) fn rank_output(rank: i64, index: i64) -> Instance {
    group(vec![
        ("Rank", Instance::Scalar(Value::Int(rank))),
        ("Index", Instance::Scalar(Value::Int(index))),
    ])
}
pub(super) fn filter_project(owner: SourceOwner, sort: Option<SortFilterOrder>) -> Project {
    project(
        &["Ref", " Ref 雪 "],
        vec![
            (0, owned_field(owner.clone(), &["Rows", "Rank"])),
            (1, owned_field(owner.clone(), &["Rows", "Keep"])),
            (
                2,
                Node::OwnedPosition(OwnedPosition {
                    owner: owner.clone(),
                    collection: Some(path(&["Rows"])),
                }),
            ),
        ],
        Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            filter: Some(1),
            sort_by: sort.map(|_| 0),
            sort_filter_order: sort.unwrap_or(SortFilterOrder::SortThenFilter),
            bindings: vec![
                Binding {
                    target_field: "Rank".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Index".into(),
                    node: 2,
                },
            ],
            ..Default::default()
        },
    )
}

#[test]
fn filter_decisions_keep_actual_phase_positions_and_exact_colliding_owner_identity() {
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
    ] {
        for sort in [
            None,
            Some(SortFilterOrder::FilterThenSort),
            Some(SortFilterOrder::SortThenFilter),
        ] {
            let p = filter_project(owner.clone(), sort);
            let primary = group(vec![(
                "Rows",
                Instance::Repeated(vec![
                    rank_row(3, true),
                    rank_row(1, false),
                    rank_row(2, true),
                ]),
            )]);
            let extras = group(vec![
                (
                    "Ref",
                    group(vec![(
                        "Rows",
                        Instance::Repeated(vec![
                            rank_row(30, true),
                            rank_row(10, false),
                            rank_row(20, true),
                        ]),
                    )]),
                ),
                (
                    " Ref 雪 ",
                    group(vec![(
                        "Rows",
                        Instance::Repeated(vec![
                            rank_row(300, true),
                            rank_row(100, false),
                            rank_row(200, true),
                        ]),
                    )]),
                ),
            ]);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let (key, multiplier, named) = match &owner {
                SourceOwner::Primary => (OwnerKey::Primary, 1, None),
                SourceOwner::Named { name } if name == "Ref" => {
                    (OwnerKey::Named(0), 10, Some((0, name.as_str())))
                }
                SourceOwner::Named { name } => (OwnerKey::Named(1), 100, Some((1, name.as_str()))),
            };
            let original = roots.root(key).unwrap();
            let items = original.field("Rows").unwrap().as_repeated().unwrap();
            let sorted_filter = sort == Some(SortFilterOrder::SortThenFilter);
            let expected_phase = if sort == Some(SortFilterOrder::FilterThenSort) {
                crate::TraceFilterPhase::BeforeSort
            } else if sorted_filter {
                crate::TraceFilterPhase::AfterSort
            } else {
                crate::TraceFilterPhase::Selection
            };
            let expected_witness_phase = if expected_phase == crate::TraceFilterPhase::BeforeSort {
                TraceOwnedPhase::Structural
            } else {
                TraceOwnedPhase::Selection
            };
            let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                let TraceEvent::FilterDecision {
                    node,
                    phase,
                    positions,
                    passed,
                    ..
                } = event
                else {
                    unreachable!()
                };
                assert_eq!(*node, 1);
                assert_eq!(*phase, expected_phase);
                assert_eq!(view.phase, expected_witness_phase);
                let index = positions[0].index;
                let original_index = if sorted_filter {
                    [1, 2, 0][index - 1]
                } else {
                    index - 1
                };
                let expected_owner =
                    named.map_or(TraceOwnedOwnerRef::Primary, |(declaration_index, name)| {
                        TraceOwnedOwnerRef::Named {
                            declaration_index,
                            name,
                        }
                    });
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.origins.len(), 1);
                assert_eq!(view.frames[0].owner, expected_owner);
                assert_eq!(view.origins[0].owner, expected_owner);
                if let TraceOwnedOwnerRef::Named {
                    declaration_index,
                    name,
                } = view.frames[0].owner
                {
                    assert_eq!(
                        name.as_ptr(),
                        p.extra_sources[declaration_index].name.as_ptr()
                    );
                }
                assert_eq!(view.frames[0].collection, path(&["Rows"]));
                assert_eq!(view.origins[0].path, path(&["Rows"]));
                assert!(std::ptr::eq(view.frames[0].item, &items[original_index]));
                assert!(std::ptr::eq(view.origins[0].item, &items[original_index]));
                assert_eq!(view.positions[view.frames[0].position_slot].index, index);
                assert!(std::ptr::eq(
                    view.roots[match key {
                        OwnerKey::Primary => 0,
                        OwnerKey::Named(i) => i + 1,
                    }]
                    .input
                    .unwrap(),
                    original
                ));
                assert_eq!(*passed, original_index != 1);
            };
            let mut paired = FilterDecisions::new();
            paired.check = Some(&check);
            let plain = Collector::default();
            let expected = Instance::Repeated(if sort.is_some() {
                vec![
                    rank_output(2 * multiplier, 1),
                    rank_output(3 * multiplier, 2),
                ]
            } else {
                vec![
                    rank_output(3 * multiplier, 1),
                    rank_output(2 * multiplier, 2),
                ]
            });
            assert_eq!(
                run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
                Ok(expected.clone())
            );
            assert!(registry.cache.get().is_none());
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
                    Some(&paired),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(expected)
            );
            assert_eq!(*paired.events.borrow(), *plain.0.borrow());
            assert_eq!(
                decision_events(&paired.events.borrow()),
                (1..=3)
                    .map(|index| decision_event(
                        TraceScope::primary(),
                        1,
                        expected_phase,
                        &[position(index)],
                        if sorted_filter {
                            index != 1
                        } else {
                            index != 2
                        }
                    ))
                    .collect::<Vec<_>>()
            );
            assert_eq!(paired.contexts.borrow().len(), 3);
            assert!(
                paired
                    .root_addresses
                    .borrow()
                    .windows(2)
                    .all(|pair| pair[0] == pair[1])
            );
            let produced = paired
                .events
                .borrow()
                .iter()
                .filter_map(|event| match event {
                    TraceEvent::TargetProduced { positions, .. } => Some(positions[0].index),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(produced, vec![1, 2]);
            assert!(state.frames.is_empty() && state.origins.is_empty());
        }
    }
}

#[test]
fn nested_filter_decisions_preserve_parent_output_positions_and_borrowed_inner_frames() {
    let mut p = filter_project(SourceOwner::Primary, None);
    let mut child = filter_project(SourceOwner::Named { name: "Ref".into() }, None).root;
    child.target_field = "Child".into();
    child.filter = Some(4);
    child.bindings[0].node = 3;
    child.bindings[1].node = 5;
    p.graph.nodes.insert(
        3,
        owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Rank"]),
    );
    p.graph.nodes.insert(
        4,
        owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Keep"]),
    );
    p.graph.nodes.insert(
        5,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Named { name: "Ref".into() },
            collection: Some(path(&["Rows"])),
        }),
    );
    p.root.children.push(child);
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            rank_row(5, false),
            rank_row(3, true),
            rank_row(2, true),
        ]),
    )]);
    let extras = group(vec![(
        "Ref",
        group(vec![(
            "Rows",
            Instance::Repeated(vec![
                rank_row(7, true),
                rank_row(8, false),
                rank_row(9, true),
            ]),
        )]),
    )]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let parents = primary.field("Rows").unwrap().as_repeated().unwrap();
    let children = roots
        .root(OwnerKey::Named(0))
        .unwrap()
        .field("Rows")
        .unwrap()
        .as_repeated()
        .unwrap();
    let child_scope = TraceScope::primary().child("Child", 0);
    let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        let TraceEvent::FilterDecision {
            scope, positions, ..
        } = event
        else {
            unreachable!()
        };
        assert_eq!(view.phase, TraceOwnedPhase::Selection);
        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
        let parent_index = if scope == &child_scope {
            positions[0].index
        } else {
            positions[0].index - 1
        };
        assert!(std::ptr::eq(view.frames[0].item, &parents[parent_index]));
        assert!(std::ptr::eq(view.origins[0].item, &parents[parent_index]));
        assert_eq!(view.frames[0].position_slot, 0);
        assert_eq!(view.positions[0].index, positions[0].index);
        if scope == &child_scope {
            assert_eq!(view.frames.len(), 2);
            assert_eq!(view.origins.len(), 2);
            assert_eq!(
                view.frames[1].owner,
                TraceOwnedOwnerRef::Named {
                    declaration_index: 0,
                    name: "Ref"
                }
            );
            assert_eq!(view.frames[1].position_slot, 1);
            assert_eq!(view.positions[1].index, positions[1].index);
            assert!(std::ptr::eq(
                view.frames[1].item,
                &children[positions[1].index - 1]
            ));
            assert!(std::ptr::eq(
                view.origins[1].item,
                &children[positions[1].index - 1]
            ));
        } else {
            assert_eq!(view.frames.len(), 1);
        }
    };
    let mut sink = FilterDecisions::new();
    sink.check = Some(&check);
    let make_parent = |rank, index| {
        group(vec![
            ("Rank", Instance::Scalar(Value::Int(rank))),
            ("Index", Instance::Scalar(Value::Int(index))),
            (
                "Child",
                Instance::Repeated(vec![rank_output(7, 1), rank_output(9, 2)]),
            ),
        ])
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
        Ok(Instance::Repeated(vec![
            make_parent(3, 1),
            make_parent(2, 2)
        ]))
    );
    let mut expected = vec![decision_event(
        TraceScope::primary(),
        1,
        crate::TraceFilterPhase::Selection,
        &[position(1)],
        false,
    )];
    for outer in 1..=2 {
        expected.push(decision_event(
            TraceScope::primary(),
            1,
            crate::TraceFilterPhase::Selection,
            &[position(outer + 1)],
            true,
        ));
        for inner in 1..=3 {
            expected.push(decision_event(
                child_scope.clone(),
                4,
                crate::TraceFilterPhase::Selection,
                &[position(outer), position(inner)],
                inner != 2,
            ));
        }
    }
    assert_eq!(decision_events(&sink.events.borrow()), expected);
    assert_eq!(sink.contexts.borrow().len(), 9);
    assert!(state.frames.is_empty() && state.origins.is_empty());
}

#[test]
fn filter_decision_preferences_keep_legacy_events_and_cache_lazy() {
    let mut p = filter_project(SourceOwner::Primary, None);
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
    let expected = Instance::Repeated(vec![rank_output(3, 1), rank_output(2, 2)]);
    let declining = FilterDecisions::new();
    declining.selected.set(false);
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            None,
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
            Some(&declining),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(expected.clone())
    );
    assert!(declining.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    let mut changing = FilterDecisions::new();
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
        Ok(expected.clone())
    );
    assert_eq!(*changing.events.borrow(), *declining.events.borrow());
    assert_eq!(changing.contexts.borrow().len(), 1);
    assert_eq!(decision_events(&changing.events.borrow()).len(), 3);
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
        Ok(expected)
    );
    assert_eq!(boolean.0.get(), 7);
    p.root.filter = None;
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = FilterDecisions::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Structural,
            &[]
        ),
        Ok(Instance::Repeated(vec![
            rank_output(3, 1),
            rank_output(1, 2),
            rank_output(2, 3)
        ]))
    );
    assert!(decision_events(&sink.events.borrow()).is_empty());
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    p.root.filter = Some(1);
    p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
    p.graph.nodes.insert(
        0,
        Node::SourceField {
            path: path(&["Rank"]),
            frame: Some(path(&["Rows"])),
        },
    );
    p.graph.nodes.insert(
        1,
        Node::SourceField {
            path: path(&["Keep"]),
            frame: Some(path(&["Rows"])),
        },
    );
    p.root.bindings.retain(|binding| binding.node == 0);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = FilterDecisions::new();
    let plain = Collector::default();
    let expected = Instance::Repeated(vec![
        group(vec![("Rank", Instance::Scalar(Value::Int(3)))]),
        group(vec![("Rank", Instance::Scalar(Value::Int(2)))]),
    ]);
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
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
}

#[test]
fn filter_decision_observation_does_not_change_first_predicate_count_or_retention_eligibility() {
    let mut p = filter_project(SourceOwner::Primary, None);
    p.root.iteration_output = mapping::IterationOutput::First;
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
    let sink = FilterDecisions::new();
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
        Ok(rank_output(3, 1))
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
        Ok(rank_output(3, 1))
    );
    assert_eq!(*sink.events.borrow(), *plain.0.borrow());
    assert_eq!(decision_events(&sink.events.borrow()).len(), 3);
    assert_eq!(
        sink.events
            .borrow()
            .iter()
            .filter(|event| matches!(event, TraceEvent::NodeValue { node: 1, .. }))
            .count(),
        3
    );
    assert_eq!(
        sink.events
            .borrow()
            .iter()
            .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
            .count(),
        1
    );
    assert_eq!(
        boundaries(&sink.events.borrow())[1],
        TraceEvent::ScopeFinished {
            scope: TraceScope::primary(),
            candidates: 3,
            produced: 1,
            kind: TraceOutputKind::Group
        }
    );
    p.root.filter = None;
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = FilterDecisions::new();
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[]
        ),
        Ok(rank_output(3, 1))
    );
    assert_eq!(candidate_events(&sink.events.borrow()).len(), 3);
    assert!(decision_events(&sink.events.borrow()).is_empty());
    assert_eq!(
        sink.events
            .borrow()
            .iter()
            .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
            .count(),
        1
    );
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
}

#[test]
fn failed_filter_predicates_keep_exact_errors_and_prefix_without_fabricated_decisions() {
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            rank_row(3, true),
            rank_row(1, false),
            rank_row(2, true),
        ]),
    )]);
    let extras = group(Vec::new());
    let bad = [
        (
            Node::Const {
                value: Value::String("not_bool".into()),
            },
            EngineError::NotABool {
                node: 1,
                found: "string",
            },
        ),
        (
            Node::Raise { message: None },
            EngineError::MappingException {
                node: 1,
                message: None,
            },
        ),
        (
            owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Keep"]),
            EngineError::OwnedSourceAccess {
                node: Some(1),
                scope: None,
                source: OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named { name: "Ref".into() },
                },
            },
        ),
    ];
    for (predicate, expected) in bad {
        let mut p = filter_project(SourceOwner::Primary, None);
        p.graph.nodes.insert(1, predicate);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = FilterDecisions::new();
        let plain = Collector::default();
        assert_eq!(
            run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]).as_ref(),
            Err(&expected)
        );
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
        assert!(decision_events(&sink.events.borrow()).is_empty());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        assert!(!sink.events.borrow().iter().any(|event| matches!(
            event,
            TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
        )));
    }
    let mut p = filter_project(SourceOwner::Primary, None);
    p.graph.nodes.insert(
        3,
        Node::If {
            condition: 1,
            then: 4,
            else_: 5,
        },
    );
    p.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(5, Node::Raise { message: None });
    p.root.filter = Some(3);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = FilterDecisions::new();
    let plain = Collector::default();
    let expected = Err(EngineError::MappingException {
        node: 5,
        message: None,
    });
    assert_eq!(
        run(
            &p,
            &state,
            &registry,
            Some(&plain),
            TraceOwnedPhase::Output,
            &[]
        ),
        expected
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
        expected
    );
    assert_eq!(*sink.events.borrow(), *plain.0.borrow());
    assert_eq!(
        decision_events(&sink.events.borrow()),
        vec![decision_event(
            TraceScope::primary(),
            3,
            crate::TraceFilterPhase::Selection,
            &[position(1)],
            true
        )]
    );
    assert_eq!(sink.contexts.borrow().len(), 1);
    assert_eq!(
        sink.events
            .borrow()
            .iter()
            .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
            .count(),
        1
    );
    assert!(
        !sink
            .events
            .borrow()
            .iter()
            .any(|event| matches!(event, TraceEvent::ScopeFinished { .. }))
    );
}

#[test]
fn filter_decision_projection_and_snapshot_failures_stay_observer_metadata() {
    let p = filter_project(SourceOwner::Primary, None);
    let foreign = constant();
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
    let wrong = TraceOwnedRegistry::new(&foreign, &roots);
    let sink = FilterDecisions::new();
    assert!(!std::ptr::eq(
        foreign.extra_sources.as_slice(),
        roots.declarations()
    ));
    let expected = Instance::Repeated(vec![rank_output(3, 1), rank_output(2, 2)]);
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
    assert_eq!(sink.contexts.borrow().len(), 3);
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
    let sink = FilterDecisions::new();
    assert_eq!(
        run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
        Ok(expected.clone())
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
        Ok(expected)
    );
    assert_eq!(sink.contexts.borrow().len(), 3);
    assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
        == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
            at: TraceOwnedLocation::Frame(0),
            slot: 7
        }))));
    assert_eq!(state.frames.len(), 1);
    assert!(std::ptr::eq(state.frames[0].item, &primary));
}
