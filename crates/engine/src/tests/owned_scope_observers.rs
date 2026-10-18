//! Real scope/failure producers supply every phase and occurrence in these tests.
//! The crate-private seam matches the existing owned trace tests; public admission stays closed.

use std::cell::{Cell, RefCell};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, Graph, NamedSource, Node,
    OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration, SortFilterOrder, SourceOwner,
};

use super::{Collector, field_trace_project};
use crate::eval_expr::EvalProgram;
use crate::owned_context::{OwnedContext, OwnedLimits, SourceRoots};
use crate::trace::TraceOwnedRegistry;
use crate::{
    DebugDecision, DebugHook, EngineError, PendingNodeFailure, TraceEvent, TraceFilterPhase,
    TraceOwnedContextError, TraceOwnedDomain, TraceOwnedOwnerRef, TraceOwnedPhase,
    TraceOwnedWitness, TraceScope, TraceSink,
};

const KEEP: u32 = 1;
const RANK: u32 = 2;
const FAIL: u32 = 3;
const LABEL: u32 = 4;
const ERROR: u32 = 5;
const PREDICATE: u32 = 6;
const TRUE: u32 = 7;
const BINDING: u32 = 8;
const FALSE: u32 = 9;
const NAMED_VALUE: u32 = 10;
const MESSAGE: u32 = 11;

type WitnessCheck<'a> = dyn Fn(&PendingNodeFailure, &TraceOwnedWitness<'_>) + 'a;

struct Observer<'a> {
    decision: DebugDecision,
    expected: &'a EngineError,
    check: &'a WitnessCheck<'a>,
    calls: Cell<usize>,
    failed_node: Cell<Option<u32>>,
    events_at_failure: Cell<usize>,
    events: RefCell<Vec<TraceEvent>>,
}

impl<'a> Observer<'a> {
    fn new(
        decision: DebugDecision,
        expected: &'a EngineError,
        check: &'a WitnessCheck<'a>,
    ) -> Self {
        Self {
            decision,
            expected,
            check,
            calls: Cell::new(0),
            failed_node: Cell::new(None),
            events_at_failure: Cell::new(0),
            events: RefCell::new(Vec::new()),
        }
    }
}

impl TraceSink for Observer<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
}

impl DebugHook for Observer<'_> {
    fn before_target_write(&self, _: &crate::PendingTargetWrite) -> DebugDecision {
        DebugDecision::Resume
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

    fn wants_owned_context_for_node_failure(&self, _: &PendingNodeFailure) -> bool {
        true
    }

    fn after_node_failure(&self, _: &PendingNodeFailure) -> DebugDecision {
        panic!("the selected failure must use the additive owned-context callback")
    }

    fn after_node_failure_with_owned_context(
        &self,
        pending: &PendingNodeFailure,
        error: &EngineError,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) -> DebugDecision {
        assert_eq!(self.calls.replace(self.calls.get() + 1), 0);
        assert_eq!(error, self.expected);
        self.failed_node.set(Some(pending.node));
        self.events_at_failure.set(self.events.borrow().len());
        let view = context.expect("authentic producer metadata must project");
        view.snapshot()
            .expect("authentic producer metadata must validate");
        (self.check)(pending, &view);
        self.decision
    }
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn named_owner() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}

fn named_witness_owner() -> TraceOwnedOwnerRef<'static> {
    TraceOwnedOwnerRef::Named {
        declaration_index: 0,
        name: "Ref",
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn rows(label: &str) -> Instance {
    group(vec![
        (
            "Note",
            Instance::Scalar(Value::String(format!("{label}.root"))),
        ),
        (
            "Rows",
            Instance::Repeated(
                [(20, false, false), (10, true, true)]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (rank, keep, fail))| {
                        group(vec![
                            (
                                "Label",
                                Instance::Scalar(Value::String(format!(
                                    "{label}.raw{}",
                                    index + 1
                                ))),
                            ),
                            ("Rank", Instance::Scalar(Value::Int(rank))),
                            ("Keep", Instance::Scalar(Value::Bool(keep))),
                            ("Fail", Instance::Scalar(Value::Bool(fail))),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn inputs() -> (Instance, Instance) {
    let mut primary = rows("P");
    let Instance::Group(fields) = &mut primary else {
        unreachable!()
    };
    // A same-spelled primary field is not the declared named source.
    fields.push(("Ref".into(), rows("shadow")));
    (primary, group(vec![("Ref", rows("N"))]))
}

fn second_row(root: &Instance) -> &Instance {
    &root.field("Rows").unwrap().as_repeated().unwrap()[1]
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Fail", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    )
}

fn field(owner: SourceOwner, name: &str) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(&["Rows", name]),
        },
        frame: Some(path(&["Rows"])),
    })
}

fn iteration(owner: SourceOwner) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(&["Rows"]),
        },
        output_path: None,
    }
}

fn project(owner: SourceOwner) -> Project {
    let mut p = field_trace_project(
        SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
        .repeating(),
        Graph {
            nodes: [
                (KEEP, field(owner.clone(), "Keep")),
                (RANK, field(owner.clone(), "Rank")),
                (FAIL, field(owner.clone(), "Fail")),
                (LABEL, field(owner.clone(), "Label")),
                (
                    ERROR,
                    Node::Raise {
                        message: Some(LABEL),
                    },
                ),
                (
                    PREDICATE,
                    Node::If {
                        condition: FAIL,
                        then: ERROR,
                        else_: TRUE,
                    },
                ),
                (
                    TRUE,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    BINDING,
                    Node::If {
                        condition: FAIL,
                        then: ERROR,
                        else_: LABEL,
                    },
                ),
                (
                    FALSE,
                    Node::Const {
                        value: Value::Bool(false),
                    },
                ),
            ]
            .into(),
        },
        Scope {
            iteration: iteration(owner),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: BINDING,
            }],
            ..Scope::default()
        },
    );
    let mut shadow = schema();
    shadow.name = "Ref".into();
    // Build the explicit collision schema without changing the named declaration.
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            schema().child("Rows").unwrap().clone(),
            shadow,
        ],
    );
    p.extra_sources.push(NamedSource {
        name: "Ref".into(),
        path: String::new(),
        schema: schema(),
        options: Default::default(),
        dynamic_path: None,
    });
    p
}

fn run_internal(
    p: &Project,
    primary: &Instance,
    extras: &Instance,
    sink: Option<&dyn TraceSink>,
    hook: Option<&dyn DebugHook>,
) -> Result<Instance, EngineError> {
    crate::check_source_ownership_capability(p, None).unwrap();
    let roots = SourceRoots::new(p, primary, extras, OwnedLimits::default()).unwrap();
    let state = OwnedContext::new(&roots);
    let registry = TraceOwnedRegistry::new(p, &roots);
    let reported = Cell::new(false);
    let program = EvalProgram::new(&p.graph, &p.user_functions, sink, &reported)
        .with_owned_context(&state)
        .with_owned_trace_registry(&registry)
        .with_primary_source(primary)
        .with_debug_hook(hook);
    let context = [extras, primary];
    // Match the real pre-target ordering. No test supplies a trace phase,
    // occurrence frame, origin, position, or preselected failure node.
    crate::failure::evaluate(program, &p.failure_rules, &context)?;
    crate::eval_scope::eval_scope(
        program,
        &p.root,
        None,
        &context,
        &[],
        &p.extra_sources,
        None,
        &TraceScope::primary(),
    )
}

fn assert_witness(
    pending: &PendingNodeFailure,
    view: &TraceOwnedWitness<'_>,
    phase: TraceOwnedPhase,
    inputs: (&Instance, &Instance),
    frames: &[(TraceOwnedOwnerRef<'_>, &Instance, usize)],
    ancestry: bool,
) {
    assert_eq!(view.domain, TraceOwnedDomain::Basic);
    assert_eq!(view.phase, phase);
    assert_eq!(view.owned_ancestry, ancestry);
    assert_eq!(view.roots.len(), 2);
    assert_eq!(view.roots[0].owner, TraceOwnedOwnerRef::Primary);
    assert_eq!(view.roots[1].owner, named_witness_owner());
    assert!(std::ptr::eq(view.roots[0].input.unwrap(), inputs.0));
    assert!(std::ptr::eq(view.roots[1].input.unwrap(), inputs.1));
    assert!(!std::ptr::eq(inputs.0.field("Ref").unwrap(), inputs.1));
    let TraceOwnedOwnerRef::Named { name, .. } = view.roots[1].owner else {
        unreachable!()
    };
    assert_eq!(name.as_ptr(), view.project.extra_sources[0].name.as_ptr());
    assert_eq!(view.frames.len(), frames.len());
    assert_eq!(view.origins.len(), frames.len());
    assert_eq!(view.positions.len(), frames.len());
    assert_eq!(pending.positions.len(), frames.len());
    assert_eq!(pending.omitted_outer_positions, 0);
    assert!(!pending.position_paths_truncated);
    for (slot, &(owner, item, index)) in frames.iter().enumerate() {
        let frame = &view.frames[slot];
        let origin = &view.origins[slot];
        let position = &view.positions[slot];
        assert_eq!(frame.owner, owner);
        assert_eq!(origin.owner, owner);
        if let TraceOwnedOwnerRef::Named { name, .. } = frame.owner {
            assert_eq!(name.as_ptr(), view.project.extra_sources[0].name.as_ptr());
        }
        assert_eq!(frame.collection, ["Rows"]);
        assert_eq!(origin.path, ["Rows"]);
        assert_eq!(frame.position_slot, slot);
        assert!(std::ptr::eq(frame.item, item));
        assert!(std::ptr::eq(origin.item, item));
        assert_eq!(position.collection, ["Rows"]);
        assert_eq!(position.index, index);
        assert!(!position.grouped);
        assert_eq!(position.join, None);
        assert_eq!(position.join_position, None);
        assert_eq!(position.document_path, None);
        assert_eq!(pending.positions[slot].collection, ["Rows"]);
        assert_eq!(pending.positions[slot].index, index);
    }
}

fn assert_failure(
    p: &Project,
    primary: &Instance,
    extras: &Instance,
    expected: EngineError,
    check: &WitnessCheck<'_>,
    check_events: &dyn Fn(&[TraceEvent]),
) {
    let baseline = Collector::default();
    assert_eq!(
        run_internal(p, primary, extras, Some(&baseline), None).as_ref(),
        Err(&expected)
    );
    check_events(&baseline.0.borrow());
    for decision in [DebugDecision::Resume, DebugDecision::Cancel] {
        let observer = Observer::new(decision, &expected, check);
        let actual = run_internal(p, primary, extras, Some(&observer), Some(&observer));
        assert_eq!(
            actual.as_ref(),
            Err(match decision {
                DebugDecision::Resume => &expected,
                DebugDecision::Cancel => &EngineError::DebugCancelled,
            })
        );
        assert_eq!(observer.calls.get(), 1);
        let events = observer.events.borrow();
        assert_eq!(
            events.len(),
            observer.events_at_failure.get(),
            "no events follow failure/cancel"
        );
        assert_eq!(
            *events,
            *baseline.0.borrow(),
            "hooks preserve the successful trace prefix"
        );
        let failed = observer.failed_node.get().unwrap();
        assert!(!events.iter().any(|event| matches!(event,
            TraceEvent::NodeValue { node, .. } if *node == failed)));
        assert!(!events.iter().any(|event| matches!(event,
            TraceEvent::NodeInputValue { input, .. } if *input == failed)));
    }
}

fn no_completed_targets(events: &[TraceEvent]) {
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
    )));
}

fn filter_decisions(events: &[TraceEvent]) -> Vec<(TraceFilterPhase, Vec<usize>, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::FilterDecision {
                phase,
                positions,
                passed,
                ..
            } => Some((
                *phase,
                positions.iter().map(|position| position.index).collect(),
                *passed,
            )),
            _ => None,
        })
        .collect()
}

fn scope_failure(phase: TraceOwnedPhase) {
    let (primary, extras) = inputs();
    let named = extras.field("Ref").unwrap();
    for is_named in [false, true] {
        let owner = if is_named {
            named_owner()
        } else {
            SourceOwner::Primary
        };
        let mut p = project(owner);
        match phase {
            TraceOwnedPhase::Structural | TraceOwnedPhase::Selection => {
                p.root.filter = Some(PREDICATE);
                p.root.sort_by = Some(RANK);
                p.root.sort_filter_order = if phase == TraceOwnedPhase::Structural {
                    SortFilterOrder::FilterThenSort
                } else {
                    SortFilterOrder::SortThenFilter
                };
            }
            TraceOwnedPhase::Output => p.root.filter = Some(KEEP),
        }
        let root = if is_named { named } else { &primary };
        let expected = EngineError::MappingException {
            node: ERROR,
            message: Some(if is_named { "N.raw2" } else { "P.raw2" }.into()),
        };
        let check = |pending: &PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
            assert_eq!(pending.node, ERROR);
            assert_witness(
                pending,
                view,
                phase,
                (&primary, named),
                &[(
                    if is_named {
                        named_witness_owner()
                    } else {
                        TraceOwnedOwnerRef::Primary
                    },
                    second_row(root),
                    if phase == TraceOwnedPhase::Structural {
                        2
                    } else {
                        1
                    },
                )],
                true,
            );
        };
        let check_events = |events: &[TraceEvent]| {
            no_completed_targets(events);
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, TraceEvent::IterationCandidate { .. }))
                    .count(),
                2
            );
            let ranks = events
                .iter()
                .filter_map(|event| match event {
                    TraceEvent::SortPosition {
                        positions,
                        output_index,
                        ..
                    } => Some((positions[0].index, *output_index)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            match phase {
                TraceOwnedPhase::Structural => {
                    assert!(ranks.is_empty());
                    assert!(
                        !events
                            .iter()
                            .any(|event| matches!(event, TraceEvent::SortCandidate { .. }))
                    );
                    assert_eq!(
                        filter_decisions(events),
                        [(TraceFilterPhase::BeforeSort, vec![1], true)]
                    );
                }
                TraceOwnedPhase::Selection => {
                    assert_eq!(ranks, [(2, 1), (1, 2)]);
                    assert!(
                        filter_decisions(events).is_empty(),
                        "first sorted predicate fails"
                    );
                }
                TraceOwnedPhase::Output => {
                    assert!(ranks.is_empty());
                    assert_eq!(
                        filter_decisions(events),
                        [
                            (TraceFilterPhase::Selection, vec![1], false),
                            (TraceFilterPhase::Selection, vec![2], true),
                        ]
                    );
                }
            }
        };
        assert_failure(&p, &primary, &extras, expected, &check, &check_events);
    }
}

#[test]
fn before_sort_failure_uses_structural_primary_and_named_occurrences() {
    scope_failure(TraceOwnedPhase::Structural);
}

#[test]
fn sort_key_failure_uses_structural_primary_and_named_occurrences_before_ranking() {
    const RAW_RANK: u32 = 12;
    let (primary, extras) = inputs();
    let named = extras.field("Ref").unwrap();
    for is_named in [false, true] {
        let owner = if is_named {
            named_owner()
        } else {
            SourceOwner::Primary
        };
        let mut p = project(owner.clone());
        p.root.sort_by = Some(RANK);
        p.graph.nodes.insert(RAW_RANK, field(owner, "Rank"));
        p.graph.nodes.insert(
            RANK,
            Node::If {
                condition: FAIL,
                then: ERROR,
                else_: RAW_RANK,
            },
        );
        let root = if is_named { named } else { &primary };
        let check = |pending: &PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
            assert_eq!(pending.node, ERROR);
            // Sort keys use the Structural program before SortPosition rebases
            // ranks. Raw row two would sort first, but has not acquired rank one.
            assert_witness(
                pending,
                view,
                TraceOwnedPhase::Structural,
                (&primary, named),
                &[(
                    if is_named {
                        named_witness_owner()
                    } else {
                        TraceOwnedOwnerRef::Primary
                    },
                    second_row(root),
                    2,
                )],
                true,
            );
        };
        let check_events = |events: &[TraceEvent]| {
            no_completed_targets(events);
            assert!(filter_decisions(events).is_empty());
            assert!(!events.iter().any(|event| matches!(
                event,
                TraceEvent::SortPosition { .. } | TraceEvent::TargetFieldWritten { .. }
            )));
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, TraceEvent::IterationCandidate { .. }))
                    .count(),
                2
            );
            let candidates = events
                .iter()
                .filter_map(|event| match event {
                    TraceEvent::SortCandidate {
                        positions, keys, ..
                    } => Some((positions, keys)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(candidates.len(), 1, "no sort candidate for the failed key");
            let (positions, keys) = candidates[0];
            assert_eq!(positions.len(), 1);
            assert_eq!(positions[0].collection, ["Rows"]);
            assert_eq!(positions[0].index, 1);
            assert_eq!(
                keys.as_slice(),
                &[crate::TraceSortKey {
                    node: RANK,
                    descending: false,
                    value: crate::TraceValue::new(&Value::Int(20)),
                }]
            );
            let completed_keys = events
                .iter()
                .filter_map(|event| match event {
                    TraceEvent::NodeValue {
                        node: RANK,
                        positions,
                        value,
                    } => Some((positions[0].index, value.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                completed_keys,
                [(1, crate::TraceValue::new(&Value::Int(20)))]
            );
        };
        assert_failure(
            &p,
            &primary,
            &extras,
            EngineError::MappingException {
                node: ERROR,
                message: Some(if is_named { "N.raw2" } else { "P.raw2" }.into()),
            },
            &check,
            &check_events,
        );
    }
}

#[test]
fn after_sort_failure_uses_selection_rank_not_raw_occurrence_index() {
    scope_failure(TraceOwnedPhase::Selection);
}

#[test]
fn binding_failure_uses_output_compaction_without_changing_occurrence_identity() {
    scope_failure(TraceOwnedPhase::Output);
}

#[test]
fn nested_failure_keeps_compacted_primary_parent_and_colliding_named_child() {
    let (primary, extras) = inputs();
    let named = extras.field("Ref").unwrap();
    let mut p = project(SourceOwner::Primary);
    p.root.filter = Some(KEEP);
    p.root.bindings[0].node = LABEL;
    p.graph
        .nodes
        .insert(NAMED_VALUE, field(named_owner(), "Label"));
    p.graph.nodes.insert(
        MESSAGE,
        Node::Call {
            function: "concat".into(),
            args: vec![LABEL, NAMED_VALUE],
        },
    );
    p.graph.nodes.insert(
        ERROR,
        Node::Raise {
            message: Some(MESSAGE),
        },
    );
    p.graph.nodes.insert(12, field(named_owner(), "Keep"));
    p.root.children.push(Scope {
        target_field: "Child".into(),
        iteration: iteration(named_owner()),
        filter: Some(12),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: ERROR,
        }],
        ..Scope::default()
    });
    let check = |pending: &PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
        assert_eq!(pending.node, ERROR);
        assert_witness(
            pending,
            view,
            TraceOwnedPhase::Output,
            (&primary, named),
            &[
                (TraceOwnedOwnerRef::Primary, second_row(&primary), 1),
                (named_witness_owner(), second_row(named), 1),
            ],
            true,
        );
    };
    let check_events = |events: &[TraceEvent]| {
        no_completed_targets(events);
        assert_eq!(
            filter_decisions(events),
            [
                (TraceFilterPhase::Selection, vec![1], false),
                (TraceFilterPhase::Selection, vec![2], true),
                (TraceFilterPhase::Selection, vec![1, 1], false),
                (TraceFilterPhase::Selection, vec![1, 2], true),
            ]
        );
        let write = events.iter().position(|event| matches!(event,
            TraceEvent::TargetFieldWritten { scope, positions, .. }
                if scope.source_scope == TraceScope::primary().source_scope && positions[0].index == 1)).unwrap();
        let child = events
            .iter()
            .position(|event| {
                matches!(event,
            TraceEvent::ScopeStarted { scope, .. } if scope.target_path == ["Child"])
            })
            .unwrap();
        assert!(
            write < child,
            "the compacted parent writes before entering its child"
        );
    };
    assert_failure(
        &p,
        &primary,
        &extras,
        EngineError::MappingException {
            node: ERROR,
            message: Some("P.raw2N.raw2".into()),
        },
        &check,
        &check_events,
    );
}

fn rule_project() -> Project {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(
        PREDICATE,
        Node::If {
            condition: FAIL,
            then: ERROR,
            else_: FALSE,
        },
    );
    p.graph.nodes.insert(
        NAMED_VALUE,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: named_owner(),
                path: path(&["Note"]),
            },
            frame: None,
        }),
    );
    p.graph.nodes.insert(
        MESSAGE,
        Node::Call {
            function: "concat".into(),
            args: vec![LABEL, NAMED_VALUE],
        },
    );
    p.graph.nodes.insert(
        ERROR,
        Node::Raise {
            message: Some(MESSAGE),
        },
    );
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: FailureSelection::WhenTrue { predicate: FAIL },
        message: Some(MESSAGE),
    });
    p
}

fn no_target_evaluation(events: &[TraceEvent]) {
    no_completed_targets(events);
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::ScopeStarted { .. }
            | TraceEvent::IterationCandidate { .. }
            | TraceEvent::TargetFieldWritten { .. }
    )));
}

#[test]
fn row_rule_predicate_error_reports_actual_leaf_and_structural_primary_provenance() {
    let (primary, extras) = inputs();
    let named = extras.field("Ref").unwrap();
    let mut p = rule_project();
    p.failure_rules[0].selection = FailureSelection::WhenTrue {
        predicate: PREDICATE,
    };
    // This complete typed error has no intrinsic node; the callback must identify
    // the evaluated leaf, not the enclosing If or the failure rule.
    p.graph.nodes.insert(
        ERROR,
        Node::SourceField {
            path: path(&["Absent"]),
            frame: None,
        },
    );
    let check = |pending: &PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
        assert_eq!(pending.node, ERROR);
        assert_witness(
            pending,
            view,
            TraceOwnedPhase::Structural,
            (&primary, named),
            &[(TraceOwnedOwnerRef::Primary, second_row(&primary), 2)],
            false,
        );
    };
    let check_events = |events: &[TraceEvent]| {
        no_target_evaluation(events);
        assert!(
            !events.iter().any(|event| matches!(
                event,
                TraceEvent::NodeValue {
                    node: LABEL | NAMED_VALUE | MESSAGE,
                    ..
                }
            )),
            "rule message stays lazy"
        );
    };
    assert_failure(
        &p,
        &primary,
        &extras,
        EngineError::MissingSourceField("Absent".into()),
        &check,
        &check_events,
    );
}

#[test]
fn row_rule_message_error_borrows_named_root_without_inventing_a_named_row() {
    let (primary, extras) = inputs();
    let named = extras.field("Ref").unwrap();
    let mut p = rule_project();
    p.failure_rules[0].message = Some(BINDING);
    let check = |pending: &PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
        assert_eq!(pending.node, ERROR);
        assert_witness(
            pending,
            view,
            TraceOwnedPhase::Structural,
            (&primary, named),
            &[(TraceOwnedOwnerRef::Primary, second_row(&primary), 2)],
            false,
        );
    };
    let check_events = |events: &[TraceEvent]| {
        no_target_evaluation(events);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event,
            TraceEvent::NodeValue { node: NAMED_VALUE, positions, .. } if positions[0].index == 2))
                .count(),
            1
        );
    };
    assert_failure(
        &p,
        &primary,
        &extras,
        EngineError::MappingException {
            node: ERROR,
            message: Some("P.raw2N.root".into()),
        },
        &check,
        &check_events,
    );
}

#[test]
fn selected_row_rule_mapping_failure_has_no_node_callback_even_for_a_cancel_hook() {
    let (primary, extras) = inputs();
    let p = rule_project();
    let expected = EngineError::MappingFailure {
        rule: 1,
        message: Some("P.raw2N.root".into()),
    };
    let baseline = Collector::default();
    assert_eq!(
        run_internal(&p, &primary, &extras, Some(&baseline), None).as_ref(),
        Err(&expected)
    );
    no_target_evaluation(&baseline.0.borrow());
    let check = |_: &PendingNodeFailure, _: &TraceOwnedWitness<'_>| panic!("no expression failed");
    for decision in [DebugDecision::Resume, DebugDecision::Cancel] {
        let observer = Observer::new(decision, &expected, &check);
        assert_eq!(
            run_internal(&p, &primary, &extras, Some(&observer), Some(&observer)).as_ref(),
            Err(&expected)
        );
        assert_eq!(observer.calls.get(), 0);
        assert_eq!(observer.failed_node.get(), None);
        assert_eq!(*observer.events.borrow(), *baseline.0.borrow());
    }
}
