#[path = "owned_scope_lifecycle/boundaries.rs"]
mod boundaries;
#[path = "owned_scope_lifecycle/boundary_errors.rs"]
mod boundary_errors;
#[path = "owned_scope_lifecycle/candidates.rs"]
mod candidates;
#[path = "owned_scope_lifecycle/failures.rs"]
mod failures;
#[path = "owned_scope_lifecycle/filtering.rs"]
mod filtering;
#[path = "owned_scope_lifecycle/sorting.rs"]
mod sorting;
#[path = "owned_scope_lifecycle/windows.rs"]
mod windows;

use super::{Collector, field_trace_project};
use crate::eval_expr::EvalProgram;
use crate::owned_context::{
    OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
};
use crate::source_iteration::PositionFrame;
use crate::trace::{
    TraceOwnedRegistry, TraceOwnedRegistryView, basic_scope_iteration, scope_iteration,
};
use crate::{
    EngineError, OwnedSourceError, TraceEvent, TraceIteration, TraceOutputKind,
    TraceOwnedContextError, TraceOwnedLocation, TraceOwnedOwnerRef, TraceOwnedPhase,
    TraceOwnedSnapshot, TraceOwnedWitness, TraceOwnedWitnessError, TraceScope, TraceSink,
    TraceTargetFieldBinding, TraceValue,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
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
type Check<'a> = dyn Fn(&TraceEvent, &TraceOwnedWitness<'_>) + 'a;
struct Lifecycle<'a> {
    events: RefCell<Vec<TraceEvent>>,
    contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
    root_addresses: RefCell<Vec<usize>>,
    selected: Cell<u8>,
    stop_after_first: bool,
    check: Option<&'a Check<'a>>,
}
impl Lifecycle<'_> {
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
impl TraceSink for Lifecycle<'_> {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
    fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
        match event {
            TraceEvent::ScopeStarted { .. } => self.selected.get() & 1 != 0,
            TraceEvent::ScopeFinished { .. } => self.selected.get() & 2 != 0,
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
            TraceEvent::ScopeStarted { .. } | TraceEvent::ScopeFinished { .. }
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
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
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
fn row(value: &str) -> Instance {
    group(vec![("Value", text(value))])
}
fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    )
}
fn project(names: &[&str], nodes: Vec<(u32, Node)>, root: Scope) -> Project {
    let mut p = field_trace_project(
        SchemaNode::group("Output", Vec::new()),
        Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    );
    p.source = schema();
    p.extra_sources = names
        .iter()
        .map(|name| NamedSource {
            name: (*name).into(),
            path: String::new(),
            schema: schema(),
            options: Default::default(),
            dynamic_path: None,
        })
        .collect();
    p
}
fn constant() -> Project {
    project(
        &[],
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
    )
}
fn owned_field(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: Some(path(&["Rows"])),
    })
}
fn repeated(owner: SourceOwner) -> Project {
    project(
        &["Ref", " Ref 雪 "],
        vec![(0, owned_field(owner.clone(), &["Rows", "Value"]))],
        Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    )
}
fn position(index: usize) -> PositionFrame {
    PositionFrame {
        binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
        generated_origin: None,
        collection: path(&["Rows"]),
        index,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    }
}
fn run(
    p: &Project,
    state: &OwnedContext<'_>,
    registry: &dyn TraceOwnedRegistryView,
    sink: Option<&dyn TraceSink>,
    phase: TraceOwnedPhase,
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let failed = Cell::new(false);
    let program = EvalProgram::new(&p.graph, &p.user_functions, sink, &failed)
        .with_owned_context(state)
        .with_owned_trace_registry(registry)
        .with_owned_trace_phase(phase);
    let result = crate::eval_scope::eval_scope(
        program,
        &p.root,
        None,
        &[state.roots.extras_wrapper, state.roots.primary],
        positions,
        &p.extra_sources,
        None,
        &TraceScope::primary(),
    );
    eprintln!(
        "INPUT lifecycle complete_project={p:#?} positions={positions:#?} phase={phase:?} RAW complete_result={result:#?}",
        positions = crate::trace::trace_positions(positions)
    );
    result
}
fn boundaries(events: &[TraceEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                TraceEvent::ScopeStarted { .. } | TraceEvent::ScopeFinished { .. }
            )
        })
        .cloned()
        .collect()
}
fn snapshot(outcome: &Outcome) -> &TraceOwnedSnapshot {
    let Outcome::Snapshot(Ok(view)) = outcome else {
        panic!("expected complete valid witness: {outcome:?}");
    };
    view
}
