use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, Value};
use mapping::Node;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dormant-json-parser/mapping.mfd")
}

fn strict(path: &Path) -> Result<mfd::ImportOutcome, mfd::MfdError> {
    mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
}

#[test]
fn dormant_parser_preserves_parameter_without_parsing_or_becoming_a_source() {
    let imported = strict(&fixture()).unwrap().imported;
    assert!(imported.warnings.is_empty());
    assert!(imported.project.extra_sources.is_empty());
    assert!(
        matches!(&imported.project.source.kind, ir::SchemaKind::Group { children, .. } if children.len() == 1)
    );
    let encoded = mapping::project_file::encode_pretty(&imported.project).unwrap();
    let project = mapping::project_file::decode_bytes(encoded.as_bytes()).unwrap();
    let parameters = project
        .graph
        .nodes
        .values()
        .filter(|node| {
            matches!(node,
                Node::RuntimeParameter { name, ty: ScalarType::String, preview: Some(preview) }
                    if name == "payload" && preview == "preview is not JSON"
            )
        })
        .count();
    assert_eq!(parameters, 1);
    assert!(!project.graph.nodes.values().any(|node| matches!(node,
        Node::Call { function, .. } if function.starts_with("json_parse")
    )));
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/dormant-json-parser/cases.json")).unwrap();
    for case in cases {
        let parsed = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
        if case["reject"] == true {
            assert!(parsed.is_err(), "{case}");
            continue;
        }
        let input = parsed.unwrap();
        for payload in [None, Some("{"), Some(r#"{"Ignored":"ok"}"#)] {
            let mut parameters = engine::RuntimeParameters::new();
            if let Some(text) = payload {
                parameters
                    .insert("payload", Value::String(text.into()))
                    .unwrap();
            }
            let context = engine::ExecutionContext::new(Path::new("mapping.mfd"))
                .with_parameters(&parameters);
            let output = engine::run_with_context(&project, &input, &context).unwrap();
            let document: serde_json::Value =
                serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap())
                    .unwrap();
            assert_eq!(document, case["expected"], "{case}");
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule-dormant-parser-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        for name in ["document.schema.json", "payload.schema.json"] {
            std::fs::copy(fixture().parent().unwrap().join(name), directory.join(name)).unwrap();
        }
        Self(directory)
    }

    fn import(&self, design: &str) -> Result<mfd::ImportOutcome, mfd::MfdError> {
        let path = self.0.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        strict(&path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dormant_parser_still_requires_schema_and_unambiguous_parameter_metadata() {
    let original = std::fs::read_to_string(fixture()).unwrap();
    for (from, to, expected) in [
        (
            "payload.schema.json",
            "missing.schema.json",
            "could not read schema",
        ),
        (
            "inpkey=\"20\"",
            "",
            "requires exactly one run-time string input",
        ),
        (
            "<entry name=\"string\"/>",
            "<entry name=\"string\" outkey=\"21\"/>",
            "has no supported property outputs",
        ),
        (
            "<entry name=\"string\"/>",
            "<entry name=\"string\" outkey=\"bad\"/>",
            "has no supported property outputs",
        ),
        (
            "<entry name=\"string\"/>",
            "<entry name=\"string\" inpkey=\"bad\"/>",
            "has no supported property outputs",
        ),
        (
            "usageKind=\"input\" name=\"payload\"",
            "usageKind=\"input\" name=\"payload\" optional=\"1\"",
            "requires a uniquely declared required global String input",
        ),
        (
            "datatype=\"string\"",
            "datatype=\"integer\"",
            "requires a uniquely declared required global String input",
        ),
        (
            "<parameter usageKind=\"stringparse\"/>",
            "<parameter usageKind=\"stringparse\"/><parameter usageKind=\"stringparse\"/>",
            "has no supported property outputs",
        ),
        (
            "vertexkey=\"41\"",
            "vertexkey=\"10\"",
            "requires a uniquely declared required global String input",
        ),
        (
            "<edge vertexkey=\"30\"/>",
            "<edge vertexkey=\"30\"/><edge vertexkey=\"40\"/>",
            "requires a uniquely declared required global String input",
        ),
        (
            "<parameter usageKind=\"stringparse\"/>",
            "<parameter usageKind=\"stringparse\"/><entry name=\"compute-when\"/>",
            "has no supported property outputs",
        ),
    ] {
        assert!(original.contains(from));
        let scratch = Scratch::new();
        let error = scratch
            .import(&original.replace(from, to))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(expected), "{from} -> {to}: {error}");
    }
    let scratch = Scratch::new();
    std::fs::write(scratch.0.join("payload.schema.json"), "{").unwrap();
    assert!(
        scratch
            .import(&original)
            .err()
            .unwrap()
            .to_string()
            .contains("could not read schema")
    );
}

#[test]
fn connecting_a_parser_output_restores_actual_json_parse_failures() {
    let original = std::fs::read_to_string(fixture()).unwrap();
    let scratch = Scratch::new();
    let design = original
        .replace(
            "<entry name=\"string\"/>",
            "<entry name=\"string\" outkey=\"21\"/>",
        )
        .replace("<vertex vertexkey=\"10\">", "<vertex vertexkey=\"21\">");
    let project = scratch.import(&design).unwrap().imported.project;
    let input = format_json::from_str(r#"{"Message":"source"}"#, &project.source).unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert("payload", Value::String("{".into()))
        .unwrap();
    let context =
        engine::ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
    let error = engine::run_with_context(&project, &input, &context).unwrap_err();
    assert!(
        matches!(error, engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, .. }) if function == "json_parse_field")
    );
    let missing = engine::run(&project, &input).unwrap_err();
    assert!(missing.to_string().contains("payload"), "{missing}");
}
