use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

use crate::{
    DebugDecision, DebugHook, EngineError, ExecutionContext, PendingTargetWrite, TraceEvent,
    TraceSink, TraceTargetFieldBinding, run_with_context,
};

struct PausingHook {
    writes: SyncSender<PendingTargetWrite>,
    commands: Receiver<DebugDecision>,
}

impl DebugHook for PausingHook {
    fn before_target_write(&self, write: &PendingTargetWrite) -> DebugDecision {
        if self.writes.send(write.clone()).is_err() {
            return DebugDecision::Cancel;
        }
        self.commands.recv().unwrap_or(DebugDecision::Cancel)
    }
}

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn two_field_project() -> Project {
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("first", ScalarType::String),
                SchemaNode::scalar("second", ScalarType::String),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("A".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("B".into()),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "first".into(),
                    node: 0,
                },
                Binding {
                    target_field: "second".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    }
}

struct PausedRun {
    writes: Receiver<PendingTargetWrite>,
    commands: SyncSender<DebugDecision>,
    result: Receiver<(Result<Instance, EngineError>, Vec<TraceEvent>)>,
}

fn start_paused_run() -> PausedRun {
    let (write_tx, write_rx) = mpsc::sync_channel(0);
    let (command_tx, command_rx) = mpsc::sync_channel(0);
    let (result_tx, result_rx) = mpsc::sync_channel(0);
    std::thread::spawn(move || {
        let hook = PausingHook {
            writes: write_tx,
            commands: command_rx,
        };
        let collector = Collector::default();
        let execution = ExecutionContext::new(Path::new("mapping.json"))
            .with_trace_sink(&collector)
            .with_debug_hook(&hook);
        let result = run_with_context(
            &two_field_project(),
            &Instance::Group(Vec::new()),
            &execution,
        );
        let _ = result_tx.send((result, collector.0.into_inner()));
    });
    PausedRun {
        writes: write_rx,
        commands: command_tx,
        result: result_rx,
    }
}

#[test]
fn debug_hook_pauses_before_insertion_and_resumes_without_changing_trace() {
    let PausedRun {
        writes,
        commands,
        result,
    } = start_paused_run();
    let first = writes.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first.field, "first");
    assert!(first.draft.fields.is_empty());
    assert_eq!(first.pending.value.as_ref().unwrap().preview, "A");
    assert_eq!(
        first.binding,
        TraceTargetFieldBinding::StaticBinding { value: 0 }
    );
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));

    commands.send(DebugDecision::Resume).unwrap();
    let second = writes.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(second.field, "second");
    assert_eq!(second.draft.fields.len(), 1);
    assert_eq!(second.draft.fields[0].name, "first");
    assert_eq!(
        second.draft.fields[0]
            .preview
            .value
            .as_ref()
            .unwrap()
            .preview,
        "A"
    );
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));

    commands.send(DebugDecision::Resume).unwrap();
    let (output, events) = result.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        output.unwrap(),
        Instance::Group(vec![
            ("first".into(), Instance::Scalar(Value::String("A".into()))),
            ("second".into(), Instance::Scalar(Value::String("B".into()))),
        ])
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
            .count(),
        2
    );
}

#[test]
fn cancelling_a_pending_write_returns_no_target_and_records_no_write() {
    let PausedRun {
        writes,
        commands,
        result,
    } = start_paused_run();
    assert_eq!(
        writes.recv_timeout(Duration::from_secs(5)).unwrap().field,
        "first"
    );
    commands.send(DebugDecision::Resume).unwrap();
    assert_eq!(
        writes.recv_timeout(Duration::from_secs(5)).unwrap().field,
        "second"
    );
    commands.send(DebugDecision::Cancel).unwrap();
    let (output, events) = result.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(output, Err(EngineError::DebugCancelled)));
    let written = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten { field, .. } => Some(field.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(written, ["first"]);
}
