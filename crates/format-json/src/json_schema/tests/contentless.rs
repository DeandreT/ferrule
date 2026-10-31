use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaKind};

use super::super::{import, import_str, import_with_root};
use crate::{ContentlessSchemaReason, JsonFormatError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-contentless-json-schema-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, std::io::Error> {
        let path = self.0.join(name);
        std::fs::write(&path, bytes)?;
        Ok(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn reason(result: Result<ir::SchemaNode, JsonFormatError>) -> Option<ContentlessSchemaReason> {
    match result {
        Err(JsonFormatError::ContentlessSchema { reason, .. }) => Some(reason),
        _ => None,
    }
}

#[test]
fn non_schema_values_reject_at_the_root_and_in_nested_positions() {
    for (text, found) in [
        ("[1,2,3]", "an array"),
        (r#""text""#, "a string"),
        ("5", "a number"),
        ("null", "null"),
        (
            r#"{"type":"object","properties":{"Value":[1,2,3]}}"#,
            "an array",
        ),
        (
            r#"{"type":"array","items":{"type":"object","properties":{"Value":7}}}"#,
            "a number",
        ),
    ] {
        assert_eq!(
            reason(import_str(text)),
            Some(ContentlessSchemaReason::NotASchema { found }),
            "{text}"
        );
    }
    let Err(error) = import_str("[1,2,3]") else {
        panic!("an array is not a schema");
    };
    assert_eq!(
        error.to_string(),
        "JSON Schema `root` describes no JSON value: a schema must be an object or a boolean, but this one is an array"
    );
}

#[test]
fn contentless_root_reference_chains_reject() {
    for text in [
        r##"{"$ref":"#"}"##,
        r##"{"title":"Loop","$ref":"#/$defs/a","$defs":{"a":{"$ref":"#/$defs/b"},"b":{"$ref":"#/$defs/a"}}}"##,
        r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/a","definitions":{"a":{"$ref":"#/definitions/a"}}}"##,
    ] {
        assert_eq!(
            reason(import_str(text)),
            Some(ContentlessSchemaReason::ReferenceCycle),
            "{text}"
        );
    }
    assert_eq!(
        reason(import_str(r##"{"$ref":"#/$defs/missing"}"##)),
        Some(ContentlessSchemaReason::UnresolvedReference)
    );
    let Err(error) = import_str(r##"{"$ref":"#"}"##) else {
        panic!("a self reference has no content");
    };
    assert_eq!(
        error.to_string(),
        "JSON Schema `root` describes no JSON value: its `$ref` chain is a cycle that never reaches a schema with content"
    );

    // A root reference chain that reaches content, and nested cycles, keep
    // their existing meaning.
    let reached = import_str(
        r##"{"$ref":"#/$defs/Input","$defs":{"Input":{"title":"Input","type":"object","properties":{"Value":{"type":"string"},"Next":{"$ref":"#/$defs/Loop"}}},"Loop":{"$ref":"#/$defs/Loop"}}}"##,
    )
    .unwrap();
    assert_eq!(reached.name, "Input");
    for field in ["Value", "Next"] {
        assert_eq!(
            reached.child(field).map(|child| &child.kind),
            Some(&SchemaKind::Scalar {
                ty: ScalarType::String
            })
        );
    }
}

#[test]
fn cross_file_reference_cycle_without_content_rejects() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    for (index, next) in [(0, 1), (1, 2), (2, 0)] {
        directory.write(
            &format!("c{index}.schema.json"),
            format!(r#"{{"$ref":"c{next}.schema.json"}}"#).as_bytes(),
        )?;
    }
    let root = directory.0.join("c0.schema.json");
    assert_eq!(
        reason(import(&root)),
        Some(ContentlessSchemaReason::ReferenceCycle)
    );
    assert_eq!(
        reason(import_with_root(&root, &directory.0)),
        Some(ContentlessSchemaReason::ReferenceCycle)
    );
    Ok(())
}

#[test]
fn accept_anything_roots_remain_arbitrary_json() {
    for text in ["true", "{}"] {
        let schema = import_str(text).unwrap();
        assert_eq!(schema.name, "root");
        assert!(schema.json_any, "{text}");
        assert_eq!(
            schema.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        );
    }
}

#[test]
fn invalid_utf8_reports_an_encoding_error_naming_the_resource()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let root = directory.write("root.schema.json", b"{\"title\":\"In\xffput\"}")?;
    let Err(error) = import(&root) else {
        panic!("invalid UTF-8 must not import");
    };
    assert_encoding(&error, &root, 12);
    assert_eq!(
        error.to_string(),
        format!(
            "JSON Schema resource `{}` is not valid UTF-8: invalid utf-8 sequence of 1 bytes from index 12",
            root.canonicalize()?.display()
        )
    );

    let referring = directory.write(
        "referring.schema.json",
        br#"{"type":"object","properties":{"Value":{"$ref":"value.schema.json"}}}"#,
    )?;
    let value = directory.write("value.schema.json", b"{\"type\":\"str\xc3\"}")?;
    let Err(error) = import(&referring) else {
        panic!("an invalid UTF-8 dependency must not import");
    };
    assert_encoding(&error, &value, 12);
    Ok(())
}

fn assert_encoding(error: &JsonFormatError, resource: &Path, valid_up_to: usize) {
    let JsonFormatError::SchemaEncoding {
        resource: actual,
        error,
    } = error
    else {
        panic!("expected an encoding error, got {error:?}");
    };
    assert_eq!(
        Some(actual.as_path()),
        resource.canonicalize().ok().as_deref()
    );
    assert_eq!(error.valid_up_to(), valid_up_to);
}
