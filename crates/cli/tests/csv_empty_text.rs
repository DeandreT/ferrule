use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    FormatOptions, Graph, Project, Scope, ScopeConstruction, ScopeIteration, TabularBoundaryKind,
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_csv_empty_text_{}_{}",
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
    let row = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("A", ScalarType::String),
            SchemaNode::scalar("B", ScalarType::String),
        ],
    );
    Project {
        source: row.clone(),
        target: SchemaNode::group("Rows", vec![row.repeating()]),
        source_path: Some("input.csv".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            has_header_row: Some(false),
            csv_preserve_empty_strings: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec![]),
                construction: ScopeConstruction::CopyCurrentSource,
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

#[test]
fn empty_text_survives_save_file_payload_and_xml_output() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let project = project();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let path = temp.0.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let project = mapping::project_file::decode_str(&std::fs::read_to_string(&path)?)?;
    assert!(project.source_options.csv_preserve_empty_strings);
    let bytes = b",beta\n\"\",gamma\ndelta\n";
    let input = temp.0.join("input.csv");
    let output = temp.0.join("output.xml");
    std::fs::write(&input, bytes)?;
    cli::run_project(&path, &input, &output)?;
    let file_bytes = std::fs::read(&output)?;
    let payload = cli::PayloadDocument::new(Path::new("input.csv"), bytes)?;
    let result =
        cli::run_project_value_payloads(&project, &path, &cli::PayloadRunOptions::new(payload))?;
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].bytes, file_bytes);
    let text = std::str::from_utf8(&file_bytes)?;
    let parsed = format_xml::from_str(text, &project.target)?;
    let rows = parsed.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 3);
    for row in &rows[..2] {
        assert_eq!(
            row.field("A").and_then(Instance::as_scalar),
            Some(&Value::String(String::new()))
        );
    }
    assert_eq!(
        rows[2].field("B").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
    // Old project documents retain the old input presence policy.
    let encoded = mapping::project_file::encode_pretty(&project)?
        .replace("\"csv_preserve_empty_strings\": true,", "");
    let legacy = mapping::project_file::decode_str(&encoded)?;
    assert!(!legacy.source_options.csv_preserve_empty_strings);
    let result =
        cli::run_project_value_payloads(&legacy, &path, &cli::PayloadRunOptions::new(payload))?;
    assert!(!std::str::from_utf8(&result.artifacts[0].bytes)?.contains("<A></A>"));
    Ok(())
}

#[test]
fn empty_text_policy_rejects_non_csv_formats_before_output_publication() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let mut project = project();
    let path = temp.0.join("project.json");
    let input = temp.0.join("input.xml");
    let output = temp.0.join("output.xml");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    std::fs::write(&input, "<Row><A>x</A><B>y</B></Row>")?;
    std::fs::write(&output, b"sentinel")?;
    let error = cli::run_project(&path, &input, &output).unwrap_err();
    assert!(format!("{error:#}").contains("requires a CSV input boundary"));
    assert_eq!(std::fs::read(&output)?, b"sentinel");
    let input = cli::PayloadDocument::new(Path::new("input.xml"), b"<Row/>")?;
    let error =
        cli::run_project_value_payloads(&project, &path, &cli::PayloadRunOptions::new(input))
            .unwrap_err();
    assert!(format!("{error:#}").contains("requires a CSV input boundary"));
    project.source_options.json_document = true;
    assert!(
        engine::validate(&project)
            .iter()
            .any(|issue| issue.message.contains("csv_preserve_empty_strings"))
    );
    Ok(())
}
