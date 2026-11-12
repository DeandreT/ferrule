use super::windows::value_rows;
use super::*;

type FailureTypedCheck<'a> = dyn Fn(&crate::PendingNodeFailure, &EngineError) + 'a;
type FailureWitnessCheck<'a> = dyn Fn(&crate::PendingNodeFailure, &TraceOwnedWitness<'_>) + 'a;
struct FailureObservations<'a> {
    pending: RefCell<Vec<crate::PendingNodeFailure>>,
    outcomes: RefCell<Vec<Outcome>>,
    root_addresses: RefCell<Vec<usize>>,
    selected: Cell<bool>,
    interested: bool,
    cancel: bool,
    select_at_value: Option<(u32, bool)>,
    typed: Option<&'a FailureTypedCheck<'a>>,
    check: Option<&'a FailureWitnessCheck<'a>>,
    function_failures: Cell<usize>,
    function_interest: bool,
}
impl FailureObservations<'_> {
    fn new() -> Self {
        Self {
            pending: RefCell::new(Vec::new()),
            outcomes: RefCell::new(Vec::new()),
            root_addresses: RefCell::new(Vec::new()),
            selected: Cell::new(true),
            interested: true,
            cancel: false,
            select_at_value: None,
            typed: None,
            check: None,
            function_failures: Cell::new(0),
            function_interest: false,
        }
    }
}
impl crate::DebugHook for FailureObservations<'_> {
    fn before_target_write(&self, _: &crate::PendingTargetWrite) -> crate::DebugDecision {
        crate::DebugDecision::Resume
    }
    fn wants_node_values(&self) -> bool {
        self.select_at_value.is_some()
    }
    fn wants_node_inputs(&self) -> bool {
        false
    }
    fn wants_node_failures(&self) -> bool {
        self.interested
    }
    fn wants_owned_context_for_node_failure(&self, _: &crate::PendingNodeFailure) -> bool {
        self.selected.get()
    }
    fn after_node_value(&self, value: &crate::PendingNodeValue) -> crate::DebugDecision {
        if let Some((node, selected)) = self.select_at_value
            && value.node == node
        {
            self.selected.set(selected);
        }
        crate::DebugDecision::Resume
    }
    fn after_node_failure(&self, pending: &crate::PendingNodeFailure) -> crate::DebugDecision {
        self.pending.borrow_mut().push(pending.clone());
        if self.cancel {
            crate::DebugDecision::Cancel
        } else {
            crate::DebugDecision::Resume
        }
    }
    fn after_node_failure_with_owned_context(
        &self,
        pending: &crate::PendingNodeFailure,
        error: &EngineError,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) -> crate::DebugDecision {
        if let Some(check) = self.typed {
            check(pending, error);
        }
        let outcome = match context {
            Ok(view) => {
                if let Some(check) = self.check {
                    check(pending, &view);
                }
                self.root_addresses
                    .borrow_mut()
                    .push(view.roots.as_ptr() as usize);
                Outcome::Snapshot(view.snapshot())
            }
            Err(error) => Outcome::Projection(error.clone()),
        };
        self.outcomes.borrow_mut().push(outcome);
        self.after_node_failure(pending)
    }
    fn wants_function_node_failures(&self) -> bool {
        self.function_interest
    }
    fn after_function_node_failure(
        &self,
        pending: &crate::PendingFunctionNodeFailure,
    ) -> crate::DebugDecision {
        assert_eq!(pending.function, mapping::FunctionId::new(9));
        assert_eq!(pending.node, 2);
        self.function_failures.set(self.function_failures.get() + 1);
        crate::DebugDecision::Resume
    }
}
#[allow(clippy::too_many_arguments)]
fn failure_expr(
    p: &Project,
    state: &OwnedContext<'_>,
    registry: Option<&dyn TraceOwnedRegistryView>,
    sink: Option<&dyn TraceSink>,
    hook: Option<&dyn crate::DebugHook>,
    reported: &Cell<bool>,
    phase: TraceOwnedPhase,
    positions: &[PositionFrame],
    node: u32,
) -> Result<Value, EngineError> {
    let mut program = EvalProgram::new(&p.graph, &p.user_functions, sink, reported)
        .with_owned_context(state)
        .with_owned_trace_phase(phase)
        .with_primary_source(state.roots.primary)
        .with_debug_hook(hook);
    if let Some(registry) = registry {
        program = program.with_owned_trace_registry(registry);
    }
    let result = crate::eval_expr::eval_expr(
        program,
        node,
        &[state.roots.extras_wrapper, state.roots.primary],
        positions,
        &mut std::collections::HashSet::new(),
    );
    eprintln!(
        "RAW failure expression node={node} phase={phase:?} positions={positions:#?} complete_result={result:#?}",
        positions = crate::trace::trace_positions(positions)
    );
    result
}
fn failure_scope(
    p: &Project,
    state: &OwnedContext<'_>,
    registry: &dyn TraceOwnedRegistryView,
    sink: Option<&dyn TraceSink>,
    hook: &dyn crate::DebugHook,
) -> Result<Instance, EngineError> {
    let reported = Cell::new(false);
    let program = EvalProgram::new(&p.graph, &p.user_functions, sink, &reported)
        .with_owned_context(state)
        .with_owned_trace_registry(registry)
        .with_primary_source(state.roots.primary)
        .with_debug_hook(Some(hook));
    let result = crate::eval_scope::eval_scope(
        program,
        &p.root,
        None,
        &[state.roots.extras_wrapper, state.roots.primary],
        &[],
        &p.extra_sources,
        None,
        &TraceScope::primary(),
    );
    eprintln!("RAW failure scope complete_result={result:#?}");
    result
}

#[test]
fn failure_owned_default_declining_and_forwarding_hooks_preserve_original_contract() {
    struct ForwardOnly {
        pending: RefCell<Vec<crate::PendingNodeFailure>>,
        owned: bool,
    }
    impl crate::DebugHook for ForwardOnly {
        fn before_target_write(&self, _: &crate::PendingTargetWrite) -> crate::DebugDecision {
            crate::DebugDecision::Resume
        }
        fn wants_node_values(&self) -> bool {
            false
        }
        fn wants_node_inputs(&self) -> bool {
            false
        }
        fn wants_node_failures(&self) -> bool {
            true
        }
        fn wants_owned_context_for_node_failure(&self, _: &crate::PendingNodeFailure) -> bool {
            self.owned
        }
        fn after_node_failure(&self, pending: &crate::PendingNodeFailure) -> crate::DebugDecision {
            self.pending.borrow_mut().push(pending.clone());
            crate::DebugDecision::Resume
        }
    }
    let mut p = constant();
    p.graph.nodes.insert(0, Node::Raise { message: None });
    let primary = row("primary");
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let expected = EngineError::MappingException {
        node: 0,
        message: None,
    };
    let plain = Collector::default();
    let flag = Cell::new(false);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    assert_eq!(
        failure_expr(
            &p,
            &state,
            Some(&registry),
            Some(&plain),
            None,
            &flag,
            TraceOwnedPhase::Selection,
            &[],
            0
        )
        .as_ref(),
        Err(&expected)
    );
    assert!(!flag.get());
    assert!(registry.cache.get().is_none());
    let mut old_pending = None;
    for owned in [false, true] {
        let hook = ForwardOnly {
            pending: RefCell::new(Vec::new()),
            owned,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Collector::default();
        let flag = Cell::new(false);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                Some(&sink),
                Some(&hook),
                &flag,
                TraceOwnedPhase::Selection,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert!(flag.get());
        assert_eq!(*sink.0.borrow(), *plain.0.borrow());
        assert_eq!(hook.pending.borrow().len(), 1);
        assert_eq!(hook.pending.borrow()[0].node, 0);
        assert_eq!(hook.pending.borrow()[0].error.preview, expected.to_string());
        assert!(!hook.pending.borrow()[0].error.truncated);
        if let Some(old) = &old_pending {
            assert_eq!(&*hook.pending.borrow(), old);
        } else {
            old_pending = Some(hook.pending.borrow().clone());
        }
        assert_eq!(registry.cache.get().is_some(), owned);
    }
    let mut hook = FailureObservations::new();
    hook.interested = false;
    let flag = Cell::new(false);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    assert_eq!(
        failure_expr(
            &p,
            &state,
            Some(&registry),
            None,
            Some(&hook),
            &flag,
            TraceOwnedPhase::Selection,
            &[],
            0
        )
        .as_ref(),
        Err(&expected)
    );
    assert!(!flag.get());
    assert!(hook.pending.borrow().is_empty());
    assert!(hook.outcomes.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    let hook = FailureObservations::new();
    let flag = Cell::new(false);
    assert_eq!(
        failure_expr(
            &p,
            &state,
            None,
            None,
            Some(&hook),
            &flag,
            TraceOwnedPhase::Selection,
            &[],
            0
        )
        .as_ref(),
        Err(&expected)
    );
    assert_eq!(hook.pending.borrow().len(), 1);
    assert!(hook.outcomes.borrow().is_empty());
}

#[test]
fn failure_owned_deepest_callback_borrows_full_typed_leaf_and_wrapped_causes() {
    for missing in [false, true] {
        let p = project(
            &[],
            vec![
                (
                    0,
                    Node::If {
                        condition: 1,
                        then: 2,
                        else_: 4,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    2,
                    if missing {
                        Node::SourceField {
                            path: path(&["Absent"]),
                            frame: None,
                        }
                    } else {
                        Node::Raise { message: Some(3) }
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("full typed message".into()),
                    },
                ),
                (4, Node::Raise { message: None }),
            ],
            Scope::default(),
        );
        let primary = row("primary");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Collector::default();
        let reported = Cell::new(false);
        let expected = if missing {
            EngineError::MissingSourceField("Absent".into())
        } else {
            EngineError::MappingException {
                node: 2,
                message: Some("full typed message".into()),
            }
        };
        let typed = |pending: &crate::PendingNodeFailure, error: &EngineError| {
            assert_eq!(pending.node, 2);
            assert_eq!(error, &expected);
        };
        let mut hook = FailureObservations::new();
        hook.typed = Some(&typed);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                Some(&sink),
                Some(&hook),
                &reported,
                TraceOwnedPhase::Structural,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert_eq!(hook.pending.borrow().len(), 1);
        assert_eq!(hook.pending.borrow()[0].node, 2);
        assert_eq!(hook.outcomes.borrow().len(), 1);
        assert!(reported.get());
        assert!(!sink.0.borrow().iter().any(|event| matches!(
            event,
            TraceEvent::NodeValue {
                node: 0 | 2 | 4,
                ..
            } | TraceEvent::NodeInputValue {
                consumer: 0,
                input: 2 | 4,
                ..
            }
        )));
        assert_eq!(
            snapshot(&hook.outcomes.borrow()[0]).phase,
            TraceOwnedPhase::Structural
        );
        let run =
            crate::filter_map::FilterMapRunState::new(crate::FilterMapLimits::default(), None);
        let boundary = crate::FilterMapBoundary {
            item: 99,
            phase: crate::FilterMapPhase::Mapper,
            capture_index: Some(3),
            source_position: Some(7),
            function: None,
            node: Some(2),
            kind: crate::FilterMapBoundaryKind::NodeEvaluation,
        };
        let wrapped = EngineError::FilterMapRuntime {
            boundary,
            source: Box::new(if missing {
                EngineError::MissingSourceField("Absent".into())
            } else {
                EngineError::MappingException {
                    node: 2,
                    message: Some("full typed message".into()),
                }
            }),
        };
        let typed = |pending: &crate::PendingNodeFailure, error: &EngineError| {
            assert_eq!(pending.node, 2);
            assert_eq!(error, &wrapped);
        };
        let mut hook = FailureObservations::new();
        hook.typed = Some(&typed);
        let reported = Cell::new(false);
        let program = EvalProgram::new(&p.graph, &p.user_functions, None, &reported)
            .with_owned_context(&state)
            .with_owned_trace_registry(&registry)
            .with_debug_hook(Some(&hook))
            .with_filter_map_work(crate::filter_map::FilterMapWork {
                state: &run,
                boundary,
            });
        let result = crate::eval_expr::eval_expr(
            program,
            0,
            &[&extras, &primary],
            &[],
            &mut std::collections::HashSet::new(),
        );
        eprintln!("RAW complete wrapped failure={result:#?}");
        assert_eq!(result.as_ref(), Err(&wrapped));
        assert_eq!(hook.pending.borrow().len(), 1);
        assert_eq!(hook.outcomes.borrow().len(), 1);
    }
}

#[test]
fn failure_owned_witness_uses_actual_named_roots_frames_phases_and_compacted_parent() {
    for phase in [
        TraceOwnedPhase::Structural,
        TraceOwnedPhase::Selection,
        TraceOwnedPhase::Output,
    ] {
        for key in [OwnerKey::Primary, OwnerKey::Named(1)] {
            let mut p = project(
                &["Ref", " Ref 雪 "],
                vec![(0, Node::Raise { message: None })],
                Scope::default(),
            );
            p.root.bindings = vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }];
            let primary = group(vec![
                ("Rows", Instance::Repeated(vec![row("P.one"), row("P.two")])),
                ("Ref", row("primary shadow")),
            ]);
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
            let check = |pending: &crate::PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
                assert_eq!(pending.node, 0);
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
                assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
                if let OwnerKey::Named(index) = key {
                    let TraceOwnedOwnerRef::Named {
                        declaration_index,
                        name,
                    } = view.frames[0].owner
                    else {
                        panic!("named owner")
                    };
                    assert_eq!(declaration_index, index);
                    assert_eq!(name, " Ref 雪 ");
                    assert_eq!(name.as_ptr(), p.extra_sources[index].name.as_ptr());
                    assert!(std::ptr::eq(view.roots[index + 1].input.unwrap(), original));
                } else {
                    assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
                }
            };
            let mut hook = FailureObservations::new();
            hook.check = Some(&check);
            let reported = Cell::new(false);
            assert_eq!(
                failure_expr(
                    &p,
                    &state,
                    Some(&registry),
                    None,
                    Some(&hook),
                    &reported,
                    phase,
                    &positions,
                    0
                ),
                Err(EngineError::MappingException {
                    node: 0,
                    message: None
                })
            );
            assert_eq!(hook.outcomes.borrow().len(), 1);
            assert!(snapshot(&hook.outcomes.borrow()[0]).owned_ancestry);
        }
    }
    let mut p = repeated(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(10, owned_field(SourceOwner::Primary, &["Rows", "Keep"]));
    p.root.filter = Some(10);
    let mut child = repeated(SourceOwner::Named { name: "Ref".into() }).root;
    child.target_field = "Child".into();
    child.bindings[0].node = 20;
    p.root.children.push(child);
    p.graph.nodes.insert(20, Node::Raise { message: Some(21) });
    p.graph.nodes.insert(
        21,
        owned_field(
            SourceOwner::Named { name: "Ref".into() },
            &["Rows", "Value"],
        ),
    );
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![
                ("Value", text("discard")),
                ("Keep", Instance::Scalar(Value::Bool(false))),
            ]),
            group(vec![
                ("Value", text("keep")),
                ("Keep", Instance::Scalar(Value::Bool(true))),
            ]),
        ]),
    )]);
    let extras = group(vec![("Ref", value_rows(&["named-child", "later"]))]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let parent = &primary.field("Rows").unwrap().as_repeated().unwrap()[1];
    let named = roots.root(OwnerKey::Named(0)).unwrap();
    let child = &named.field("Rows").unwrap().as_repeated().unwrap()[0];
    let check = |pending: &crate::PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
        assert_eq!(pending.node, 20);
        assert_eq!(view.phase, TraceOwnedPhase::Output);
        assert_eq!(view.frames.len(), 2);
        assert_eq!(view.origins.len(), 2);
        assert_eq!(view.positions.len(), 2);
        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
        assert!(std::ptr::eq(view.frames[0].item, parent));
        assert!(std::ptr::eq(view.origins[0].item, parent));
        assert_eq!(view.positions[0].index, 1);
        assert_eq!(view.frames[0].position_slot, 0);
        assert!(std::ptr::eq(view.frames[1].item, child));
        assert!(std::ptr::eq(view.origins[1].item, child));
        assert_eq!(view.positions[1].index, 1);
        assert_eq!(view.frames[1].position_slot, 1);
        let TraceOwnedOwnerRef::Named {
            declaration_index,
            name,
        } = view.frames[1].owner
        else {
            panic!("child owner")
        };
        assert_eq!(declaration_index, 0);
        assert_eq!(name.as_ptr(), p.extra_sources[0].name.as_ptr());
    };
    let mut hook = FailureObservations::new();
    hook.check = Some(&check);
    assert_eq!(
        failure_scope(&p, &state, &registry, None, &hook),
        Err(EngineError::MappingException {
            node: 20,
            message: Some("named-child".into())
        })
    );
    assert_eq!(hook.outcomes.borrow().len(), 1);
}

#[test]
fn failure_owned_selection_and_projection_snapshot_errors_remain_metadata() {
    let p = project(
        &[],
        vec![
            (0, Node::Raise { message: Some(1) }),
            (
                1,
                Node::Const {
                    value: Value::String("message".into()),
                },
            ),
        ],
        Scope::default(),
    );
    let primary = row("primary");
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let expected = EngineError::MappingException {
        node: 0,
        message: Some("message".into()),
    };
    for selected in [false, true] {
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let mut hook = FailureObservations::new();
        hook.selected.set(!selected);
        hook.select_at_value = Some((1, selected));
        let flag = Cell::new(false);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Output,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert_eq!(hook.pending.borrow().len(), 1);
        assert_eq!(hook.outcomes.borrow().len(), usize::from(selected));
        assert_eq!(registry.cache.get().is_some(), selected);
    }
    let mut foreign = constant();
    foreign.extra_sources = project(&["Unused"], Vec::new(), Scope::default()).extra_sources;
    let registry = TraceOwnedRegistry::new(&foreign, &roots);
    let hook = FailureObservations::new();
    let flag = Cell::new(false);
    assert_eq!(
        failure_expr(
            &p,
            &state,
            Some(&registry),
            None,
            Some(&hook),
            &flag,
            TraceOwnedPhase::Output,
            &[],
            0
        )
        .as_ref(),
        Err(&expected)
    );
    assert_eq!(
        *hook.outcomes.borrow(),
        vec![Outcome::Projection(
            TraceOwnedContextError::MismatchedRegistry
        )]
    );
    assert_eq!(hook.pending.borrow().len(), 1);
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
    let hook = FailureObservations::new();
    let flag = Cell::new(false);
    assert_eq!(
        failure_expr(
            &p,
            &state,
            Some(&registry),
            None,
            Some(&hook),
            &flag,
            TraceOwnedPhase::Selection,
            &[],
            0
        )
        .as_ref(),
        Err(&expected)
    );
    assert_eq!(
        *hook.outcomes.borrow(),
        vec![Outcome::Snapshot(Err(
            TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 7
            }
        ))]
    );
    assert_eq!(hook.pending.borrow().len(), 1);
}

#[test]
fn failure_owned_cancel_function_priority_lazy_branches_and_public_failures_stay_exact() {
    let p = project(
        &[],
        vec![
            (
                0,
                Node::If {
                    condition: 1,
                    then: 2,
                    else_: 3,
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (2, Node::Raise { message: None }),
            (
                3,
                Node::Const {
                    value: Value::Int(7),
                },
            ),
        ],
        Scope::default(),
    );
    let primary = row("primary");
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let error = EngineError::MappingException {
        node: 2,
        message: None,
    };
    for cancel in [false, true] {
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let mut hook = FailureObservations::new();
        hook.cancel = cancel;
        let flag = Cell::new(false);
        let result = failure_expr(
            &p,
            &state,
            Some(&registry),
            None,
            Some(&hook),
            &flag,
            TraceOwnedPhase::Selection,
            &[],
            0,
        );
        assert_eq!(
            result.as_ref(),
            if cancel {
                Err(&EngineError::DebugCancelled)
            } else {
                Err(&error)
            }
        );
        assert_eq!(hook.pending.borrow().len(), 1);
        assert_eq!(hook.pending.borrow()[0].node, 2);
        assert_eq!(hook.outcomes.borrow().len(), 1);
    }
    for function_interest in [false, true] {
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let mut hook = FailureObservations::new();
        hook.function_interest = function_interest;
        let flag = Cell::new(false);
        assert_eq!(
            crate::debug::after_function_node_failure(
                Some(&hook),
                &flag,
                mapping::FunctionId::new(9),
                2,
                &error,
                &[]
            ),
            Ok(())
        );
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Selection,
                &[],
                0
            )
            .as_ref(),
            Err(&error)
        );
        assert_eq!(hook.function_failures.get(), usize::from(function_interest));
        assert_eq!(hook.pending.borrow().len(), usize::from(!function_interest));
        assert_eq!(registry.cache.get().is_some(), !function_interest);
    }
    let registry = TraceOwnedRegistry::new(&p, &roots);
    let hook = FailureObservations::new();
    let flag = Cell::new(false);
    let program = EvalProgram::new(&p.graph, &p.user_functions, None, &flag)
        .with_owned_context(&state)
        .with_owned_trace_registry(&registry)
        .with_debug_hook(Some(&hook));
    assert_eq!(
        crate::debug::after_node_failure(
            program,
            2,
            &EngineError::DebugCancelled,
            &[],
            &[&primary],
            Some(&primary)
        ),
        Ok(())
    );
    assert!(!flag.get());
    assert!(hook.pending.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    let mut lazy = p.clone();
    lazy.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    let registry = TraceOwnedRegistry::new(&lazy, &roots);
    let flag = Cell::new(false);
    assert_eq!(
        failure_expr(
            &lazy,
            &state,
            Some(&registry),
            None,
            Some(&hook),
            &flag,
            TraceOwnedPhase::Structural,
            &[],
            0
        ),
        Ok(Value::Int(7))
    );
    assert!(!flag.get());
    assert!(hook.pending.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    let mut owned = repeated(SourceOwner::Named { name: "Ref".into() });
    owned.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    owned.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: Vec::new(),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 10 },
        message: None,
    });
    let roots = SourceRoots::new(&owned, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&owned, &roots);
    let hook = FailureObservations::new();
    assert_eq!(
        failure_scope(&owned, &state, &registry, None, &hook),
        Err(EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(TraceScope::primary().source_scope)),
            source: OwnedSourceError::SourceUnavailable {
                owner: SourceOwner::Named { name: "Ref".into() }
            }
        })
    );
    assert!(hook.pending.borrow().is_empty());
    assert!(hook.outcomes.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    let flag = Cell::new(false);
    let program = EvalProgram::new(&owned.graph, &owned.user_functions, None, &flag)
        .with_owned_context(&state)
        .with_owned_trace_registry(&registry)
        .with_debug_hook(Some(&hook));
    assert_eq!(
        crate::failure::evaluate(program, &owned.failure_rules, &[&extras, &primary]),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
    assert!(hook.pending.borrow().is_empty());
    assert!(!flag.get());
    let sink = Collector::default();
    let execution =
        crate::ExecutionContext::new(std::path::Path::new("mapping.json")).with_debug_hook(&hook);
    assert_eq!(
        crate::run_with_context(&owned, &primary, &execution),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
    let execution = crate::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_debug_hook(&hook)
        .with_trace_sink(&sink);
    assert_eq!(
        crate::run_with_context(&owned, &primary, &execution),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
    assert!(
        sink.0
            .borrow()
            .iter()
            .all(|event| matches!(event, TraceEvent::NodeValue { node: 10, .. }))
    );
    assert_eq!(sink.0.borrow().len(), 1);
    assert!(hook.pending.borrow().is_empty());
}

#[test]
fn failure_owned_late_large_source_retains_one_bounded_observation_without_success_history() {
    struct Counts {
        candidates: Cell<usize>,
        produced: Cell<usize>,
        values: Cell<usize>,
    }
    impl TraceSink for Counts {
        fn wants_source_row(&self) -> bool {
            false
        }
        fn record(&self, event: TraceEvent) {
            match event {
                TraceEvent::IterationCandidate { .. } => {
                    self.candidates.set(self.candidates.get() + 1)
                }
                TraceEvent::TargetProduced { .. } => self.produced.set(self.produced.get() + 1),
                TraceEvent::NodeValue { .. } => self.values.set(self.values.get() + 1),
                _ => {}
            }
        }
    }
    let mut p = repeated(SourceOwner::Primary);
    p.graph.nodes.insert(
        0,
        Node::If {
            condition: 10,
            then: 11,
            else_: 12,
        },
    );
    p.graph
        .nodes
        .insert(10, owned_field(SourceOwner::Primary, &["Rows", "Keep"]));
    p.graph.nodes.insert(11, Node::Raise { message: Some(13) });
    p.graph
        .nodes
        .insert(12, owned_field(SourceOwner::Primary, &["Rows", "Value"]));
    p.graph
        .nodes
        .insert(13, owned_field(SourceOwner::Primary, &["Rows", "Value"]));
    let message = "late full typed message 雪 ".repeat(300);
    let primary = group(vec![(
        "Rows",
        Instance::Repeated(
            (0..4096)
                .map(|index| {
                    group(vec![
                        ("Value", text(if index == 4095 { &message } else { "ok" })),
                        ("Keep", Instance::Scalar(Value::Bool(index == 4095))),
                    ])
                })
                .collect(),
        ),
    )]);
    let extras = group(Vec::new());
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let last = &primary.field("Rows").unwrap().as_repeated().unwrap()[4095];
    let expected = EngineError::MappingException {
        node: 11,
        message: Some(message.clone()),
    };
    let mut baseline = None;
    for selected in [false, true] {
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let counts = Counts {
            candidates: Cell::new(0),
            produced: Cell::new(0),
            values: Cell::new(0),
        };
        let mut hook = FailureObservations::new();
        hook.selected.set(selected);
        let typed = |pending: &crate::PendingNodeFailure, error: &EngineError| {
            assert_eq!(pending.node, 11);
            assert_eq!(error, &expected);
            assert!(pending.error.truncated);
            assert_eq!(pending.error.preview.chars().count(), 512);
        };
        let check = |_: &crate::PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
            assert_eq!(view.phase, TraceOwnedPhase::Output);
            assert_eq!(view.frames.len(), 1);
            assert_eq!(view.positions[0].index, 4096);
            assert!(std::ptr::eq(view.frames[0].item, last));
            assert!(std::ptr::eq(view.origins[0].item, last));
            assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
        };
        hook.typed = Some(&typed);
        hook.check = Some(&check);
        assert_eq!(
            failure_scope(&p, &state, &registry, Some(&counts), &hook).as_ref(),
            Err(&expected)
        );
        assert_eq!(hook.pending.borrow().len(), 1);
        assert_eq!(hook.pending.borrow()[0].node, 11);
        assert!(hook.pending.borrow()[0].error.truncated);
        assert_eq!(hook.pending.borrow()[0].error.preview.chars().count(), 512);
        assert_eq!(hook.outcomes.borrow().len(), usize::from(selected));
        assert_eq!(hook.root_addresses.borrow().len(), usize::from(selected));
        assert_eq!(counts.candidates.get(), 4096);
        assert_eq!(counts.produced.get(), 4095);
        let summary = (
            counts.candidates.get(),
            counts.produced.get(),
            counts.values.get(),
        );
        if let Some(old) = baseline {
            assert_eq!(summary, old);
        } else {
            baseline = Some(summary);
        }
        if selected {
            assert_eq!(
                snapshot(&hook.outcomes.borrow()[0]).phase,
                TraceOwnedPhase::Output
            );
        }
        assert_eq!(registry.cache.get().is_some(), selected);
    }
}
