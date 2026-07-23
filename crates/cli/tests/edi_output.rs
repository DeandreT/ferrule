use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, EdiBoundaryKind, EdiValueConstraint, FormatOptions, Graph, Node, Project, Scope,
};

fn test_dir(label: &str) -> Result<PathBuf, std::io::Error> {
    let path = std::env::temp_dir().join(format!(
        "ferrule_cli_edi_output_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn write_project(directory: &Path, project: &Project) -> Result<PathBuf, std::io::Error> {
    let path = directory.join("project.json");
    let encoded = serde_json::to_vec_pretty(project).map_err(std::io::Error::other)?;
    std::fs::write(&path, encoded)?;
    Ok(path)
}

fn constant_graph(values: &[&str]) -> Graph {
    Graph {
        nodes: values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                (
                    index as u32,
                    Node::Const {
                        value: Value::String((*value).to_string()),
                    },
                )
            })
            .collect(),
    }
}

fn binding(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.to_string(),
        node,
    }
}

fn project(target: SchemaNode, kind: EdiBoundaryKind, graph: Graph, root: Scope) -> Project {
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target,
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            edi_kind: Some(kind),
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root,
    }
}

#[test]
fn hl7_target_identity_writes_a_neutral_output_path() -> Result<(), Box<dyn std::error::Error>> {
    let target = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group(
                "MSH",
                vec![
                    SchemaNode::scalar("MSH-1", ScalarType::String),
                    SchemaNode::scalar("MSH-2", ScalarType::String),
                    SchemaNode::scalar("MSH-3", ScalarType::String),
                ],
            ),
            SchemaNode::group("PID", vec![SchemaNode::scalar("PID-1", ScalarType::String)]),
        ],
    );
    let root = Scope {
        children: vec![
            Scope {
                target_field: "MSH".into(),
                bindings: vec![binding("MSH-3", 0)],
                ..Scope::default()
            },
            Scope {
                target_field: "PID".into(),
                bindings: vec![binding("PID-1", 1)],
                ..Scope::default()
            },
        ],
        ..Scope::default()
    };
    let directory = test_dir("hl7")?;
    let project_path = write_project(
        &directory,
        &project(
            target,
            EdiBoundaryKind::Hl7,
            constant_graph(&["SEND|APP", "patient&one"]),
            root,
        ),
    )?;
    let input = directory.join("input.capture");
    let output = directory.join("output.capture");
    std::fs::write(&input, "{}")?;

    assert_eq!(cli::run_project(&project_path, &input, &output)?, 1);
    assert_eq!(
        std::fs::read_to_string(&output)?,
        "MSH|^~\\&|SEND\\F\\APP\rPID|patient\\T\\one\r"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn tradacoms_target_identity_writes_a_neutral_output_path() -> Result<(), Box<dyn std::error::Error>>
{
    let target = SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::group(
                "STX",
                vec![
                    SchemaNode::group(
                        "Syntax",
                        vec![
                            SchemaNode::scalar("Code", ScalarType::String),
                            SchemaNode::scalar("Version", ScalarType::Int),
                        ],
                    ),
                    SchemaNode::scalar("Sender", ScalarType::String),
                ],
            ),
            SchemaNode::group("END", vec![SchemaNode::scalar("Count", ScalarType::Int)]),
        ],
    );
    let root = Scope {
        children: vec![
            Scope {
                target_field: "STX".into(),
                bindings: vec![binding("Sender", 2)],
                children: vec![Scope {
                    target_field: "Syntax".into(),
                    bindings: vec![binding("Code", 0), binding("Version", 1)],
                    ..Scope::default()
                }],
                ..Scope::default()
            },
            Scope {
                target_field: "END".into(),
                bindings: vec![binding("Count", 3)],
                ..Scope::default()
            },
        ],
        ..Scope::default()
    };
    let directory = test_dir("tradacoms")?;
    let project_path = write_project(
        &directory,
        &project(
            target,
            EdiBoundaryKind::Tradacoms,
            constant_graph(&["ANA", "1", "A+B", "1"]),
            root,
        ),
    )?;
    let input = directory.join("input.capture");
    let output = directory.join("output.capture");
    std::fs::write(&input, "{}")?;

    assert_eq!(cli::run_project(&project_path, &input, &output)?, 1);
    assert_eq!(
        std::fs::read_to_string(&output)?,
        "STX=ANA:1+A?+B'\nEND=1'\n"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn invalid_edi_values_report_all_issues_before_output_is_replaced()
-> Result<(), Box<dyn std::error::Error>> {
    let target = SchemaNode::group(
        "Message",
        vec![SchemaNode::group(
            "PID",
            vec![SchemaNode::scalar("PID-1", ScalarType::String)],
        )],
    );
    let root = Scope {
        children: vec![Scope {
            target_field: "PID".into(),
            bindings: vec![binding("PID-1", 0)],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let mut project = project(target, EdiBoundaryKind::Hl7, constant_graph(&["X"]), root);
    let Some(constraint) = EdiValueConstraint::new(
        vec!["PID".into(), "PID-1".into()],
        2,
        3,
        vec!["AA".into(), "BB".into()],
    ) else {
        return Err("test constraint must be valid".into());
    };
    project.target_options.edi_value_constraints = vec![constraint];

    let directory = test_dir("validation")?;
    let project_path = write_project(&directory, &project)?;
    let input = directory.join("input.capture");
    let output = directory.join("output.capture");
    std::fs::write(&input, "{}")?;
    std::fs::write(&output, "preserved")?;

    let error = cli::run_project(&project_path, &input, &output)
        .expect_err("invalid values must prevent EDI output");
    let message = format!("{error:#}");
    assert!(message.contains("minimum is 2"), "{message}");
    assert!(message.contains("configured code-list"), "{message}");
    assert_eq!(std::fs::read_to_string(&output)?, "preserved");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
