use super::{Collector, field_trace_project};
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::owned_context::{
    OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
};
use crate::source_iteration::PositionFrame;
use crate::trace::TraceOwnedRegistry;
use crate::{
    EngineError, TraceEvent, TraceOwnedContextError, TraceOwnedLocation, TraceOwnedPhase,
    TraceOwnedSnapshot, TraceOwnedWitness, TraceOwnedWitnessError, TraceSink,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, NamedSource, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project,
    Scope, ScopeIteration, SequenceWindow, SortFilterOrder, SourceOwner,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Snapshot(Result<TraceOwnedSnapshot, TraceOwnedWitnessError>),
    Projection(TraceOwnedContextError),
}

#[derive(Debug)]
struct Observation {
    node: u32,
    input: Option<(u32, usize)>,
    outcome: Outcome,
}

type WitnessCheck<'a> = dyn Fn(&TraceOwnedWitness<'_>) + 'a;

struct Paired<'a> {
    events: RefCell<Vec<TraceEvent>>,
    observations: RefCell<Vec<Observation>>,
    root_addresses: RefCell<Vec<usize>>,
    enabled: Cell<bool>,
    check: Option<&'a WitnessCheck<'a>>,
}

impl Paired<'_> {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            observations: RefCell::new(Vec::new()),
            root_addresses: RefCell::new(Vec::new()),
            enabled: Cell::new(true),
            check: None,
        }
    }
}

impl TraceSink for Paired<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
    fn wants_owned_context(&self) -> bool {
        self.enabled.get()
    }
    fn record_with_owned_context(
        &self,
        event: TraceEvent,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) {
        let outcome = match context {
            Ok(view) => {
                if let Some(check) = self.check {
                    check(&view);
                }
                self.root_addresses
                    .borrow_mut()
                    .push(view.roots.as_ptr() as usize);
                Outcome::Snapshot(view.snapshot())
            }
            Err(error) => Outcome::Projection(error.clone()),
        };
        let (node, input) = match &event {
            TraceEvent::NodeValue { node, .. } => (*node, None),
            TraceEvent::NodeInputValue {
                consumer,
                input,
                input_index,
                ..
            } => (*consumer, Some((*input, *input_index))),
            _ => panic!("only expression value/input events receive owned context"),
        };
        self.observations.borrow_mut().push(Observation {
            node,
            input,
            outcome,
        });
        self.record(event);
    }
}

fn trace_project(names: &[&str], nodes: Vec<(u32, Node)>) -> Project {
    let mut project = field_trace_project(
        SchemaNode::group("Output", Vec::new()),
        Graph {
            nodes: nodes.into_iter().collect(),
        },
        Scope::default(),
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

fn owned_field(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|name| (*name).into()).collect(),
        },
        frame: frame.map(|path| path.iter().map(|name| (*name).into()).collect()),
    })
}

fn evaluate(
    project: &Project,
    owned: &OwnedContext<'_>,
    registry: &dyn crate::trace::TraceOwnedRegistryView,
    sink: Option<&dyn TraceSink>,
    node: u32,
    positions: &[PositionFrame],
    phase: TraceOwnedPhase,
) -> Result<Value, EngineError> {
    let first_failure = Cell::new(false);
    let program = EvalProgram::new(
        &project.graph,
        &project.user_functions,
        sink,
        &first_failure,
    )
    .with_owned_context(owned)
    .with_owned_trace_registry(registry)
    .with_owned_trace_phase(phase);
    eval_expr(
        program,
        node,
        &[owned.roots.extras_wrapper, owned.roots.primary],
        positions,
        &mut HashSet::new(),
    )
}

#[test]
fn real_expression_producer_borrows_original_registry_frames_origins_and_positions() {
    let names = [" Ref/Rows α ", " ref/Rows α "];
    let project = trace_project(
        &names,
        vec![
            (
                0,
                owned_field(
                    SourceOwner::Named {
                        name: names[0].into(),
                    },
                    &["Rows", "Value"],
                    Some(&["Rows"]),
                ),
            ),
            (
                1,
                owned_field(
                    SourceOwner::Named {
                        name: names[1].into(),
                    },
                    &["Rows", "Value"],
                    Some(&["Rows"]),
                ),
            ),
            (
                2,
                Node::Call {
                    function: "concat".into(),
                    args: vec![0, 1],
                },
            ),
        ],
    );
    let primary = Instance::Scalar(Value::Int(9));
    let row = || {
        Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String("same".into())),
            )]
            .into(),
        )
    };
    let extras = Instance::Group(
        names
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![row()]))].into()),
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
    let positions = names
        .iter()
        .enumerate()
        .map(|(index, name)| PositionFrame {
            binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
            generated_origin: None,
            collection: vec![(*name).into(), "Rows".into()],
            index: [9, 2][index],
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        })
        .collect::<Vec<_>>();
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let check = |view: &TraceOwnedWitness<'_>| {
        assert!(std::ptr::eq(view.project, &project));
        assert!(std::ptr::eq(view.roots[0].input.unwrap(), roots.primary));
        assert!(std::ptr::eq(view.roots[1].input.unwrap(), first));
        assert!(std::ptr::eq(view.roots[2].input.unwrap(), second));
        for (index, position) in positions.iter().enumerate() {
            let crate::TraceOwnedOwnerRef::Named {
                declaration_index,
                name,
            } = view.roots[index + 1].owner
            else {
                panic!("named registry identity lost");
            };
            assert_eq!(declaration_index, index);
            assert_eq!(name.as_ptr(), project.extra_sources[index].name.as_ptr());
            assert_eq!(view.frames[index].owner, view.roots[index + 1].owner);
            assert!(std::ptr::eq(
                view.frames[index].item,
                owned.frames[index].item
            ));
            assert_eq!(
                view.frames[index].collection.as_ptr(),
                owned.frames[index].collection.as_ptr()
            );
            assert_eq!(
                view.positions[index].collection.as_ptr(),
                position.collection.as_ptr()
            );
            assert_eq!(view.positions[index].index, position.index);
        }
        assert!(std::ptr::eq(view.origins[0].item, owned.origins[0].item));
        assert_eq!(
            view.origins[0].path.as_ptr(),
            owned.origins[0].path.as_ptr()
        );
    };
    let mut paired = Paired::new();
    paired.check = Some(&check);
    let baseline = evaluate(
        &project,
        &owned,
        &registry,
        None,
        2,
        &positions,
        TraceOwnedPhase::Output,
    )
    .unwrap();
    assert_eq!(baseline, Value::String("samesame".into()));
    assert!(registry.cache.get().is_none());
    assert_eq!(
        evaluate(
            &project,
            &owned,
            &registry,
            Some(&paired),
            2,
            &positions,
            TraceOwnedPhase::Output
        )
        .unwrap(),
        baseline
    );
    let observations = paired.observations.borrow();
    assert_eq!(observations.len(), 5);
    assert_eq!(
        observations
            .iter()
            .map(|o| (o.node, o.input))
            .collect::<Vec<_>>(),
        vec![
            (0, None),
            (2, Some((0, 0))),
            (1, None),
            (2, Some((1, 1))),
            (2, None)
        ]
    );
    assert!(observations.iter().all(|o| matches!(&o.outcome, Outcome::Snapshot(Ok(view)) if view.phase == TraceOwnedPhase::Output)));
    let addresses = paired.root_addresses.borrow();
    assert!(addresses.iter().all(|address| *address == addresses[0]));
}

#[test]
fn default_opt_in_dispatch_preserves_exact_events_and_once_delivery() {
    struct DefaultPair(RefCell<Vec<TraceEvent>>);
    impl TraceSink for DefaultPair {
        fn record(&self, event: TraceEvent) {
            self.0.borrow_mut().push(event);
        }
        fn wants_owned_context(&self) -> bool {
            true
        }
    }
    let project = trace_project(
        &[],
        vec![
            (
                0,
                Node::Const {
                    value: Value::String("a".into()),
                },
            ),
            (
                1,
                Node::Call {
                    function: "concat".into(),
                    args: vec![0, 0],
                },
            ),
        ],
    );
    let primary = Instance::Scalar(Value::Int(8));
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let legacy = Collector::default();
    let contextual = DefaultPair(RefCell::new(Vec::new()));
    let before = evaluate(
        &project,
        &owned,
        &registry,
        Some(&legacy),
        1,
        &[],
        TraceOwnedPhase::Structural,
    )
    .unwrap();
    assert!(registry.cache.get().is_none());
    assert_eq!(
        evaluate(
            &project,
            &owned,
            &registry,
            Some(&contextual),
            1,
            &[],
            TraceOwnedPhase::Structural
        )
        .unwrap(),
        before
    );
    assert_eq!(*legacy.0.borrow(), *contextual.0.borrow());
    assert_eq!(contextual.0.borrow().len(), 5);
}

#[test]
fn changing_observer_requests_builds_once_and_reuses_the_root_projection() {
    let project = trace_project(
        &[],
        vec![(
            0,
            Node::Const {
                value: Value::Int(1),
            },
        )],
    );
    let primary = Instance::Scalar(Value::Int(8));
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let paired = Paired::new();
    for enabled in [false, true, false, true] {
        paired.enabled.set(enabled);
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &registry,
                Some(&paired),
                0,
                &[],
                TraceOwnedPhase::Structural
            )
            .unwrap(),
            Value::Int(1)
        );
        if paired.events.borrow().len() == 1 {
            assert!(registry.cache.get().is_none());
        }
    }
    assert_eq!(paired.events.borrow().len(), 4);
    assert_eq!(paired.observations.borrow().len(), 2);
    let addresses = paired.root_addresses.borrow();
    assert_eq!(addresses[0], addresses[1]);
}

#[test]
fn unavailable_null_and_ambiguous_unused_inputs_stay_observer_metadata() {
    let project = trace_project(
        &["Null", "Missing"],
        vec![(
            0,
            Node::Const {
                value: Value::Int(8),
            },
        )],
    );
    let primary = Instance::Scalar(Value::Int(0));
    let extras = Instance::Group(vec![("Null".into(), Instance::Scalar(Value::Null))].into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let paired = Paired::new();
    assert_eq!(
        evaluate(
            &project,
            &owned,
            &registry,
            Some(&paired),
            0,
            &[],
            TraceOwnedPhase::Structural
        )
        .unwrap(),
        Value::Int(8)
    );
    let observations = paired.observations.borrow();
    let Outcome::Snapshot(Ok(snapshot)) = &observations[0].outcome else {
        panic!("expected snapshot");
    };
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
    assert!(snapshot.frames.is_empty());
    drop(observations);
    let ambiguous = trace_project(
        &["Unused"],
        vec![(
            0,
            Node::Const {
                value: Value::Int(8),
            },
        )],
    );
    let extras = Instance::Group(
        vec![
            ("Unused".into(), Instance::Scalar(Value::Int(1))),
            ("Unused".into(), Instance::Scalar(Value::Int(2))),
        ]
        .into(),
    );
    let roots = SourceRoots::new(&ambiguous, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&ambiguous, &roots);
    let paired = Paired::new();
    let baseline = evaluate(
        &ambiguous,
        &owned,
        &registry,
        None,
        0,
        &[],
        TraceOwnedPhase::Structural,
    );
    assert_eq!(
        evaluate(
            &ambiguous,
            &owned,
            &registry,
            Some(&paired),
            0,
            &[],
            TraceOwnedPhase::Structural
        ),
        baseline
    );
    assert_eq!(
        paired.observations.borrow()[0].outcome,
        Outcome::Projection(TraceOwnedContextError::Registry {
            declaration_index: 0,
            source: crate::OwnedSourceError::AmbiguousOwner {
                name: "Unused".into()
            }
        })
    );
}

#[test]
fn projection_and_snapshot_failures_never_replace_expression_results() {
    let project = trace_project(
        &["A"],
        vec![(
            0,
            Node::Const {
                value: Value::Int(8),
            },
        )],
    );
    let other_project = trace_project(&["A"], vec![]);
    let primary = Instance::Scalar(Value::Int(1));
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let wrong = TraceOwnedRegistry::new(&other_project, &roots);
    let paired = Paired::new();
    assert_eq!(
        evaluate(
            &project,
            &owned,
            &wrong,
            Some(&paired),
            0,
            &[],
            TraceOwnedPhase::Structural
        )
        .unwrap(),
        Value::Int(8)
    );
    assert_eq!(
        paired.observations.borrow()[0].outcome,
        Outcome::Projection(TraceOwnedContextError::MismatchedRegistry)
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
    assert_eq!(
        evaluate(
            &project,
            &malformed,
            &registry,
            Some(&paired),
            0,
            &[],
            TraceOwnedPhase::Structural
        )
        .unwrap(),
        Value::Int(8)
    );
    assert_eq!(
        paired.observations.borrow()[0].outcome,
        Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
            at: TraceOwnedLocation::Frame(0),
            slot: 7
        }))
    );
}

#[test]
fn actual_scope_phases_preserve_sorted_selection_compaction_and_child_reentry() {
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
        let mut project = trace_project(
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
        );
        project.root = Scope {
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
        };
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        let legacy = Collector::default();
        let first = Cell::new(false);
        let base = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            Some(&legacy),
            &first,
        )
        .with_owned_context(&owned)
        .with_owned_trace_registry(&registry);
        let context = [&extras, &primary];
        let before = crate::eval_scope::eval_scope(
            base,
            &project.root,
            None,
            &context,
            &[],
            &project.extra_sources,
            None,
            &crate::TraceScope::primary(),
        )
        .unwrap();
        let program = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            Some(&paired),
            &first,
        )
        .with_owned_context(&owned)
        .with_owned_trace_registry(&registry);
        let after = crate::eval_scope::eval_scope(
            program,
            &project.root,
            None,
            &context,
            &[],
            &project.extra_sources,
            None,
            &crate::TraceScope::primary(),
        )
        .unwrap();
        assert_eq!(after, before);
        assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
        let items = after.as_repeated().unwrap();
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
        let snapshots = observations
            .iter()
            .filter_map(|o| match &o.outcome {
                Outcome::Snapshot(Ok(snapshot)) => Some((o.node, snapshot)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), observations.len());
        assert!(snapshots.iter().any(
            |(node, snapshot)| *node == 1 && snapshot.phase == TraceOwnedPhase::Structural
        ));
        assert!(snapshots.iter().any(|(node, snapshot)| *node == 3
            && snapshot.phase == TraceOwnedPhase::Output
            && snapshot.frames[0].index == 1));
        assert!(snapshots.iter().any(|(node, snapshot)| *node == 0
            && snapshot.phase == TraceOwnedPhase::Structural
            && snapshot.frames.len() == 1
            && snapshot.frames[0].index == 1));
        for (node, snapshot) in &snapshots {
            if *node == 2 {
                assert_eq!(
                    snapshot.phase,
                    if order == SortFilterOrder::FilterThenSort {
                        TraceOwnedPhase::Structural
                    } else {
                        TraceOwnedPhase::Selection
                    }
                );
            }
        }
    }
}

#[test]
fn lazy_and_failed_inputs_preserve_full_errors_and_do_not_fabricate_values() {
    let project = trace_project(
        &[],
        vec![
            (
                0,
                Node::Const {
                    value: Value::Bool(false),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("ok".into()),
                },
            ),
            (2, Node::Raise { message: None }),
            (
                3,
                Node::If {
                    condition: 0,
                    then: 2,
                    else_: 1,
                },
            ),
            (
                4,
                Node::Call {
                    function: "concat".into(),
                    args: vec![1, 2],
                },
            ),
        ],
    );
    let primary = Instance::Scalar(Value::Int(1));
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let paired = Paired::new();
    assert_eq!(
        evaluate(
            &project,
            &owned,
            &registry,
            Some(&paired),
            3,
            &[],
            TraceOwnedPhase::Structural
        )
        .unwrap(),
        Value::String("ok".into())
    );
    assert!(
        !paired
            .observations
            .borrow()
            .iter()
            .any(|o| o.node == 2 || o.input.is_some_and(|(input, _)| input == 2))
    );
    paired.events.borrow_mut().clear();
    paired.observations.borrow_mut().clear();
    let expected = evaluate(
        &project,
        &owned,
        &registry,
        None,
        4,
        &[],
        TraceOwnedPhase::Output,
    );
    assert!(expected.is_err());
    assert_eq!(
        evaluate(
            &project,
            &owned,
            &registry,
            Some(&paired),
            4,
            &[],
            TraceOwnedPhase::Output
        ),
        expected
    );
    assert_eq!(
        paired
            .observations
            .borrow()
            .iter()
            .map(|o| (o.node, o.input))
            .collect::<Vec<_>>(),
        vec![(1, None), (4, Some((1, 0)))]
    );
}

#[test]
fn function_body_events_keep_qualified_identity_without_caller_witnesses() {
    let function = mapping::FunctionId::new(9);
    let parameter = mapping::FunctionParameterId::new(8);
    let mut project = trace_project(
        &[],
        vec![
            (0, owned_field(SourceOwner::Primary, &["Value"], None)),
            (
                1,
                Node::UserFunctionCall {
                    function,
                    args: vec![0],
                },
            ),
        ],
    );
    project.user_functions.insert(
        function,
        mapping::UserFunction {
            library: "Local".into(),
            name: "Echo".into(),
            description: None,
            parameters: vec![mapping::FunctionParameter {
                id: parameter,
                name: "Input".into(),
                ty: ScalarType::String,
            }],
            output_name: "Output".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(0, Node::FunctionParameter { parameter })]
                    .into_iter()
                    .collect(),
            },
            output: 0,
        },
    );
    let primary = Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("hello".into())),
        )]
        .into(),
    );
    let extras = Instance::Group(Vec::new().into());
    let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let owned = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(&project, &roots);
    let paired = Paired::new();
    assert_eq!(
        evaluate(
            &project,
            &owned,
            &registry,
            Some(&paired),
            1,
            &[],
            TraceOwnedPhase::Output
        )
        .unwrap(),
        Value::String("hello".into())
    );
    assert_eq!(paired.observations.borrow().len(), 3);
    assert!(paired.events.borrow().iter().any(|event| matches!(event, TraceEvent::FunctionNodeValue {
        function: observed, node: 0, positions, .. } if *observed == function && positions.is_empty())));
}

#[test]
fn public_basic_trace_runs_with_expression_witnesses() {
    let mut project = trace_project(
        &[],
        vec![(0, owned_field(SourceOwner::Primary, &["Value"], None))],
    );
    project.root.bindings.push(Binding {
        target_field: "Result".into(),
        node: 0,
    });
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
    assert!(!paired.events.borrow().is_empty());
    assert_eq!(paired.observations.borrow().len(), 1);
    assert!(matches!(
        &paired.observations.borrow()[0].outcome,
        Outcome::Snapshot(Ok(snapshot)) if snapshot.phase == TraceOwnedPhase::Output
    ));
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
}
