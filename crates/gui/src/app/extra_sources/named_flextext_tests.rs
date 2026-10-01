use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Node, Scope, ScopeIteration};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-named-flextext-source-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn layout(
        &self,
        name: &str,
        delimiter: &str,
        bom: bool,
        crlf: bool,
    ) -> anyhow::Result<PathBuf> {
        let path = self.0.join(name);
        std::fs::write(
            &path,
            format!(
                r#"<FlexText><Commands><Project FileName="not-present.txt" LineEnding="{}" ByteOrderMark="{}">
<RootName Value="document"/><Connections><Connection><CSV>
<RecordName Value="rows"/><FieldSeparator Value="{delimiter}"/><RecordSeparator Value="{}"/>
<Fields><Field Type="string"><Name Value="text"/></Field><Field Type="integer"><Name Value="count"/></Field></Fields>
</CSV></Connection></Connections></Project></Commands></FlexText>"#,
                if crlf { "CRLF" } else { "LF" },
                u8::from(bom),
                if crlf { "%0D%0A" } else { "%0A" },
            ),
        )?;
        Ok(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source_flextext(app: &FerruleApp) -> &crate::new_mapping::FlexTextBoundaryDraft {
    app.extra_source_draft
        .as_ref()
        .unwrap()
        .flextext_draft
        .as_ref()
        .unwrap()
}

#[test]
fn named_flextext_layouts_survive_history_deleted_configs_save_reopen_and_both_hosts()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source_config = directory.layout("source.MFT", ";", false, false)?;
    let target_config = directory.layout("target.mft", "|", true, true)?;
    let source_path = directory.0.join("records.db");
    let target_path = directory.0.join("result.xlsx");
    let driver_path = directory.0.join("driver.json");
    let main_path = directory.0.join("main.xml");
    let project_path = directory.0.join("mapping.json");
    let driver = br#"{"name":"driver"}"#;
    let input = "Café;2\n\"München;West\";7".as_bytes();
    let expected = "\u{feff}Café|2\r\nMünchen;West|7".as_bytes();
    std::fs::write(&driver_path, driver)?;
    std::fs::write(&source_path, input)?;
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Driver",
        vec![SchemaNode::scalar("name", ScalarType::String)],
    );
    app.project.target =
        SchemaNode::group("Main", vec![SchemaNode::scalar("name", ScalarType::String)]);
    app.project.source_path = Some(driver_path.to_str().unwrap().to_owned());
    app.project.target_path = Some(main_path.to_str().unwrap().to_owned());
    app.project.target_options.xml_document = true;
    app.mark_clean();
    app.rebase_history();
    let initial = mapping::project_file::encode_pretty(&app.project)?;

    app.begin_extra_source();
    app.stage_extra_source_schema(source_config.clone());
    assert_eq!(app.extra_source_draft.as_ref().unwrap().name, "document");
    assert!(source_flextext(&app).instance_path.is_empty());
    assert!(
        app.extra_source_draft
            .as_ref()
            .unwrap()
            .instance_path
            .is_empty()
    );
    app.finish_extra_source();
    assert!(app.project.extra_sources.is_empty());
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, initial);
    let draft = app.extra_source_draft.as_mut().unwrap();
    draft.name = "records".into();
    draft.set_instance_path(source_path.to_str().unwrap().to_owned());
    assert!(draft.schema_is_ready());
    let source_schema = source_flextext(&app).schema()?;
    let source_options = source_flextext(&app).options()?;
    let staged = format!("{:?}", app.extra_source_draft);
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_source_setup(ui.ctx())
    });
    assert_eq!(format!("{:?}", app.extra_source_draft), staged);
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, initial);
    app.finish_extra_source();
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.project.extra_sources[0].schema, source_schema);
    assert_eq!(app.project.extra_sources[0].options, source_options);
    assert_eq!(
        app.project.extra_sources[0].path,
        source_path.to_str().unwrap()
    );
    app.undo_project();
    assert!(app.project.extra_sources.is_empty());
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(app.project.extra_sources[0].options, source_options);

    app.begin_extra_target();
    app.stage_extra_target_schema(target_config.clone());
    let draft = app.extra_target_draft.as_mut().unwrap();
    assert!(draft.output_path.is_empty());
    assert!(
        draft
            .flextext_draft
            .as_ref()
            .unwrap()
            .instance_path
            .is_empty()
    );
    draft.name = "text".into();
    draft.output_path = target_path.to_str().unwrap().to_owned();
    let target_schema = draft.flextext_draft.as_ref().unwrap().schema()?;
    let target_options = draft.flextext_draft.as_ref().unwrap().options()?;
    assert_eq!(source_schema, target_schema);
    assert_ne!(source_options, target_options);
    let staged = format!("{:?}", app.extra_target_draft);
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_target_setup(ui.ctx())
    });
    assert_eq!(format!("{:?}", app.extra_target_draft), staged);
    app.finish_extra_target();
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(app.project.extra_targets.is_empty());
    assert_eq!(app.project.extra_sources[0].options, source_options);
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].schema, target_schema);
    assert_eq!(app.project.extra_targets[0].options, target_options);

    std::fs::remove_file(&source_config)?;
    std::fs::remove_file(&target_config)?;
    assert!(!directory.0.join("not-present.txt").exists());
    app.undo_project();
    app.undo_project();
    assert!(app.project.extra_sources.is_empty());
    assert!(app.project.extra_targets.is_empty());
    app.redo_project();
    app.redo_project();
    assert_eq!(app.project.extra_sources[0].options, source_options);
    assert_eq!(app.project.extra_targets[0].options, target_options);

    for (id, path) in [
        (0, vec!["name".into()]),
        (1, vec!["records".into(), "rows".into(), "text".into()]),
        (2, vec!["records".into(), "rows".into(), "count".into()]),
    ] {
        app.project
            .graph
            .nodes
            .insert(id, Node::SourceField { path, frame: None });
    }
    app.project.root.bindings = vec![Binding {
        target_field: "name".into(),
        node: 0,
    }];
    app.project.extra_targets[0].root = Scope {
        children: vec![Scope {
            target_field: "rows".into(),
            iteration: ScopeIteration::Source(vec!["records".into(), "rows".into()]),
            bindings: vec![
                Binding {
                    target_field: "text".into(),
                    node: 1,
                },
                Binding {
                    target_field: "count".into(),
                    node: 2,
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    app.load_project_from(&project_path);
    assert_eq!(app.project.extra_sources[0].schema, source_schema);
    assert_eq!(app.project.extra_targets[0].schema, target_schema);
    assert_eq!(app.project.extra_sources[0].options, source_options);
    assert_eq!(app.project.extra_targets[0].options, target_options);
    assert!(!app.is_dirty());
    app.edit_extra_target(0);
    let saved = mapping::project_file::encode_pretty(&app.project)?;
    let edited = format!("{:?}", app.extra_target_draft);
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_target_setup(ui.ctx())
    });
    assert_eq!(format!("{:?}", app.extra_target_draft), edited);
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, saved);
    app.finish_extra_target();
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, saved);
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&target_path)?, expected);
    let extras = [cli::NamedPayloadInput::new(
        "records",
        cli::PayloadDocument::new(&source_path, input)?,
    )?];
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&driver_path, driver)?)
            .with_extra_sources(&extras),
    )?;
    let artifact = payload
        .artifacts
        .iter()
        .find(|artifact| artifact.target == "text")
        .unwrap();
    assert_eq!(artifact.path, target_path);
    assert_eq!(artifact.bytes, expected);
    Ok(())
}

#[test]
fn failed_named_flextext_imports_preserve_both_drafts_and_open_mapping() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source = directory.layout("source.mft", ";", false, false)?;
    let target = directory.layout("target.mft", "|", true, true)?;
    let malformed = directory.0.join("malformed.mft");
    let unsupported = directory.0.join("unsupported.mft");
    let oversized = directory.0.join("oversized.mft");
    std::fs::write(&malformed, "<FlexText>")?;
    std::fs::write(
        &unsupported,
        r#"<FlexText><Commands><Project><RootName Value="document"/><Connections><Connection><LoadFile/></Connection></Connections></Project></Commands></FlexText>"#,
    )?;
    std::fs::File::create(&oversized)?
        .set_len((mfd::MAX_FLEXTEXT_CONFIGURATION_BYTES + 1) as u64)?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_extra_source();
    app.extra_source_draft
        .as_mut()
        .unwrap()
        .set_schema(SchemaNode::scalar("old", ScalarType::String));
    app.extra_source_draft
        .as_mut()
        .unwrap()
        .set_instance_path("records.db".into());
    app.begin_extra_target();
    app.extra_target_draft.as_mut().unwrap().schema =
        Some(SchemaNode::scalar("old", ScalarType::String));
    app.stage_extra_source_schema(source);
    app.stage_extra_target_schema(target);
    let source_draft = format!("{:?}", app.extra_source_draft);
    let target_draft = format!("{:?}", app.extra_target_draft);
    for failed in [&malformed, &unsupported, &oversized] {
        app.stage_extra_source_schema(failed.clone());
        assert_eq!(app.status, "failed to load source FlexText layout");
        app.stage_extra_target_schema(failed.clone());
        assert_eq!(app.status, "failed to load target FlexText layout");
        assert_eq!(format!("{:?}", app.extra_source_draft), source_draft);
        assert_eq!(format!("{:?}", app.extra_target_draft), target_draft);
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
    }
    app.stage_extra_source_sqlite_table();
    assert_eq!(app.status, "SQLite table import blocked");
    assert_eq!(format!("{:?}", app.extra_source_draft), source_draft);
    Ok(())
}

#[test]
fn abandoning_flextext_after_sqlite_location_changes_requires_a_new_schema() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let config = directory.layout("records.mft", ";", false, false)?;
    for change_path in [true, false] {
        let mut app = FerruleApp::default();
        app.begin_extra_source();
        let draft = app.extra_source_draft.as_mut().unwrap();
        draft.name = "records".into();
        draft.set_instance_path("first.sqlite".into());
        draft.set_sqlite_table("people".into());
        let original = SchemaNode::group(
            "people",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        )
        .repeating();
        draft.set_sqlite_schema(original.clone());
        app.stage_extra_source_schema(config.clone());
        let draft = app.extra_source_draft.as_mut().unwrap();
        draft.use_path_format();
        assert_eq!(draft.clone().build(&[])?.schema, original);
        app.stage_extra_source_schema(config.clone());
        let draft = app.extra_source_draft.as_mut().unwrap();
        if change_path {
            draft.set_instance_path("second.sqlite".into());
        } else {
            draft.set_sqlite_table("other_people".into());
        }
        assert!(draft.schema.is_none());
        assert!(draft.clone().build(&[])?.options.flextext.is_some());
        draft.use_path_format();
        assert!(matches!(
            draft.clone().build(&[]),
            Err(crate::extra_sources::ExtraSourceDraftError::MissingSchema)
        ));
        app.finish_extra_source();
        assert!(app.project.extra_sources.is_empty());
    }
    Ok(())
}

#[test]
fn named_flextext_source_preserves_codec_until_explicit_schema_format_change() -> anyhow::Result<()>
{
    let directory = TestDirectory::new()?;
    let config = directory.layout("records.mft", ";", false, false)?;
    let matching = directory.0.join("matching.xsd");
    let different = directory.0.join("different.json");
    let proto = directory.0.join("message.proto");
    std::fs::write(
        &matching,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="document"><xs:complexType><xs:sequence><xs:element name="rows" maxOccurs="unbounded"><xs:complexType><xs:sequence>
<xs:element name="text" type="xs:string"/><xs:element name="count" type="xs:integer"/>
</xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        &different,
        r#"{"type":"object","properties":{"changed":{"type":"string"}}}"#,
    )?;
    std::fs::write(
        &proto,
        r#"syntax = "proto3"; message Binary { string value = 1; }"#,
    )?;
    let mut app = FerruleApp::default();
    app.begin_extra_source();
    app.extra_source_draft
        .as_mut()
        .unwrap()
        .set_instance_path("records.csv".into());
    app.stage_extra_source_schema(config.clone());
    let options = source_flextext(&app).options()?;
    let schema = source_flextext(&app).schema()?;
    assert_eq!(crate::new_mapping::import_schema(&matching)?, schema);
    app.stage_extra_source_schema(different.clone());
    assert_eq!(app.status, "failed to load extra source schema");
    assert_eq!(source_flextext(&app).options()?, options);
    app.stage_extra_source_schema(matching);
    let draft = app.extra_source_draft.as_ref().unwrap();
    assert!(draft.flextext_draft.is_none());
    assert_eq!(draft.options, options);
    assert_eq!(draft.schema.as_ref(), Some(&schema));
    let preserved = format!("{:?}", app.extra_source_draft);
    app.stage_extra_source_schema(different.clone());
    assert_eq!(format!("{:?}", app.extra_source_draft), preserved);
    app.stage_extra_source_schema(proto.clone());
    assert!(
        app.extra_source_draft
            .as_ref()
            .unwrap()
            .flextext_draft
            .is_none()
    );
    app.extra_source_draft
        .as_mut()
        .unwrap()
        .protobuf_draft
        .as_mut()
        .unwrap()
        .set_root_message("Binary".into());
    app.stage_extra_source_schema(config);
    assert!(
        app.extra_source_draft
            .as_ref()
            .unwrap()
            .protobuf_draft
            .is_none()
    );
    app.extra_source_draft.as_mut().unwrap().use_path_format();
    app.stage_extra_source_schema(different);
    let draft = app.extra_source_draft.as_ref().unwrap();
    assert!(draft.flextext_draft.is_none());
    assert!(draft.options.flextext.is_none());
    assert!(draft.schema.as_ref().unwrap().child("changed").is_some());
    app.finish_extra_source();
    assert!(app.project.extra_sources[0].options.flextext.is_none());
    Ok(())
}
