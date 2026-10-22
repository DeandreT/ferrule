use super::*;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_request_writer_{}_{}",
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

fn node_event() -> TraceEvent {
    TraceEvent::NodeValue {
        node: 7,
        positions: Vec::new(),
        value: TraceValue {
            value_type: "string",
            preview: "ok".into(),
            truncated: false,
        },
    }
}

fn snapshot(trace: &JsonTraceFile) -> (u64, u64, u64, Option<String>) {
    let state = trace.state.borrow();
    (
        state.sequence,
        state.requests,
        state.request_bytes,
        state.error.clone(),
    )
}

fn lines(path: &Path) -> Vec<JsonValue> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn request_identity_roundtrips_exactly_in_shared_sequence() {
    let dir = TestDir::new();
    let path = dir.0.join("trace.jsonl");
    let trace = JsonTraceFile::create(&path).unwrap();
    trace.record(node_event());
    trace.dynamic_source_load_requested("雪\"\\", "./雪\"\\\n.xml");
    trace.record(node_event());
    let before_finish = snapshot(&trace);
    let finished = trace.finish(&[]);
    eprintln!("R06 complete finish={finished:?}");
    let bytes = std::fs::read(&path).unwrap();
    let records = lines(&path);
    eprintln!(
        "R06 sequence={before_finish:?} finish={finished:?} bytes={bytes:?} records={records:?}"
    );
    finished.unwrap();
    assert_eq!(records.len(), 3);
    for (sequence, record) in records.iter().enumerate() {
        assert_eq!(record["schema_version"], 5);
        assert_eq!(record["sequence"], sequence);
    }
    assert_eq!(records[0]["event"]["kind"], "node_value");
    assert_eq!(records[2]["event"]["kind"], "node_value");
    assert_eq!(
        records[1]["event"],
        json!({
            "kind": "dynamic_source_load_requested", "source": "雪\"\\", "path": "./雪\"\\\n.xml"
        })
    );
    assert_eq!(before_finish.0, 3);
    assert_eq!(before_finish.1, 1);
    assert!(before_finish.3.is_none());
}

#[test]
fn borrowed_identity_limits_use_utf8_bytes_and_fail_stickily() {
    let dir = TestDir::new();
    for (label, source, path, expected_error) in [
        ("source_below", "a", "x", None),
        ("source_equal", "é", "x", None),
        (
            "source_above",
            "雪",
            "x",
            Some("dynamic source request source exceeds its byte limit"),
        ),
        ("path_below", "x", "a", None),
        ("path_equal", "x", "é", None),
        (
            "path_above",
            "x",
            "雪",
            Some("dynamic source request path exceeds its byte limit"),
        ),
    ] {
        let destination = dir.0.join(format!("{label}.jsonl"));
        std::fs::write(&destination, b"previous\n").unwrap();
        let trace = JsonTraceFile::create_with_request_limits(
            &destination,
            RequestLimits {
                source_bytes: 2,
                path_bytes: 2,
                ..RequestLimits::default()
            },
        )
        .unwrap();
        trace.dynamic_source_load_requested(source, path);
        let first = snapshot(&trace);
        if expected_error.is_some() {
            trace.dynamic_source_load_requested("x", "x");
            trace.record(node_event());
        }
        let last = snapshot(&trace);
        let result = trace.finish(&[]);
        let bytes = std::fs::read(&destination).unwrap();
        eprintln!("R06 {label} first={first:?} last={last:?} result={result:?} bytes={bytes:?}");
        assert_eq!(first.3.as_deref(), expected_error);
        if let Some(error) = expected_error {
            assert_eq!(first, last);
            assert!(result.unwrap_err().to_string().contains(error));
            assert_eq!(bytes, b"previous\n");
        } else {
            result.unwrap();
            assert_eq!(first.1, 1);
        }
    }
    assert!(std::fs::read_dir(&dir.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("trace-stage")
    }));
}

// Independent exact byte oracle: ASCII identity, sequence zero, sorted object keys.
const REQUEST: &[u8] = b"{\"event\":{\"kind\":\"dynamic_source_load_requested\",\"path\":\"b\",\"source\":\"a\"},\"schema_version\":5,\"sequence\":0}\n";

#[test]
fn serialized_line_and_total_request_byte_limits_include_lf() {
    let dir = TestDir::new();
    for (label, line_bytes, bytes, allowed) in [
        ("line_below", REQUEST.len() + 1, 1024, true),
        ("line_equal", REQUEST.len(), 1024, true),
        ("line_above", REQUEST.len() - 1, 1024, false),
        ("bytes_below", 65536, REQUEST.len() as u64 + 1, true),
        ("bytes_equal", 65536, REQUEST.len() as u64, true),
        ("bytes_above", 65536, REQUEST.len() as u64 - 1, false),
    ] {
        let destination = dir.0.join(format!("{label}.jsonl"));
        std::fs::write(&destination, b"previous\n").unwrap();
        let trace = JsonTraceFile::create_with_request_limits(
            &destination,
            RequestLimits {
                line_bytes,
                bytes,
                ..RequestLimits::default()
            },
        )
        .unwrap();
        trace.dynamic_source_load_requested("a", "b");
        let raw = snapshot(&trace);
        let result = trace.finish(&[]);
        let actual = std::fs::read(&destination).unwrap();
        eprintln!("R06 {label} raw={raw:?} result={result:?} bytes={actual:?}");
        if allowed {
            result.unwrap();
            assert_eq!(actual, REQUEST);
            assert_eq!(raw.2, REQUEST.len() as u64);
        } else {
            assert!(result.is_err());
            assert_eq!(actual, b"previous\n");
            assert_eq!(raw.1, 0);
            assert_eq!(raw.2, 0);
        }
    }
}

#[test]
fn request_count_and_cumulative_bytes_are_independent_of_graph_events() {
    let dir = TestDir::new();
    for (label, requests, bytes, permitted) in [
        ("count_below", 3, 1024, 2),
        ("count_equal", 2, 1024, 2),
        ("count_above", 1, 1024, 1),
        ("total_equal", 3, REQUEST.len() as u64 * 2, 2),
        ("total_above", 3, REQUEST.len() as u64 * 2 - 1, 1),
    ] {
        let destination = dir.0.join(format!("{label}.jsonl"));
        std::fs::write(&destination, b"previous\n").unwrap();
        let trace = JsonTraceFile::create_with_request_limits(
            &destination,
            RequestLimits {
                requests,
                bytes,
                ..RequestLimits::default()
            },
        )
        .unwrap();
        trace.record(node_event());
        trace.dynamic_source_load_requested("a", "b");
        trace.record(node_event());
        trace.dynamic_source_load_requested("a", "b");
        let raw = snapshot(&trace);
        let result = trace.finish(&[]);
        let actual = std::fs::read(&destination).unwrap();
        eprintln!("R06 {label} raw={raw:?} result={result:?} bytes={actual:?}");
        assert_eq!(raw.1, permitted);
        if permitted == 2 {
            result.unwrap();
            assert_eq!(lines(&destination).len(), 4);
        } else {
            assert!(result.is_err());
            assert_eq!(actual, b"previous\n");
        }
    }
}

#[test]
fn sequence_and_writer_failure_refuse_complete_publication() {
    let dir = TestDir::new();
    for sequence_overflow in [true, false] {
        let destination = dir.0.join(format!("{sequence_overflow}.jsonl"));
        std::fs::write(&destination, b"previous\n").unwrap();
        let trace = JsonTraceFile::create(&destination).unwrap();
        if sequence_overflow {
            trace.state.borrow_mut().sequence = u64::MAX;
        } else {
            drop(trace.state.borrow_mut().writer.take());
        }
        trace.dynamic_source_load_requested("a", "b");
        let first = snapshot(&trace);
        trace.record(node_event());
        trace.dynamic_source_load_requested("later", "later");
        let last = snapshot(&trace);
        let result = trace.finish(&[]);
        let actual = std::fs::read(&destination).unwrap();
        eprintln!(
            "R06 overflow={sequence_overflow} first={first:?} last={last:?} result={result:?} bytes={actual:?}"
        );
        assert_eq!(first, last);
        assert_eq!(first.1, 0);
        assert_eq!(
            first.3.as_deref(),
            Some(if sequence_overflow {
                "trace event sequence overflowed"
            } else {
                "JSON trace writer was already finalized"
            })
        );
        assert!(result.is_err());
        assert_eq!(actual, b"previous\n");
    }
}

#[test]
fn request_record_does_not_bypass_output_collision_guard() {
    let dir = TestDir::new();
    let destination = dir.0.join("output.xml");
    std::fs::write(&destination, b"mapping output").unwrap();
    let trace = JsonTraceFile::create(&destination).unwrap();
    trace.dynamic_source_load_requested("a", "b");
    let raw = snapshot(&trace);
    let result = trace.finish(&[WrittenOutput {
        name: "primary".into(),
        records_written: 1,
        path: destination.clone(),
    }]);
    let bytes = std::fs::read(&destination).unwrap();
    eprintln!("R06 collision raw={raw:?} result={result:?} bytes={bytes:?}");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("also a mapped output")
    );
    assert_eq!(bytes, b"mapping output");
}

#[test]
fn default_identity_boundaries_admit_exact_4096_utf8_bytes() {
    for length in [4095, 4096, 4097] {
        let identity = "a".repeat(length);
        for source_field in [true, false] {
            let (source, path) = if source_field {
                (identity.as_str(), "b")
            } else {
                ("a", identity.as_str())
            };
            let result = request_line(0, source, path, RequestLimits::default());
            eprintln!(
                "R06 default_bound length={length} source={source_field} input={identity:?} result={result:?}"
            );
            if length <= 4096 {
                let bytes = result.unwrap();
                assert!(bytes.len() <= 65536);
                assert_eq!(bytes.last(), Some(&b'\n'));
                let record: JsonValue = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(record["event"]["source"], source);
                assert_eq!(record["event"]["path"], path);
            } else {
                assert_eq!(
                    result.unwrap_err(),
                    if source_field {
                        "dynamic source request source exceeds its byte limit"
                    } else {
                        "dynamic source request path exceeds its byte limit"
                    }
                );
            }
        }
    }
}

#[test]
fn writer_refusal_does_not_change_mapping_result_or_published_output() {
    use ir::{ScalarType, SchemaNode, Value};
    use mapping::{
        Binding, DynamicSourcePath, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
    };
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
    let schema = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Item",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let project = Project {
        source: schema.clone(),
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
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema,
            options: Default::default(),
            dynamic_path: Some(DynamicSourcePath::legacy(0, Vec::new())),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("secondary.xml".into()),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["document".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let destination = dir.0.join("trace.jsonl");
    let output = dir.0.join("output.xml");
    std::fs::write(&destination, b"previous trace\n").unwrap();
    let trace = JsonTraceFile::create_with_request_limits(
        &destination,
        RequestLimits {
            requests: 0,
            ..RequestLimits::default()
        },
    )
    .unwrap();
    let mapped = crate::run_project_value_with_options(
        &project,
        &dir.0.join("project.json"),
        &crate::RunOptions {
            output_path: Some(&output),
            trace_sink: Some(&trace),
            ..crate::RunOptions::default()
        },
    );
    let state = snapshot(&trace);
    eprintln!("R06 complete map return={mapped:?} writer={state:?}");
    let output_bytes = std::fs::read(&output).unwrap();
    eprintln!("R06 map={mapped:?} writer={state:?} output_bytes={output_bytes:?}");
    let mapped = mapped.unwrap();
    let finished = trace.finish(&mapped.artifacts);
    let trace_bytes = std::fs::read(&destination).unwrap();
    eprintln!("R06 finish={finished:?} trace_bytes={trace_bytes:?}");
    assert_eq!(mapped.records_written, 1);
    assert_eq!(output_bytes, b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Value>Hello</Value>\n  </Row>\n</Output>");
    assert_eq!(
        state.3.as_deref(),
        Some("dynamic source request count exceeds its limit")
    );
    assert!(finished.is_err());
    assert_eq!(trace_bytes, b"previous trace\n");
}
