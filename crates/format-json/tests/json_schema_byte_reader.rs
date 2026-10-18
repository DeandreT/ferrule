use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use format_json::json_schema::{JsonSchemaByteReader, import_with_byte_reader};
use ir::{ScalarType, SchemaKind};

#[derive(Default)]
struct MemoryReader {
    documents: BTreeMap<PathBuf, Arc<[u8]>>,
    requests: RefCell<Vec<PathBuf>>,
}

impl JsonSchemaByteReader for MemoryReader {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        self.requests.borrow_mut().push(path.to_path_buf());
        self.documents
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no supplied document"))
    }
}

fn root() -> PathBuf {
    // This identity never needs to exist on disk.
    std::env::temp_dir().join("ferrule-memory-only-json-schema")
}

#[test]
fn supplied_root_and_repeated_relative_dependencies_need_no_filesystem() {
    let root = root();
    let input = root.join("input.json");
    let types = root.join("types.json");
    let reader = Rc::new(MemoryReader {
        documents: BTreeMap::from([
            (input.clone(), Arc::from(br#"{"type":"object","additionalProperties":false,"properties":{"first":{"$ref":"nested/../types.json#/$defs/count"},"second":{"$ref":"types.json#/$defs/count"}}}"#.as_slice())),
            (types.clone(), Arc::from(br#"{"$defs":{"count":{"type":"integer","minimum":2}}}"#.as_slice())),
        ]),
        ..MemoryReader::default()
    });
    let schema = import_with_byte_reader(&input, &root, reader.clone()).unwrap();
    for field in ["first", "second"] {
        assert_eq!(
            schema.child(field).unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        );
    }
    assert!(format_json::from_str(r#"{"first":2,"second":5}"#, &schema).is_ok());
    assert!(format_json::from_str(r#"{"first":1,"second":5}"#, &schema).is_err());
    assert_eq!(*reader.requests.borrow(), vec![input, types]);
}

#[test]
fn unsafe_references_are_refused_before_requesting_a_dependency() {
    let root = root();
    let input = root.join("input.json");
    for reference in [
        "../outside.json",
        "%2e%2e/outside.json",
        "https://example.test/a.json",
        "C:/a.json",
        "/a.json",
        "\\outside.json",
        "folder/a.json:stream",
        "bad%00.json",
        "types.json?query",
    ] {
        let bytes = serde_json::to_vec(&serde_json::json!({"$ref": reference})).unwrap();
        let reader = Rc::new(MemoryReader {
            documents: BTreeMap::from([(input.clone(), Arc::from(bytes))]),
            ..MemoryReader::default()
        });
        assert!(
            import_with_byte_reader(&input, &root, reader.clone()).is_err(),
            "{reference}"
        );
        assert_eq!(
            *reader.requests.borrow(),
            vec![input.clone()],
            "{reference}"
        );
    }
}

#[test]
fn missing_supplied_dependency_propagates_reader_error() {
    let root = root();
    let input = root.join("input.json");
    let reader = Rc::new(MemoryReader {
        documents: BTreeMap::from([(
            input.clone(),
            Arc::from(br#"{"$ref":"missing.json"}"#.as_slice()),
        )]),
        ..MemoryReader::default()
    });
    let error = import_with_byte_reader(&input, &root, reader.clone()).unwrap_err();
    assert!(
        error.to_string().contains("no supplied document"),
        "{error}"
    );
    assert_eq!(
        *reader.requests.borrow(),
        vec![input, root.join("missing.json")]
    );
}

#[test]
fn invalid_root_identity_never_reaches_the_reader() {
    let reader = Rc::new(MemoryReader::default());
    let root = root();
    for (path, authority) in [
        (root.join("input.json"), PathBuf::from("relative")),
        (root.join("../outside.json"), root.clone()),
        (root.clone(), root.clone()),
    ] {
        assert!(import_with_byte_reader(&path, &authority, reader.clone()).is_err());
    }
    assert!(reader.requests.borrow().is_empty());
}

#[test]
fn supplied_dependencies_retain_the_document_count_limit() {
    let root = root();
    let input = root.join("input.json");
    let mut properties = serde_json::Map::new();
    let mut reader = MemoryReader::default();
    for index in 0..256 {
        let name = format!("child-{index}.json");
        properties.insert(format!("value{index}"), serde_json::json!({"$ref":name}));
        reader.documents.insert(
            root.join(name),
            Arc::from(br#"{"type":"string"}"#.as_slice()),
        );
    }
    reader.documents.insert(
        input.clone(),
        Arc::from(
            serde_json::to_vec(&serde_json::json!({"type":"object","properties":properties}))
                .unwrap(),
        ),
    );
    let reader = Rc::new(reader);
    assert!(matches!(
        import_with_byte_reader(&input, &root, reader.clone()),
        Err(format_json::JsonFormatError::SchemaResourceLimit {
            kind: "documents",
            limit: 256
        })
    ));
    assert_eq!(reader.requests.borrow().len(), 256);
}
