use std::error::Error;
use std::process::Command;

/// A label, the schema files to write, and the expected refusal text.
type Case = (
    &'static str,
    &'static [(&'static str, &'static [u8])],
    &'static str,
);

#[test]
fn import_json_schema_refuses_schemas_without_content() -> Result<(), Box<dyn Error>> {
    let cases: [Case; 4] = [
        (
            "not-an-object",
            &[("c0.schema.json", b"[1,2,3]")],
            "a schema must be an object or a boolean, but this one is an array",
        ),
        (
            "file-cycle",
            &[
                ("c0.schema.json", br#"{"$ref":"c1.schema.json"}"#),
                ("c1.schema.json", br#"{"$ref":"c2.schema.json"}"#),
                ("c2.schema.json", br#"{"$ref":"c0.schema.json"}"#),
            ],
            "its `$ref` chain is a cycle that never reaches a schema with content",
        ),
        (
            "self-reference",
            &[("c0.schema.json", br##"{"$ref":"#"}"##)],
            "its `$ref` chain is a cycle that never reaches a schema with content",
        ),
        (
            "invalid-utf8",
            &[("c0.schema.json", b"{\"title\":\"In\xffput\"}")],
            "is not valid UTF-8",
        ),
    ];
    for (label, schemas, reason) in cases {
        let directory = TempDir::new("cli_contentless_json_schema")?;
        for (name, bytes) in schemas {
            std::fs::write(directory.path().join(name), bytes)?;
        }
        let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
            .args(["import-json-schema", "--schema"])
            .arg(directory.path().join("c0.schema.json"))
            .output()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{label}: {stderr}");
        assert!(output.stdout.is_empty(), "{label}");
        assert!(stderr.contains(reason), "{label}: {stderr}");
    }
    Ok(())
}

#[test]
fn import_json_schema_keeps_accept_anything_roots() -> Result<(), Box<dyn Error>> {
    for schema in ["true", "{}"] {
        let directory = TempDir::new("cli_contentless_json_schema")?;
        let path = directory.path().join("any.schema.json");
        std::fs::write(&path, schema)?;
        let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
            .args(["import-json-schema", "--schema"])
            .arg(&path)
            .output()?;
        assert!(
            output.status.success(),
            "{schema}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let imported: ir::SchemaNode = serde_json::from_slice(&output.stdout)?;
        assert_eq!(imported.name, "root");
        assert!(imported.json_any, "{schema}: {imported:#?}");
    }
    Ok(())
}

use test_support::TempDir;
