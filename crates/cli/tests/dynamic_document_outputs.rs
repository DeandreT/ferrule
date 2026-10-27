use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-dynamic-documents-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(output_path: u32) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("input/records-*.xml".into()),
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..mapping::FormatOptions::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..mapping::FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (0, Node::SourceDocumentPath),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "remove_folder".into(),
                        args: vec![0],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

fn prepare(project: &Project) -> Result<(TempDir, PathBuf), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let input = directory.0.join("input");
    std::fs::create_dir(&input)?;
    std::fs::write(
        input.join("records-b.xml"),
        "<Source><Value>B</Value></Source>",
    )?;
    std::fs::write(
        input.join("records-a.xml"),
        "<Source><Value>A</Value></Source>",
    )?;
    let project_path = directory.0.join("mapping.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(project)?)?;
    Ok((directory, project_path))
}

#[test]
fn writes_every_dynamic_document_beneath_the_explicit_base() -> Result<(), Box<dyn Error>> {
    let (directory, project_path) = prepare(&project(3))?;
    let output = directory.0.join("output");

    let outcome = cli::run_project_with_paths(&project_path, None, Some(&output))?;

    assert_eq!(outcome.records_written, 2);
    assert_eq!(outcome.output_path, output);
    assert_eq!(outcome.primary_outputs.len(), 2);
    assert_eq!(outcome.artifacts, outcome.primary_outputs);
    assert_eq!(
        outcome
            .primary_outputs
            .iter()
            .map(|written| written.path.file_name().and_then(|name| name.to_str()))
            .collect::<Vec<_>>(),
        vec![Some("records-a.xml"), Some("records-b.xml")]
    );
    assert_eq!(
        format_xml::read(&output.join("records-b.xml"), &project(3).target)?
            .field("Value")
            .and_then(ir::Instance::as_scalar),
        Some(&Value::String("B".into()))
    );
    Ok(())
}

#[test]
fn nested_collectors_publish_all_paths_and_payloads_without_partial_failures()
-> Result<(), Box<dyn Error>> {
    let project: Project = serde_json::from_str(include_str!(
        "../../codegen/src/tests/fixtures/nested_document_collection.json"
    ))?;
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "code_generation/document_collections/cases.json"
    ))?;
    let directory = TempDir::new()?;
    let project_path = directory.0.join("mapping.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    for case in cases.as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let input = directory.0.join(format!("{id}.xml"));
        let output = directory.0.join(id);
        let xml = case["input"].as_str().unwrap();
        std::fs::write(&input, xml)?;
        let source = cli::PayloadDocument::new(std::path::Path::new("input.xml"), xml.as_bytes())?;
        let payload = cli::run_project_value_payloads(
            &project,
            &project_path,
            &cli::PayloadRunOptions::new(source),
        );
        let files = cli::run_project_with_paths(&project_path, Some(&input), Some(&output));
        if case.get("error").is_some() {
            assert!(
                payload
                    .unwrap_err()
                    .to_string()
                    .contains("late document failure")
            );
            assert!(
                files
                    .unwrap_err()
                    .to_string()
                    .contains("late document failure")
            );
            assert!(
                !output.exists(),
                "a late failure cannot publish earlier documents"
            );
            continue;
        }
        let payload = payload?;
        let files = files?;
        let expected = case["documents"].as_array().unwrap();
        assert_eq!(payload.artifacts.len(), expected.len());
        assert_eq!(files.artifacts.len(), expected.len());
        for ((payload, file), expected) in
            payload.artifacts.iter().zip(&files.artifacts).zip(expected)
        {
            let path = expected["path"].as_str().unwrap();
            assert_eq!(
                payload.path.file_name().and_then(|n| n.to_str()),
                Some(path)
            );
            assert_eq!(file.path, output.join(path));
            assert_eq!(std::fs::read(&file.path)?, payload.bytes);
            let parsed =
                format_xml::from_str(std::str::from_utf8(&payload.bytes)?, &project.target)?;
            let ir::Instance::Repeated(people) = parsed.field("Person").unwrap() else {
                panic!("ordered people")
            };
            let names: Vec<_> = people
                .iter()
                .map(|person| {
                    person
                        .field("Name")
                        .and_then(ir::Instance::as_scalar)
                        .unwrap()
                })
                .collect();
            assert_eq!(
                names,
                expected["names"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|name| Value::String(name.as_str().unwrap().into()))
                    .collect::<Vec<_>>()
                    .iter()
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}

#[test]
fn selected_primary_and_named_collectors_keep_document_destinations() -> Result<(), Box<dyn Error>>
{
    let mut project: Project = serde_json::from_str(include_str!(
        "../../codegen/src/tests/fixtures/nested_document_collection.json"
    ))?;
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "code_generation/document_collections/cases.json"
    ))?;
    let xml = cases[0]["input"].as_str().unwrap();
    let input = cli::PayloadDocument::new(std::path::Path::new("input.xml"), xml.as_bytes())?;
    let directory = TempDir::new()?;
    let path = directory.0.join("mapping.json");
    let primary = cli::run_project_value_payloads(
        &project,
        &path,
        &cli::PayloadRunOptions::new(input).with_target(cli::TargetSelection::Primary),
    )?;
    assert_eq!(primary.artifacts.len(), 4);
    project.extra_targets.push(NamedTarget {
        name: "grouped".into(),
        path: None,
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    project.root = Scope::default();
    let named = cli::run_project_value_payloads(
        &project,
        &path,
        &cli::PayloadRunOptions::new(input).with_target(cli::TargetSelection::Named("grouped")),
    )?;
    assert_eq!(named.artifacts.len(), primary.artifacts.len());
    for (left, right) in primary.artifacts.iter().zip(named.artifacts) {
        assert_eq!(left.path, right.path);
        assert_eq!(left.bytes, right.bytes);
    }
    Ok(())
}

#[test]
fn collector_payload_limit_counts_documents_across_parent_contexts() -> Result<(), Box<dyn Error>> {
    let mut project: Project = serde_json::from_str(include_str!(
        "../../codegen/src/tests/fixtures/nested_document_collection.json"
    ))?;
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(1),
        },
    );
    let directory = TempDir::new()?;
    let path = directory.0.join("mapping.json");
    for count in [4096, 4097] {
        let mut xml = String::from("<Input>");
        // Every parent produces one document. The final count must not reset
        // at each collector item or nested document-producing child.
        for index in 0..count {
            xml.push_str(&format!(
                "<Dept><Name>D{index}</Name><Person><Name>P</Name></Person></Dept>"
            ));
        }
        xml.push_str("</Input>");
        let input = cli::PayloadDocument::new(std::path::Path::new("input.xml"), xml.as_bytes())?;
        let result =
            cli::run_project_value_payloads(&project, &path, &cli::PayloadRunOptions::new(input));
        if count == 4096 {
            assert_eq!(result?.artifacts.len(), count);
        } else {
            assert!(result.unwrap_err().to_string().contains("4096"));
        }
    }
    Ok(())
}

#[test]
fn protected_dynamic_output_base_fails_before_publication() -> Result<(), Box<dyn Error>> {
    let (directory, project_path) = prepare(&project(3))?;
    let output = directory.0.join("trace.jsonl");
    let trace = cli::JsonTraceFile::create(&output)?;
    let protected = [trace.destination()];

    let error = cli::run_project_with_options(
        &project_path,
        &cli::RunOptions::new()
            .with_output_path(&output)
            .with_trace_sink(&trace)
            .with_protected_output_paths(&protected),
    )
    .expect_err("a dynamic output base must not replace a protected trace path");

    assert!(
        error
            .to_string()
            .contains("overlaps a path reserved by the host"),
        "{error:#}"
    );
    assert!(!output.exists());
    Ok(())
}

#[test]
fn resolved_source_paths_are_rejected_as_dynamic_output_names() -> Result<(), Box<dyn Error>> {
    let (directory, project_path) = prepare(&project(0))?;
    let output = directory.0.join("output");

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("resolved source paths must not escape the dynamic output base");

    assert!(
        error
            .to_string()
            .contains("dynamic output path must be relative")
    );
    assert!(!output.exists());
    Ok(())
}

#[test]
fn duplicate_paths_fail_before_any_document_is_published() -> Result<(), Box<dyn Error>> {
    let mut project = project(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("same.xml".into()),
        },
    );
    let (directory, project_path) = prepare(&project)?;
    let output = directory.0.join("output");

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("duplicate output paths must fail");

    assert!(error.to_string().contains("duplicate dynamic output path"));
    assert!(!output.exists());
    Ok(())
}

#[test]
fn escaping_paths_fail_before_any_document_is_published() -> Result<(), Box<dyn Error>> {
    let mut project = project(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("../outside.xml".into()),
        },
    );
    let (directory, project_path) = prepare(&project)?;
    let output = directory.0.join("output");

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("escaping output paths must fail");

    assert!(error.to_string().contains("cannot contain `..`"));
    assert!(!output.exists());
    assert!(!directory.0.join("outside.xml").exists());
    Ok(())
}

#[test]
fn empty_paths_fail_before_the_output_directory_is_created() -> Result<(), Box<dyn Error>> {
    let mut project = project(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    let (directory, project_path) = prepare(&project)?;
    let output = directory.0.join("output");

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("empty output paths must fail");

    assert!(error.to_string().contains("cannot be empty"));
    assert!(!output.exists());
    Ok(())
}

#[test]
fn destination_conflicts_do_not_replace_an_earlier_existing_file() -> Result<(), Box<dyn Error>> {
    let (directory, project_path) = prepare(&project(3))?;
    let output = directory.0.join("output");
    std::fs::create_dir(&output)?;
    std::fs::write(output.join("records-a.xml"), "keep me")?;
    std::fs::create_dir(output.join("records-b.xml"))?;

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("directory destinations must fail before publication");

    assert!(error.to_string().contains("is a directory"));
    assert_eq!(
        std::fs::read_to_string(output.join("records-a.xml"))?,
        "keep me"
    );
    assert!(output.join("records-b.xml").is_dir());
    Ok(())
}

#[test]
fn windows_drive_paths_are_rejected_portably() -> Result<(), Box<dyn Error>> {
    let mut project = project(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("C:/outside.xml".into()),
        },
    );
    let (directory, project_path) = prepare(&project)?;
    let output = directory.0.join("output");

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("Windows drive paths must fail on every host platform");

    assert!(error.to_string().contains("Windows drive prefix"));
    assert!(!output.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_output_base_is_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let (directory, project_path) = prepare(&project(3))?;
    let actual = directory.0.join("actual-output");
    let output = directory.0.join("output");
    std::fs::create_dir(&actual)?;
    symlink(&actual, &output)?;

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("a symlinked output base must fail");

    assert!(error.to_string().contains("cannot be a symlink"));
    assert!(std::fs::read_dir(actual)?.next().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_output_component_is_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let mut project = project(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("linked/output.xml".into()),
        },
    );
    let (directory, project_path) = prepare(&project)?;
    std::fs::remove_file(directory.0.join("input/records-b.xml"))?;
    let output = directory.0.join("output");
    let outside = directory.0.join("outside");
    std::fs::create_dir(&output)?;
    std::fs::create_dir(&outside)?;
    symlink(&outside, output.join("linked"))?;

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("an output path crossing a symlink must fail");

    let message = format!("{error:#}");
    assert!(message.contains("crosses symlink"), "{message}");
    assert!(!outside.join("output.xml").exists());
    Ok(())
}

#[test]
fn dynamic_primary_child_cannot_collide_with_static_extra_target() -> Result<(), Box<dyn Error>> {
    let mut project = project(3);
    project.extra_targets.push(NamedTarget {
        name: "collision".into(),
        path: Some("output/records-a.xml".into()),
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    });
    let (directory, project_path) = prepare(&project)?;
    let output = directory.0.join("output");

    let error = cli::run_project_with_paths(&project_path, None, Some(&output))
        .expect_err("cross-target output collisions must fail before publication");

    let message = format!("{error:#}");
    assert!(message.contains("resolve to the same path"), "{message}");
    assert!(message.contains("collision"), "{message}");
    assert!(!output.exists());
    Ok(())
}
