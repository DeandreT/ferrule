use super::*;
use ir::ScalarType;
use mapping::{Binding, Node, Scope, ScopeIteration, TabularBoundaryKind};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-primary-csv-dispatch-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn csv_target(path: &std::path::Path) -> CsvBoundaryDraft {
    let mut draft = CsvBoundaryDraft::target();
    draft.path = path.to_str().expect("UTF-8 test path").to_owned();
    draft.columns = vec![
        CsvColumnDraft {
            name: "Name".to_owned(),
            ty: ScalarType::String,
        },
        CsvColumnDraft {
            name: "Age".to_owned(),
            ty: ScalarType::Int,
        },
    ];
    draft
}

fn bind_columns(project: &mut mapping::Project) {
    project.root = Scope {
        iteration: ScopeIteration::Source(Vec::new()),
        ..Scope::default()
    };
    for (id, name) in [(0, "Name"), (1, "Age")] {
        project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.to_owned()],
                frame: None,
            },
        );
        project.root.bindings.push(Binding {
            target_field: name.to_owned(),
            node: id,
        });
    }
}

#[test]
fn primary_csv_setup_rejects_known_non_csv_source_identities_without_replacing_project()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    for suffix in ["db", "json", "xlsx", "PDF", "json5", "xml"] {
        let path = directory.0.join(format!("source.{suffix}"));
        std::fs::write(&path, b"Name,Age\nJane,42\n")?;
        let mut app = FerruleApp::default();
        let original = mapping::project_file::encode_pretty(&app.project)?;
        app.begin_new_mapping();
        app.stage_mapping_csv_source(path);
        let setup = app.new_mapping_setup.as_mut().expect("mapping setup");
        setup.target = Some(MappingBoundary::Csv(csv_target(
            &directory.0.join("output.csv"),
        )));
        let Some(MappingBoundary::Csv(source)) = setup.source.as_ref() else {
            panic!("explicit CSV source sample should remain editable");
        };
        assert!(
            source
                .validate()
                .unwrap_err()
                .to_string()
                .contains("different format")
        );
        assert!(!setup.can_create());
        app.finish_new_mapping();
        assert!(app.new_mapping_setup.is_some());
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        assert!(!directory.0.join("output.csv").exists());
    }
    Ok(())
}

#[test]
fn primary_csv_setup_rejects_known_non_csv_target_identities_and_preserves_the_draft()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source_path = directory.0.join("source.csv");
    std::fs::write(&source_path, b"Name,Age\nJane,42\n")?;
    for suffix in ["sqlite", "json", "xlsx", "XBRL", "jsonl", "xml"] {
        let output_path = directory.0.join(format!("output.{suffix}"));
        let mut app = FerruleApp::default();
        let original = mapping::project_file::encode_pretty(&app.project)?;
        app.begin_new_mapping();
        app.stage_mapping_csv_source(source_path.clone());
        app.new_mapping_setup
            .as_mut()
            .expect("mapping setup")
            .target = Some(MappingBoundary::Csv(csv_target(
            &directory.0.join("initial.csv"),
        )));
        app.stage_mapping_csv_target_output(output_path.to_str().unwrap().to_owned());
        let setup = app.new_mapping_setup.as_ref().expect("mapping setup");
        let Some(MappingBoundary::Csv(target)) = setup.target.as_ref() else {
            panic!("CSV target");
        };
        assert!(
            target
                .validate()
                .unwrap_err()
                .to_string()
                .contains("different format")
        );
        assert!(!setup.can_create());
        app.finish_new_mapping();
        let Some(MappingBoundary::Csv(target)) =
            app.new_mapping_setup.as_ref().unwrap().target.as_ref()
        else {
            panic!("invalid target must remain editable");
        };
        assert_eq!(target.path, output_path.to_str().unwrap());
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        assert!(!output_path.exists());
    }
    Ok(())
}

#[test]
fn primary_csv_fallback_names_and_tsv_save_reopen_and_execute_as_csv() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    for (source_name, output_name, text, delimiter) in [
        ("source", "output", "Name,Age\nJane,42\n", ','),
        ("source.dat", "output.unknown", "Name,Age\nJane,42\n", ','),
        ("source.tsv", "output.tsv", "Name\tAge\nJane\t42\n", '\t'),
    ] {
        let source_path = directory.0.join(source_name);
        let output_path = directory.0.join(output_name);
        let project_path = directory.0.join(format!("{source_name}-mapping.json"));
        std::fs::write(&source_path, text)?;
        let mut app = FerruleApp::default();
        app.begin_new_mapping();
        app.stage_mapping_csv_source(source_path.clone());
        let setup = app.new_mapping_setup.as_mut().unwrap();
        let Some(MappingBoundary::Csv(source)) = setup.source.as_mut() else {
            panic!("CSV source");
        };
        assert_eq!(source.delimiter, delimiter);
        assert_eq!(source.preview_rows, vec![vec!["Jane", "42"]]);
        source.columns[1].ty = ScalarType::Int;
        setup.target = Some(MappingBoundary::Csv(csv_target(
            &directory.0.join("initial.csv"),
        )));
        app.stage_mapping_csv_target_output(output_path.to_str().unwrap().to_owned());
        assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
        app.finish_new_mapping();
        assert!(app.new_mapping_setup.is_none());
        bind_columns(&mut app.project);
        assert!(cli::validate(&app.project).is_empty());
        app.save_document_to(&project_path)?;
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&project_path);
        assert!(!reopened.is_dirty());
        for options in [
            &reopened.project.source_options,
            &reopened.project.target_options,
        ] {
            assert_eq!(options.tabular_kind, Some(TabularBoundaryKind::Csv));
            assert_eq!(options.delimiter, Some(delimiter));
            assert_eq!(options.has_header_row, Some(true));
        }
        cli::run_project_with_paths(&project_path, None, None)?;
        assert_eq!(std::fs::read(&output_path)?, text.as_bytes());
        std::fs::remove_file(&source_path)?;
        std::fs::write(&output_path, b"sentinel")?;
        let options =
            cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, text.as_bytes())?)
                .with_output_path(&output_path);
        let outcome = cli::run_project_payloads(&project_path, &options)?;
        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(outcome.artifacts[0].records_written, 1);
        assert_eq!(outcome.artifacts[0].bytes, text.as_bytes());
        assert_eq!(std::fs::read(&output_path)?, b"sentinel");
    }
    Ok(())
}
