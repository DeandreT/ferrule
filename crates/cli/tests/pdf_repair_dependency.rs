use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mapping::{
    Binding, FormatOptions, Graph, Node, PdfCapture, PdfCommand, PdfLayout, PdfPageSelection,
    PdfRegion, PdfRepairDependency, Project, Scope,
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_pdf_repair_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    let layout = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap()
    .with_repair_dependency(PdfRepairDependency::ObjectFind);
    Project {
        source: layout.schema(),
        target: layout.schema(),
        source_path: Some("missing.pdf".into()),
        target_path: Some("out.json".into()),
        source_options: FormatOptions {
            pdf: Some(layout),
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn assert_dependency(error: &anyhow::Error) {
    assert!(
        error.chain().any(|cause| matches!(
            cause.downcast_ref::<format_pdf::PdfError>(),
            Some(format_pdf::PdfError::RepairDependency(
                PdfRepairDependency::ObjectFind
            ))
        )),
        "{error:#}"
    );
}

#[test]
fn saved_pdf_repair_blocks_file_overrides_and_payload_identities_without_outputs() {
    let temp = TempDir::new();
    let saved = temp.0.join("project.json");
    std::fs::write(
        &saved,
        mapping::project_file::encode_pretty(&project()).unwrap(),
    )
    .unwrap();
    let output = temp.0.join("sentinel.json");
    std::fs::write(&output, b"sentinel").unwrap();
    for input in [temp.0.join("missing.pdf"), temp.0.join("renamed.csv")] {
        // An existing override must not switch this configured boundary to CSV.
        if input.extension() == Some(std::ffi::OsStr::new("csv")) {
            std::fs::write(&input, b"Value\ndata\n").unwrap();
        }
        assert_dependency(
            &cli::run_project_with_paths(&saved, Some(&input), Some(&output)).unwrap_err(),
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
    }
    for purpose in [
        engine::ExecutionPurpose::Run,
        engine::ExecutionPurpose::Preview,
    ] {
        for identity in ["payload.pdf", "renamed.json"] {
            let document =
                cli::PayloadDocument::new(Path::new(identity), b"{\"Value\":\"data\"}").unwrap();
            let options = cli::PayloadRunOptions::new(document)
                .with_output_path(&output)
                .with_execution_purpose(purpose);
            assert_dependency(&cli::run_project_payloads(&saved, &options).unwrap_err());
            assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
        }
    }
}
