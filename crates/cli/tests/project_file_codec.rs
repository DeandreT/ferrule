use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, Node, Pipeline, PipelineInput, PipelineStage, Project, Scope,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp-cli-project-file-codec");
        std::fs::create_dir_all(&base)?;
        let path = base.join(format!(
            "case-{}-{}",
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

fn project(value: Value) -> Project {
    let mut nodes = BTreeMap::new();
    nodes.insert(0, Node::Const { value });
    nodes.insert(
        1,
        Node::Call {
            function: "string".into(),
            args: vec![0],
        },
    );
    let json = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Text", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: json.clone(),
        target_options: json,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Text".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

fn pipeline(project: Project) -> Pipeline {
    Pipeline {
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "map".into(),
            mapping_path: None,
            project,
            source: PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: Vec::new(),
        }],
    }
}

fn text_output(path: &Path) -> Result<String, Box<dyn Error>> {
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    Ok(result["Text"]
        .as_str()
        .ok_or("mapped Text is a JSON string")?
        .to_owned())
}

#[test]
fn physical_project_and_pipeline_files_preserve_adjacent_float_bits() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    let low = f64::from_bits(0x0031_fa18_2c40_c60d);
    let high = f64::from_bits(0x0031_fa18_2c40_c60e);
    assert_ne!(low.to_string(), high.to_string());
    let project = project(Value::Float(low));
    let input = directory.0.join("input.json");
    std::fs::write(&input, "{}")?;

    let project_path = directory.0.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    assert!(cli::validate_project(&project_path)?.is_empty());
    let project_output = directory.0.join("project-output.json");
    assert_eq!(cli::run_project(&project_path, &input, &project_output)?, 1);
    assert_eq!(text_output(&project_output)?, low.to_string());

    let pipeline_path = directory.0.join("pipeline.json");
    std::fs::write(
        &pipeline_path,
        mapping::pipeline_file::encode_pretty(&pipeline(project))?,
    )?;
    let pipeline_output = directory.0.join("pipeline-output.json");
    let outcome = cli::run_pipeline_file(
        &pipeline_path,
        &[cli::PipelineHostFile {
            name: "input".into(),
            path: input.clone(),
        }],
        &[cli::PipelineOutputFile {
            stage: "map".into(),
            target: None,
            path: pipeline_output.clone(),
        }],
    )?;
    assert_eq!(outcome.stages_executed, ["map"]);
    assert_eq!(text_output(&pipeline_output)?, low.to_string());
    Ok(())
}

#[test]
fn legacy_unmarked_project_and_pipeline_files_still_run() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let project = project(Value::String("legacy".into()));
    let input = directory.0.join("input.json");
    std::fs::write(&input, "{}")?;
    let project_path = directory.0.join("legacy-project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let project_output = directory.0.join("legacy-project-output.json");
    assert_eq!(cli::run_project(&project_path, &input, &project_output)?, 1);
    assert_eq!(text_output(&project_output)?, "legacy");

    let pipeline_path = directory.0.join("legacy-pipeline.json");
    std::fs::write(
        &pipeline_path,
        serde_json::to_vec_pretty(&pipeline(project))?,
    )?;
    let pipeline_output = directory.0.join("legacy-pipeline-output.json");
    cli::run_pipeline_file(
        &pipeline_path,
        &[cli::PipelineHostFile {
            name: "input".into(),
            path: input,
        }],
        &[cli::PipelineOutputFile {
            stage: "map".into(),
            target: None,
            path: pipeline_output.clone(),
        }],
    )?;
    assert_eq!(text_output(&pipeline_output)?, "legacy");
    Ok(())
}
