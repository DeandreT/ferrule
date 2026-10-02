use super::*;
use crate::new_mapping::ProtobufBoundaryDraft;

impl FerruleApp {
    pub(in crate::app) fn stage_mapping_protobuf(&mut self, side: SchemaSide, path: PathBuf) {
        match ProtobufBoundaryDraft::from_schema(path) {
            Ok(draft) => {
                if let Some(setup) = self.new_mapping_setup.as_mut() {
                    let boundary = MappingBoundary::Protobuf(Box::new(draft));
                    match side {
                        SchemaSide::Source => setup.source = Some(boundary),
                        SchemaSide::Target => setup.target = Some(boundary),
                    }
                    self.status = format!(
                        "loaded {} Protocol Buffers schema; choose a root message",
                        side.label().to_lowercase()
                    );
                    self.diagnostics.clear();
                }
            }
            Err(error) => {
                self.status = format!(
                    "failed to load {} Protocol Buffers schema",
                    side.label().to_lowercase()
                );
                self.diagnostics.error(
                    "Protocol Buffers schema import failed",
                    format!("{error:#}"),
                );
            }
        }
    }
}

pub(super) fn show_options(
    ui: &mut egui::Ui,
    draft: &mut ProtobufBoundaryDraft,
    side: &str,
    target: bool,
) {
    crate::new_mapping::show_protobuf_root_message(ui, draft, ("primary", side));
    ui.horizontal(|ui| {
        ui.label(if target {
            "Output file (optional)"
        } else {
            "Input file (optional)"
        });
        ui.add(
            egui::TextEdit::singleline(&mut draft.instance_path)
                .desired_width(380.0)
                .hint_text(if target { "output.bin" } else { "input.bin" }),
        );
    });
    ui.weak(
        "The project keeps its schema and local imports. You can choose a data file when running.",
    );
    if let Err(error) = draft.validate() {
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{Instance, Value};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> anyhow::Result<Self> {
            let directory = std::env::temp_dir().join(format!(
                "ferrule-gui-protobuf-workflow-{}-{}",
                std::process::id(),
                NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&directory)?;
            Ok(Self(directory))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn staged_draft(app: &mut FerruleApp, side: SchemaSide) -> &mut ProtobufBoundaryDraft {
        let setup = app.new_mapping_setup.as_mut().unwrap();
        let boundary = match side {
            SchemaSide::Source => setup.source.as_mut(),
            SchemaSide::Target => setup.target.as_mut(),
        };
        match boundary {
            Some(MappingBoundary::Protobuf(draft)) => draft,
            _ => panic!("expected a staged Protocol Buffers boundary"),
        }
    }

    #[test]
    fn protobuf_boundaries_save_reopen_and_run_without_original_schema_files() -> anyhow::Result<()>
    {
        let directory = TestDirectory::new()?;
        std::fs::create_dir_all(directory.0.join("types"))?;
        let schema_path = directory.0.join("messages.proto");
        let imported_path = directory.0.join("types/address.proto");
        std::fs::write(
            &schema_path,
            r#"syntax = "proto3"; package demo;
import "types/address.proto";
message Person { string name = 1; int32 count = 2; repeated shared.Address addresses = 3; }
message Other { bool enabled = 1; }"#,
        )?;
        std::fs::write(
            &imported_path,
            r#"syntax = "proto3"; package shared; message Address { string city = 1; }"#,
        )?;
        let source_path = directory.0.join("source.dat");
        let target_path = directory.0.join("target.binary");
        let project_path = directory.0.join("mapping.json");
        let mut app = FerruleApp::default();
        app.begin_new_mapping();
        app.stage_mapping_schema(SchemaSide::Source, schema_path.clone());
        app.stage_mapping_schema(SchemaSide::Target, schema_path.clone());
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
        assert_eq!(
            app.project.source.name, "root",
            "staging preserves the open mapping"
        );
        for side in [SchemaSide::Source, SchemaSide::Target] {
            staged_draft(&mut app, side).set_root_message("demo.Person".to_owned());
        }
        staged_draft(&mut app, SchemaSide::Source).instance_path =
            source_path.to_str().unwrap().to_owned();
        staged_draft(&mut app, SchemaSide::Target).instance_path =
            target_path.to_str().unwrap().to_owned();
        assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
        app.finish_new_mapping();
        assert!(app.new_mapping_setup.is_none());
        assert_eq!(app.project.source.name, "Person");
        assert_eq!(app.project.target.name, "Person");
        assert!(
            app.main_canvas
                .snarl
                .nodes()
                .any(|node| matches!(node, crate::canvas::CanvasNode::SourceBlock(_)))
        );
        assert!(
            app.main_canvas
                .snarl
                .nodes()
                .any(|node| matches!(node, crate::canvas::CanvasNode::TargetBlock(_)))
        );
        assert!(app.project.graph.nodes.is_empty());
        assert!(app.is_dirty());
        assert_eq!(app.project.source_path.as_deref(), source_path.to_str());
        assert_eq!(app.project.target_path.as_deref(), target_path.to_str());
        let source_options = app.project.source_options.protobuf.clone().unwrap();
        let target_options = app.project.target_options.protobuf.clone().unwrap();
        let layout = format_protobuf::Layout::parse_files(
            source_options
                .schema_path
                .as_deref()
                .unwrap_or("schema.proto"),
            &source_options.schema,
            source_options
                .imports
                .iter()
                .map(|file| (file.path.as_str(), file.source.as_str())),
        )?;
        let city = |value: &str| {
            Instance::Group(
                (vec![(
                    "city".into(),
                    Instance::Scalar(Value::String(value.to_owned())),
                )])
                .into(),
            )
        };
        let source = Instance::Group(
            (vec![
                (
                    "name".into(),
                    Instance::Scalar(Value::String("Café".to_owned())),
                ),
                ("count".into(), Instance::Scalar(Value::Int(2))),
                (
                    "addresses".into(),
                    Instance::Repeated(vec![city("Paris"), city("München")]),
                ),
            ])
            .into(),
        );
        let expected_bytes = format_protobuf::to_vec(&layout, "demo.Person", &source)?;
        std::fs::write(&source_path, &expected_bytes)?;
        app.project.root.construction = mapping::ScopeConstruction::CopyCurrentSource;
        assert!(cli::validate(&app.project).is_empty());
        app.save_document_to(&project_path)?;
        std::fs::remove_file(&schema_path)?;
        std::fs::remove_file(&imported_path)?;
        app.load_project_from(&project_path);
        assert_eq!(app.project.source_options.protobuf, Some(source_options));
        assert_eq!(app.project.target_options.protobuf, Some(target_options));
        assert!(cli::validate(&app.project).is_empty());
        let outcome = cli::run_project_with_paths(&project_path, None, None)?;
        assert_eq!(outcome.output_path, target_path);
        assert_eq!(std::fs::read(&target_path)?, expected_bytes);
        assert_eq!(
            format_protobuf::read(&target_path, &layout, "demo.Person")?,
            source
        );
        Ok(())
    }

    #[test]
    fn failed_protobuf_import_keeps_staged_boundaries_and_the_open_mapping() -> anyhow::Result<()> {
        let directory = TestDirectory::new()?;
        let schema_path = directory.0.join("good.proto");
        let bad_path = directory.0.join("bad.proto");
        std::fs::write(
            &schema_path,
            r#"syntax = "proto3"; package demo; message Person { string name = 1; }"#,
        )?;
        std::fs::write(&bad_path, "syntax = broken;")?;
        let mut app = FerruleApp::default();
        let original = mapping::project_file::encode_pretty(&app.project)?;
        app.begin_new_mapping();
        app.stage_mapping_schema(SchemaSide::Source, schema_path.clone());
        staged_draft(&mut app, SchemaSide::Source).set_root_message("demo.Person".to_owned());
        app.stage_mapping_schema(SchemaSide::Source, bad_path);
        assert_eq!(
            staged_draft(&mut app, SchemaSide::Source).root_message,
            "demo.Person"
        );
        assert_eq!(
            staged_draft(&mut app, SchemaSide::Source).schema_path,
            schema_path
        );
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        assert!(app.status.contains("failed to load source"));
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
        app.finish_new_mapping();
        assert!(app.new_mapping_setup.is_some());
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        Ok(())
    }

    #[test]
    fn unselected_target_root_keeps_create_disabled_until_both_boundaries_are_ready()
    -> anyhow::Result<()> {
        let directory = TestDirectory::new()?;
        let schema_path = directory.0.join("messages.proto");
        std::fs::write(
            &schema_path,
            r#"syntax = "proto3"; package demo;
message Source { string name = 1; }
message Target { int32 value = 1; }"#,
        )?;
        let mut app = FerruleApp::default();
        app.begin_new_mapping();
        app.stage_mapping_schema(SchemaSide::Source, schema_path.clone());
        app.stage_mapping_schema(SchemaSide::Target, schema_path);
        staged_draft(&mut app, SchemaSide::Source).set_root_message("demo.Source".to_owned());
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
        staged_draft(&mut app, SchemaSide::Target).set_root_message("demo.Target".to_owned());
        assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
        app.finish_new_mapping();
        assert_eq!(app.project.source.name, "Source");
        assert_eq!(app.project.target.name, "Target");
        assert_eq!(
            app.project
                .source_options
                .protobuf
                .as_ref()
                .unwrap()
                .root_message,
            "demo.Source"
        );
        assert_eq!(
            app.project
                .target_options
                .protobuf
                .as_ref()
                .unwrap()
                .root_message,
            "demo.Target"
        );
        assert!(app.project.source_path.is_none());
        assert!(app.project.target_path.is_none());
        Ok(())
    }
}
