use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, NamedTarget, Node, Pipeline,
    PipelineInput, PipelineNamedInput, PipelineStage, Project, Scope, ScopeConstruction,
    ScopeIteration,
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
        main_mapping_path: None,
        stages: vec![
            PipelineStage {
                id: "consume".into(),
                mapping_path: None,
                project: project(),
                source: PipelineInput::StageTarget {
                    stage: "prepare".into(),
                    target: Some("selected".into()),
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "prepare".into(),
                mapping_path: None,
                project: producer,
                source: PipelineInput::Host {
                    name: "orders".into(),
                },
                extra_sources: Vec::new(),
            },
        ],
    }
}

fn mixed_format_pipeline() -> Pipeline {
    let mut combine = project();
    combine.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Lookup",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        dynamic_path: None,
    });
    combine.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    combine.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["lookup".into(), "Value".into()],
            frame: None,
        },
    );
    combine.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("-".into()),
        },
    );
    combine.graph.nodes.insert(
        3,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 2, 1],
        },
    );
    combine.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 3,
        }],
        ..Scope::default()
    };

    let mut finish = project();
    finish.target_options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    Pipeline {
        main_mapping_path: None,
        // Deliberately reverse dependency order to prove topological execution.
        stages: vec![
            PipelineStage {
                id: "finish".into(),
                mapping_path: None,
                project: finish,
                source: PipelineInput::StageTarget {
                    stage: "combine".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "combine".into(),
                mapping_path: None,
                project: combine,
                source: PipelineInput::StageTarget {
                    stage: "prepare".into(),
                    target: None,
                },
                extra_sources: vec![PipelineNamedInput {
                    name: "lookup".into(),
                    from: PipelineInput::Host { name: "xml".into() },
                }],
            },
            PipelineStage {
                id: "prepare".into(),
                mapping_path: None,
                project: project(),
                source: PipelineInput::Host {
                    name: "json".into(),
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
fn three_stage_mixed_format_pipeline_publishes_only_after_every_stage_succeeds()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut pipeline = mixed_format_pipeline();
    let pipeline_path = dir.pipeline(&pipeline)?;
    let json_input = dir.0.join("source.json");
    let xml_input = dir.0.join("lookup.xml");
    std::fs::write(&json_input, r#"{"Value":"north"}"#)?;
    std::fs::write(&xml_input, "<Lookup><Value>east</Value></Lookup>")?;
    let inputs = [
        cli::PipelineHostFile {
            name: "json".into(),
            path: json_input,
        },
        cli::PipelineHostFile {
            name: "xml".into(),
            path: xml_input,
        },
    ];
    let combined = dir.0.join("combined.json");
    let final_xml = dir.0.join("final.xml");
    let outcome = cli::run_pipeline_file(
        &pipeline_path,
        &inputs,
        &[
            output("combine", None, &combined),
            output("finish", None, &final_xml),
        ],
    )?;
    assert_eq!(outcome.stages_executed, ["prepare", "combine", "finish"]);
    assert_eq!(outcome.artifacts.len(), 2);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&combined)?)?,
        serde_json::json!({"Value": "north-east"})
    );
    let xml = std::fs::read_to_string(&final_xml)?;
    let parsed = format_xml::from_str(&xml, &schema())?;
    assert_eq!(
        parsed.field("Value").and_then(ir::Instance::as_scalar),
        Some(&Value::String("north-east".into()))
    );

    pipeline.stages[0].project.graph.nodes.insert(
        4,
        Node::RuntimeParameter {
            name: "missing".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    pipeline.stages[0].project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 4,
        }],
        ..Scope::default()
    };
    dir.pipeline(&pipeline)?;
    let previous = std::fs::read(&combined)?;
    let unpublished = dir.0.join("unpublished.xml");
    let error = cli::run_pipeline_file(
        &pipeline_path,
        &inputs,
        &[
            output("combine", None, &combined),
            output("finish", None, &unpublished),
        ],
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("missing"));
    assert_eq!(std::fs::read(&combined)?, previous);
    assert!(!unpublished.exists());
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
            preview: None,
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
        mapping_path: None,
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
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "load".into(),
            mapping_path: None,
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
fn stage_local_dynamic_sources_can_share_names_with_distinct_schemas_and_paths()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut pipeline = dynamic_xml_file_set_pipeline();
    let mut second = pipeline.stages[0].clone();
    second.id = "other".into();
    second.project.extra_sources[0].schema.name = "OtherDocument".into();
    second.source = PipelineInput::Host {
        name: "other_files".into(),
    };
    pipeline.stages.push(second);
    let pipeline_path = dir.pipeline(&pipeline)?;
    let first_input = dir.0.join("files.json");
    let second_input = dir.0.join("other-files.json");
    std::fs::write(&first_input, r#"{"File":["records-*.xml"]}"#)?;
    std::fs::write(&second_input, r#"{"File":["other-*.xml"]}"#)?;
    let first_member = dir.0.join("records-a.xml");
    let second_member = dir.0.join("other-b.xml");
    std::fs::write(
        &first_member,
        "<Document><Item><Value>alpha</Value></Item></Document>",
    )?;
    let second_xml = "<OtherDocument><Item><Value>beta</Value></Item></OtherDocument>";
    std::fs::write(&second_member, second_xml)?;
    let inputs = [
        cli::PipelineHostFile {
            name: "files".into(),
            path: first_input,
        },
        cli::PipelineHostFile {
            name: "other_files".into(),
            path: second_input,
        },
    ];
    let first_output = dir.0.join("first.json");
    let second_output = dir.0.join("second.json");
    let outcome = cli::run_pipeline_file(
        &pipeline_path,
        &inputs,
        &[
            output("load", None, &first_output),
            output("other", None, &second_output),
        ],
    )?;
    assert_eq!(outcome.stages_executed, ["load", "other"]);
    assert_eq!(outcome.artifacts.len(), 2);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&first_output)?)?,
        serde_json::json!({"Row": [{"Value": "alpha"}]})
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&second_output)?)?,
        serde_json::json!({"Row": [{"Value": "beta"}]})
    );

    // A source loaded by the second stage is still reserved when the first
    // stage's target is selected for publication at that physical path.
    let unpublished = dir.0.join("unpublished.json");
    let error = cli::run_pipeline_file(
        &pipeline_path,
        &inputs,
        &[
            output("load", None, &second_member),
            output("other", None, &unpublished),
        ],
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("reserved by the host"));
    assert_eq!(std::fs::read_to_string(&second_member)?, second_xml);
    assert!(!unpublished.exists());
    Ok(())
}

#[test]
fn reused_host_file_sources_reject_ambiguous_root_schemas() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
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
        mapping_path: None,
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

#[test]
fn stage_mapping_paths_supply_runtime_identity_and_are_protected() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut pipeline = pipeline();
    let active = dir.0.join("stages/prepare.ferrule.json");
    let main = dir.0.join("maps/original.mfd");
    std::fs::create_dir_all(active.parent().unwrap())?;
    std::fs::create_dir_all(main.parent().unwrap())?;
    std::fs::write(&active, "keep this stage file")?;
    std::fs::write(&main, "keep this main mapping file")?;
    pipeline.main_mapping_path = Some("maps/original.mfd".into());
    pipeline.stages[1].mapping_path = Some("stages/prepare.ferrule.json".into());
    pipeline.stages[0].mapping_path = Some("stages/consume.ferrule.json".into());
    for (index, value) in [
        (1, mapping::RuntimeValue::MappingFilePath),
        (0, mapping::RuntimeValue::MainMappingFilePath),
    ] {
        let stage = &mut pipeline.stages[index];
        stage
            .project
            .graph
            .nodes
            .insert(10, Node::RuntimeValue { value });
        stage.project.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..Scope::default()
        };
    }
    let pipeline_path = dir.pipeline(&pipeline)?;
    let input_path = dir.input()?;
    let prepared = dir.0.join("prepared.json");
    let consumed = dir.0.join("consumed.json");
    let selections = [
        output("prepare", None, &prepared),
        output("consume", None, &consumed),
    ];
    cli::run_pipeline_file(&pipeline_path, &[input(&input_path)], &selections)?;
    let prepared_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&prepared)?)?;
    let consumed_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&consumed)?)?;
    assert_eq!(
        prepared_json["Value"].as_str(),
        active.canonicalize()?.to_str()
    );
    assert_eq!(
        consumed_json["Value"].as_str(),
        main.canonicalize()?.to_str()
    );

    let error = cli::run_pipeline_file(
        &pipeline_path,
        &[input(&input_path)],
        &[output("prepare", None, &active)],
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("reserved by the host"));
    assert_eq!(std::fs::read_to_string(&active)?, "keep this stage file");
    let error = cli::run_pipeline_file(
        &pipeline_path,
        &[input(&input_path)],
        &[output("consume", None, &main)],
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("reserved by the host"));
    assert_eq!(
        std::fs::read_to_string(&main)?,
        "keep this main mapping file"
    );

    pipeline.stages[1].mapping_path = None;
    pipeline.main_mapping_path = None;
    dir.pipeline(&pipeline)?;
    let legacy_json = std::fs::read_to_string(&pipeline_path)?;
    assert!(!legacy_json.contains("main_mapping_path"));
    let legacy: Pipeline = serde_json::from_str(&legacy_json)?;
    assert_eq!(legacy.main_mapping_path, None);
    cli::run_pipeline_file(
        &pipeline_path,
        &[input(&input_path)],
        &[output("prepare", None, &prepared)],
    )?;
    let default_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&prepared)?)?;
    assert_eq!(
        default_json["Value"].as_str(),
        pipeline_path.canonicalize()?.to_str()
    );
    Ok(())
}

mod source_ownership_admission {
    use super::*;

    struct Case {
        name: &'static str,
        pipeline: Pipeline,
        stage: &'static str,
        published_stage: &'static str,
        expected: engine::EngineError,
        display: &'static str,
    }

    fn owned_field() -> Node {
        Node::OwnedSourceField(mapping::OwnedSourceField {
            source: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: vec!["Value".into()],
            },
            frame: None,
        })
    }

    fn bound(node: u32) -> Scope {
        Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node,
            }],
            ..Scope::default()
        }
    }

    fn one(project: Project) -> Pipeline {
        Pipeline {
            main_mapping_path: None,
            stages: vec![PipelineStage {
                id: "one".into(),
                mapping_path: None,
                project,
                source: PipelineInput::Host {
                    name: "orders".into(),
                },
                extra_sources: vec![],
            }],
        }
    }

    fn unused_owned_project(lazy: bool) -> Project {
        let mut project = project();
        project.graph.nodes.insert(47, owned_field());
        if lazy {
            project.graph.nodes.insert(
                62,
                Node::Const {
                    value: Value::Bool(true),
                },
            );
            project.graph.nodes.insert(
                63,
                Node::Const {
                    value: Value::String("original".into()),
                },
            );
            project
                .graph
                .nodes
                .insert(65, Node::Raise { message: None });
            project.graph.nodes.insert(
                64,
                Node::If {
                    condition: 62,
                    then: 63,
                    else_: 65,
                },
            );
            project.root = bound(64);
        }
        project
    }

    fn cases() -> Vec<Case> {
        let mut direct = unused_owned_project(false);
        direct.root = bound(47);
        let expected = || engine::EngineError::UnsupportedSourceOwnership {
            node: 47,
            function: None,
            kind: "owned_source_field",
        };
        let field_display =
            "source ownership for owned_source_field node 47 (function None) is not supported";
        let mut position = project();
        position.target =
            SchemaNode::group("Record", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
        position.graph.nodes.insert(
            48,
            Node::OwnedPosition(mapping::OwnedPosition {
                owner: mapping::SourceOwner::Primary,
                collection: Some(vec![]),
            }),
        );
        position.root = bound(48);

        let mut hidden = unused_owned_project(false);
        hidden.extra_targets.push(NamedTarget {
            name: "hidden".into(),
            path: Some("hidden.json".into()),
            schema: hidden.target.clone(),
            options: hidden.target_options.clone(),
            root: bound(47),
        });
        let mut disconnected = one(project());
        disconnected.stages.push(PipelineStage {
            id: "disconnected".into(),
            mapping_path: None,
            project: direct.clone(),
            source: PipelineInput::Host {
                name: "orders".into(),
            },
            extra_sources: vec![],
        });

        let function = mapping::FunctionId::new(7);
        let mut called = project();
        called.user_functions.insert(
            function,
            mapping::UserFunction {
                library: "tests".into(),
                name: "owned_value".into(),
                description: None,
                parameters: vec![],
                output_name: "value".into(),
                output_type: ScalarType::String,
                body: Graph {
                    nodes: BTreeMap::from([(47, owned_field())]),
                },
                output: 47,
            },
        );
        called.graph.nodes.insert(
            61,
            Node::UserFunctionCall {
                function,
                args: vec![],
            },
        );
        called.root = bound(61);

        let mut potential = unused_owned_project(false);
        potential.graph.nodes.insert(
            62,
            Node::Const {
                value: Value::Bool(false),
            },
        );
        potential.graph.nodes.insert(
            63,
            Node::Const {
                value: Value::String("original".into()),
            },
        );
        potential.graph.nodes.insert(
            64,
            Node::If {
                condition: 62,
                then: 47,
                else_: 63,
            },
        );
        potential.root = bound(64);
        let mut forward = one(direct.clone());
        forward.stages[0].id = "consume".into();
        forward.stages[0].source = PipelineInput::StageTarget {
            stage: "prepare".into(),
            target: None,
        };
        let mut producer = one(direct.clone()).stages.remove(0);
        producer.id = "prepare".into();
        forward.stages.push(producer);
        vec![
            Case {
                name: "direct-field",
                pipeline: one(direct),
                stage: "one",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "direct-position",
                pipeline: one(position),
                stage: "one",
                published_stage: "one",
                expected: engine::EngineError::UnsupportedSourceOwnership {
                    node: 48,
                    function: None,
                    kind: "owned_position",
                },
                display: "source ownership for owned_position node 48 (function None) is not supported",
            },
            Case {
                name: "unselected-named-target",
                pipeline: one(hidden),
                stage: "one",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "disconnected-stage",
                pipeline: disconnected,
                stage: "disconnected",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "called-function",
                pipeline: one(called),
                stage: "one",
                published_stage: "one",
                expected: engine::EngineError::UnsupportedSourceOwnership {
                    node: 47,
                    function: Some(function),
                    kind: "owned_source_field",
                },
                display: "source ownership for owned_source_field node 47 (function Some(FunctionId(7))) is not supported",
            },
            Case {
                name: "potential-if",
                pipeline: one(potential),
                stage: "one",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "forward-stage-order",
                pipeline: forward,
                stage: "prepare",
                published_stage: "consume",
                expected: expected(),
                display: field_display,
            },
        ]
    }

    fn assert_refusal(error: &anyhow::Error, case: &Case) {
        let chain = error.chain().map(ToString::to_string).collect::<Vec<_>>();
        eprintln!(
            "{} full_chain={chain:?} typed={:?}",
            case.name,
            error.downcast_ref::<engine::PipelineError>()
        );
        match error.downcast_ref::<engine::PipelineError>() {
            Some(engine::PipelineError::StageExecution { stage, source }) => {
                assert_eq!(stage.as_str(), case.stage);
                assert_eq!(source, &case.expected);
            }
            other => panic!("expected stage ownership failure, got {other:?}"),
        }
        assert_eq!(
            chain,
            [
                format!("stage `{}` failed: {}", case.stage, case.display),
                case.display.to_owned()
            ]
        );
    }

    #[test]
    fn all_stages_and_targets_refuse_before_file_acquisition() -> Result<(), Box<dyn Error>> {
        for case in cases() {
            let dir = TempDir::new()?;
            let pipeline_path = dir.pipeline(&case.pipeline)?;
            let input_path = dir.0.join("unavailable.json");
            if case.name == "direct-position" {
                std::fs::write(&input_path, b"not-json")?;
            }
            let destination = dir.0.join("selected.json");
            std::fs::write(&destination, b"old-output")?;
            let debug_calls = std::cell::RefCell::new(Vec::<String>::new());
            let trace_calls = std::cell::RefCell::new(Vec::<(String, String)>::new());
            let debug = |stage: &str, _: &engine::PendingTargetWrite| {
                debug_calls.borrow_mut().push(stage.to_owned());
                engine::DebugDecision::Resume
            };
            let trace = |stage: &str, event: engine::TraceEvent| {
                trace_calls
                    .borrow_mut()
                    .push((stage.to_owned(), format!("{event:?}")));
            };
            eprintln!(
                "{} fixture={:?} input_before={:?} output_before={:?}",
                case.name,
                std::fs::read(&pipeline_path)?,
                std::fs::read(&input_path),
                std::fs::read(&destination)?
            );
            let result = cli::run_pipeline_file_with_options(
                &pipeline_path,
                &[input(&input_path)],
                &[output(case.published_stage, None, &destination)],
                &cli::PipelineRunOptions::default()
                    .with_stage_debug_hook(&debug)
                    .with_stage_trace_sink(&trace),
            );
            let output_after = std::fs::read(&destination)?;
            eprintln!(
                "{} result={result:?} output_after={output_after:?} debug={:?} trace={:?}",
                case.name,
                debug_calls.borrow(),
                trace_calls.borrow()
            );
            let error =
                result.expect_err("capability refusal must precede missing/invalid file input");
            assert_refusal(&error, &case);
            assert_eq!(output_after.as_slice(), b"old-output".as_slice());
            assert!(debug_calls.borrow().is_empty());
            assert!(trace_calls.borrow().is_empty());
        }
        Ok(())
    }

    #[test]
    fn demanded_dynamic_path_refuses_before_primary_or_dynamic_file_read()
    -> Result<(), Box<dyn Error>> {
        let dir = TempDir::new()?;
        let mut project = unused_owned_project(false);
        project.extra_sources.push(NamedSource {
            name: "dynamic".into(),
            path: "absent-dynamic.json".into(),
            schema: schema(),
            options: project.source_options.clone(),
            dynamic_path: Some(DynamicSourcePath {
                iteration: vec![],
                node: 47,
            }),
        });
        project.graph.nodes.insert(
            61,
            Node::SourceField {
                path: vec!["dynamic".into(), "Value".into()],
                frame: None,
            },
        );
        project.root = bound(61);
        let case = Case {
            name: "demanded-dynamic-path",
            pipeline: one(project),
            stage: "one",
            published_stage: "one",
            expected: engine::EngineError::UnsupportedSourceOwnership {
                node: 47,
                function: None,
                kind: "owned_source_field",
            },
            display: "source ownership for owned_source_field node 47 (function None) is not supported",
        };
        let pipeline_path = dir.pipeline(&case.pipeline)?;
        let input_path = dir.0.join("absent-primary.json");
        let destination = dir.0.join("selected.json");
        std::fs::write(&destination, b"old-output")?;
        eprintln!(
            "dynamic fixture={:?} primary_before={:?} dynamic_before={:?}",
            std::fs::read(&pipeline_path)?,
            std::fs::read(&input_path),
            std::fs::read(dir.0.join("absent-dynamic.json"))
        );
        let result = cli::run_pipeline_file(
            &pipeline_path,
            &[input(&input_path)],
            &[output("one", None, &destination)],
        );
        let output_after = std::fs::read(&destination)?;
        eprintln!("dynamic result={result:?} output_after={output_after:?}");
        assert_refusal(
            &result.expect_err("demanded path must be admitted before acquisition"),
            &case,
        );
        assert_eq!(output_after.as_slice(), b"old-output".as_slice());
        Ok(())
    }

    #[test]
    fn host_and_output_identity_errors_keep_precedence_over_ownership() -> Result<(), Box<dyn Error>>
    {
        let dir = TempDir::new()?;
        let case = cases().remove(0);
        let pipeline_path = dir.pipeline(&case.pipeline)?;
        let input_path = dir.0.join("absent.json");
        let destination = dir.0.join("selected.json");
        std::fs::write(&destination, b"old-output")?;
        for (name, inputs, outputs, expected) in [
            (
                "missing-binding",
                vec![],
                vec![output("one", None, &destination)],
                "host input `orders` is missing",
            ),
            (
                "unknown-output-stage",
                vec![input(&input_path)],
                vec![output("ghost", None, &destination)],
                "selected output has unknown stage `ghost`",
            ),
        ] {
            let result = cli::run_pipeline_file(&pipeline_path, &inputs, &outputs);
            let output_after = std::fs::read(&destination)?;
            eprintln!("{name} result={result:?} output_after={output_after:?}");
            let error = result.expect_err("pure identity error must precede capability admission");
            let chain = error.chain().map(ToString::to_string).collect::<Vec<_>>();
            eprintln!(
                "{name} full_chain={chain:?} typed={:?}",
                error.downcast_ref::<engine::EngineError>()
            );
            assert_eq!(chain, [expected.to_owned()]);
            assert!(error.downcast_ref::<engine::EngineError>().is_none());
            assert_eq!(output_after.as_slice(), b"old-output".as_slice());
        }
        Ok(())
    }

    #[test]
    fn unused_owned_storage_preserves_copy_and_legacy_lazy_execution() -> Result<(), Box<dyn Error>>
    {
        for lazy in [false, true] {
            let dir = TempDir::new()?;
            let pipeline_path = dir.pipeline(&one(unused_owned_project(lazy)))?;
            let input_path = dir.input()?;
            let destination = dir.0.join("selected.json");
            let result = cli::run_pipeline_file(
                &pipeline_path,
                &[input(&input_path)],
                &[output("one", None, &destination)],
            );
            eprintln!("unused lazy={lazy} result={result:?}");
            let outcome = result?;
            let output_body = std::fs::read(&destination);
            eprintln!("unused lazy={lazy} complete_output={output_body:?}");
            let tree: serde_json::Value = serde_json::from_slice(&output_body?)?;
            assert_eq!(
                outcome,
                cli::PipelineRunOutcome {
                    stages_executed: vec!["one".into()],
                    artifacts: vec![cli::PipelineArtifact {
                        stage: "one".into(),
                        target: None,
                        path: destination,
                        records_written: 1
                    }],
                }
            );
            assert_eq!(tree, serde_json::json!({"Value":"original"}));
        }
        Ok(())
    }
}
