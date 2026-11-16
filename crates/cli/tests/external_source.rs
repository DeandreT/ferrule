use std::error::Error;
use std::path::Path;

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions, Graph,
    HttpTimeoutSeconds, Node, Project, Scope,
};

use test_support::TempDir;

fn project() -> Result<Project, Box<dyn Error>> {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["answer".to_owned()],
            frame: None,
        },
    );
    let source_options = mapping::FormatOptions {
        external_source: Some(ExternalSourceOptions::http_post(
            ExternalHttpMode::Manual,
            HttpTimeoutSeconds::default(),
            None,
            None,
            ExternalPayloadFormat::Json,
            Vec::new(),
        )?),
        ..mapping::FormatOptions::default()
    };
    Ok(Project {
        execution_contract: None,
        source: SchemaNode::group(
            "response",
            vec![SchemaNode::scalar("answer", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Answer", ScalarType::String)],
        ),
        source_path: Some("https://example.test/analyze".to_owned()),
        target_path: Some("output.xml".to_owned()),
        source_options,
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "Answer".to_owned(),
                node: 0,
            }],
            ..Scope::default()
        },
    })
}

fn write_project(path: &Path, project: &Project) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_vec_pretty(project)?)?;
    Ok(())
}

#[test]
fn local_capture_uses_declared_payload_and_stored_url_never_sends_post()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new("cli_external_source")?;
    let project_path = dir.path().join("mapping.json");
    let capture_path = dir.path().join("captured.response");
    let output_path = dir.path().join("output.xml");
    write_project(&project_path, &project()?)?;
    std::fs::write(&capture_path, r#"{"answer":"ready"}"#)?;

    let outcome =
        cli::run_project_with_paths(&project_path, Some(&capture_path), Some(&output_path))?;
    assert_eq!(outcome.records_written, 1);
    assert_eq!(
        std::fs::read_to_string(&output_path)?,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Answer>ready</Answer>\n</Output>"
    );

    let error = cli::run_project_with_paths(&project_path, None, Some(&output_path))
        .expect_err("stored HTTP POST URL must not be fetched");
    assert!(error.to_string().contains("does not send POST requests"));
    Ok(())
}
