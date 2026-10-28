use ir::Value;
use mapping::{Node, Project};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-document-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("source.mfd"),
            include_str!("../../mfd/tests/fixtures/csv_document_output/grouped.mfd"),
        )
        .unwrap();
        std::fs::write(
            path.join("input.csv"),
            "Code,Note\nA,one\nB,two\nC,three\nD,four\nE,five\n",
        )
        .unwrap();
        Self(path)
    }
    fn project(&self) -> Project {
        let imported = mfd::import(&self.0.join("source.mfd")).unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        imported.project
    }
    fn save(&self, project: &Project) -> PathBuf {
        let path = self.0.join("project.json");
        std::fs::write(
            &path,
            mapping::project_file::encode_pretty(project).unwrap(),
        )
        .unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn csv_documents_publish_every_row_and_empty_input_publishes_no_files() {
    let root = Directory::new();
    let path = root.save(&root.project());
    let output = root.0.join("output");
    let result = cli::run_project_with_paths(&path, None, Some(&output)).unwrap();
    assert_eq!(result.records_written, 5);
    assert_eq!(result.artifacts.len(), 3);
    for (index, expected) in [
        "Code;Note\nA;one\nB;two\n",
        "Code;Note\nC;three\nD;four\n",
        "Code;Note\nE;five\n",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            std::fs::read(output.join(format!("batch-{}.csv", index + 1))).unwrap(),
            expected.as_bytes()
        );
    }
    std::fs::write(root.0.join("input.csv"), "Code,Note\n").unwrap();
    let empty = root.0.join("empty");
    let result = cli::run_project_with_paths(&path, None, Some(&empty)).unwrap();
    assert!(result.artifacts.is_empty());
    assert!(!empty.exists() || std::fs::read_dir(empty).unwrap().next().is_none());
}

#[test]
fn late_filename_and_duplicate_path_failures_preserve_existing_outputs() {
    for duplicate in [false, true] {
        let root = Directory::new();
        let mut project = root.project();
        let first = project.graph.nodes.keys().max().copied().unwrap() + 1;
        let original = project.root.output_path().unwrap();
        let filename = if duplicate {
            project.graph.nodes.insert(
                first,
                Node::Const {
                    value: Value::String("batch-1.csv".into()),
                },
            );
            first
        } else {
            project.graph.nodes.extend([
                (first, Node::Position { collection: vec![] }),
                (
                    first + 1,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (
                    first + 2,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![first, first + 1],
                    },
                ),
                (
                    first + 3,
                    Node::Const {
                        value: Value::String(String::new()),
                    },
                ),
                (
                    first + 4,
                    Node::If {
                        condition: first + 2,
                        then: first + 3,
                        else_: original,
                    },
                ),
            ]);
            first + 4
        };
        assert!(project.root.set_output_path(Some(filename)));
        let path = root.save(&project);
        let output = root.0.join("output");
        std::fs::create_dir(&output).unwrap();
        std::fs::write(output.join("batch-1.csv"), b"existing").unwrap();
        let error = cli::run_project_with_paths(&path, None, Some(&output)).unwrap_err();
        assert!(
            format!("{error:#}").contains(if duplicate { "duplicate" } else { "empty" }),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read(output.join("batch-1.csv")).unwrap(),
            b"existing"
        );
        assert_eq!(std::fs::read_dir(output).unwrap().count(), 1);
    }
}

#[test]
fn csv_document_generation_does_not_reuse_the_singular_profile() {
    let root = Directory::new();
    let path = root.save(&root.project());
    let output = root.0.join("singular");
    assert!(
        cli::generate_project_with_profile(
            &path,
            &output,
            cli::GenerateTarget::CSharp,
            cli::GenerateProfile::CsvOutput
        )
        .is_err()
    );
    assert!(!output.exists());
    cli::generate_project_with_profile(
        &path,
        &root.0.join("documents"),
        cli::GenerateTarget::CSharp,
        cli::GenerateProfile::CsvDocumentOutput,
    )
    .unwrap();
}

#[test]
fn late_csv_encoding_failure_preserves_existing_files() {
    let root = Directory::new();
    let mut project = root.project();
    project.target_options.csv_quote_disabled = true;
    project.target_options.csv_quote = None;
    std::fs::write(
        root.0.join("input.csv"),
        "Code,Note\nA,one\nB,two\nC,three\nD,four\nE,bad;value\n",
    )
    .unwrap();
    let path = root.save(&project);
    let output = root.0.join("output");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("batch-1.csv"), b"existing").unwrap();
    let error = cli::run_project_with_paths(&path, None, Some(&output)).unwrap_err();
    assert!(
        matches!(error.downcast_ref::<format_csv::CsvDocumentsError>(),
        Some(format_csv::CsvDocumentsError::Document { document: 2, source: format_csv::CsvBoundedError::Format(format_csv::CsvFormatError::UnquotedFieldBoundary { row: 0, field }), .. }) if field == "Note")
    );
    assert_eq!(
        std::fs::read(output.join("batch-1.csv")).unwrap(),
        b"existing"
    );
    assert_eq!(std::fs::read_dir(output).unwrap().count(), 1);
}
