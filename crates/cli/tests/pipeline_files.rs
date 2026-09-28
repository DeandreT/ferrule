use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, NamedTarget, Node, Pipeline,
    PipelineInput, PipelineStage, Project, Scope, ScopeConstruction, ScopeIteration,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_pipeline_files_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn pipeline(&self, pipeline: &Pipeline) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join("pipeline.json");
        std::fs::write(&path, serde_json::to_vec_pretty(pipeline)?)?;
        Ok(path)
    }

    fn input(&self) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join("orders.json");
        std::fs::write(&path, r#"{"Value":"original"}"#)?;
        Ok(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
}

fn project() -> Project {
    Project {
        source: schema(),
        target: schema(),
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

fn pipeline() -> Pipeline {
    let mut producer = project();
    producer.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("named value".into()),
        },
    );
    producer.extra_targets.push(NamedTarget {
        name: "selected".into(),
        path: None,
        schema: schema(),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    });

    // A forward reference proves execution uses dependencies rather than
    // declaration order. The later stage consumes the producer's named target.
    Pipeline {
        stages: vec![
            PipelineStage {
                id: "consume".into(),
                project: project(),
                source: PipelineInput::StageTarget {
                    stage: "prepare".into(),
                    target: Some("selected".into()),
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "prepare".into(),
                project: producer,
                source: PipelineInput::Host {
                    name: "orders".into(),
                },
                extra_sources: Vec::new(),
            },
        ],
    }
}

fn input(path: &Path) -> cli::PipelineHostFile {
    cli::PipelineHostFile {
        name: "orders".into(),
        path: path.to_path_buf(),
    }
}

fn output(stage: &str, target: Option<&str>, path: &Path) -> cli::PipelineOutputFile {
    cli::PipelineOutputFile {
        stage: stage.into(),
        target: target.map(str::to_owned),
        path: path.to_path_buf(),
    }
}

fn ferrule(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_runs_forward_stage_graph_and_publishes_selected_named_target() -> Result<(), Box<dyn Error>>
{
    let dir = TempDir::new()?;
    let pipeline_path = dir.pipeline(&pipeline())?;
    let input_path = dir.input()?;
    let named = dir.0.join("named.json");
    let final_path = dir.0.join("final.json");
    let output = ferrule(&[
        "run-pipeline",
        "--pipeline",
        pipeline_path.to_str().unwrap(),
        "--input",
        "orders",
        input_path.to_str().unwrap(),
        "--output",
        "consume",
        final_path.to_str().unwrap(),
        "--named-output",
        "prepare",
        "selected",
        named.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = serde_json::json!({"Value": "named value"});
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&named)?)?,
        expected
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&final_path)?)?,
        expected
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("completed 2 stage(s)"));
    assert_eq!(std::fs::read_dir(&dir.0)?.count(), 4);
    Ok(())
}

#[test]
fn later_stage_failure_publishes_nothing_and_preserves_existing_files() -> Result<(), Box<dyn Error>>
{
    let dir = TempDir::new()?;
    let mut pipeline = pipeline();
    let consumer = &mut pipeline.stages[0].project;
    consumer.graph.nodes.insert(
        1,
        Node::RuntimeParameter {
            name: "required".into(),
            ty: ScalarType::String,
        },
    );
    consumer.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    let pipeline_path = dir.pipeline(&pipeline)?;
    let input_path = dir.input()?;
    let existing = dir.0.join("named.json");
    let absent = dir.0.join("final.json");
    std::fs::write(&existing, "preserve")?;
    let error = cli::run_pipeline_file(
        &pipeline_path,
        &[input(&input_path)],
        &[
            output("prepare", Some("selected"), &existing),
            output("consume", None, &absent),
        ],
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("required"));
    assert_eq!(std::fs::read_to_string(&existing)?, "preserve");
    assert!(!absent.exists());
    assert!(
        std::fs::read_dir(&dir.0)?
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".ferrule-stage-"))
    );
    Ok(())
}

#[test]
fn rejects_invalid_host_contracts_and_output_publication_paths() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut pipeline = pipeline();
    let pipeline_path = dir.pipeline(&pipeline)?;
    let input_path = dir.input()?;
    let publication = output("consume", None, &dir.0.join("final.json"));
    let run = |inputs: &[cli::PipelineHostFile], outputs: &[cli::PipelineOutputFile]| {
        cli::run_pipeline_file(&pipeline_path, inputs, outputs)
            .unwrap_err()
            .to_string()
    };
    assert!(run(&[], std::slice::from_ref(&publication)).contains("missing"));
    assert!(
        run(
            &[input(&input_path), input(&input_path)],
            std::slice::from_ref(&publication)
        )
        .contains("more than once")
    );
    assert!(
        run(
            &[cli::PipelineHostFile {
                name: "extra".into(),
                path: input_path.clone()
            }],
            std::slice::from_ref(&publication)
        )
        .contains("not used")
    );
    assert!(run(&[input(&input_path)], &[]).contains("at least one"));
    assert!(
        run(
            &[input(&input_path)],
            &[publication.clone(), publication.clone()]
        )
        .contains("selected more than once")
    );

    let overlap = dir.0.join("overlap.json");
    let nested = overlap.join("nested.json");
    assert!(
        run(
            &[input(&input_path)],
            &[
                output("consume", None, &overlap),
                output("prepare", Some("selected"), &nested)
            ]
        )
        .contains("overlap as file and directory")
    );
    assert!(!overlap.exists());

    assert!(
        run(
            &[input(&input_path)],
            &[output("consume", None, &pipeline_path)]
        )
        .contains("reserved by the host")
    );
    assert!(
        run(
            &[input(&input_path)],
            &[output("consume", None, &input_path)]
        )
        .contains("reserved by the host")
    );
    assert_eq!(
        std::fs::read(&pipeline_path)?,
        serde_json::to_vec_pretty(&pipeline)?
    );
    assert_eq!(
        std::fs::read_to_string(&input_path)?,
        r#"{"Value":"original"}"#
    );

    let mut other = project();
    other.source.name = "OtherRoot".into();
    pipeline.stages.push(PipelineStage {
        id: "other".into(),
        project: other,
        source: PipelineInput::Host {
            name: "orders".into(),
        },
        extra_sources: Vec::new(),
    });
    dir.pipeline(&pipeline)?;
    let renamed_output = dir.0.join("root-renamed.json");
    let outcome = cli::run_pipeline_file(
        &pipeline_path,
        &[input(&input_path)],
        &[output("other", None, &renamed_output)],
    )?;
    assert_eq!(outcome.stages_executed.len(), 3);
    assert!(renamed_output.exists());

    pipeline.stages[2].project.source_options.json_document = false;
    dir.pipeline(&pipeline)?;
    assert!(
        run(&[input(&input_path)], std::slice::from_ref(&publication))
            .contains("inconsistent schema or format options")
    );
    assert!(!publication.path.exists());
    Ok(())
}

fn dynamic_xml_file_set_pipeline() -> Pipeline {
    let mut mapping = project();
    mapping.source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    mapping.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                .repeating(),
        ],
    );
    mapping.extra_sources.push(NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Document",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        options: FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..FormatOptions::default()
        },
        dynamic_path: Some(DynamicSourcePath {
            node: 0,
            iteration: vec!["File".into()],
        }),
    });
    mapping.graph.nodes.insert(
        0,
        Node::SourceField {
            path: Vec::new(),
            frame: Some(vec!["File".into()]),
        },
    );
    mapping.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: Some(vec!["document".into(), "Item".into()]),
        },
    );
    mapping.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    Pipeline {
        stages: vec![PipelineStage {
            id: "load".into(),
            project: mapping,
            source: PipelineInput::Host {
                name: "files".into(),
            },
            extra_sources: Vec::new(),
        }],
    }
}

#[test]
fn dynamic_xml_file_set_members_cannot_be_overwritten() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let pipeline_path = dir.pipeline(&dynamic_xml_file_set_pipeline())?;
    let input_path = dir.0.join("files.json");
    std::fs::write(&input_path, r#"{"File":["records-*.xml"]}"#)?;
    let member = dir.0.join("records-a.xml");
    let original = b"<Document><Item><Value>a</Value></Item></Document>";
    std::fs::write(&member, original)?;

    let error = cli::run_pipeline_file(
        &pipeline_path,
        &[cli::PipelineHostFile {
            name: "files".into(),
            path: input_path,
        }],
        &[output("load", None, &member)],
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("reserved by the host"));
    assert_eq!(std::fs::read(member)?, original);
    Ok(())
}

#[test]
fn reused_file_sources_reject_ambiguous_root_schemas() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut dynamic = dynamic_xml_file_set_pipeline();
    let mut second = dynamic.stages[0].clone();
    second.id = "other".into();
    second.project.extra_sources[0].schema.name = "OtherDocument".into();
    dynamic.stages.push(second);
    let pipeline_path = dir.pipeline(&dynamic)?;
    let input_path = dir.0.join("files.json");
    std::fs::write(&input_path, r#"{"File":["records-*.xml"]}"#)?;
    let selected = output("load", None, &dir.0.join("result.json"));
    let error = cli::run_pipeline_file(
        &pipeline_path,
        &[cli::PipelineHostFile {
            name: "files".into(),
            path: input_path,
        }],
        &[selected],
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("dynamic source `document` has inconsistent schema"));

    let mut host = pipeline();
    let mut second = project();
    second.source.name = "OtherRecord".into();
    second.source_options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    host.stages[1].project.source_options = second.source_options.clone();
    host.stages.push(PipelineStage {
        id: "other".into(),
        project: second,
        source: PipelineInput::Host {
            name: "orders".into(),
        },
        extra_sources: Vec::new(),
    });
    let pipeline_path = dir.pipeline(&host)?;
    let input_path = dir.0.join("orders.xml");
    std::fs::write(&input_path, "<Record><Value>a</Value></Record>")?;
    let selected = output("consume", None, &dir.0.join("result.json"));
    let error =
        cli::run_pipeline_file(&pipeline_path, &[input(&input_path)], &[selected]).unwrap_err();
    assert!(format!("{error:#}").contains("host input `orders` has inconsistent schema"));
    Ok(())
}
