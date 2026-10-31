//! X12 projects that select an LF or CR segment terminator write it alone.
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    EdiBoundaryKind, FormatOptions, Graph, Project, Scope, ScopeConstruction, X12Separators,
};

const LF_INTERCHANGE: &str = "ISA*00*          *00*          *ZZ*SENDER-01      *ZZ*RECEIVER-01    *261009*1030*U*00401*000000321*0*P*>\n\
GS*OW*SENDER-01*RECEIVER-01*20261009*1030*00321*X*004010\n\
ST*940*0009\n\
W05*N*DEMO-ORDER-0042\n\
SE*3*0009\n\
GE*1*00321\n\
IEA*1*000000321\n";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-x12-line-break-{}-{}",
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

fn segment(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| SchemaNode::scalar(format!("{index:02}"), ScalarType::String))
            .collect(),
    )
}

fn project(segment_terminator: char) -> Project {
    let schema = SchemaNode::group(
        "Interchange",
        vec![
            segment("ISA", 16),
            segment("GS", 8),
            segment("ST", 2),
            segment("W05", 2),
            segment("SE", 2),
            segment("GE", 2),
            segment("IEA", 2),
        ],
    );
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::X12),
        x12_separators: Some(X12Separators {
            element: '*',
            component: '>',
            segment: segment_terminator,
            repetition: None,
            release: None,
        }),
        ..FormatOptions::default()
    };
    Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("input.x12".into()),
        target_path: Some("output.x12".into()),
        source_options: options.clone(),
        target_options: options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

#[test]
fn line_break_terminated_interchanges_write_back_byte_identical() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    for terminator in ['\n', '\r'] {
        let project = project(terminator);
        assert!(engine::validate(&project).is_empty());
        let project_path = temp.0.join("project.json");
        std::fs::write(
            &project_path,
            mapping::project_file::encode_pretty(&project)?,
        )?;
        let wire = LF_INTERCHANGE.replace('\n', &terminator.to_string());
        let input = temp.0.join("input.x12");
        let output = temp.0.join("output.x12");
        std::fs::write(&input, &wire)?;
        cli::run_project(&project_path, &input, &output)?;
        assert_eq!(std::fs::read(&output)?, wire.as_bytes(), "{terminator:?}");
    }
    Ok(())
}

#[test]
fn a_cr_lf_segment_terminator_cannot_be_selected() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let encoded = mapping::project_file::encode_pretty(&project('\r'))?;
    let crlf = encoded.replace("\"segment\": \"\\r\"", "\"segment\": \"\\r\\n\"");
    assert_ne!(crlf, encoded);
    let project_path = temp.0.join("project.json");
    std::fs::write(&project_path, crlf)?;
    let input = temp.0.join("input.x12");
    let output = temp.0.join("output.x12");
    std::fs::write(&input, LF_INTERCHANGE.replace('\n', "\r\n"))?;
    let error = cli::run_project(&project_path, &input, &output).unwrap_err();
    assert!(
        format!("{error:#}").contains("expected a character"),
        "{error:#}"
    );
    assert!(!output.exists());
    Ok(())
}
