use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    FormatOptions, Graph, NamedTarget, Project, Scope, ScopeConstruction, ScopeIteration,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-csv-bom-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_rows() -> Scope {
    Scope {
        iteration: ScopeIteration::Source(Vec::new()),
        construction: ScopeConstruction::CopyCurrentSource,
        ..Scope::default()
    }
}

fn project() -> Project {
    let schema = SchemaNode::group(
        "People",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Age", ScalarType::Int),
        ],
    );
    Project {
        source: schema.clone(),
        target: schema.clone(),
        source_path: Some("input.csv".into()),
        target_path: Some("output.csv".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions {
            csv_utf8_bom: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "plain".into(),
            path: Some("plain.csv".into()),
            schema,
            options: FormatOptions::default(),
            root: copy_rows(),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: copy_rows(),
    }
}

#[test]
fn csv_bom_is_exact_across_saved_project_file_payload_and_named_targets() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let project_path = temp.0.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let mut loaded = mapping::project_file::decode_str(&std::fs::read_to_string(&project_path)?)?;
    assert!(loaded.target_options.csv_utf8_bom);
    assert!(!loaded.extra_targets[0].options.csv_utf8_bom);

    let input = b"Name,Age\nCaf\xc3\xa9,29\n";
    let plain = b"Name,Age\nCaf\xc3\xa9,29\n";
    let marked = b"\xef\xbb\xbfName,Age\nCaf\xc3\xa9,29\n";
    let input_path = temp.0.join("input.csv");
    let output_path = temp.0.join("output.csv");
    std::fs::write(&input_path, input)?;
    cli::run_project(&project_path, &input_path, &output_path)?;
    assert_eq!(std::fs::read(&output_path)?, marked);
    assert_eq!(std::fs::read(temp.0.join("plain.csv"))?, plain);

    let payload = cli::PayloadDocument::new(Path::new("input.csv"), input)?;
    let outcome = cli::run_project_value_payloads(
        &loaded,
        &project_path,
        &cli::PayloadRunOptions::new(payload).with_output_path(Path::new("output.csv")),
    )?;
    assert_eq!(outcome.artifacts.len(), 2);
    assert_eq!(outcome.artifacts[0].bytes, marked);
    assert_eq!(outcome.artifacts[1].bytes, plain);

    loaded.target_options.csv_utf8_bom = false;
    loaded.extra_targets[0].options.csv_utf8_bom = true;
    let payload = cli::PayloadDocument::new(Path::new("input.csv"), input)?;
    let swapped = cli::run_project_value_payloads(
        &loaded,
        &project_path,
        &cli::PayloadRunOptions::new(payload).with_output_path(Path::new("output.csv")),
    )?;
    assert_eq!(swapped.artifacts[0].bytes, plain);
    assert_eq!(swapped.artifacts[1].bytes, marked);

    let mut legacy = serde_json::to_value(&project)?;
    legacy["target_options"]
        .as_object_mut()
        .unwrap()
        .remove("csv_utf8_bom");
    let old = mapping::project_file::decode_str(&serde_json::to_string(&legacy)?)?;
    assert!(!old.target_options.csv_utf8_bom);
    Ok(())
}

#[test]
fn bom_on_an_xml_target_rejects_without_changing_existing_output() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let mut project = project();
    project.target_path = Some("output.xml".into());
    let project_path = temp.0.join("project.json");
    let input_path = temp.0.join("input.csv");
    let output_path = temp.0.join("output.xml");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    std::fs::write(&input_path, b"Name,Age\nAda,37\n")?;
    std::fs::write(&output_path, b"sentinel")?;
    let error = cli::run_project(&project_path, &input_path, &output_path).unwrap_err();
    assert!(format!("{error:#}").contains("csv_utf8_bom"));
    assert_eq!(std::fs::read(&output_path)?, b"sentinel");
    Ok(())
}
