use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, Scope};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-named-protobuf-target-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
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

fn target_protobuf(app: &mut FerruleApp) -> &mut crate::new_mapping::ProtobufBoundaryDraft {
    app.extra_target_draft
        .as_mut()
        .unwrap()
        .protobuf_draft
        .as_mut()
        .unwrap()
}

#[test]
fn failed_named_protobuf_target_import_and_invalid_root_preserve_the_saved_target()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let good = directory.0.join("good.proto");
    let bad = directory.0.join("bad.proto");
    std::fs::write(
        &good,
        r#"syntax = "proto3"; package demo;
message Result { string name = 1; }
message Recursive { Recursive next = 1; }"#,
    )?;
    std::fs::write(&bad, "syntax = broken;")?;
    let mut app = FerruleApp::default();
    app.project.extra_targets.push(mapping::NamedTarget {
        name: "existing".into(),
        path: Some("output.json".into()),
        schema: SchemaNode::scalar("old", ScalarType::String),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        root: Scope::default(),
    });
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.edit_extra_target(0);
    app.stage_extra_target_schema(good.clone());
    let old_schema = app.extra_target_draft.as_ref().unwrap().schema.clone();
    let old_options = app.extra_target_draft.as_ref().unwrap().options.clone();
    app.finish_extra_target();
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    assert_eq!(app.status, "target is incomplete");
    target_protobuf(&mut app).set_root_message("demo.Result".into());
    let selected = target_protobuf(&mut app).options()?;
    app.stage_extra_target_schema(bad);
    assert_eq!(target_protobuf(&mut app).schema_path, good);
    assert_eq!(target_protobuf(&mut app).options()?, selected);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert_eq!(draft.schema, old_schema);
    assert_eq!(draft.options, old_options);
    assert_eq!(app.status, "failed to load target schema");
    for root in ["demo.Recursive", "demo.Missing"] {
        target_protobuf(&mut app).set_root_message(root.into());
        assert!(!app.extra_target_draft.as_ref().unwrap().schema_is_ready());
        app.finish_extra_target();
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        assert!(app.extra_target_draft.is_some());
    }
    target_protobuf(&mut app).set_root_message("demo.Result".into());
    app.finish_extra_target();
    assert!(app.extra_target_draft.is_none());
    assert_eq!(app.project.extra_targets[0].schema.name, "Result");
    assert_eq!(app.project.extra_targets[0].options, selected);
    Ok(())
}

#[test]
fn pending_named_protobuf_schema_requires_an_explicit_switch_for_incompatible_replacements()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let proto = directory.0.join("message.proto");
    let matching = directory.0.join("matching.xsd");
    let json = directory.0.join("schema.json");
    std::fs::write(
        &proto,
        r#"syntax = "proto3"; message Binary { string name = 1; }"#,
    )?;
    std::fs::write(
        &matching,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="Binary"><xs:complexType><xs:sequence>
<xs:element name="name" type="xs:string"/>
</xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        &json,
        r#"{"type":"object","properties":{"changed":{"type":"string"}}}"#,
    )?;
    let mut app = FerruleApp::default();
    app.begin_extra_target();
    app.extra_target_draft.as_mut().unwrap().name = "binary".into();
    app.stage_extra_target_schema(proto);
    target_protobuf(&mut app).set_root_message("Binary".into());
    let selected = target_protobuf(&mut app).options()?;
    app.stage_extra_target_schema(json.clone());
    assert_eq!(app.status, "failed to load target schema");
    assert_eq!(target_protobuf(&mut app).options()?, selected);
    app.stage_extra_target_schema(matching);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert!(draft.protobuf_draft.is_none());
    assert!(draft.schema_is_ready());
    assert_eq!(draft.options, selected);
    app.extra_target_draft.as_mut().unwrap().use_path_format();
    app.stage_extra_target_schema(json);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert!(draft.protobuf_draft.is_none());
    assert!(draft.schema.as_ref().unwrap().child("changed").is_some());
    assert!(draft.options.protobuf.is_none());
    app.finish_extra_target();
    assert_eq!(app.project.extra_targets[0].name, "binary");
    assert!(app.project.extra_targets[0].options.protobuf.is_none());
    Ok(())
}
