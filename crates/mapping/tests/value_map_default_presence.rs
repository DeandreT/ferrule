use std::{cell::Cell, path::PathBuf};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FunctionId, Graph, Node, Project, Scope, UserFunction, project_file};

const FUNCTION: FunctionId = FunctionId::new(7);
const ENABLED_NODE: &str = r#"{"kind":"value_map","input":0,"table":[[null,"null-key"],["hit","matched"]],"default":{"$value_map_absent":true}}"#;
const DISABLED_NODE: &str = r#"{"kind":"value_map","input":0,"table":[[null,"null-key"],["hit","matched"]],"default":null}"#;
const HAND_PROJECT: &str = r#"{
  "source": {"name":"Input","kind":{"kind":"group","children":[]}},
  "target": {"name":"Output","kind":{"kind":"group","children":[
    {"name":"Result","kind":{"kind":"scalar","ty":"string"}},
    {"name":"Function","kind":{"kind":"scalar","ty":"string"}}
  ]}},
  "source_options":{"json_document":true},
  "target_options":{"json_document":true},
  "graph":{"nodes":{
    "0":{"kind":"const","value":"miss"},
    "1":{"kind":"value_map","input":0,"table":[[null,"null-key"],["hit","matched"]],"default":{"$value_map_absent":true}},
    "2":{"kind":"user_function_call","function":7,"args":[]},
    "3":{"kind":"const","value":null}
  }},
  "user_functions":{"7":{
    "library":"local","name":"absent_default","parameters":[],
    "output_name":"result","output_type":"string",
    "body":{"nodes":{
      "0":{"kind":"const","value":"miss"},
      "1":{"kind":"value_map","input":0,"table":[[null,"null-key"],["hit","matched"]],"default":{"$value_map_absent":true}}
    }},"output":1
  }},
  "root":{"bindings":[{"target_field":"Result","node":1},{"target_field":"Function","node":2}],"children":[]}
}
"#;

struct Retained {
    path: PathBuf,
    next: Cell<u64>,
    complete: bool,
}
impl Retained {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-value-map-default-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self {
            path,
            next: Cell::new(0),
            complete: false,
        }
    }
    fn record(&self, label: &str, value: impl std::fmt::Debug) {
        let next = self.next.get();
        self.next.set(next + 1);
        std::fs::write(
            self.path.join(format!("{next:04}-{label}.txt")),
            format!("{value:#?}\n"),
        )
        .unwrap();
    }
}
impl Drop for Retained {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained default-presence originals: {}",
                self.path.display()
            );
        }
    }
}

fn table() -> Vec<(Value, Value)> {
    vec![
        (Value::Null, Value::String("null-key".into())),
        (Value::String("hit".into()), Value::String("matched".into())),
    ]
}
fn map(default: Option<Value>) -> Node {
    Node::ValueMap {
        input: 0,
        input_type: None,
        table: table(),
        default,
    }
}
fn project(default: Option<Value>) -> Project {
    let body = Graph {
        nodes: [
            (
                0,
                Node::Const {
                    value: Value::String("miss".into()),
                },
            ),
            (1, map(default.clone())),
        ]
        .into(),
    };
    let mut graph = body.clone();
    graph.nodes.insert(
        2,
        Node::UserFunctionCall {
            function: FUNCTION,
            args: vec![],
        },
    );
    graph.nodes.insert(3, Node::Const { value: Value::Null });
    let source_options = mapping::FormatOptions {
        json_document: true,
        ..mapping::FormatOptions::default()
    };
    let target_options = mapping::FormatOptions {
        json_document: true,
        ..mapping::FormatOptions::default()
    };
    Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Result", ScalarType::String),
                SchemaNode::scalar("Function", ScalarType::String),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options,
        target_options,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: [(
            FUNCTION,
            UserFunction {
                library: "local".into(),
                name: "absent_default".into(),
                description: None,
                parameters: vec![],
                output_name: "result".into(),
                output_type: ScalarType::String,
                body,
                output: 1,
            },
        )]
        .into(),
        graph,
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Result".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Function".into(),
                    node: 2,
                },
            ],
            ..Scope::default()
        },
    }
}
fn assert_default(actual: &Option<Value>, expected: &Option<Value>) {
    match (actual, expected) {
        (Some(Value::Float(actual)), Some(Value::Float(expected))) => {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
        _ => assert_eq!(actual, expected),
    }
}
fn assert_map(node: &Node, expected: &Option<Value>) {
    let Node::ValueMap {
        input,
        input_type,
        table: actual_table,
        default,
    } = node
    else {
        panic!("expected ValueMap, observed {node:#?}");
    };
    assert_eq!(*input, 0);
    assert_eq!(*input_type, None);
    assert_eq!(actual_table, &table());
    assert_default(default, expected);
}
fn assert_project(
    actual: &Project,
    expected: &Project,
    default: &Option<Value>,
    retained: &Retained,
) {
    retained.record("complete-reloaded-model", actual);
    assert_map(&actual.graph.nodes[&1], default);
    assert_map(&actual.user_functions[&FUNCTION].body.nodes[&1], default);
    assert!(matches!(
        actual.graph.nodes[&3],
        Node::Const { value: Value::Null }
    ));
    let actual_wire = serde_json::to_vec(actual);
    let expected_wire = serde_json::to_vec(expected);
    retained.record(
        "complete-project-equality-serialization-results",
        (&actual_wire, &expected_wire),
    );
    assert_eq!(actual_wire.unwrap(), expected_wire.unwrap());
}

#[test]
fn enabled_absent_default_literal_node_text_and_bytes_are_distinct_from_disabled() {
    let mut retained = Retained::new();
    let enabled = map(Some(Value::Null));
    let disabled = map(None);
    let enabled_text = serde_json::to_string(&enabled);
    let enabled_bytes = serde_json::to_vec(&enabled);
    let disabled_text = serde_json::to_string(&disabled);
    let enabled_from_text = serde_json::from_str::<Node>(ENABLED_NODE);
    let enabled_from_bytes = serde_json::from_slice::<Node>(ENABLED_NODE.as_bytes());
    retained.record(
        "all-before-first-assert-original-node-results",
        (
            &enabled,
            &disabled,
            ENABLED_NODE,
            DISABLED_NODE,
            &enabled_text,
            &enabled_bytes,
            &disabled_text,
            &enabled_from_text,
            &enabled_from_bytes,
        ),
    );
    assert_eq!(enabled_text.unwrap(), ENABLED_NODE);
    assert_eq!(enabled_bytes.unwrap(), ENABLED_NODE.as_bytes());
    assert_eq!(disabled_text.unwrap(), DISABLED_NODE);
    assert_map(&enabled_from_text.unwrap(), &Some(Value::Null));
    assert_map(&enabled_from_bytes.unwrap(), &Some(Value::Null));
    retained.complete = true;
}

#[test]
fn main_and_function_enabled_absence_survive_complete_public_project_roundtrips() {
    let mut retained = Retained::new();
    let expected = project(Some(Value::Null));
    let ordinary_text = serde_json::to_string(&expected);
    let ordinary_bytes = serde_json::to_vec(&expected);
    let file_text = project_file::encode_pretty(&expected);
    retained.record(
        "original-project-and-all-encoding-results",
        (&expected, &ordinary_text, &ordinary_bytes, &file_text),
    );
    let ordinary_text = ordinary_text.unwrap();
    let ordinary_bytes = ordinary_bytes.unwrap();
    let file_text = file_text.unwrap();
    let ordinary_from_text = serde_json::from_str::<Project>(&ordinary_text);
    let ordinary_from_bytes = serde_json::from_slice::<Project>(&ordinary_bytes);
    let public_from_text = project_file::decode_str(&file_text);
    let public_from_bytes = project_file::decode_bytes(file_text.as_bytes());
    retained.record(
        "four-complete-roundtrip-results-before-comparison",
        (
            &ordinary_from_text,
            &ordinary_from_bytes,
            &public_from_text,
            &public_from_bytes,
        ),
    );
    let literal_text = project_file::decode_str(HAND_PROJECT);
    let literal_bytes = project_file::decode_bytes(HAND_PROJECT.as_bytes());
    let envelope = format!(
        r#"{{"__ferrule_file":{{"kind":"project","version":2,"float_bits":{{}}}},"document":{HAND_PROJECT}}}"#
    );
    let envelope_text = project_file::decode_str(&envelope);
    let envelope_bytes = project_file::decode_bytes(envelope.as_bytes());
    retained.record(
        "full-independent-hand-design-and-version2-results",
        (
            HAND_PROJECT,
            &envelope,
            &literal_text,
            &literal_bytes,
            &envelope_text,
            &envelope_bytes,
        ),
    );
    for actual in [
        ordinary_from_text.unwrap(),
        ordinary_from_bytes.unwrap(),
        public_from_text.unwrap(),
        public_from_bytes.unwrap(),
    ] {
        assert_project(&actual, &expected, &Some(Value::Null), &retained);
    }
    for actual in [
        literal_text.unwrap(),
        literal_bytes.unwrap(),
        envelope_text.unwrap(),
        envelope_bytes.unwrap(),
    ] {
        assert_project(&actual, &expected, &Some(Value::Null), &retained);
    }
    let legacy = HAND_PROJECT.replace(r#"{"$value_map_absent":true}"#, "null");
    let legacy_text = project_file::decode_str(&legacy);
    let legacy_bytes = project_file::decode_bytes(legacy.as_bytes());
    retained.record(
        "complete-hand-legacy-project-and-results",
        (&legacy, &legacy_text, &legacy_bytes),
    );
    for actual in [legacy_text.unwrap(), legacy_bytes.unwrap()] {
        assert_project(&actual, &project(None), &None, &retained);
    }
    assert_eq!(
        ordinary_text.matches(r#""$value_map_absent":true"#).count(),
        2
    );
    assert!(
        !file_text.contains("__ferrule_file"),
        "ordinary finite model needs no envelope"
    );
    // File markers are local to the defaults: a table Null and Const Null stay plain null.
    assert!(ordinary_text.contains(r#""table":[[null,"null-key"]"#));
    assert!(ordinary_text.contains(r#""3":{"kind":"const","value":null}"#));
    retained.complete = true;
}

#[test]
fn disabled_null_and_missing_defaults_keep_legacy_meaning_and_other_values_keep_wire() {
    let mut retained = Retained::new();
    for wire in [
        DISABLED_NODE,
        r#"{"kind":"value_map","input":0,"table":[[null,"null-key"],["hit","matched"]]}"#,
    ] {
        let text = serde_json::from_str::<Node>(wire);
        let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
        retained.record(
            "legacy-wire-and-both-original-results",
            (wire, &text, &bytes),
        );
        for node in [text.unwrap(), bytes.unwrap()] {
            assert_map(&node, &None);
            let encoded = serde_json::to_string(&node);
            retained.record("legacy-complete-reserialize-result", &encoded);
            assert_eq!(encoded.unwrap(), DISABLED_NODE);
        }
    }
    for (default, suffix) in [
        (None, "null"),
        (Some(Value::json_null()), r#"{"$json_null":true}"#),
        (Some(Value::xml_nil()), r#"{"$xml_nil":true}"#),
        (Some(Value::String(String::new())), r#""""#),
        (
            Some(Value::String(r#"{"$value_map_absent":true}"#.into())),
            r#""{\"$value_map_absent\":true}""#,
        ),
        (Some(Value::Int(i64::MIN)), "-9223372036854775808"),
        (
            Some(Value::Float(f64::from_bits(0x433fffffffffffff))),
            "9007199254740991.0",
        ),
        (
            Some(Value::Float(f64::from_bits(0x8000000000000000))),
            "-0.0",
        ),
        (Some(Value::Bool(false)), "false"),
    ] {
        let expected_node = format!(
            r#"{{"kind":"value_map","input":0,"table":[[null,"null-key"],["hit","matched"]],"default":{suffix}}}"#
        );
        let node = map(default.clone());
        let wire = serde_json::to_string(&node);
        let node_text = serde_json::from_str::<Node>(&expected_node);
        let node_bytes = serde_json::from_slice::<Node>(expected_node.as_bytes());
        let original = project(default.clone());
        let encoded = project_file::encode_pretty(&original);
        retained.record(
            "all-unchanged-default-wire-and-public-encode-results",
            (
                &default,
                &expected_node,
                &wire,
                &node_text,
                &node_bytes,
                &original,
                &encoded,
            ),
        );
        assert_eq!(wire.unwrap(), expected_node);
        assert_map(&node_text.unwrap(), &default);
        assert_map(&node_bytes.unwrap(), &default);
        let encoded = encoded.unwrap();
        let text = project_file::decode_str(&encoded);
        let bytes = project_file::decode_bytes(encoded.as_bytes());
        retained.record(
            "complete-unchanged-default-public-decode-results",
            (&text, &bytes),
        );
        assert_project(&text.unwrap(), &original, &default, &retained);
        assert_project(&bytes.unwrap(), &original, &default, &retained);
    }
    retained.complete = true;
}

#[test]
fn malformed_absent_default_markers_reject_and_cannot_change_global_value_wire() {
    let mut retained = Retained::new();
    for marker in [
        r#"{"$value_map_absent":false}"#,
        r#"{"$value_map_absent":null}"#,
        r#"{"$value_map_absent":0}"#,
        r#"{"$value_map_absent":"true"}"#,
        r#"{"$value_map_absent":[]}"#,
        r#"{"$value_map_absent":true,"extra":0}"#,
        r#"{"$value_map_absent":true,"$json_null":true}"#,
        r#"{"$value_map_absent":true,"$xml_nil":true}"#,
        r#"{"$value_map_absent":true,"$value_map_absent":true}"#,
        r#"{}"#,
    ] {
        let wire = format!(r#"{{"kind":"value_map","input":0,"table":[],"default":{marker}}}"#);
        let text = serde_json::from_str::<Node>(&wire);
        let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
        let design = HAND_PROJECT.replace(r#"{"$value_map_absent":true}"#, marker);
        let project_text = project_file::decode_str(&design);
        let project_bytes = project_file::decode_bytes(design.as_bytes());
        retained.record(
            "malformed-node-and-public-project-marker-complete-results",
            (&wire, &text, &bytes, &design, &project_text, &project_bytes),
        );
        assert!(text.is_err() && bytes.is_err(), "{wire}");
        assert!(project_text.is_err() && project_bytes.is_err(), "{design}");
    }
    for wire in [
        r#"{"kind":"const","value":{"$value_map_absent":true}}"#,
        r#"{"kind":"value_map","input":0,"table":[[{"$value_map_absent":true},1]],"default":null}"#,
        r#"{"kind":"value_map","input":0,"table":[[1,{"$value_map_absent":true}]],"default":null}"#,
        r#"{"kind":"raise","message":{"$value_map_absent":true}}"#,
    ] {
        let text = serde_json::from_str::<Node>(wire);
        let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
        retained.record("out-of-default-marker-results", (wire, &text, &bytes));
        assert!(text.is_err() && bytes.is_err());
    }
    let global_absence = serde_json::to_string(&Value::Null);
    let unrelated_option = serde_json::to_string(&Some(Value::Null));
    let global_marker = serde_json::from_str::<Value>(r#"{"$value_map_absent":true}"#);
    retained.record(
        "global-value-and-unrelated-option-complete-results",
        (&global_absence, &unrelated_option, &global_marker),
    );
    assert_eq!(global_absence.unwrap(), "null");
    assert_eq!(unrelated_option.unwrap(), "null");
    assert!(global_marker.is_err());
    retained.complete = true;
}
