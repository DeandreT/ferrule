use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::{
    DebugDecision, DebugHook, PendingNodeInput, PendingNodeValue, PendingTargetWrite, TraceEvent,
    TraceSink,
};
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, NamedTarget, Node,
    Project, Scope, ScopeConstruction, ScopeIteration, SequenceExpr, SortKey,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_item_context_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Observers {
    nodes: Cell<usize>,
    inputs: Cell<usize>,
    writes: Cell<usize>,
    traces: Cell<usize>,
    publications: Cell<usize>,
}

impl Observers {
    fn assert_idle(&self) {
        assert_eq!(
            [
                self.nodes.get(),
                self.inputs.get(),
                self.writes.get(),
                self.traces.get(),
                self.publications.get()
            ],
            [0; 5]
        );
    }
}

impl DebugHook for Observers {
    fn before_target_write(&self, _: &PendingTargetWrite) -> DebugDecision {
        self.writes.set(self.writes.get() + 1);
        DebugDecision::Resume
    }
    fn after_node_value(&self, _: &PendingNodeValue) -> DebugDecision {
        self.nodes.set(self.nodes.get() + 1);
        DebugDecision::Resume
    }
    fn after_node_input(&self, _: &PendingNodeInput) -> DebugDecision {
        self.inputs.set(self.inputs.get() + 1);
        DebugDecision::Resume
    }
}

impl TraceSink for Observers {
    fn record(&self, _: TraceEvent) {
        self.traces.set(self.traces.get() + 1);
    }
}

fn json_options() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    }
}
fn item() -> Node {
    Node::SourceField {
        path: Vec::new(),
        frame: None,
    }
}
fn constant(value: i64) -> Node {
    Node::Const {
        value: Value::Int(value),
    }
}
fn call(function: &str, args: &[u32]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}
fn range(item: u32, to: u32) -> SequenceExpr {
    SequenceExpr::Generate {
        from: None,
        to,
        item,
    }
}

fn rows(item: u32) -> Scope {
    Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Sequence(range(item, 1)),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: item,
        }],
        ..Scope::default()
    }
}

fn project() -> Project {
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: target.clone(),
        source_path: Some("input.json".into()),
        target_path: Some("primary.json".into()),
        source_options: json_options(),
        target_options: json_options(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "Other".into(),
            path: Some("other.json".into()),
            schema: target,
            options: json_options(),
            root: Scope {
                children: vec![rows(20)],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (1, constant(3)),
                (2, constant(0)),
                (10, item()),
                (20, item()),
            ]
            .into(),
        },
        root: Scope {
            children: vec![rows(10)],
            ..Scope::default()
        },
    }
}

fn add_dynamic_input(project: &mut Project) {
    project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
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
        options: json_options(),
        dynamic_path: Some(DynamicSourcePath {
            node: 90,
            iteration: vec!["File".into()],
        }),
    });
    project.graph.nodes.extend([
        (
            90,
            Node::SourceField {
                path: Vec::new(),
                frame: Some(vec!["File".into()]),
            },
        ),
        (
            91,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: Some(vec!["document".into(), "Item".into()]),
            },
        ),
    ]);
    project.extra_targets.push(NamedTarget {
        name: "Fetched".into(),
        path: Some("fetched.json".into()),
        options: json_options(),
        schema: SchemaNode::group(
            "Fetched",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 91,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    });
}

#[derive(Clone, Copy, Debug)]
enum InvalidSite {
    OtherTarget,
    SecondarySort,
    ScalarChild,
    DynamicPathScope,
    DynamicPathReducer,
}

fn invalid_project(site: InvalidSite, indirect: bool) -> (Project, String) {
    let mut project = project();
    let (expression, item) = match site {
        InvalidSite::OtherTarget => {
            project.graph.nodes.insert(31, call("add", &[10, 2]));
            let expression = if indirect { 31 } else { 10 };
            project.extra_targets[0].root.children[0].bindings[0].node = expression;
            (expression, 10)
        }
        InvalidSite::DynamicPathScope => {
            project.graph.nodes.insert(31, call("string", &[10]));
            (if indirect { 31 } else { 10 }, 10)
        }
        InvalidSite::DynamicPathReducer => {
            project.graph.nodes.extend([
                (30, item()),
                (35, call("string", &[30])),
                (
                    40,
                    Node::SequenceAggregate {
                        function: AggregateOp::Count,
                        sequence: range(30, 1),
                        predicate: None,
                        expression: None,
                        arg: None,
                    },
                ),
            ]);
            (if indirect { 35 } else { 30 }, 30)
        }
        InvalidSite::SecondarySort | InvalidSite::ScalarChild => {
            project.graph.nodes.extend([
                (30, item()),
                (35, call("greater_than", &[30, 2])),
                (
                    40,
                    Node::SequenceExists {
                        sequence: range(30, 1),
                        predicate: 35,
                    },
                ),
            ]);
            let expression = if indirect { 35 } else { 30 };
            let SchemaKind::Group { children, .. } = &mut project.target.kind else {
                unreachable!()
            };
            let SchemaKind::Group {
                children: fields, ..
            } = &mut children[0].kind
            else {
                unreachable!()
            };
            let scope = &mut project.root.children[0];
            fields.push(SchemaNode::scalar("Flag", ScalarType::Bool));
            scope.bindings.push(Binding {
                target_field: "Flag".into(),
                node: 40,
            });
            match site {
                InvalidSite::SecondarySort => {
                    scope.sort_by = Some(2);
                    scope.sort_then_by.push(SortKey {
                        node: expression,
                        descending: false,
                    });
                }
                InvalidSite::ScalarChild => {
                    fields[0] = SchemaNode::scalar(
                        "Value",
                        if indirect {
                            ScalarType::Bool
                        } else {
                            ScalarType::Int
                        },
                    );
                    scope
                        .bindings
                        .retain(|binding| binding.target_field != "Value");
                    scope.children.push(Scope {
                        target_field: "Value".into(),
                        construction: ScopeConstruction::Scalar { value: expression },
                        ..Scope::default()
                    });
                }
                InvalidSite::OtherTarget
                | InvalidSite::DynamicPathScope
                | InvalidSite::DynamicPathReducer => unreachable!(),
            }
            (expression, 30)
        }
    };
    add_dynamic_input(&mut project);
    if matches!(
        site,
        InvalidSite::DynamicPathScope | InvalidSite::DynamicPathReducer
    ) {
        project.extra_sources[0].dynamic_path.as_mut().unwrap().node = expression;
    }
    (
        project,
        format!(
            "expression {expression} references generated sequence item node {item} outside its owning context"
        ),
    )
}

fn save_fixture(dir: &Path, project: &Project) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("project.json");
    let encoded = mapping::project_file::encode_pretty(project)?;
    std::fs::write(&path, &encoded)?;
    let reopened = mapping::project_file::decode_bytes(&std::fs::read(&path)?)?;
    assert_eq!(mapping::project_file::encode_pretty(&reopened)?, encoded);
    std::fs::write(dir.join("input.json"), br#"{"File":["dynamic.json"]}"#)?;
    std::fs::write(dir.join("dynamic.json"), b"{malformed dynamic payload")?;
    std::fs::write(dir.join("primary.json"), b"keep primary")?;
    std::fs::write(dir.join("other.json"), b"keep other")?;
    Ok(path)
}

fn snapshot(dir: &Path) -> anyhow::Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut files = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    files
        .into_iter()
        .map(|path| Ok((path.clone(), std::fs::read(path)?)))
        .collect()
}

fn assert_context_error(error: anyhow::Error, expected: &str) {
    let message = format!("{error:#}");
    assert!(message.contains(expected), "{message}");
}

#[test]
fn invalid_item_contexts_precede_file_payload_decoding_and_all_execution_callbacks()
-> anyhow::Result<()> {
    let dir = TempDir::new()?;
    for site in [
        InvalidSite::OtherTarget,
        InvalidSite::SecondarySort,
        InvalidSite::ScalarChild,
        InvalidSite::DynamicPathScope,
        InvalidSite::DynamicPathReducer,
    ] {
        for indirect in [false, true] {
            let (project, expected) = invalid_project(site, indirect);
            let sandbox = dir.0.join(format!("{site:?}-{indirect}"));
            let project_path = save_fixture(&sandbox, &project)?;
            let before = snapshot(&sandbox)?;
            let observers = Observers::default();
            let before_publish = || {
                observers.publications.set(observers.publications.get() + 1);
                true
            };
            let options = cli::RunOptions::new()
                .with_trace_sink(&observers)
                .with_debug_hook(&observers)
                .with_before_publish(&before_publish);
            assert_context_error(
                cli::run_project_with_options(&project_path, &options)
                    .expect_err("invalid item context must reject before evaluation"),
                &expected,
            );
            observers.assert_idle();
            assert_eq!(snapshot(&sandbox)?, before);
            assert!(!sandbox.join("fetched.json").exists());

            let primary = cli::PayloadDocument::new(
                Path::new("input.json"),
                br#"{"File":["dynamic.json"]}"#,
            )?;
            let malformed = cli::PayloadDocument::new(
                Path::new("dynamic.json"),
                b"{malformed dynamic payload",
            )?;
            let extras = [cli::NamedPayloadInput::new("document", malformed)?];
            let absent_output = sandbox.join("absent-parent").join("payload.json");
            let options = cli::PayloadRunOptions::new(primary)
                .with_extra_sources(&extras)
                .with_output_path(&absent_output)
                .with_trace_sink(&observers)
                .with_debug_hook(&observers);
            assert_context_error(
                cli::run_project_payloads(&project_path, &options)
                    .expect_err("validation must precede malformed dynamic payload decoding"),
                &expected,
            );
            observers.assert_idle();
            assert!(!absent_output.parent().unwrap().exists());
            assert_eq!(snapshot(&sandbox)?, before);
        }
    }

    // The same dynamic payload route is live when ownership is valid, so the
    // malformed bytes above are an observable competing failure, not dead data.
    let sandbox = dir.0.join("valid-dynamic-control");
    let mut valid = project();
    add_dynamic_input(&mut valid);
    let project_path = save_fixture(&sandbox, &valid)?;
    let before = snapshot(&sandbox)?;
    let primary =
        cli::PayloadDocument::new(Path::new("input.json"), br#"{"File":["dynamic.json"]}"#)?;
    let malformed =
        cli::PayloadDocument::new(Path::new("dynamic.json"), b"{malformed dynamic payload")?;
    let extras = [cli::NamedPayloadInput::new("document", malformed)?];
    let observers = Observers::default();
    let error = cli::run_project_payloads(
        &project_path,
        &cli::PayloadRunOptions::new(primary)
            .with_extra_sources(&extras)
            .with_trace_sink(&observers)
            .with_debug_hook(&observers),
    )
    .expect_err("valid mapping must reach dynamic input decoding");
    assert!(
        format!("{error:#}").contains("reading payload source `document`"),
        "{error:#}"
    );
    observers.assert_idle();
    let valid_dynamic = cli::PayloadDocument::new(
        Path::new("dynamic.json"),
        br#"{"Item":[{"Value":"decoded"}]}"#,
    )?;
    let extras = [cli::NamedPayloadInput::new("document", valid_dynamic)?];
    let outcome = cli::run_project_payloads(
        &project_path,
        &cli::PayloadRunOptions::new(primary)
            .with_extra_sources(&extras)
            .with_trace_sink(&observers)
            .with_debug_hook(&observers),
    )?;
    assert_eq!(outcome.artifacts.len(), 3);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&outcome.artifacts[2].bytes)?,
        serde_json::json!({"Rows":[{"Value":"decoded"}]})
    );
    assert!(observers.nodes.get() > 0 && observers.writes.get() > 0 && observers.traces.get() > 0);
    assert_eq!(snapshot(&sandbox)?, before);
    assert!(!sandbox.join("fetched.json").exists());
    Ok(())
}

#[test]
fn invalid_item_context_generation_creates_no_artifact_or_staging_directories() -> anyhow::Result<()>
{
    let dir = TempDir::new()?;
    for site in [
        InvalidSite::OtherTarget,
        InvalidSite::SecondarySort,
        InvalidSite::ScalarChild,
        InvalidSite::DynamicPathScope,
        InvalidSite::DynamicPathReducer,
    ] {
        for indirect in [false, true] {
            let (project, expected) = invalid_project(site, indirect);
            let sandbox = dir.0.join(format!("{site:?}-{indirect}"));
            let project_path = save_fixture(&sandbox, &project)?;
            let before = snapshot(&sandbox)?;
            for csharp in [false, true] {
                let output = sandbox
                    .join(if csharp {
                        "absent-csharp-parent"
                    } else {
                        "absent-rust-parent"
                    })
                    .join("generated");
                let target = if csharp {
                    cli::GenerateTarget::CSharp
                } else {
                    // Lowering must reject before resolving even this missing
                    // runtime dependency path or creating the output parent.
                    cli::GenerateTarget::Rust {
                        runtime_path: sandbox.join("absent-runtime"),
                    }
                };
                assert_context_error(
                    cli::generate_project(&project_path, &output, target)
                        .expect_err("invalid context cannot emit artifacts"),
                    &expected,
                );
                assert!(!output.parent().unwrap().exists());
                assert_eq!(snapshot(&sandbox)?, before);
            }
        }
    }
    Ok(())
}

#[test]
fn json_payload_parent_aggregate_inputs_and_arguments_survive_saved_project_loading()
-> anyhow::Result<()> {
    let dir = TempDir::new()?;
    for argument in [false, true] {
        let sandbox = dir.0.join(if argument {
            "parent-argument"
        } else {
            "parent-input"
        });
        let mut project = project();
        project.graph.nodes.extend([
            (30, item()),
            (
                40,
                Node::SequenceAggregate {
                    function: if argument {
                        AggregateOp::ItemAt
                    } else {
                        AggregateOp::Sum
                    },
                    sequence: range(30, if argument { 1 } else { 10 }),
                    predicate: None,
                    expression: None,
                    arg: argument.then_some(10),
                },
            ),
        ]);
        project.root.children[0].bindings[0].node = 40;
        let project_path = save_fixture(&sandbox, &project)?;
        let before = snapshot(&sandbox)?;
        let observers = Observers::default();
        let primary = cli::PayloadDocument::new(Path::new("input.json"), b"{}")?;
        let options = cli::PayloadRunOptions::new(primary)
            .with_trace_sink(&observers)
            .with_debug_hook(&observers);
        let saved = cli::run_project_payloads(&project_path, &options)?;
        let memory = cli::run_project_value_payloads(&project, &project_path, &options)?;
        assert_eq!(saved, memory);
        assert_eq!(saved.artifacts.len(), 2);
        let expected = if argument {
            serde_json::json!({"Rows":[{"Value":1},{"Value":2},{"Value":3}]})
        } else {
            serde_json::json!({"Rows":[{"Value":1},{"Value":3},{"Value":6}]})
        };
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&saved.artifacts[0].bytes)?,
            expected
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&saved.artifacts[1].bytes)?,
            serde_json::json!({"Rows":[{"Value":1},{"Value":2},{"Value":3}]})
        );
        assert!(
            observers.nodes.get() > 0
                && observers.inputs.get() > 0
                && observers.writes.get() > 0
                && observers.traces.get() > 0
        );
        assert_eq!(snapshot(&sandbox)?, before);
    }
    Ok(())
}
