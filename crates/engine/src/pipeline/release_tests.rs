//! Releasing runs must stay observably identical to retain-all runs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, NamedTarget, Node, Pipeline, PipelineInput,
    PipelineNamedInput, PipelineStage, Project, Scope, ScopeConstruction, ScopeIteration,
};

use super::{PipelineError, PipelineTargetRef, run_retaining_all, run_retaining_selected};
use crate::{DynamicSourceLoader, ExecutionContext, TraceEvent, TraceSink};

fn record() -> SchemaNode {
    SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
}

fn value(text: &str) -> Instance {
    Instance::Group((vec![("Value".into(), Instance::Scalar(Value::String(text.into())))]).into())
}

fn copy_project() -> Project {
    Project {
        source: record(),
        target: record(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

fn bind(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}

/// Copies its source and adds named `shadow` and `unused` constant targets.
fn producer_project() -> Project {
    let mut project = copy_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("shadow".into()),
        },
    );
    for name in ["shadow", "unused"] {
        project.extra_targets.push(NamedTarget {
            name: name.into(),
            path: None,
            schema: record(),
            options: Default::default(),
            root: bind(0),
        });
    }
    project
}

/// Concatenates its primary source with each declared named source.
fn joining_project(names: &[&str]) -> Project {
    let mut project = copy_project();
    let mut arguments = vec![0];
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    for (index, name) in names.iter().enumerate() {
        let node = u32::try_from(index + 1).unwrap();
        project.graph.nodes.insert(
            node,
            Node::SourceField {
                path: vec![(*name).into(), "Value".into()],
                frame: None,
            },
        );
        arguments.push(node);
        project.extra_sources.push(NamedSource {
            name: (*name).into(),
            path: String::new(),
            schema: record(),
            options: Default::default(),
            dynamic_path: None,
        });
    }
    let call = u32::try_from(names.len() + 1).unwrap();
    project.graph.nodes.insert(
        call,
        Node::Call {
            function: "concat".into(),
            args: arguments,
        },
    );
    project.root = bind(call);
    project
}

fn host(name: &str) -> PipelineInput {
    PipelineInput::Host { name: name.into() }
}

fn output(stage: &str, target: Option<&str>) -> PipelineInput {
    PipelineInput::StageTarget {
        stage: stage.into(),
        target: target.map(str::to_string),
    }
}

fn stage(
    id: &str,
    project: Project,
    source: PipelineInput,
    extras: &[(&str, PipelineInput)],
) -> PipelineStage {
    PipelineStage {
        id: id.into(),
        mapping_path: None,
        project,
        source,
        extra_sources: extras
            .iter()
            .map(|(name, from)| PipelineNamedInput {
                name: (*name).into(),
                from: from.clone(),
            })
            .collect(),
    }
}

/// Fan-out from `prepare`, a stage reading one value as its source and a named
/// source, another reading one value through two named sources, a named target
/// that is read later, and a named target that nothing reads.
fn fan_out_pipeline() -> Pipeline {
    Pipeline {
        main_mapping_path: None,
        stages: vec![
            stage(
                "final",
                joining_project(&["left", "right"]),
                output("branch", None),
                &[
                    ("left", output("echo", None)),
                    ("right", output("echo", None)),
                ],
            ),
            stage("prepare", producer_project(), host("main"), &[]),
            stage(
                "branch",
                joining_project(&["same", "aside"]),
                output("prepare", None),
                &[
                    ("same", output("prepare", None)),
                    ("aside", output("prepare", Some("shadow"))),
                ],
            ),
            stage(
                "echo",
                joining_project(&["host"]),
                output("prepare", None),
                &[("host", host("main"))],
            ),
        ],
    }
}

fn hosts() -> BTreeMap<String, Instance> {
    BTreeMap::from([("main".to_string(), value("m"))])
}

fn context(_: &str) -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("pipeline.json"))
}

#[test]
fn releasing_run_keeps_selected_outputs_and_order_with_fewer_live_values() {
    let pipeline = fan_out_pipeline();
    let (all, all_peak) = run_retaining_all(&pipeline, &hosts(), context).unwrap();
    let retain = [
        PipelineTargetRef {
            stage: "final",
            target: None,
        },
        PipelineTargetRef {
            stage: "prepare",
            target: Some("shadow"),
        },
    ];
    let (released, released_peak) =
        run_retaining_selected(&pipeline, hosts(), &retain, context).unwrap();
    assert_eq!(
        released.stages_executed,
        all.stages
            .iter()
            .map(|stage| stage.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        released.stages_executed,
        ["prepare", "branch", "echo", "final"]
    );
    let expected = all.stage("final").unwrap().primary.clone();
    assert_eq!(expected, value("mmshadowmmmm"));
    assert_eq!(released.get("final", None), Some(&expected));
    assert_eq!(
        released.get("prepare", Some("shadow")),
        Some(&all.stage("prepare").unwrap().extras[0].instance)
    );
    assert_eq!(released.get("prepare", None), None);
    assert_eq!(released.get("echo", None), None);
    // Retain-all ends holding the host plus all six stage targets.
    assert_eq!(all_peak, 7);
    assert_eq!(released_peak, 4);
}

#[test]
fn a_linear_chain_holds_at_most_one_input_and_one_output() {
    let ids = ["a", "b", "c", "d", "e", "f"];
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let source = match index {
                    0 => host("main"),
                    _ => output(ids[index - 1], None),
                };
                stage(id, copy_project(), source, &[])
            })
            .collect(),
    };
    let retain = [PipelineTargetRef {
        stage: "f",
        target: None,
    }];
    let (_, all_peak) = run_retaining_all(&pipeline, &hosts(), context).unwrap();
    let (released, released_peak) =
        run_retaining_selected(&pipeline, hosts(), &retain, context).unwrap();
    assert_eq!(released.get("f", None), Some(&value("m")));
    assert_eq!((all_peak, released_peak), (7, 2));
}

/// Answers every request with one `Item` whose value is the requested path.
struct RecordingLoader {
    requests: RefCell<Vec<(String, String)>>,
}

impl DynamicSourceLoader for RecordingLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        self.requests
            .borrow_mut()
            .push((source.to_owned(), path.to_owned()));
        Ok(Arc::new(Instance::Group(
            (vec![("Item".into(), Instance::Repeated(vec![value(path)]))]).into(),
        )))
    }
}

struct RecordingSink {
    stage: RefCell<String>,
    events: RefCell<Vec<(String, TraceEvent)>>,
}

impl TraceSink for RecordingSink {
    fn record(&self, event: TraceEvent) {
        self.events
            .borrow_mut()
            .push((self.stage.borrow().clone(), event));
    }
}

fn loaded_schema() -> SchemaNode {
    SchemaNode::group(
        "Loaded",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                .repeating(),
        ],
    )
}

/// Loads the document named by the source `Value` and copies its items.
fn loading_project() -> Project {
    let mut project = copy_project();
    project.target = loaded_schema();
    project.extra_sources.push(NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Document",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 0,
            iteration: Vec::new(),
        }),
    });
    project.graph.nodes.extend([
        (
            0,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: Some(vec!["document".into(), "Item".into()]),
            },
        ),
    ]);
    project.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
            ..bind(1)
        }],
        ..Scope::default()
    };
    project
}

type Observation = (
    Result<Option<Instance>, String>,
    Vec<String>,
    Vec<(String, String)>,
    Vec<(String, TraceEvent)>,
);

fn observe(pipeline: &Pipeline, retain: Option<&[PipelineTargetRef<'_>]>) -> Observation {
    let loader = RecordingLoader {
        requests: RefCell::new(Vec::new()),
    };
    let sink = RecordingSink {
        stage: RefCell::new(String::new()),
        events: RefCell::new(Vec::new()),
    };
    let calls = RefCell::new(Vec::new());
    let path = Path::new("pipeline.json");
    let for_stage = |stage: &str| {
        calls.borrow_mut().push(stage.to_owned());
        sink.stage.replace(stage.to_owned());
        ExecutionContext::new(path)
            .with_dynamic_source_loader(&loader)
            .with_trace_sink(&sink)
    };
    let last = "load";
    let result = match retain {
        None => run_retaining_all(pipeline, &hosts(), for_stage)
            .map(|(outputs, _)| outputs.stage(last).map(|outputs| outputs.primary.clone())),
        Some(retain) => run_retaining_selected(pipeline, hosts(), retain, for_stage)
            .map(|(outputs, _)| outputs.get(last, None).cloned()),
    }
    .map_err(|error| format!("{error:?}"));
    (
        result,
        calls.into_inner(),
        loader.requests.into_inner(),
        sink.events.into_inner(),
    )
}

#[test]
fn trace_events_loader_requests_and_context_calls_are_unchanged() {
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![
            stage("prepare", producer_project(), host("main"), &[]),
            stage(
                "echo",
                joining_project(&["aside"]),
                output("prepare", None),
                &[("aside", output("prepare", Some("shadow")))],
            ),
            stage("load", loading_project(), output("echo", None), &[]),
        ],
    };
    let retain = [PipelineTargetRef {
        stage: "load",
        target: None,
    }];
    let all = observe(&pipeline, None);
    let released = observe(&pipeline, Some(&retain));
    let expected =
        Instance::Group((vec![("Row".into(), Instance::Repeated(vec![value("mshadow")]))]).into());
    assert_eq!(all.0, Ok(Some(expected)));
    assert_eq!(all.2, [("document".to_string(), "mshadow".to_string())]);
    assert!(all.3.iter().any(|(stage, _)| stage == "load"));
    assert_eq!(released, all);
}

#[test]
fn a_failing_later_stage_keeps_failure_precedence_and_observations() {
    let mut failing = copy_project();
    failing.graph.nodes.insert(
        0,
        Node::RuntimeParameter {
            name: "required".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    failing.root = bind(0);
    failing.extra_sources.push(NamedSource {
        name: "loaded".into(),
        path: String::new(),
        schema: loaded_schema(),
        options: Default::default(),
        dynamic_path: None,
    });
    let reads_load = [("loaded", output("load", None))];
    // `after` would also fail, but `failing` runs first in stable order.
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![
            stage("prepare", producer_project(), host("main"), &[]),
            stage("load", loading_project(), output("prepare", None), &[]),
            stage(
                "failing",
                failing.clone(),
                output("prepare", Some("shadow")),
                &reads_load,
            ),
            stage(
                "after",
                failing,
                output("prepare", Some("shadow")),
                &reads_load,
            ),
        ],
    };
    let retain = [PipelineTargetRef {
        stage: "after",
        target: None,
    }];
    let all = observe(&pipeline, None);
    let released = observe(&pipeline, Some(&retain));
    let Err(error) = &all.0 else {
        panic!("later stage must fail");
    };
    assert!(error.contains("stage: \"failing\""), "{error}");
    assert_eq!(all.1, ["prepare", "load", "failing"]);
    assert_eq!(all.2, [("document".to_string(), "m".to_string())]);
    assert_eq!(released, all);
}

#[test]
fn unknown_retained_targets_reject_before_any_stage_runs() {
    let pipeline = fan_out_pipeline();
    for (stage, target) in [("missing", None), ("prepare", Some("missing"))] {
        let calls = RefCell::new(0);
        let error = run_retaining_selected(
            &pipeline,
            hosts(),
            &[PipelineTargetRef { stage, target }],
            |stage| {
                *calls.borrow_mut() += 1;
                context(stage)
            },
        )
        .unwrap_err();
        assert!(matches!(
            &error,
            PipelineError::UnknownRetainedTarget { stage: actual, target: named }
                if actual == stage && named.as_deref() == target
        ));
        assert_eq!(calls.into_inner(), 0);
    }
}
