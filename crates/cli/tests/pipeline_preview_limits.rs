use std::collections::BTreeMap;
use std::path::Path;

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, FormatOptions, Graph, NamedTarget, Node, Pipeline, PipelineInput, PipelineStage,
    Project, Scope, ScopeConstruction,
};

use test_support::TempDir;

fn pipeline() -> Pipeline {
    let schema = SchemaNode::group(
        "Document",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let options = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    Pipeline {
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "copy".into(),
            mapping_path: None,
            source: PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: Vec::new(),
            project: Project {
                execution_contract: None,
                source: schema.clone(),
                target: schema,
                source_path: None,
                target_path: Some("existing.json".into()),
                source_options: options.clone(),
                target_options: options,
                extra_sources: Vec::new(),
                extra_targets: Vec::new(),
                failure_rules: Vec::new(),
                user_functions: BTreeMap::new(),
                graph: Graph::default(),
                root: Scope {
                    construction: ScopeConstruction::CopyCurrentSource,
                    ..Scope::default()
                },
            },
        }],
    }
}

fn input_bytes(length: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(length + 12);
    bytes.extend_from_slice(br#"{"Value":""#);
    bytes.resize(bytes.len() + length, b'x');
    bytes.extend_from_slice(br#""}"#);
    bytes
}

fn preview_error(pipeline: &Pipeline, bytes: &[u8], directory: &TempDir) -> anyhow::Result<String> {
    let sentinel = directory.path().join("existing.json");
    std::fs::write(&sentinel, b"existing output")?;
    let hosts = [cli::PipelineHostPayload::new(
        "input",
        cli::PayloadDocument::new(Path::new("input.json"), bytes)?,
    )?];
    let error = cli::preview_pipeline_value_payloads(
        pipeline,
        &directory.path().join("flow.json"),
        &cli::PipelinePreviewOptions::new(&hosts),
    )
    .unwrap_err();
    assert_eq!(std::fs::read(sentinel)?, b"existing output");
    Ok(format!("{error:#}"))
}

#[test]
fn an_expanding_mapping_cannot_return_an_oversized_document() -> anyhow::Result<()> {
    let directory = TempDir::new("pipeline-preview-limits")?;
    let mut pipeline = pipeline();
    let project = &mut pipeline.stages[0].project;
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 0],
            },
        ),
    ]);
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    let bytes = input_bytes(cli::MAX_PAYLOAD_DOCUMENT_BYTES / 2);
    let error = preview_error(&pipeline, &bytes, &directory)?;
    assert!(error.contains("64 MiB per-document"), "{error}");
    Ok(())
}

#[test]
fn individually_valid_documents_cannot_exceed_the_total_output_limit() -> anyhow::Result<()> {
    let directory = TempDir::new("pipeline-preview-limits")?;
    let mut pipeline = pipeline();
    let project = &mut pipeline.stages[0].project;
    project.extra_targets = (0..8)
        .map(|index| NamedTarget {
            name: format!("copy{index}"),
            path: Some("existing.json".into()),
            schema: project.target.clone(),
            options: project.target_options.clone(),
            root: project.root.clone(),
        })
        .collect();
    let bytes = input_bytes(cli::MAX_PAYLOAD_RUN_BYTES / 9 + 1);
    assert!(bytes.len() < cli::MAX_PAYLOAD_DOCUMENT_BYTES);
    let error = preview_error(&pipeline, &bytes, &directory)?;
    assert!(error.contains("256 MiB total"), "{error}");
    Ok(())
}
