use std::error::Error;
use std::path::Path;

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope, XbrlBoundaryOptions};

use test_support::TempDir;

fn project() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["value".to_owned()],
            frame: None,
        },
    );
    Project {
        execution_contract: None,
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions::default(),
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "value".to_owned(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn write_project(path: &Path, project: &Project) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_vec_pretty(project)?)?;
    Ok(())
}

fn error_message(result: anyhow::Result<usize>) -> Result<String, Box<dyn Error>> {
    match result {
        Ok(_) => Err(std::io::Error::other("XBRL run unexpectedly succeeded").into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn external_xbrl_source_dispatches_to_the_bounded_reader() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new("cli_xbrl")?;
    let mut project = project();
    project.source_options.xbrl = Some(XbrlBoundaryOptions::external_source("taxonomy.xsd")?);
    let project_path = dir.path().join("mapping.json");
    let output_path = dir.path().join("output.xml");
    write_project(&project_path, &project)?;

    let input_path = dir.path().join("facts.xbrl");
    std::fs::write(
        &input_path,
        r#"<xbrli:xbrl xmlns:xbrli="http://www.xbrl.org/2003/instance"/>"#,
    )?;
    let message = error_message(cli::run_project(&project_path, &input_path, &output_path))?;
    assert!(
        message.contains("reading XBRL input")
            && !message.contains("unsupported input file extension"),
        "{message}"
    );
    assert!(!output_path.exists());
    Ok(())
}

#[test]
fn external_xbrl_target_validates_before_replacing_output() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new("cli_xbrl")?;
    let mut project = project();
    project.target_options.xbrl = Some(XbrlBoundaryOptions::external_target(
        "taxonomy.xsd",
        Some("table.sps"),
    )?);
    let project_path = dir.path().join("mapping.json");
    let input_path = dir.path().join("input.xml");
    let output_path = dir.path().join("output.xml");
    write_project(&project_path, &project)?;
    std::fs::write(&input_path, "<Source><value>test</value></Source>")?;
    std::fs::write(&output_path, "preserve")?;

    let message = error_message(cli::run_project(&project_path, &input_path, &output_path))?;
    assert!(message.contains("writing XBRL output"), "{message}");
    assert_eq!(std::fs::read_to_string(&output_path)?, "preserve");
    Ok(())
}
