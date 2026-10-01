use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Node, Scope};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-named-protobuf-source-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn schema_graph(&self) -> anyhow::Result<(PathBuf, PathBuf)> {
        std::fs::create_dir_all(self.0.join("types"))?;
        let root = self.0.join("messages.proto");
        let imported = self.0.join("types/address.proto");
        std::fs::write(
            &root,
            r#"syntax = "proto3"; package demo;
import "types/address.proto";
message Person { string name = 1; int32 count = 2; shared.Address address = 3; }
message Snapshot { string label = 1; int32 count = 2; shared.Address address = 3; }
message Recursive { Recursive next = 1; }"#,
        )?;
        std::fs::write(
            &imported,
            r#"syntax = "proto3"; package shared; message Address { string city = 1; }"#,
        )?;
        Ok((root, imported))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source_protobuf(app: &mut FerruleApp) -> &mut crate::new_mapping::ProtobufBoundaryDraft {
    app.extra_source_draft
        .as_mut()
        .unwrap()
        .protobuf_draft
        .as_mut()
        .unwrap()
}

fn target_protobuf(app: &mut FerruleApp) -> &mut crate::new_mapping::ProtobufBoundaryDraft {
    app.extra_target_draft
        .as_mut()
        .unwrap()
        .protobuf_draft
        .as_mut()
        .unwrap()
}

#[test]
fn named_protobuf_creation_preserves_imports_through_history_save_reopen_and_execution()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let (schema_path, imported_path) = directory.schema_graph()?;
    let source_path = directory.0.join("contacts.sqlite");
    let target_path = directory.0.join("snapshot.csv");
    let driver_path = directory.0.join("driver.json");
    let main_path = directory.0.join("main.xml");
    let project_path = directory.0.join("mapping.json");
    let driver_bytes = br#"{"name":"driver"}"#;
    std::fs::write(&driver_path, driver_bytes)?;
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
    let draft = app.extra_source_draft.as_mut().unwrap();
    draft.name = "contacts".into();
    draft.set_instance_path(source_path.to_str().unwrap().to_owned());
    app.stage_extra_source_schema(schema_path.clone());
    assert!(!app.extra_source_draft.as_ref().unwrap().schema_is_ready());
    app.finish_extra_source();
    assert!(app.project.extra_sources.is_empty());
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, initial);
    source_protobuf(&mut app).set_root_message("demo.Person".into());
    let selected = source_protobuf(&mut app).options()?;
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_source_setup(ui.ctx())
    });
    assert_eq!(source_protobuf(&mut app).options()?, selected);
    app.finish_extra_source();
    app.observe_editor_history(std::time::Instant::now(), false);
    let source_options = app.project.extra_sources[0].options.clone();
    let protobuf = source_options.protobuf.as_ref().unwrap();
    assert_eq!(protobuf.root_message, "demo.Person");
    assert_eq!(protobuf.imports[0].path, "types/address.proto");
    app.undo_project();
    assert!(app.project.extra_sources.is_empty());
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(app.project.extra_sources[0].options, source_options);

    app.begin_extra_target();
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.name = "binary".into();
    draft.output_path = target_path.to_str().unwrap().to_owned();
    app.stage_extra_target_schema(schema_path.clone());
    assert!(!app.extra_target_draft.as_ref().unwrap().schema_is_ready());
    app.finish_extra_target();
    assert!(app.project.extra_targets.is_empty());
    target_protobuf(&mut app).set_root_message("demo.Snapshot".into());
    let selected = target_protobuf(&mut app).options()?;
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_target_setup(ui.ctx())
    });
    assert_eq!(target_protobuf(&mut app).options()?, selected);
    app.finish_extra_target();
    app.observe_editor_history(std::time::Instant::now(), false);
    let target_options = app.project.extra_targets[0].options.clone();
    assert_eq!(
        target_options.protobuf.as_ref().unwrap().root_message,
        "demo.Snapshot"
    );
    app.undo_project();
    assert!(app.project.extra_targets.is_empty());
    assert_eq!(app.project.extra_sources[0].options, source_options);
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].options, target_options);

    std::fs::remove_file(schema_path)?;
    std::fs::remove_file(imported_path)?;
    app.undo_project();
    app.undo_project();
    assert!(app.project.extra_sources.is_empty());
    assert!(app.project.extra_targets.is_empty());
    app.redo_project();
    app.redo_project();
    assert_eq!(app.project.extra_sources[0].options, source_options);
    assert_eq!(app.project.extra_targets[0].options, target_options);
    let protobuf = source_options.protobuf.as_ref().unwrap();
    let layout = format_protobuf::Layout::parse_files(
        protobuf.schema_path.as_deref().unwrap(),
        &protobuf.schema,
        protobuf
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )?;
    let address = Instance::Group(vec![(
        "city".into(),
        Instance::Scalar(Value::String("München".into())),
    )]);
    let source = Instance::Group(vec![
        (
            "name".into(),
            Instance::Scalar(Value::String("Café".into())),
        ),
        ("count".into(), Instance::Scalar(Value::Int(2))),
        ("address".into(), address.clone()),
    ]);
    let expected = Instance::Group(vec![
        (
            "label".into(),
            Instance::Scalar(Value::String("Café".into())),
        ),
        ("count".into(), Instance::Scalar(Value::Int(2))),
        ("address".into(), address),
    ]);
    let input_bytes = format_protobuf::to_vec(&layout, "demo.Person", &source)?;
    let output_bytes = format_protobuf::to_vec(&layout, "demo.Snapshot", &expected)?;
    std::fs::write(&source_path, &input_bytes)?;
    for (id, path) in [
        (0, vec!["name".into()]),
        (1, vec!["contacts".into(), "name".into()]),
        (2, vec!["contacts".into(), "count".into()]),
        (3, vec!["contacts".into(), "address".into(), "city".into()]),
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
        bindings: vec![
            Binding {
                target_field: "label".into(),
                node: 1,
            },
            Binding {
                target_field: "count".into(),
                node: 2,
            },
        ],
        children: vec![Scope {
            target_field: "address".into(),
            bindings: vec![Binding {
                target_field: "city".into(),
                node: 3,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    app.load_project_from(&project_path);
    assert_eq!(app.project.extra_sources[0].options, source_options);
    assert_eq!(app.project.extra_targets[0].options, target_options);
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&target_path)?, output_bytes);
    assert_eq!(
        format_protobuf::read(&target_path, &layout, "demo.Snapshot")?,
        expected
    );
    let extras = [cli::NamedPayloadInput::new(
        "contacts",
        cli::PayloadDocument::new(&source_path, &input_bytes)?,
    )?];
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&driver_path, driver_bytes)?)
            .with_extra_sources(&extras),
    )?;
    let artifact = payload
        .artifacts
        .iter()
        .find(|artifact| artifact.target == "binary")
        .unwrap();
    assert_eq!(artifact.path, target_path);
    assert_eq!(artifact.bytes, output_bytes);
    Ok(())
}

#[test]
fn failed_named_protobuf_source_import_preserves_selection_and_open_mapping() -> anyhow::Result<()>
{
    let directory = TestDirectory::new()?;
    let (schema_path, _) = directory.schema_graph()?;
    let bad = directory.0.join("bad.proto");
    let escape = directory.0.join("escape.proto");
    std::fs::write(&bad, "syntax = broken;")?;
    std::fs::write(
        &escape,
        r#"syntax = "proto3"; import "../outside.proto"; message Item { string value = 1; }"#,
    )?;
    let mut app = FerruleApp::default();
    let initial = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_extra_source();
    app.extra_source_draft.as_mut().unwrap().name = "contacts".into();
    app.extra_source_draft.as_mut().unwrap().instance_path = "contacts.sqlite".into();
    app.stage_extra_source_schema(schema_path.clone());
    source_protobuf(&mut app).set_root_message("demo.Person".into());
    let selected = source_protobuf(&mut app).options()?;
    for failed in [bad, escape] {
        app.stage_extra_source_schema(failed);
        assert_eq!(source_protobuf(&mut app).schema_path, schema_path);
        assert_eq!(source_protobuf(&mut app).options()?, selected);
        assert_eq!(app.status, "failed to load extra source schema");
        assert_eq!(mapping::project_file::encode_pretty(&app.project)?, initial);
    }
    app.stage_extra_source_sqlite_table();
    assert_eq!(source_protobuf(&mut app).options()?, selected);
    assert_eq!(app.status, "SQLite table import blocked");
    Ok(())
}

#[test]
fn unsupported_named_protobuf_source_root_cannot_add_the_previous_schema() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let (schema_path, _) = directory.schema_graph()?;
    let mut app = FerruleApp::default();
    app.begin_extra_source();
    let draft = app.extra_source_draft.as_mut().unwrap();
    draft.name = "contacts".into();
    draft.instance_path = "contacts.bin".into();
    draft.set_schema(SchemaNode::scalar("old", ScalarType::String));
    app.stage_extra_source_schema(schema_path);
    source_protobuf(&mut app).set_root_message("demo.Person".into());
    assert!(app.extra_source_draft.as_ref().unwrap().schema_is_ready());
    for root in ["demo.Recursive", "demo.Missing"] {
        source_protobuf(&mut app).set_root_message(root.into());
        assert!(!app.extra_source_draft.as_ref().unwrap().schema_is_ready());
        app.finish_extra_source();
        assert!(app.project.extra_sources.is_empty());
        assert!(app.extra_source_draft.is_some());
        assert_eq!(app.status, "extra source is incomplete");
    }
    source_protobuf(&mut app).set_root_message("demo.Person".into());
    app.finish_extra_source();
    assert_eq!(app.project.extra_sources[0].schema.name, "Person");
    assert!(app.project.extra_sources[0].options.protobuf.is_some());
    Ok(())
}

#[test]
fn abandoning_protobuf_after_sqlite_location_changes_requires_a_new_schema() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let (schema_path, _) = directory.schema_graph()?;
    for change_path in [true, false] {
        let mut app = FerruleApp::default();
        app.begin_extra_source();
        let draft = app.extra_source_draft.as_mut().unwrap();
        draft.name = "contacts".into();
        draft.set_instance_path("first.sqlite".into());
        draft.set_sqlite_table("people".into());
        let original = SchemaNode::group(
            "people",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        )
        .repeating();
        draft.set_sqlite_schema(original.clone());

        app.stage_extra_source_schema(schema_path.clone());
        source_protobuf(&mut app).set_root_message("demo.Person".into());
        let draft = app.extra_source_draft.as_mut().unwrap();
        draft.use_path_format();
        assert_eq!(draft.clone().build(&[])?.schema, original);

        app.stage_extra_source_schema(schema_path.clone());
        source_protobuf(&mut app).set_root_message("demo.Person".into());
        let draft = app.extra_source_draft.as_mut().unwrap();
        if change_path {
            draft.set_instance_path("second.sqlite".into());
        } else {
            draft.set_sqlite_table("other_people".into());
        }
        assert!(draft.schema.is_none());
        assert!(draft.clone().build(&[])?.options.protobuf.is_some());
        draft.use_path_format();
        assert!(matches!(
            draft.clone().build(&[]),
            Err(crate::extra_sources::ExtraSourceDraftError::MissingSchema)
        ));
        app.finish_extra_source();
        assert!(app.project.extra_sources.is_empty());
        assert_eq!(app.status, "extra source is incomplete");
    }
    Ok(())
}
