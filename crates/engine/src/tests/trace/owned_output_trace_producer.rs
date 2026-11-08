use super::{Collector, field_trace_project};
use crate::eval_expr::EvalProgram;
use crate::owned_context::{
    OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
};
use crate::source_iteration::PositionFrame;
use crate::trace::{TraceOwnedRegistry, record_owned_event};
use crate::{
    EngineError, TraceEvent, TraceOutputKind, TraceOwnedContextError, TraceOwnedLocation,
    TraceOwnedOwnerRef, TraceOwnedPhase, TraceOwnedSnapshot, TraceOwnedWitness,
    TraceOwnedWitnessError, TraceScope, TraceSink, TraceTargetFieldBinding, TraceValue,
};
use ir::{Instance, SchemaNode, Value};
use mapping::{
    Binding, Graph, NamedSource, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project,
    Scope, ScopeIteration, SequenceWindow, SortFilterOrder, SourceOwner,
};
use std::cell::{Cell, RefCell};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Snapshot(Result<TraceOwnedSnapshot, TraceOwnedWitnessError>),
    Projection(TraceOwnedContextError),
}

#[derive(Debug)]
struct Observation {
    event: TraceEvent,
    outcome: Outcome,
}

type WitnessCheck<'a> = dyn Fn(&TraceEvent, &TraceOwnedWitness<'_>) + 'a;

struct Paired<'a> {
    events: RefCell<Vec<TraceEvent>>,
    observations: RefCell<Vec<Observation>>,
    root_addresses: RefCell<Vec<usize>>,
    selected: Cell<u8>,
    stop_after_first: bool,
    check: Option<&'a WitnessCheck<'a>>,
}

impl Paired<'_> {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            observations: RefCell::new(Vec::new()),
            root_addresses: RefCell::new(Vec::new()),
            selected: Cell::new(3),
            stop_after_first: false,
            check: None,
        }
    }
}

impl TraceSink for Paired<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }

    fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
        let selected = self.selected.get();
        match event {
            TraceEvent::TargetFieldWritten { .. } => selected & 1 != 0,
            TraceEvent::TargetProduced { .. } => selected & 2 != 0,
            _ => false,
        }
    }

    fn record_with_owned_context(
        &self,
        event: TraceEvent,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) {
        assert!(matches!(
            &event,
            TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
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
        self.observations.borrow_mut().push(Observation {
            event: event.clone(),
            outcome,
        });
        if self.stop_after_first {
            self.selected.set(0);
        }
        self.record(event);
    }
}

fn project(names: &[&str], nodes: Vec<(u32, Node)>, root: Scope) -> Project {
    let mut project = field_trace_project(
        SchemaNode::group("Output", Vec::new()),
        Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    );
    project.extra_sources = names
        .iter()
        .map(|name| NamedSource {
            name: (*name).into(),
            path: String::new(),
            schema: SchemaNode::group("Input", Vec::new()),
            options: Default::default(),
            dynamic_path: None,
        })
        .collect();
    project
}

fn constant_project(names: &[&str]) -> Project {
    project(
        names,
        vec![
            (
                0,
                Node::Const {
                    value: Value::Int(8),
                },
            ),
            (1, Node::Const { value: Value::Null }),
        ],
        Scope {
            bindings: vec![
                Binding {
                    target_field: "Result".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Optional".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    )
}

fn owned_field(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: frame.map(|path| path.iter().map(|part| (*part).into()).collect()),
    })
}

fn position(collection: &[&str], index: usize) -> PositionFrame {
    PositionFrame {
        binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
        generated_origin: None,
        collection: collection.iter().map(|part| (*part).into()).collect(),
        index,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    }
}

fn evaluate(
    project: &Project,
    owned: &OwnedContext<'_>,
    registry: &dyn crate::trace::TraceOwnedRegistryView,
    sink: Option<&dyn TraceSink>,
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let first_failure = Cell::new(false);
    let program = EvalProgram::new(
        &project.graph,
        &project.user_functions,
        sink,
        &first_failure,
    )
    .with_owned_context(owned)
    .with_owned_trace_registry(registry);
    crate::eval_scope::eval_scope(
        program,
        &project.root,
        None,
        &[owned.roots.extras_wrapper, owned.roots.primary],
        positions,
        &project.extra_sources,
        None,
        &TraceScope::primary(),
    )
}

fn output_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
            )
        })
        .cloned()
        .collect()
}

fn snapshot(observation: &Observation) -> &TraceOwnedSnapshot {
    let Outcome::Snapshot(Ok(snapshot)) = &observation.outcome else {
        panic!(
            "expected successful output witness: {:?}",
            observation.outcome
        );
    };
    snapshot
}

#[test]
fn event_selective_opt_in_preserves_legacy_callbacks_and_exact_output_events() {
    struct ExpressionOnly {
        events: RefCell<Vec<TraceEvent>>,
        contexts: Cell<usize>,
    }
    impl TraceSink for ExpressionOnly {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }
        fn wants_owned_context(&self) -> bool {
            true
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(
                &event,
                TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
            ));
            assert!(context.is_ok());
            self.contexts.set(self.contexts.get() + 1);
            self.record(event);
        }
    }
    struct DefaultOutput(RefCell<Vec<TraceEvent>>);
    impl TraceSink for DefaultOutput {
        fn record(&self, event: TraceEvent) {
            self.0.borrow_mut().push(event);
        }
        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            matches!(
                event,
                TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
            )
        }
    }
    let project = constant_project(&[]);
    let primary = Instance::Scalar(Value::Int(1));
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut owned = OwnedContext::new(&roots);
    owned.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let legacy = Collector::default();
    let expected = evaluate(&project, &owned, &registry, Some(&legacy), &[]).unwrap();
    assert!(registry.cache.get().is_none());
    let first_failure = Cell::new(false);
    let silent = EvalProgram::new(
        &project.graph,
        &project.user_functions,
        None,
        &first_failure,
    )
    .with_owned_context(&owned)
    .with_owned_trace_registry(&registry);
    record_owned_event(silent, &[], || {
        panic!("an absent sink must not construct the event")
    });
    let declining = Paired::new();
    declining.selected.set(0);
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&declining), &[]).unwrap(),
        expected
    );
    assert_eq!(*declining.events.borrow(), *legacy.0.borrow());
    assert!(declining.observations.borrow().is_empty());
    assert!(registry.cache.get().is_none());
    let old = ExpressionOnly {
        events: RefCell::new(Vec::new()),
        contexts: Cell::new(0),
    };
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&old), &[]).unwrap(),
        expected
    );
    assert_eq!(*old.events.borrow(), *legacy.0.borrow());
    assert_eq!(old.contexts.get(), 2);
    let default = DefaultOutput(RefCell::new(Vec::new()));
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&default), &[]).unwrap(),
        expected
    );
    assert_eq!(*default.0.borrow(), *legacy.0.borrow());
    for (selected, count) in [(1, 2), (2, 1)] {
        let selective = Paired::new();
        selective.selected.set(selected);
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&selective), &[]).unwrap(),
            expected
        );
        assert_eq!(*selective.events.borrow(), *legacy.0.borrow());
        assert_eq!(selective.observations.borrow().len(), count);
        assert!(selective.observations.borrow().iter().all(|observation| {
            match &observation.event {
                TraceEvent::TargetFieldWritten { .. } => selected == 1,
                TraceEvent::TargetProduced { .. } => selected == 2,
                _ => false,
            }
        }));
    }
    let mut changing = Paired::new();
    changing.stop_after_first = true;
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&changing), &[]).unwrap(),
        expected
    );
    assert_eq!(*changing.events.borrow(), *legacy.0.borrow());
    assert_eq!(changing.observations.borrow().len(), 1);
    assert!(matches!(&changing.observations.borrow()[0].event,
        TraceEvent::TargetFieldWritten { field, .. } if field == "Result"));
    let paired = Paired::new();
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap(),
        expected
    );
    assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
    assert_eq!(paired.observations.borrow().len(), 3);
    assert_eq!(
        output_events(&paired.events.borrow()),
        vec![
            TraceEvent::TargetFieldWritten {
                scope: TraceScope::primary(),
                field: "Result".into(),
                binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
                positions: Vec::new(),
                kind: TraceOutputKind::Scalar,
                value: Some(TraceValue::new(&Value::Int(8))),
            },
            TraceEvent::TargetFieldWritten {
                scope: TraceScope::primary(),
                field: "Optional".into(),
                binding: TraceTargetFieldBinding::StaticBinding { value: 1 },
                positions: Vec::new(),
                kind: TraceOutputKind::Scalar,
                value: Some(TraceValue::new(&Value::Null)),
            },
            TraceEvent::TargetProduced {
                scope: TraceScope::primary(),
                positions: Vec::new(),
                output_path: None,
                kind: TraceOutputKind::Group,
            },
        ]
    );
    let first_address = changing.root_addresses.borrow()[0];
    assert!(
        paired
            .root_addresses
            .borrow()
            .iter()
            .all(|address| *address == first_address)
    );
}

#[test]
fn actual_output_producers_borrow_original_roots_frames_origins_and_positions() {
    let names = [" Ref/Rows α ", " ref/Rows α "];
    let project = constant_project(&names);
    let primary = Instance::Scalar(Value::Int(1));
    let extras = Instance::Group(
        names
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    Instance::Group(
                        vec![(
                            "Rows".into(),
                            Instance::Repeated(vec![Instance::Group(
                                vec![(
                                    "Value".into(),
                                    Instance::Scalar(Value::String("same".into())),
                                )]
                                .into(),
                            )]),
                        )]
                        .into(),
                    ),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    );
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let first = roots.root(OwnerKey::Named(0)).unwrap();
    let second = roots.root(OwnerKey::Named(1)).unwrap();
    let first_row = &first.field("Rows").unwrap().as_repeated().unwrap()[0];
    let second_row = &second.field("Rows").unwrap().as_repeated().unwrap()[0];
    assert_eq!(first_row, second_row);
    assert!(!std::ptr::eq(first_row, second_row));
    let owned = OwnedContext {
        roots: &roots,
        frames: vec![
            SourceFrame {
                owner: OwnerKey::Named(0),
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: vec!["Rows".into()],
                item: first_row,
                position_slot: 0,
            },
            SourceFrame {
                owner: OwnerKey::Named(1),
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: vec!["Rows".into()],
                item: second_row,
                position_slot: 1,
            },
        ],
        origins: vec![SourceOrigin {
            kind: crate::owned_context::SourceOriginKind::Ordinary,
            owner: OwnerKey::Named(0),
            path: Vec::new(),
            item: first,
        }],
        owned_ancestry: true,
    };
    let positions = vec![
        position(&[names[0], "Rows"], 1),
        position(&[names[1], "Rows"], 1),
    ];
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let path_addresses = RefCell::new(Vec::new());
    let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        assert!(std::ptr::eq(view.project, &project));
        assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
        assert!(std::ptr::eq(view.roots[1].input.unwrap(), first));
        assert!(std::ptr::eq(view.roots[2].input.unwrap(), second));
        assert_eq!(view.phase, TraceOwnedPhase::Output);
        assert!(view.owned_ancestry);
        for (index, position) in positions.iter().enumerate() {
            let TraceOwnedOwnerRef::Named {
                declaration_index,
                name,
            } = view.frames[index].owner
            else {
                panic!("named frame identity lost");
            };
            assert_eq!(declaration_index, index);
            assert_eq!(name.as_ptr(), project.extra_sources[index].name.as_ptr());
            assert!(std::ptr::eq(
                view.frames[index].item,
                owned.frames[index].item
            ));
            assert_eq!(
                view.frames[index].collection,
                owned.frames[index].collection.as_slice()
            );
            assert_eq!(view.frames[index].position_slot, index);
            assert_eq!(
                view.positions[index].collection,
                position.collection.as_slice()
            );
            assert_eq!(view.positions[index].index, position.index);
        }
        assert!(std::ptr::eq(view.origins[0].item, first));
        assert_eq!(view.origins[0].path, owned.origins[0].path.as_slice());
        let raw = match event {
            TraceEvent::TargetFieldWritten { positions, .. }
            | TraceEvent::TargetProduced { positions, .. } => positions,
            _ => panic!("unexpected contextual event"),
        };
        assert_eq!(*raw, crate::trace::trace_positions(&positions));
        path_addresses.borrow_mut().push((
            view.frames[0].collection.as_ptr() as usize,
            view.origins[0].path.as_ptr() as usize,
            view.positions[0].collection.as_ptr() as usize,
        ));
    };
    let mut paired = Paired::new();
    paired.check = Some(&check);
    let legacy = Collector::default();
    let expected = evaluate(&project, &owned, &registry, Some(&legacy), &positions).unwrap();
    assert!(registry.cache.get().is_none());
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&paired), &positions).unwrap(),
        expected
    );
    assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
    assert_eq!(paired.observations.borrow().len(), 3);
    assert!(
        paired
            .observations
            .borrow()
            .iter()
            .all(|observation| snapshot(observation).frames.len() == 2)
    );
    let addresses = path_addresses.borrow();
    assert!(addresses.iter().all(|address| *address == addresses[0]));
    assert!(
        paired
            .root_addresses
            .borrow()
            .iter()
            .all(|address| *address == paired.root_addresses.borrow()[0])
    );

    // The shared factory borrows exactly the supplied metadata and phase;
    // the scope checks above exercise the real output producer call sites.
    let direct_check = |_: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        assert_eq!(view.phase, TraceOwnedPhase::Selection);
        assert_eq!(
            view.frames[0].collection.as_ptr(),
            owned.frames[0].collection.as_ptr()
        );
        assert_eq!(
            view.origins[0].path.as_ptr(),
            owned.origins[0].path.as_ptr()
        );
        assert_eq!(
            view.positions[0].collection.as_ptr(),
            positions[0].collection.as_ptr()
        );
    };
    let mut direct = Paired::new();
    direct.check = Some(&direct_check);
    let first_failure = Cell::new(false);
    let program = EvalProgram::new(
        &project.graph,
        &project.user_functions,
        Some(&direct),
        &first_failure,
    )
    .with_owned_context(&owned)
    .with_owned_trace_registry(&registry)
    .with_owned_trace_phase(TraceOwnedPhase::Selection);
    record_owned_event(program, &positions, || TraceEvent::TargetProduced {
        scope: TraceScope::primary(),
        positions: crate::trace::trace_positions(&positions),
        output_path: None,
        kind: TraceOutputKind::Group,
    });
    assert_eq!(direct.observations.borrow().len(), 1);
    assert_eq!(
        snapshot(&direct.observations.borrow()[0]).phase,
        TraceOwnedPhase::Selection
    );
}

#[test]
fn nested_child_and_parent_writes_keep_distinct_receiving_contexts() {
    let project = project(
        &["Ref"],
        vec![
            (
                0,
                owned_field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            (
                1,
                owned_field(
                    SourceOwner::Named { name: "Ref".into() },
                    &["Rows", "Value"],
                    Some(&["Rows"]),
                ),
            ),
        ],
        Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: vec!["Rows".into()],
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
                        path: vec!["Rows".into()],
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
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    let row = |value: &str| {
        Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            )]
            .into(),
        )
    };
    let primary =
        Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![row("parent")]))].into());
    let extras = Instance::Group(
        vec![(
            "Ref".into(),
            Instance::Group(
                vec![(
                    "Rows".into(),
                    Instance::Repeated(vec![row("one"), row("two")]),
                )]
                .into(),
            ),
        )]
        .into(),
    );
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let parent = &primary.field("Rows").unwrap().as_repeated().unwrap()[0];
    let named = roots.root(OwnerKey::Named(0)).unwrap();
    let child_rows = named.field("Rows").unwrap().as_repeated().unwrap();
    let parent_scope = TraceScope::primary();
    let child_scope = parent_scope.child("Child", 0);
    let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
        let scope = match event {
            TraceEvent::TargetFieldWritten { scope, .. }
            | TraceEvent::TargetProduced { scope, .. } => scope,
            _ => panic!("unexpected event"),
        };
        assert_eq!(view.phase, TraceOwnedPhase::Output);
        assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
        assert!(std::ptr::eq(view.roots[1].input.unwrap(), named));
        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
        assert!(std::ptr::eq(view.frames[0].item, parent));
        assert_eq!(view.origins[0].owner, TraceOwnedOwnerRef::Primary);
        assert!(std::ptr::eq(view.origins[0].item, parent));
        assert_eq!(view.positions[view.frames[0].position_slot].index, 1);
        if scope == &child_scope {
            assert_eq!(view.frames.len(), 2);
            let TraceOwnedOwnerRef::Named {
                declaration_index: 0,
                name,
            } = view.frames[1].owner
            else {
                panic!("child owner was not retained");
            };
            assert_eq!(name.as_ptr(), project.extra_sources[0].name.as_ptr());
            let index = view.positions[view.frames[1].position_slot].index;
            assert!(std::ptr::eq(view.frames[1].item, &child_rows[index - 1]));
            assert_eq!(view.frames[0].collection, view.frames[1].collection);
            assert_eq!(view.origins.len(), 2);
            assert_eq!(view.origins[1].owner, view.frames[1].owner);
            assert!(std::ptr::eq(view.origins[1].item, &child_rows[index - 1]));
        } else {
            assert_eq!(scope, &parent_scope);
            assert_eq!(view.frames.len(), 1);
            assert_eq!(view.origins.len(), 1);
            assert_eq!(view.positions.len(), 1);
        }
    };
    let mut paired = Paired::new();
    paired.check = Some(&check);
    let legacy = Collector::default();
    let expected = evaluate(&project, &owned, &registry, Some(&legacy), &[]).unwrap();
    let actual = evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
    let parent_output = &actual.as_repeated().unwrap()[0];
    assert_eq!(
        parent_output.field("Parent").unwrap().as_scalar(),
        Some(&Value::String("parent".into()))
    );
    let children = parent_output.field("Child").unwrap().as_repeated().unwrap();
    assert_eq!(children.len(), 2);
    assert!(
        children
            .iter()
            .all(|child| child.field("Outer").unwrap().as_scalar()
                == Some(&Value::String("parent".into())))
    );
    let observations = paired.observations.borrow();
    assert_eq!(observations.len(), 9);
    assert!(
        observations
            .iter()
            .all(|observation| snapshot(observation).owned_ancestry)
    );
    let parent_write = observations.iter().find(|observation| matches!(&observation.event,
        TraceEvent::TargetFieldWritten { scope, binding: TraceTargetFieldBinding::StaticChild, field, .. }
            if scope == &parent_scope && field == "Child")).unwrap();
    assert_eq!(snapshot(parent_write).frames.len(), 1);
    let child_positions = observations
        .iter()
        .filter_map(|observation| match &observation.event {
            TraceEvent::TargetProduced { scope, .. } if scope == &child_scope => {
                Some(snapshot(observation).frames[1].index)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(child_positions, vec![1, 2]);
}

#[test]
fn actual_output_phase_and_positions_survive_sorting_and_compaction() {
    let primary = Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                [(3, true), (1, false), (2, true)]
                    .into_iter()
                    .map(|(rank, keep)| {
                        Instance::Group(
                            vec![
                                ("Rank".into(), Instance::Scalar(Value::Int(rank))),
                                ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    );
    for order in [
        SortFilterOrder::FilterThenSort,
        SortFilterOrder::SortThenFilter,
    ] {
        let project = project(
            &[],
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    1,
                    owned_field(SourceOwner::Primary, &["Rows", "Rank"], Some(&["Rows"])),
                ),
                (
                    2,
                    owned_field(SourceOwner::Primary, &["Rows", "Keep"], Some(&["Rows"])),
                ),
                (
                    3,
                    Node::OwnedPosition(OwnedPosition {
                        owner: SourceOwner::Primary,
                        collection: Some(vec!["Rows".into()]),
                    }),
                ),
            ],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: vec!["Rows".into()],
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
                children: vec![Scope {
                    target_field: "Echo".into(),
                    windows: vec![SequenceWindow::First { count: 0 }],
                    bindings: vec![Binding {
                        target_field: "Position".into(),
                        node: 3,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        );
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let legacy = Collector::default();
        let paired = Paired::new();
        let expected = evaluate(&project, &owned, &registry, Some(&legacy), &[]).unwrap();
        let actual = evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
        let items = actual.as_repeated().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].field("Rank").unwrap().as_scalar(),
            Some(&Value::Int(3))
        );
        assert_eq!(
            items[0].field("Position").unwrap().as_scalar(),
            Some(&Value::Int(1))
        );
        assert_eq!(
            items[0]
                .field("Echo")
                .unwrap()
                .field("Position")
                .unwrap()
                .as_scalar(),
            Some(&Value::Int(1))
        );
        let observations = paired.observations.borrow();
        assert_eq!(observations.len(), 6);
        for observation in observations.iter() {
            let snapshot = snapshot(observation);
            assert_eq!(snapshot.phase, TraceOwnedPhase::Output);
            assert_eq!(snapshot.frames.len(), 1);
            assert_eq!(snapshot.frames[0].index, 1);
            assert_eq!(
                snapshot.positions[snapshot.frames[0].position_slot].index,
                1
            );
            let positions = match &observation.event {
                TraceEvent::TargetFieldWritten { positions, .. }
                | TraceEvent::TargetProduced { positions, .. } => positions,
                _ => panic!("unexpected contextual event"),
            };
            assert_eq!(positions.len(), snapshot.positions.len());
            for (raw, copied) in positions.iter().zip(&snapshot.positions) {
                assert_eq!(raw.collection, copied.collection.segments);
                assert_eq!(raw.index, copied.index);
            }
        }
    }
}

#[test]
fn failed_output_preserves_prefix_events_and_exact_engine_error() {
    let project = project(
        &[],
        vec![
            (
                0,
                Node::Const {
                    value: Value::Int(8),
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
            ..Scope::default()
        },
    );
    let primary = Instance::Scalar(Value::Int(1));
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut owned = OwnedContext::new(&roots);
    owned.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let legacy = Collector::default();
    let paired = Paired::new();
    let expected = Err(EngineError::MappingException {
        node: 1,
        message: None,
    });
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&legacy), &[]),
        expected
    );
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&paired), &[]),
        expected
    );
    assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
    assert_eq!(
        output_events(&paired.events.borrow()),
        vec![TraceEvent::TargetFieldWritten {
            scope: TraceScope::primary(),
            field: "Good".into(),
            binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
            positions: Vec::new(),
            kind: TraceOutputKind::Scalar,
            value: Some(TraceValue::new(&Value::Int(8))),
        }]
    );
    assert_eq!(paired.observations.borrow().len(), 1);
    assert_eq!(
        snapshot(&paired.observations.borrow()[0]).phase,
        TraceOwnedPhase::Output
    );
}

#[test]
fn output_projection_and_snapshot_errors_are_observer_metadata() {
    let project = constant_project(&["A"]);
    let other = constant_project(&["A"]);
    let primary = Instance::Scalar(Value::Int(1));
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut owned = OwnedContext::new(&roots);
    owned.owned_ancestry = true;
    let wrong = TraceOwnedRegistry::new(&other, &roots);
    let paired = Paired::new();
    let expected = evaluate(&project, &owned, &wrong, None, &[]).unwrap();
    assert!(wrong.cache.get().is_none());
    assert_eq!(
        evaluate(&project, &owned, &wrong, Some(&paired), &[]).unwrap(),
        expected
    );
    assert_eq!(paired.observations.borrow().len(), 3);
    assert!(
        paired
            .observations
            .borrow()
            .iter()
            .all(|observation| observation.outcome
                == Outcome::Projection(TraceOwnedContextError::MismatchedRegistry))
    );
    let malformed = OwnedContext {
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
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let paired = Paired::new();
    let expected = evaluate(&project, &malformed, &registry, None, &[]).unwrap();
    assert_eq!(
        evaluate(&project, &malformed, &registry, Some(&paired), &[]).unwrap(),
        expected
    );
    assert_eq!(paired.observations.borrow().len(), 3);
    assert!(
        paired
            .observations
            .borrow()
            .iter()
            .all(|observation| observation.outcome
                == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
                    at: TraceOwnedLocation::Frame(0),
                    slot: 7,
                })))
    );
    let project = constant_project(&["Null", "Missing"]);
    let extras = Instance::Group(vec![("Null".into(), Instance::Scalar(Value::Null))].into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut owned = OwnedContext::new(&roots);
    owned.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let paired = Paired::new();
    let expected = evaluate(&project, &owned, &registry, None, &[]).unwrap();
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap(),
        expected
    );
    assert_eq!(paired.observations.borrow().len(), 3);
    for observation in paired.observations.borrow().iter() {
        let snapshot = snapshot(observation);
        assert_eq!(
            snapshot.roots[1]
                .input
                .as_ref()
                .unwrap()
                .scalar
                .as_ref()
                .unwrap()
                .value_type,
            "null"
        );
        assert!(snapshot.roots[2].input.is_none());
    }
    let project = constant_project(&["Unused"]);
    let extras = Instance::Group(
        vec![
            ("Unused".into(), Instance::Scalar(Value::Int(1))),
            ("Unused".into(), Instance::Scalar(Value::Int(2))),
        ]
        .into(),
    );
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let mut owned = OwnedContext::new(&roots);
    owned.owned_ancestry = true;
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let paired = Paired::new();
    let expected = evaluate(&project, &owned, &registry, None, &[]).unwrap();
    assert_eq!(
        evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap(),
        expected
    );
    assert_eq!(paired.observations.borrow().len(), 3);
    assert!(
        paired
            .observations
            .borrow()
            .iter()
            .all(|observation| observation.outcome
                == Outcome::Projection(TraceOwnedContextError::Registry {
                    declaration_index: 0,
                    source: crate::OwnedSourceError::AmbiguousOwner {
                        name: "Unused".into()
                    },
                }))
    );
}

#[test]
fn public_output_witnesses_and_expression_only_opt_in_stay_distinct() {
    let project = project(
        &[],
        vec![(0, owned_field(SourceOwner::Primary, &["Value"], None))],
        Scope {
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    );
    let primary = Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("hello".into())),
        )]
        .into(),
    );
    let paired = Paired::new();
    let execution =
        crate::ExecutionContext::new(std::path::Path::new("project.json")).with_trace_sink(&paired);
    assert_eq!(
        crate::run_with_context(&project, &primary, &execution),
        Ok(Instance::Group(
            vec![(
                "Result".into(),
                Instance::Scalar(Value::String("hello".into()))
            )]
            .into()
        ))
    );
    assert_eq!(paired.observations.borrow().len(), 2);
    assert!(
        paired
            .observations
            .borrow()
            .iter()
            .all(|observation| matches!(
                &observation.outcome,
                Outcome::Snapshot(Ok(snapshot)) if snapshot.phase == TraceOwnedPhase::Output
            ))
    );
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
            crate::check_source_ownership_capability_with_observers(&project, None, observers),
            Ok(())
        );
    }
    let plain = Collector::default();
    let event = TraceEvent::TargetProduced {
        scope: TraceScope::primary(),
        positions: Vec::new(),
        output_path: None,
        kind: TraceOutputKind::Group,
    };
    assert!(!plain.wants_owned_context_for(&event));
    struct OldOptIn;
    impl TraceSink for OldOptIn {
        fn record(&self, _: TraceEvent) {}
        fn wants_owned_context(&self) -> bool {
            true
        }
    }
    assert!(!OldOptIn.wants_owned_context_for(&event));
    assert!(OldOptIn.wants_owned_context_for(&TraceEvent::NodeValue {
        node: 0,
        positions: Vec::new(),
        value: TraceValue::new(&Value::Int(8)),
    }));
    assert!(
        crate::check_source_ownership_capability_with_observers(
            &project,
            None,
            crate::SourceOwnershipObservers::default(),
        )
        .is_ok()
    );
}
