//! Native JSON reading and writing refuse tuple frames until they are
//! supported (#1225), instead of treating a tuple as an object.

use std::path::PathBuf;

use format_json::{JsonFormatError, PreparedSchema};
use ir::{Instance, ItemCountRange, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode, Value};

/// A closed `[string, integer]` tuple of length 0..2.
fn frame() -> SchemaNode {
    SchemaNode::group(
        "Point",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    )
    .with_json_tuple(JsonTupleLayout {
        length: ItemCountRange::new(0, Some(2)),
        tail: JsonTupleTail::Closed,
    })
    .expect("the test frame is canonical")
}

fn document(point: SchemaNode) -> SchemaNode {
    SchemaNode::group(
        "Document",
        vec![SchemaNode::scalar("Label", ScalarType::String), point],
    )
}

/// The same document with the frame's members as an ordinary object.
fn object_document() -> SchemaNode {
    let mut point = frame();
    point.json_tuple = None;
    document(point)
}

fn instance() -> Instance {
    Instance::Group(
        vec![
            ("Label".into(), Instance::Scalar(Value::String("a".into()))),
            (
                "Point".into(),
                Instance::Group(
                    vec![
                        ("item1".into(), Instance::Scalar(Value::String("x".into()))),
                        ("item2".into(), Instance::Scalar(Value::Int(1))),
                    ]
                    .into(),
                ),
            ),
        ]
        .into(),
    )
}

const OBJECT_TEXT: &str = r#"{"Label":"a","Point":{"item1":"x","item2":1}}"#;
const TUPLE_TEXT: &str = r#"{"Label":"a","Point":["x",1]}"#;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-format-json-tuple-refusal-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_refused<T: std::fmt::Debug>(label: &str, result: Result<T, JsonFormatError>) {
    match result {
        Err(JsonFormatError::UnsupportedJsonTupleFrame { schema }) => {
            assert_eq!(schema, "Document", "{label}");
        }
        other => panic!("{label}: expected the tuple frame refusal, got {other:?}"),
    }
}

#[test]
fn every_native_reader_refuses_tuple_frames() {
    let schema = document(frame());
    let dir = TempDir::new("read");
    for text in [OBJECT_TEXT, TUPLE_TEXT] {
        assert_refused("from_str", format_json::from_str(text, &schema));
        assert_refused("from_json5_str", format_json::from_json5_str(text, &schema));
        assert_refused("from_lines", format_json::from_lines(text, &schema));
        let prepared = PreparedSchema::new(schema.clone());
        assert_refused("prepared from_str", prepared.from_str(text));

        let path = dir.0.join("input.json");
        std::fs::write(&path, text).expect("input file");
        assert_refused("read", format_json::read(&path, &schema));
        assert_refused("read_json5", format_json::read_json5(&path, &schema));
        assert_refused("read_lines", format_json::read_lines(&path, &schema));
    }
}

#[test]
fn every_native_writer_refuses_tuple_frames_before_publishing() {
    let schema = document(frame());
    let instance = instance();
    assert_refused("to_string", format_json::to_string(&schema, &instance));
    assert_refused("to_value", format_json::to_value(&schema, &instance));
    assert_refused(
        "to_json5_string",
        format_json::to_json5_string(&schema, &instance),
    );
    assert_refused("to_lines", format_json::to_lines(&schema, &instance));
    let prepared = PreparedSchema::new(schema.clone());
    assert_refused("prepared to_value", prepared.to_value(&instance));

    let dir = TempDir::new("write");
    let path = dir.0.join("output.json");
    assert_refused("write", format_json::write(&path, &schema, &instance));
    assert_refused(
        "write_json5",
        format_json::write_json5(&path, &schema, &instance),
    );
    assert_refused(
        "write_lines",
        format_json::write_lines(&path, &schema, &instance),
    );
    assert!(!path.exists());
}

#[test]
fn frames_are_refused_wherever_they_appear_in_the_schema() {
    // A repeating frame root and a frame below an array of objects.
    let mut root = frame().repeating();
    root.name = "Document".into();
    root.json_object_item_nullable = Some(false);
    let nested = SchemaNode::group(
        "Document",
        vec![SchemaNode::group("Rows", vec![frame()]).repeating()],
    );
    for schema in [root, nested] {
        assert_refused("from_str", format_json::from_str("[]", &schema));
        assert_refused(
            "to_string",
            format_json::to_string(&schema, &Instance::Repeated(Vec::new())),
        );
    }
}

#[test]
fn schemas_without_frames_read_and_write_unchanged() {
    let schema = object_document();
    let instance = instance();
    assert_eq!(
        format_json::from_str(OBJECT_TEXT, &schema).unwrap(),
        instance
    );
    assert_eq!(
        PreparedSchema::new(schema.clone())
            .from_str(OBJECT_TEXT)
            .unwrap(),
        instance
    );
    assert_eq!(
        format_json::to_value(&schema, &instance).unwrap(),
        serde_json::from_str::<serde_json::Value>(OBJECT_TEXT).unwrap()
    );
    let text = format_json::to_string(&schema, &instance).unwrap();
    assert_eq!(format_json::from_str(&text, &schema).unwrap(), instance);
    let lines = format_json::to_lines(&schema, &instance).unwrap();
    assert_eq!(lines, format!("{OBJECT_TEXT}\n"));
}

#[test]
fn json_schema_export_refuses_tuple_frames() {
    let mut root = frame().repeating();
    root.name = "Document".into();
    root.json_object_item_nullable = Some(false);
    for schema in [document(frame()), root] {
        match format_json::json_schema::export(&schema) {
            Err(JsonFormatError::UnsupportedJsonTupleFrameExport { schema }) => {
                assert_eq!(schema, "Document");
            }
            other => panic!("expected the tuple frame export refusal, got {other:?}"),
        }
    }
    let exported = format_json::json_schema::export(&object_document()).unwrap();
    assert_eq!(
        format_json::json_schema::import_str(&exported).unwrap(),
        object_document()
    );
}
