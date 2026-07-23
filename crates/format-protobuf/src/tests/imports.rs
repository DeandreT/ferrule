use ir::{SchemaKind, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    Layout, MAX_IMPORT_DEPTH, MAX_SCHEMA_FILES, MAX_SCHEMA_GRAPH_BYTES, ProtobufError,
    SchemaBundle, from_slice, to_ir_schema, to_vec,
};

use super::{error_text, group, scalar};

const ROOT: &str = r#"
syntax = "proto3";
package app;
import "shared/model.proto";
message Envelope {
  shared.model.Record record = 1;
  shared.types.Status status = 2;
}
"#;

const MODEL_PUBLIC: &str = r#"
syntax = "proto3";
package shared.model;
import public "common/status.proto";
message Record {
  string name = 1;
  shared.types.Status status = 2;
}
"#;

const STATUS: &str = r#"
syntax = "proto3";
package shared.types;
enum Status { STATUS_UNSPECIFIED = 0; READY = 1; }
"#;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_protobuf_imports_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap_or_else(|error| {
            panic!("temporary schema directory should be created: {error}")
        });
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn imported_layout(model: &str) -> Result<Layout, ProtobufError> {
    Layout::parse_files(
        "api/root.proto",
        ROOT,
        [
            ("shared/model.proto", model),
            ("common/status.proto", STATUS),
        ],
    )
}

#[test]
fn public_imports_resolve_across_packages_and_execute_wire_io() {
    let layout = match imported_layout(MODEL_PUBLIC) {
        Ok(layout) => layout,
        Err(error) => panic!("multi-file schema should parse: {error}"),
    };
    let instance = group(vec![
        (
            "record",
            group(vec![
                ("name", scalar(Value::String("invoice".to_string()))),
                ("status", scalar(Value::String("READY".to_string()))),
            ]),
        ),
        ("status", scalar(Value::String("READY".to_string()))),
    ]);
    let bytes = match to_vec(&layout, "app.Envelope", &instance) {
        Ok(bytes) => bytes,
        Err(error) => panic!("multi-file message should encode: {error}"),
    };
    let decoded = match from_slice(&layout, "app.Envelope", &bytes) {
        Ok(decoded) => decoded,
        Err(error) => panic!("multi-file message should decode: {error}"),
    };
    assert_eq!(
        decoded,
        group(vec![
            (
                "record",
                group(vec![
                    ("name", scalar(Value::String("invoice".to_string()))),
                    ("status", scalar(Value::Int(1))),
                ]),
            ),
            ("status", scalar(Value::Int(1))),
        ])
    );

    let schema = match to_ir_schema(&layout, "app.Envelope") {
        Ok(schema) => schema,
        Err(error) => panic!("multi-file root should project to IR: {error}"),
    };
    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("root should project as a group");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["record", "status"]
    );
}

#[test]
fn ordinary_imports_do_not_reexport_declarations() {
    let ordinary = MODEL_PUBLIC.replace("import public", "import");
    let error = error_text(imported_layout(&ordinary));
    assert!(
        error.contains("unknown type `shared.types.Status`"),
        "unexpected error: {error}"
    );
}

#[test]
fn diamond_imports_are_deduplicated_deterministically() {
    let root = r#"
        syntax = "proto3";
        import "left.proto";
        import "right.proto";
        message Root { .common.Value value = 1; }
    "#;
    let left = r#"syntax = "proto3"; import public "common.proto"; message Left {}"#;
    let right = r#"syntax = "proto3"; import public "common.proto"; message Right {}"#;
    let common = r#"syntax = "proto3"; package common; message Value { string text = 1; }"#;
    let layout = Layout::parse_files(
        "root.proto",
        root,
        [
            ("left.proto", left),
            ("right.proto", right),
            ("common.proto", common),
        ],
    );
    assert!(layout.is_ok(), "diamond graph should parse: {layout:?}");
    let layout = layout.unwrap_or_else(|error| panic!("diamond graph should parse: {error}"));
    assert_eq!(
        layout
            .messages()
            .iter()
            .filter(|message| message.full_name() == "common.Value")
            .count(),
        1
    );
}

#[test]
fn import_graph_rejects_cycles_duplicates_missing_files_and_escape() {
    let cases = [
        (
            Layout::parse_files(
                "root.proto",
                r#"import "a.proto"; message Root {}"#,
                [
                    ("a.proto", r#"import "b.proto"; message A {}"#),
                    ("b.proto", r#"import "a.proto"; message B {}"#),
                ],
            ),
            "import cycle",
        ),
        (
            Layout::parse_files(
                "root.proto",
                r#"import "a.proto"; import "./a.proto"; message Root {}"#,
                [("a.proto", "message A {}")],
            ),
            "more than once",
        ),
        (
            Layout::parse_files(
                "root.proto",
                r#"import "missing.proto"; message Root {}"#,
                std::iter::empty(),
            ),
            "missing imported schema",
        ),
        (
            Layout::parse_files(
                "root.proto",
                r#"import "../outside.proto"; message Root {}"#,
                [("outside.proto", "message Outside {}")],
            ),
            "escapes its virtual root",
        ),
        (
            Layout::parse_files(
                "root.proto",
                "message Root {}",
                [
                    ("dir/../same.proto", "message A {}"),
                    ("same.proto", "message B {}"),
                ],
            ),
            "duplicate file",
        ),
        (
            Layout::parse_files(
                "root.proto",
                r#"import "C:/outside.proto"; message Root {}"#,
                std::iter::empty(),
            ),
            "non-portable",
        ),
    ];
    for (result, expected) in cases {
        let error = error_text(result);
        assert!(
            error.contains(expected),
            "`{error}` should contain `{expected}`"
        );
    }
}

#[test]
fn imported_files_cannot_see_root_or_sibling_declarations_without_imports() {
    let cases = [
        (
            "message Left { optional Root root = 1; }",
            "message Right {}",
            "unknown type `Root`",
        ),
        (
            "message Left { optional Right right = 1; }",
            "message Right {}",
            "unknown type `Right`",
        ),
    ];
    for (left, right, expected) in cases {
        let result = Layout::parse_files(
            "root.proto",
            r#"import "left.proto"; import "right.proto"; message Root {}"#,
            [("left.proto", left), ("right.proto", right)],
        );
        let error = error_text(result);
        assert!(
            error.contains(expected),
            "`{error}` should contain `{expected}`"
        );
    }
}

#[test]
fn import_graph_enforces_file_depth_and_total_byte_limits() {
    let files = (0..MAX_SCHEMA_FILES)
        .map(|index| (format!("unused-{index}.proto"), "".to_string()))
        .collect::<Vec<_>>();
    let error = error_text(Layout::parse_files(
        "root.proto",
        "message Root {}",
        files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str())),
    ));
    assert!(error.contains("file limit"), "unexpected error: {error}");

    let mut depth_files = Vec::new();
    for index in 0..=MAX_IMPORT_DEPTH {
        let source = format!("import \"{}.proto\";", index + 1);
        depth_files.push((format!("{index}.proto"), source));
    }
    depth_files.push((
        format!("{}.proto", MAX_IMPORT_DEPTH + 1),
        "message Leaf {}".to_string(),
    ));
    let error = error_text(Layout::parse_files(
        "root.proto",
        "import \"0.proto\"; message Root {}",
        depth_files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str())),
    ));
    assert!(error.contains("import depth"), "unexpected error: {error}");

    let large = " ".repeat(900_000);
    let total_files = (0..10)
        .map(|index| (format!("large-{index}.proto"), large.clone()))
        .collect::<Vec<_>>();
    let error = error_text(Layout::parse_files(
        "root.proto",
        "message Root {}",
        total_files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str())),
    ));
    assert!(
        error.contains(&MAX_SCHEMA_GRAPH_BYTES.to_string()),
        "unexpected error: {error}"
    );
}

#[test]
fn single_source_parse_requires_imports_to_be_supplied() {
    let error = error_text(Layout::parse(r#"import "missing.proto"; message Root {}"#));
    assert!(error.contains("missing imported schema"));
}

#[test]
fn filesystem_bundle_loads_root_relative_nested_imports_in_canonical_order() {
    let temp = TempDir::new();
    for directory in ["api", "shared", "common"] {
        std::fs::create_dir_all(temp.0.join(directory)).unwrap_or_else(|error| {
            panic!("temporary schema subdirectory should be created: {error}")
        });
    }
    std::fs::write(temp.0.join("api/root.proto"), ROOT)
        .unwrap_or_else(|error| panic!("root schema should be written: {error}"));
    std::fs::write(temp.0.join("shared/model.proto"), MODEL_PUBLIC)
        .unwrap_or_else(|error| panic!("model schema should be written: {error}"));
    std::fs::write(temp.0.join("common/status.proto"), STATUS)
        .unwrap_or_else(|error| panic!("status schema should be written: {error}"));

    let bundle = SchemaBundle::read_relative(&temp.0, "api/root.proto")
        .unwrap_or_else(|error| panic!("filesystem schema graph should load: {error}"));
    assert_eq!(bundle.root_path(), "api/root.proto");
    assert_eq!(
        bundle
            .imports()
            .iter()
            .map(|file| file.path())
            .collect::<Vec<_>>(),
        ["common/status.proto", "shared/model.proto"]
    );
    assert!(bundle.layout().is_ok());
}

#[cfg(unix)]
#[test]
fn filesystem_bundle_rejects_symlink_escape_and_duplicate_physical_files() {
    use std::os::unix::fs::symlink;

    let base = TempDir::new();
    let outside = TempDir::new();
    std::fs::write(outside.0.join("outside.proto"), "message Outside {}")
        .unwrap_or_else(|error| panic!("outside schema should be written: {error}"));
    symlink(&outside.0, base.0.join("linked"))
        .unwrap_or_else(|error| panic!("escape symlink should be created: {error}"));
    std::fs::write(
        base.0.join("root.proto"),
        r#"import "linked/outside.proto"; message Root {}"#,
    )
    .unwrap_or_else(|error| panic!("root schema should be written: {error}"));
    let error = error_text(SchemaBundle::read_relative(&base.0, "root.proto"));
    assert!(
        error.contains("escapes its configured base"),
        "unexpected error: {error}"
    );

    std::fs::write(base.0.join("shared.proto"), "message Shared {}")
        .unwrap_or_else(|error| panic!("shared schema should be written: {error}"));
    symlink(base.0.join("shared.proto"), base.0.join("alias.proto"))
        .unwrap_or_else(|error| panic!("alias symlink should be created: {error}"));
    std::fs::write(
        base.0.join("root.proto"),
        r#"import "shared.proto"; import "alias.proto"; message Root {}"#,
    )
    .unwrap_or_else(|error| panic!("root schema should be replaced: {error}"));
    let error = error_text(SchemaBundle::read_relative(&base.0, "root.proto"));
    assert!(
        error.contains("resolve to the same file"),
        "unexpected error: {error}"
    );
}
