use super::filtering::{decision_event, decision_events, filter_project};
use super::*;

struct SortObservations<'a> {
    events: RefCell<Vec<TraceEvent>>,
    contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
    root_addresses: RefCell<Vec<usize>>,
    selected: Cell<u8>,
    stop_after_first: bool,
    check: Option<&'a Check<'a>>,
}
impl SortObservations<'_> {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            contexts: RefCell::new(Vec::new()),
            root_addresses: RefCell::new(Vec::new()),
            selected: Cell::new(3),
            stop_after_first: false,
            check: None,
        }
    }
}
impl TraceSink for SortObservations<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
    fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
        match event {
            TraceEvent::SortCandidate { .. } => self.selected.get() & 1 != 0,
            TraceEvent::SortPosition { .. } => self.selected.get() & 2 != 0,
            _ => false,
        }
    }
    fn record_with_owned_context(
        &self,
        event: TraceEvent,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) {
        assert!(matches!(
            event,
            TraceEvent::SortCandidate { .. } | TraceEvent::SortPosition { .. }
        ));
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
            self.selected.set(0);
        }
        self.record(event);
    }
}
fn sort_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                TraceEvent::SortCandidate { .. } | TraceEvent::SortPosition { .. }
            )
        })
        .cloned()
        .collect()
}
fn sort_candidate(
    scope: TraceScope,
    positions: &[PositionFrame],
    keys: &[(mapping::NodeId, bool, Value)],
) -> TraceEvent {
    TraceEvent::SortCandidate {
        scope,
        positions: crate::trace::trace_positions(positions),
        keys: keys
            .iter()
            .map(|(node, descending, value)| crate::TraceSortKey {
                node: *node,
                descending: *descending,
                value: TraceValue::new(value),
            })
            .collect(),
    }
}
fn sort_position(
    scope: TraceScope,
    positions: &[PositionFrame],
    output_index: usize,
) -> TraceEvent {
    TraceEvent::SortPosition {
        scope,
        positions: crate::trace::trace_positions(positions),
        output_index,
    }
}
fn tagged_row(rank: i64, keep: bool, tag: &str) -> Instance {
    group(vec![
        ("Rank", Instance::Scalar(Value::Int(rank))),
        ("Keep", Instance::Scalar(Value::Bool(keep))),
        ("Tag", text(tag)),
    ])
}
fn tagged_output(rank: i64, index: i64, tag: &str) -> Instance {
    group(vec![
        ("Rank", Instance::Scalar(Value::Int(rank))),
        ("Index", Instance::Scalar(Value::Int(index))),
        ("Tag", text(tag)),
    ])
}
fn sort_project(owner: SourceOwner, order: SortFilterOrder) -> Project {
    let mut p = filter_project(owner.clone(), Some(order));
    p.graph
        .nodes
        .insert(3, owned_field(owner, &["Rows", "Tag"]));
    p.root.bindings.push(Binding {
        target_field: "Tag".into(),
        node: 3,
    });
    p
}
fn tie_sort_events(scope: &TraceScope, original: &[usize], sorted: &[usize]) -> Vec<TraceEvent> {
    let mut events = original
        .iter()
        .map(|index| {
            sort_candidate(
                scope.clone(),
                &[position(*index)],
                &[(0, false, Value::Int(if *index == 2 { 1 } else { 2 }))],
            )
        })
        .collect::<Vec<_>>();
    events.extend(
        sorted
            .iter()
            .enumerate()
            .map(|(rank, index)| sort_position(scope.clone(), &[position(*index)], rank + 1)),
    );
    events
}

#[test]
fn sort_events_keep_stable_ties_filter_order_and_exact_primary_named_structural_identity() {
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
    ] {
        for order in [
            SortFilterOrder::FilterThenSort,
            SortFilterOrder::SortThenFilter,
        ] {
            let p = sort_project(owner.clone(), order);
            let primary = group(vec![(
                "Rows",
                Instance::Repeated(vec![
                    tagged_row(2, true, "P.A"),
                    tagged_row(1, false, "P.B"),
                    tagged_row(2, true, "P.C"),
                ]),
            )]);
            let extras = group(vec![
                (
                    "Ref",
                    group(vec![(
                        "Rows",
                        Instance::Repeated(vec![
                            tagged_row(2, true, "N.A"),
                            tagged_row(1, false, "N.B"),
                            tagged_row(2, true, "N.C"),
                        ]),
                    )]),
                ),
                (
                    " Ref 雪 ",
                    group(vec![(
                        "Rows",
                        Instance::Repeated(vec![
                            tagged_row(2, true, "U.A"),
                            tagged_row(1, false, "U.B"),
                            tagged_row(2, true, "U.C"),
                        ]),
                    )]),
                ),
            ]);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let (key, prefix) = match &owner {
                SourceOwner::Primary => (OwnerKey::Primary, "P"),
                SourceOwner::Named { name } if name == "Ref" => (OwnerKey::Named(0), "N"),
                SourceOwner::Named { .. } => (OwnerKey::Named(1), "U"),
            };
            let original = roots.root(key).unwrap();
            let items = original.field("Rows").unwrap().as_repeated().unwrap();
            let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                let positions = match event {
                    TraceEvent::SortCandidate { positions, .. }
                    | TraceEvent::SortPosition { positions, .. } => positions,
                    _ => unreachable!(),
                };
                let raw = positions[0].index;
                assert_eq!(view.phase, TraceOwnedPhase::Structural);
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.origins.len(), 1);
                let expected_owner = match key {
                    OwnerKey::Primary => TraceOwnedOwnerRef::Primary,
                    OwnerKey::Named(index) => TraceOwnedOwnerRef::Named {
                        declaration_index: index,
                        name: &p.extra_sources[index].name,
                    },
                };
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
                assert!(std::ptr::eq(view.frames[0].item, &items[raw - 1]));
                assert!(std::ptr::eq(view.origins[0].item, &items[raw - 1]));
                assert_eq!(view.frames[0].collection, path(&["Rows"]));
                assert_eq!(view.origins[0].path, path(&["Rows"]));
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
            let mut paired = SortObservations::new();
            paired.check = Some(&check);
            let plain = Collector::default();
            let expected = Instance::Repeated(vec![
                tagged_output(2, 1, &format!("{prefix}.A")),
                tagged_output(2, 2, &format!("{prefix}.C")),
            ]);
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
            let (original_order, ranked_order) = if order == SortFilterOrder::FilterThenSort {
                (vec![1, 3], vec![1, 3])
            } else {
                (vec![1, 2, 3], vec![2, 1, 3])
            };
            assert_eq!(
                sort_events(&paired.events.borrow()),
                tie_sort_events(&TraceScope::primary(), &original_order, &ranked_order)
            );
            let phase = if order == SortFilterOrder::FilterThenSort {
                crate::TraceFilterPhase::BeforeSort
            } else {
                crate::TraceFilterPhase::AfterSort
            };
            assert_eq!(
                decision_events(&paired.events.borrow()),
                (1..=3)
                    .map(|i| decision_event(
                        TraceScope::primary(),
                        1,
                        phase,
                        &[position(i)],
                        if order == SortFilterOrder::FilterThenSort {
                            i != 2
                        } else {
                            i != 1
                        }
                    ))
                    .collect::<Vec<_>>()
            );
            let events = paired.events.borrow();
            let first_sort = events
                .iter()
                .position(|e| matches!(e, TraceEvent::SortCandidate { .. }))
                .unwrap();
            let first_filter = events
                .iter()
                .position(|e| matches!(e, TraceEvent::FilterDecision { .. }))
                .unwrap();
            assert_eq!(
                first_filter < first_sort,
                order == SortFilterOrder::FilterThenSort
            );
            assert_eq!(
                paired.contexts.borrow().len(),
                original_order.len() + ranked_order.len()
            );
            assert!(
                paired
                    .root_addresses
                    .borrow()
                    .windows(2)
                    .all(|pair| pair[0] == pair[1])
            );
            assert!(state.frames.is_empty() && state.origins.is_empty());
        }
    }
}

#[test]
fn multiple_sort_keys_keep_directions_bounded_values_and_exact_evaluation_count() {
    let long_a = format!("A{}", "é".repeat(400));
    let long_c = format!("C{}", "é".repeat(400));
    let tags = [long_a.as_str(), "B", long_c.as_str()];
    let ranks = [2, 1, 2];
    for rank_descending in [false, true] {
        for tag_descending in [false, true] {
            let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
            p.root.filter = None;
            p.root.sort_descending = rank_descending;
            p.root.sort_then_by = vec![mapping::SortKey {
                node: 3,
                descending: tag_descending,
            }];
            let primary = group(vec![(
                "Rows",
                Instance::Repeated(
                    (0..3)
                        .map(|i| tagged_row(ranks[i], true, tags[i]))
                        .collect(),
                ),
            )]);
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = SortObservations::new();
            let indices = match (rank_descending, tag_descending) {
                (false, false) => [1, 0, 2],
                (false, true) => [1, 2, 0],
                (true, false) => [0, 2, 1],
                (true, true) => [2, 0, 1],
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
                Ok(Instance::Repeated(
                    indices
                        .iter()
                        .enumerate()
                        .map(|(i, original)| tagged_output(
                            ranks[*original],
                            i64::try_from(i + 1).unwrap(),
                            tags[*original]
                        ))
                        .collect()
                ))
            );
            let mut expected = (0..3)
                .map(|i| {
                    sort_candidate(
                        TraceScope::primary(),
                        &[position(i + 1)],
                        &[
                            (0, rank_descending, Value::Int(ranks[i])),
                            (3, tag_descending, Value::String(tags[i].into())),
                        ],
                    )
                })
                .collect::<Vec<_>>();
            expected.extend(indices.iter().enumerate().map(|(rank, original)| {
                sort_position(TraceScope::primary(), &[position(original + 1)], rank + 1)
            }));
            assert_eq!(sort_events(&sink.events.borrow()), expected);
            for node in [0, 3] {
                assert_eq!(sink.events.borrow().iter().filter(|e| matches!(e, TraceEvent::NodeValue { node: actual, .. } if *actual == node)).count(), 6);
            }
            let events = sink.events.borrow();
            for event in sort_events(&events).into_iter().take(3) {
                let TraceEvent::SortCandidate { keys, .. } = event else {
                    unreachable!()
                };
                assert_eq!(keys.len(), 2);
                assert!(keys[1].value.preview.chars().count() <= 160);
                if keys[1].value.preview.starts_with('A') || keys[1].value.preview.starts_with('C')
                {
                    assert!(keys[1].value.truncated);
                }
            }
            assert_eq!(sink.contexts.borrow().len(), 6);
        }
    }
}

#[test]
fn nested_sort_witnesses_preserve_compacted_parent_context_and_original_inner_ranks() {
    let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
    p.root.filter = None;
    let mut child = sort_project(
        SourceOwner::Named { name: "Ref".into() },
        SortFilterOrder::SortThenFilter,
    )
    .root;
    child.target_field = "Child".into();
    child.filter = None;
    child.sort_by = Some(4);
    child.bindings[0].node = 4;
    child.bindings[1].node = 6;
    child.bindings[2].node = 7;
    p.graph.nodes.insert(
        4,
        owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Rank"]),
    );
    p.graph.nodes.insert(
        6,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Named { name: "Ref".into() },
            collection: Some(path(&["Rows"])),
        }),
    );
    p.graph.nodes.insert(
        7,
        owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Tag"]),
    );
    p.root.children.push(child);
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![tagged_row(30, true, "A"), tagged_row(10, true, "B")]),
    )]);
    let extras = group(vec![(
        "Ref",
        group(vec![(
            "Rows",
            Instance::Repeated(vec![tagged_row(4, true, "X"), tagged_row(3, true, "Y")]),
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
        let (scope, positions) = match event {
            TraceEvent::SortCandidate {
                scope, positions, ..
            }
            | TraceEvent::SortPosition {
                scope, positions, ..
            } => (scope, positions),
            _ => unreachable!(),
        };
        assert_eq!(view.phase, TraceOwnedPhase::Structural);
        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
        let parent_index = if scope == &child_scope {
            if positions[0].index == 1 { 1 } else { 0 }
        } else {
            positions[0].index - 1
        };
        assert!(std::ptr::eq(view.frames[0].item, &parents[parent_index]));
        assert!(std::ptr::eq(view.origins[0].item, &parents[parent_index]));
        assert_eq!(view.positions[0].index, positions[0].index);
        if scope == &child_scope {
            assert_eq!(view.frames.len(), 2);
            assert_eq!(view.frames[1].position_slot, 1);
            assert_eq!(
                view.frames[1].owner,
                TraceOwnedOwnerRef::Named {
                    declaration_index: 0,
                    name: "Ref"
                }
            );
            assert!(std::ptr::eq(
                view.frames[1].item,
                &children[positions[1].index - 1]
            ));
            assert!(std::ptr::eq(
                view.origins[1].item,
                &children[positions[1].index - 1]
            ));
            assert_eq!(view.positions[1].index, positions[1].index);
        } else {
            assert_eq!(view.frames.len(), 1);
        }
    };
    let mut sink = SortObservations::new();
    sink.check = Some(&check);
    let parent_output = |rank, index, tag| {
        group(vec![
            ("Rank", Instance::Scalar(Value::Int(rank))),
            ("Index", Instance::Scalar(Value::Int(index))),
            ("Tag", text(tag)),
            (
                "Child",
                Instance::Repeated(vec![tagged_output(3, 1, "Y"), tagged_output(4, 2, "X")]),
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
            parent_output(10, 1, "B"),
            parent_output(30, 2, "A")
        ]))
    );
    let mut expected = vec![
        sort_candidate(
            TraceScope::primary(),
            &[position(1)],
            &[(0, false, Value::Int(30))],
        ),
        sort_candidate(
            TraceScope::primary(),
            &[position(2)],
            &[(0, false, Value::Int(10))],
        ),
        sort_position(TraceScope::primary(), &[position(2)], 1),
        sort_position(TraceScope::primary(), &[position(1)], 2),
    ];
    for outer in 1..=2 {
        expected.extend([
            sort_candidate(
                child_scope.clone(),
                &[position(outer), position(1)],
                &[(4, false, Value::Int(4))],
            ),
            sort_candidate(
                child_scope.clone(),
                &[position(outer), position(2)],
                &[(4, false, Value::Int(3))],
            ),
            sort_position(child_scope.clone(), &[position(outer), position(2)], 1),
            sort_position(child_scope.clone(), &[position(outer), position(1)], 2),
        ]);
    }
    assert_eq!(sort_events(&sink.events.borrow()), expected);
    assert_eq!(sink.contexts.borrow().len(), 12);
    assert!(state.frames.is_empty() && state.origins.is_empty());
}

#[test]
fn sort_event_preferences_keep_default_legacy_vectors_and_root_cache_lazy() {
    let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
    p.root.filter = None;
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            tagged_row(2, true, "A"),
            tagged_row(1, true, "B"),
            tagged_row(2, true, "C"),
        ]),
    )]);
    let extras = group(Vec::new());
    for (selected, callbacks) in [(0, 0), (1, 3), (2, 3), (3, 6)] {
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        sink.selected.set(selected);
        let plain = Collector::default();
        let expected = Instance::Repeated(vec![
            tagged_output(1, 1, "B"),
            tagged_output(2, 2, "A"),
            tagged_output(2, 3, "C"),
        ]);
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
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert_eq!(sink.contexts.borrow().len(), callbacks);
        assert_eq!(registry.cache.get().is_some(), callbacks != 0);
        assert_eq!(
            sort_events(&sink.events.borrow()),
            tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
        );
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
    let mut changing = SortObservations::new();
    changing.stop_after_first = true;
    run(
        &p,
        &state,
        &registry,
        Some(&changing),
        TraceOwnedPhase::Output,
        &[],
    )
    .unwrap();
    assert_eq!(changing.contexts.borrow().len(), 1);
    assert_eq!(sort_events(&changing.events.borrow()).len(), 6);
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
    run(
        &p,
        &state,
        &registry,
        Some(&old),
        TraceOwnedPhase::Output,
        &[],
    )
    .unwrap();
    assert_eq!(old.0.get(), 12);
    p.root.sort_by = None;
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = SortObservations::new();
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
            tagged_output(2, 1, "A"),
            tagged_output(1, 2, "B"),
            tagged_output(2, 3, "C")
        ]))
    );
    assert!(sort_events(&sink.events.borrow()).is_empty());
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    p.root.sort_by = Some(0);
    p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
    p.graph.nodes.insert(
        0,
        Node::SourceField {
            path: path(&["Rank"]),
            frame: Some(path(&["Rows"])),
        },
    );
    p.graph.nodes.insert(
        3,
        Node::SourceField {
            path: path(&["Tag"]),
            frame: Some(path(&["Rows"])),
        },
    );
    p.root.bindings.retain(|binding| binding.node != 2);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = SortObservations::new();
    let plain = Collector::default();
    let expected = Instance::Repeated(
        [("B", 1), ("A", 2), ("C", 2)]
            .into_iter()
            .map(|(tag, rank)| {
                group(vec![
                    ("Rank", Instance::Scalar(Value::Int(rank))),
                    ("Tag", text(tag)),
                ])
            })
            .collect(),
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
        sort_events(&sink.events.borrow()),
        tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
    );
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
}

#[test]
fn sorted_first_and_explicit_windows_observe_full_rank_stage_before_output_truncation() {
    for first in [false, true] {
        let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
        p.root.filter = None;
        if first {
            p.root.iteration_output = mapping::IterationOutput::First;
        } else {
            p.graph.nodes.insert(
                4,
                Node::Const {
                    value: Value::Int(2),
                },
            );
            p.root.windows.push(SequenceWindow::First { count: 4 });
        }
        assert!(!crate::eval_scope::retain_first_source_candidate(&p.root));
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                tagged_row(2, true, "A"),
                tagged_row(1, true, "B"),
                tagged_row(2, true, "C"),
            ]),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Ok(if first {
                tagged_output(1, 1, "B")
            } else {
                Instance::Repeated(vec![tagged_output(1, 1, "B"), tagged_output(2, 2, "A")])
            })
        );
        assert_eq!(
            sort_events(&sink.events.borrow()),
            tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
        );
        assert_eq!(sink.contexts.borrow().len(), 6);
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|e| matches!(e, TraceEvent::NodeValue { node: 0, .. }))
                .count(),
            if first { 4 } else { 5 }
        );
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|e| matches!(e, TraceEvent::TargetProduced { .. }))
                .count(),
            if first { 1 } else { 2 }
        );
        assert_eq!(
            boundaries(&sink.events.borrow())[1],
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: 3,
                produced: if first { 1 } else { 2 },
                kind: if first {
                    TraceOutputKind::Group
                } else {
                    TraceOutputKind::Repeated
                }
            }
        );
        let events = sink.events.borrow();
        let last_rank = events
            .iter()
            .rposition(|e| matches!(e, TraceEvent::SortPosition { .. }))
            .unwrap();
        let first_target = events
            .iter()
            .position(|e| matches!(e, TraceEvent::TargetProduced { .. }))
            .unwrap();
        assert!(last_rank < first_target);
    }
    for first in [false, true] {
        let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
        p.root.filter = None;
        if first {
            p.root.iteration_output = mapping::IterationOutput::First;
        } else {
            p.graph.nodes.insert(
                4,
                Node::Const {
                    value: Value::Int(2),
                },
            );
            p.root.windows.push(SequenceWindow::First { count: 4 });
        }
        let primary = group(vec![("Rows", Instance::Repeated(Vec::new()))]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
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
                group(Vec::new())
            } else {
                Instance::Repeated(Vec::new())
            })
        );
        assert!(sort_events(&sink.events.borrow()).is_empty());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        assert!(
            !sink
                .events
                .borrow()
                .iter()
                .any(|e| matches!(e, TraceEvent::TargetProduced { .. }))
        );
        assert_eq!(
            boundaries(&sink.events.borrow())[1],
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: 0,
                produced: 0,
                kind: if first {
                    TraceOutputKind::Group
                } else {
                    TraceOutputKind::Repeated
                }
            }
        );
    }
}

#[test]
fn sort_key_target_and_observer_failures_keep_exact_prefixes_and_mapping_priority() {
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            tagged_row(2, true, "A"),
            tagged_row(1, false, "B"),
            tagged_row(2, true, "C"),
        ]),
    )]);
    let extras = group(Vec::new());
    let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
    p.root.filter = None;
    p.root.sort_then_by.push(mapping::SortKey {
        node: 4,
        descending: false,
    });
    p.graph.nodes.insert(
        4,
        Node::If {
            condition: 1,
            then: 5,
            else_: 6,
        },
    );
    p.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::Int(7),
        },
    );
    p.graph.nodes.insert(6, Node::Raise { message: None });
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = SortObservations::new();
    let plain = Collector::default();
    let expected = Err(EngineError::MappingException {
        node: 6,
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
        sort_events(&sink.events.borrow()),
        vec![sort_candidate(
            TraceScope::primary(),
            &[position(1)],
            &[(0, false, Value::Int(2)), (4, false, Value::Int(7))]
        )]
    );
    assert_eq!(sink.contexts.borrow().len(), 1);
    assert!(!sink.events.borrow().iter().any(|e| matches!(
        e,
        TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
    )));
    p.graph.nodes.insert(
        4,
        owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Rank"]),
    );
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = SortObservations::new();
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
            node: Some(4),
            scope: None,
            source: OwnedSourceError::SourceUnavailable {
                owner: SourceOwner::Named { name: "Ref".into() }
            }
        })
    );
    assert!(sort_events(&sink.events.borrow()).is_empty());
    assert!(sink.contexts.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    p.root.sort_then_by.clear();
    p.graph.nodes.insert(3, Node::Raise { message: None });
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let sink = SortObservations::new();
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
            node: 3,
            message: None
        })
    );
    assert_eq!(
        sort_events(&sink.events.borrow()),
        tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
    );
    assert_eq!(
        sink.events
            .borrow()
            .iter()
            .filter(|e| matches!(e, TraceEvent::TargetFieldWritten { .. }))
            .count(),
        2
    );
    assert!(!sink.events.borrow().iter().any(|e| matches!(
        e,
        TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
    )));
    let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
    p.root.filter = None;
    let foreign = constant();
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let wrong = TraceOwnedRegistry::new(&foreign, &roots);
    let sink = SortObservations::new();
    let expected = Instance::Repeated(vec![
        tagged_output(1, 1, "B"),
        tagged_output(2, 2, "A"),
        tagged_output(2, 3, "C"),
    ]);
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
    assert_eq!(sink.contexts.borrow().len(), 6);
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
    let sink = SortObservations::new();
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
    assert_eq!(sink.contexts.borrow().len(), 6);
    assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
        == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
            at: TraceOwnedLocation::Frame(0),
            slot: 7
        }))));
    assert!(std::ptr::eq(state.frames[0].item, &primary));
}
