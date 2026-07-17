use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions, Graph,
    HttpTimeoutSeconds, Node, Project, Scope,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_external_source_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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
    let dir = TempDir::new()?;
    let project_path = dir.0.join("mapping.json");
    let capture_path = dir.0.join("captured.response");
    let output_path = dir.0.join("output.xml");
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
