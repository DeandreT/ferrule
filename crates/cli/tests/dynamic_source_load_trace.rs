use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_dynamic_trace_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Default)]
struct Requests(RefCell<Vec<(String, String)>>);

impl cli::TraceSink for Requests {
    fn record(&self, _: cli::TraceEvent) {}
    fn dynamic_source_load_requested(&self, source: &str, path: &str) {
        self.0.borrow_mut().push((source.into(), path.into()));
    }
}

fn document_schema() -> SchemaNode {
    SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Item",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    )
}

fn project(primary: bool, dynamic: bool) -> Project {
    let frame = if primary {
        vec!["Item".into()]
    } else {
        vec!["document".into(), "Item".into()]
    };
    Project {
        source: document_schema(),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                    .repeating(),
            ],
        ),
        source_path: Some("primary.xml".into()),
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: if primary {
            Vec::new()
        } else {
            vec![NamedSource {
                name: "document".into(),
                path: if dynamic {
                    String::new()
                } else {
                    "secondary.xml".into()
                },
                schema: document_schema(),
                options: Default::default(),
                dynamic_path: dynamic.then_some(DynamicSourcePath::legacy(0, Vec::new())),
            }]
        },
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::String("secondary.xml".into()),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(frame.clone()),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(frame),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

const HELLO: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Value>Hello</Value>\n  </Row>\n</Output>";

fn run(
    project: &Project,
    root: &Path,
    output: &Path,
    sink: Option<&dyn cli::TraceSink>,
) -> anyhow::Result<cli::RunOutcome> {
    cli::run_project_value_with_options(
        project,
        &root.join("project.json"),
        &cli::RunOptions {
            output_path: Some(output),
            trace_sink: sink,
            target: Some(cli::TargetSelection::Primary),
            ..cli::RunOptions::default()
        },
    )
}

#[test]
fn filesystem_primary_static_and_dynamic_outputs_preserve_callback_scope() {
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("primary.xml"),
        "<Document><Item><Value>Hello</Value></Item></Document>",
    )
    .unwrap();
    std::fs::write(
        dir.0.join("secondary.xml"),
        "<Document><Item><Value>Hello</Value></Item></Document>",
    )
    .unwrap();
    for (name, primary, dynamic, expected_requests) in [
        ("primary", true, false, Vec::new()),
        ("static", false, false, Vec::new()),
        (
            "dynamic",
            false,
            true,
            vec![("document".into(), "secondary.xml".into())],
        ),
    ] {
        let project = project(primary, dynamic);
        let requests = Requests::default();
        let plain_path = dir.0.join(format!("{name}-plain.xml"));
        let trace_path = dir.0.join(format!("{name}-observed.xml"));
        let plain = run(&project, &dir.0, &plain_path, None);
        let observed = run(&project, &dir.0, &trace_path, Some(&requests));
        eprintln!("R04/R05 complete returns plain={plain:?} observed={observed:?}");
        let plain_bytes = std::fs::read(&plain_path).unwrap();
        let observed_bytes = std::fs::read(&trace_path).unwrap();
        eprintln!(
            "R04 {name} plain={plain:?} observed={observed:?} plain_bytes={plain_bytes:?} observed_bytes={observed_bytes:?} requests={:?}",
            requests.0.borrow()
        );
        assert_eq!(plain.unwrap().records_written, 1);
        assert_eq!(observed.unwrap().records_written, 1);
        assert_eq!(plain_bytes, HELLO);
        assert_eq!(observed_bytes, HELLO);
        assert_eq!(*requests.0.borrow(), expected_requests);
    }
}

#[test]
fn unused_dynamic_source_has_no_loader_request() {
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("primary.xml"),
        "<Document><Item><Value>Hello</Value></Item></Document>",
    )
    .unwrap();
    let mut unused = project(true, false);
    unused.extra_sources = project(false, true).extra_sources;
    unused.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("missing.xml".into()),
        },
    );
    let requests = Requests::default();
    let output = dir.0.join("unused.xml");
    let result = run(&unused, &dir.0, &output, Some(&requests));
    eprintln!("R05 unused complete return={result:?}");
    let bytes = std::fs::read(&output).unwrap();
    eprintln!(
        "R05 unused result={result:?} requests={:?} bytes={bytes:?}",
        requests.0.borrow()
    );
    assert_eq!(result.unwrap().records_written, 1);
    assert_eq!(bytes, HELLO);
    assert!(requests.0.borrow().is_empty());
}

#[test]
fn json_trace_load_event_is_deterministic_and_excludes_primary_static_reads() {
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("primary.xml"),
        "<Document><Item><Value>ignored</Value></Item></Document>",
    )
    .unwrap();
    std::fs::write(
        dir.0.join("secondary.xml"),
        "<Document><Item><Value>Hello</Value></Item></Document>",
    )
    .unwrap();
    let project = project(false, true);
    let output = dir.0.join("output.xml");
    let mut bodies = Vec::new();
    for label in ["first", "repeat"] {
        let trace_path = dir.0.join(format!("{label}.jsonl"));
        let trace = cli::JsonTraceFile::create(&trace_path).unwrap();
        let result = run(&project, &dir.0, &output, Some(&trace));
        eprintln!("R04/R06 {label} result={result:?}");
        let result = result.unwrap();
        let finished = trace.finish(&result.artifacts);
        eprintln!("R04/R06 complete finish={finished:?}");
        let bytes = std::fs::read(&trace_path).unwrap();
        let records = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        eprintln!("R04/R06 {label} finish={finished:?} bytes={bytes:?} records={records:?}");
        finished.unwrap();
        let requests = records
            .iter()
            .filter(|record| record["event"]["kind"] == "dynamic_source_load_requested")
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["event"],
            serde_json::json!({
                "kind": "dynamic_source_load_requested", "source": "document", "path": "secondary.xml"
            })
        );
        for (sequence, record) in records.iter().enumerate() {
            assert_eq!(record["schema_version"], 5);
            assert_eq!(record["sequence"], sequence);
        }
        assert_eq!(std::fs::read(&output).unwrap(), HELLO);
        bodies.push(bytes);
    }
    assert_eq!(bodies[0], bodies[1]);
}

#[test]
fn null_json_null_and_non_string_paths_never_enter_loader() {
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("primary.xml"),
        "<Document><Item><Value>ignored</Value></Item></Document>",
    )
    .unwrap();
    for (label, value, refused) in [
        ("null", Value::Null, false),
        ("json_null", Value::json_null(), false),
        ("integer", Value::Int(7), true),
    ] {
        let mut project = project(false, true);
        project.graph.nodes.insert(0, Node::Const { value });
        let plain_path = dir.0.join(format!("{label}-plain.xml"));
        let observed_path = dir.0.join(format!("{label}-observed.xml"));
        std::fs::write(&plain_path, b"previous").unwrap();
        std::fs::write(&observed_path, b"previous").unwrap();
        let requests = Requests::default();
        let plain = run(&project, &dir.0, &plain_path, None);
        let observed = run(&project, &dir.0, &observed_path, Some(&requests));
        eprintln!("R04/R05 complete returns plain={plain:?} observed={observed:?}");
        let plain_bytes = std::fs::read(&plain_path).unwrap();
        let observed_bytes = std::fs::read(&observed_path).unwrap();
        eprintln!(
            "R05 {label} plain={plain:?} observed={observed:?} plain_bytes={plain_bytes:?} observed_bytes={observed_bytes:?} requests={:?}",
            requests.0.borrow()
        );
        assert!(requests.0.borrow().is_empty());
        assert_eq!(plain_bytes, observed_bytes);
        if refused {
            for result in [plain, observed] {
                let error = result.unwrap_err();
                assert!(matches!(error.downcast_ref::<engine::EngineError>(),
                    Some(engine::EngineError::DynamicSourcePath { source_name, found })
                    if source_name == "document" && *found == "int"
                ));
            }
            assert_eq!(observed_bytes, b"previous");
        } else {
            plain.unwrap();
            observed.unwrap();
            let output = format_xml::read(&observed_path, &project.target).unwrap();
            let expected = ir::Instance::Group(
                vec![("Row".into(), ir::Instance::Repeated(Vec::new()))].into(),
            );
            eprintln!("R05 {label} whole_tree={output:?}");
            assert_eq!(output, expected);
        }
    }
}
