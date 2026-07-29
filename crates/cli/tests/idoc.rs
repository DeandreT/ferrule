use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, EdiBoundaryKind, FormatOptions, Graph, IdocFieldLayout, IdocLayout, IdocSegmentLayout,
    Node, Project, Scope,
};

fn test_dir(label: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("ferrule_cli_idoc_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn field(name: &str, first: u32, last: u32) -> IdocFieldLayout {
    IdocFieldLayout::new(
        name,
        NonZeroU32::new(first).unwrap(),
        NonZeroU32::new(last).unwrap(),
    )
    .unwrap()
}

fn project() -> Project {
    let source = SchemaNode::group(
        "IDOC",
        vec![SchemaNode::group(
            "HEADER0001",
            vec![SchemaNode::scalar("number", ScalarType::String)],
        )],
    );
    let target = SchemaNode::group(
        "Result",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    );
    let layout = IdocLayout::new(vec![
        IdocSegmentLayout::new("HEADER0001", vec![field("number", 12, 16)]).unwrap(),
    ])
    .unwrap();
    let mut nodes = BTreeMap::new();
    nodes.insert(
        0,
        Node::SourceField {
            path: vec!["HEADER0001".into(), "number".into()],
            frame: None,
        },
    );
    let mut root = Scope::default();
    root.bindings.push(Binding {
        target_field: "value".into(),
        node: 0,
    });
    Project {
        source,
        target,
        source_path: Some("input.idoc".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            idoc: Some(layout),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root,
    }
}

fn output_project() -> Project {
    let source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    );
    let target = SchemaNode::group(
        "IDOC",
        vec![SchemaNode::group(
            "HEADER0001",
            vec![SchemaNode::scalar("number", ScalarType::String)],
        )],
    );
    let layout = IdocLayout::new(vec![
        IdocSegmentLayout::new("HEADER0001", vec![field("number", 12, 16)]).unwrap(),
    ])
    .unwrap();
    let mut nodes = BTreeMap::new();
    nodes.insert(
        0,
        Node::SourceField {
            path: vec!["value".into()],
            frame: None,
        },
    );
    let mut header = Scope {
        target_field: "HEADER0001".into(),
        ..Scope::default()
    };
    header.bindings.push(Binding {
        target_field: "number".into(),
        node: 0,
    });
    let mut root = Scope::default();
    root.children.push(header);
    Project {
        source,
        target,
        source_path: Some("input.json".into()),
        target_path: Some("output.idoc".into()),
        source_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            edi_kind: Some(EdiBoundaryKind::Idoc),
            idoc: Some(layout),
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root,
    }
}

fn write_project(directory: &Path, project: &Project) -> PathBuf {
    let path = directory.join("project.json");
    std::fs::write(&path, serde_json::to_string_pretty(project).unwrap()).unwrap();
    path
}

#[test]
fn embedded_idoc_layout_dispatches_independent_of_extension() {
    let directory = test_dir("dispatch");
    std::fs::write(
        directory.join("input.idoc"),
        "EDI_DC40 ignored\rHEADER0001 ABC12\r",
    )
    .unwrap();
    let project_path = write_project(&directory, &project());

    let outcome = cli::run_project_with_paths(&project_path, None, None).unwrap();
    let output = std::fs::read_to_string(outcome.output_path).unwrap();
    assert!(output.contains("<value>ABC12</value>"), "{output}");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn idoc_layout_rejects_conflicting_csv_options() {
    let directory = test_dir("conflict");
    std::fs::write(directory.join("input.idoc"), "HEADER0001 ABC12\r").unwrap();
    let mut project = project();
    project.source_options.delimiter = Some('|');
    let project_path = write_project(&directory, &project);

    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("`idoc` cannot be combined"), "{message}");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn embedded_idoc_layout_writes_filesystem_and_payload_outputs() -> anyhow::Result<()> {
    let directory = test_dir("output");
    std::fs::write(directory.join("input.json"), r#"{"value":"ABC12"}"#)?;
    let project = output_project();
    let project_path = write_project(&directory, &project);

    let outcome = cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(
        std::fs::read(&outcome.output_path)?,
        b"HEADER0001 ABC12\r\n"
    );

    let input = cli::PayloadDocument::new(Path::new("input.json"), br#"{"value":"ZX999"}"#)?;
    let payload = cli::run_project_value_payloads(
        &project,
        Path::new("/virtual/project.json"),
        &cli::PayloadRunOptions::new(input),
    )?;
    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].bytes, b"HEADER0001 ZX999\r\n");

    std::fs::remove_dir_all(directory)?;
    Ok(())
}
