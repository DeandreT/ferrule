use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, EdiBoundaryKind, Graph, Node, Project, Scope};

fn test_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ferrule_cli_format_identity_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write_project(directory: &Path, project: &Project) -> PathBuf {
    let path = directory.join("project.json");
    std::fs::write(&path, serde_json::to_vec(project).unwrap()).unwrap();
    path
}

#[test]
fn json_document_identity_overrides_neutral_instance_extensions() {
    let directory = test_dir("json");
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    );
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["value".into()],
            frame: None,
        },
    );
    let mut project = Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions::default(),
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    };
    project.source_options.json_document = true;
    project.target_options.json_document = true;

    let project_path = write_project(&directory, &project);
    let input_path = directory.join("input.capture");
    let output_path = directory.join("output.capture");
    std::fs::write(&input_path, r#"{"value":"retained"}"#).unwrap();

    assert_eq!(
        cli::run_project(&project_path, &input_path, &output_path).unwrap(),
        1
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&output_path).unwrap()).unwrap(),
        serde_json::json!({"value": "retained"})
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn xml_document_identity_overrides_neutral_instance_extensions() {
    let directory = test_dir("xml");
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    );
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["value".into()],
            frame: None,
        },
    );
    let project = Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            ..mapping::FormatOptions::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..mapping::FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    };

    let project_path = write_project(&directory, &project);
    let input_path = directory.join("input.capture");
    let output_path = directory.join("output.capture");
    std::fs::write(&input_path, "<Root><value>retained</value></Root>").unwrap();

    assert_eq!(
        cli::run_project(&project_path, &input_path, &output_path).unwrap(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(&output_path).unwrap(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Root>\n  <value>retained</value>\n</Root>"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn edi_identity_overrides_neutral_input_extension() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edi");
    let mut project: Project =
        serde_json::from_slice(&std::fs::read(fixture.join("project.json")).unwrap()).unwrap();
    project.source_options.edi_kind = Some(EdiBoundaryKind::X12);

    let directory = test_dir("edi");
    let project_path = write_project(&directory, &project);
    let input_path = directory.join("purchase-order.capture");
    let output_path = directory.join("output.csv");
    std::fs::copy(fixture.join("po850.edi"), &input_path).unwrap();

    assert_eq!(
        cli::run_project(&project_path, &input_path, &output_path).unwrap(),
        3
    );
    assert_eq!(
        std::fs::read_to_string(&output_path).unwrap(),
        std::fs::read_to_string(fixture.join("expected_po_lines.csv")).unwrap()
    );
    std::fs::remove_dir_all(directory).unwrap();
}
