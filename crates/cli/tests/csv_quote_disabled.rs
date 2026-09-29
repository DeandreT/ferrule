use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, Graph, Project, Scope, ScopeConstruction, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_csv_quote_disabled_{}_{}",
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

fn project() -> Project {
    let schema = SchemaNode::group(
        "People",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Age", ScalarType::Int),
        ],
    );
    let csv_options = FormatOptions {
        delimiter: Some(';'),
        csv_quote_disabled: true,
        ..FormatOptions::default()
    };
    Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("input.csv".into()),
        target_path: Some("output.csv".into()),
        source_options: csv_options.clone(),
        target_options: csv_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

#[test]
fn disabled_csv_quoting_applies_to_file_and_payload_runs() -> anyhow::Result<()> {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let csv = b"Name;Age\n\"Jane\";29\n";
    let temp = TempDir::new()?;
    let project_path = temp.0.join("project.json");
    let input_path = temp.0.join("input.csv");
    let output_path = temp.0.join("file.csv");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    std::fs::write(&input_path, csv)?;
    cli::run_project(&project_path, &input_path, &output_path)?;
    assert_eq!(std::fs::read(&output_path)?, csv);

    let input = cli::PayloadDocument::new(Path::new("input.csv"), csv)?;
    let output = cli::run_project_value_payloads(
        &project,
        &project_path,
        &cli::PayloadRunOptions::new(input).with_output_path(Path::new("output.csv")),
    )?;
    assert_eq!(output.artifacts.len(), 1);
    assert_eq!(output.artifacts[0].bytes, csv);
    Ok(())
}
