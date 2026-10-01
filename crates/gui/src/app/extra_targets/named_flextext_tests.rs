use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, Scope};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-named-flextext-target-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn layout(&self) -> anyhow::Result<PathBuf> {
        let path = self.0.join("target.mft");
        std::fs::write(
            &path,
            r#"<FlexText><Commands><Project FileName="must-not-be-opened.txt" LineEnding="CRLF" ByteOrderMark="1">
<RootName Value="Result"/><Connections><Connection><Store><Name Value="text"/></Store></Connection></Connections>
</Project></Commands></FlexText>"#,
        )?;
        Ok(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn target_flextext(app: &FerruleApp) -> &crate::new_mapping::FlexTextBoundaryDraft {
    app.extra_target_draft
        .as_ref()
        .unwrap()
        .flextext_draft
        .as_ref()
        .unwrap()
}

#[test]
fn named_flextext_target_preserves_saved_output_through_failed_import_or_aborted_stage()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let good = directory.layout()?;
    let bad = directory.0.join("bad.mft");
    std::fs::write(&bad, "<FlexText>")?;
    let mut app = FerruleApp::default();
    app.project.extra_targets.push(mapping::NamedTarget {
        name: "existing".into(),
        path: Some("output.json".into()),
        schema: SchemaNode::scalar("old", ScalarType::String),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        root: Scope {
            target_field: "preserved".into(),
            ..Scope::default()
        },
    });
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.edit_extra_target(0);
    app.stage_extra_target_schema(good.clone());
    let old_schema = app.extra_target_draft.as_ref().unwrap().schema.clone();
    let old_options = app.extra_target_draft.as_ref().unwrap().options.clone();
    let selected = target_flextext(&app).options()?;
    app.stage_extra_target_schema(bad);
    assert_eq!(target_flextext(&app).configuration_path, good);
    assert_eq!(target_flextext(&app).options()?, selected);
    assert_eq!(app.extra_target_draft.as_ref().unwrap().schema, old_schema);
    assert_eq!(
        app.extra_target_draft.as_ref().unwrap().options,
        old_options
    );
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    app.extra_target_draft = None;
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );

    app.edit_extra_target(0);
    app.stage_extra_target_schema(good.clone());
    app.extra_target_draft.as_mut().unwrap().use_path_format();
    assert!(
        app.extra_target_draft
            .as_ref()
            .unwrap()
            .flextext_draft
            .is_none()
    );
    assert_eq!(app.extra_target_draft.as_ref().unwrap().schema, old_schema);
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    app.stage_extra_target_schema(good);
    app.finish_extra_target();
    assert!(app.extra_target_draft.is_none());
    assert_eq!(app.project.extra_targets[0].schema.name, "Result");
    assert_eq!(app.project.extra_targets[0].options, selected);
    assert_eq!(app.project.extra_targets[0].root.target_field, "preserved");
    assert_eq!(
        app.project.extra_targets[0].path.as_deref(),
        Some("output.json")
    );
    Ok(())
}

#[test]
fn named_flextext_target_keeps_compatible_codec_and_requires_explicit_incompatible_switch()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let config = directory.layout()?;
    let matching = directory.0.join("matching.xsd");
    let different = directory.0.join("different.json");
    let proto = directory.0.join("result.proto");
    let bad_proto = directory.0.join("bad.proto");
    std::fs::write(
        &matching,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="Result"><xs:complexType><xs:sequence><xs:element name="text" type="xs:string"/>
</xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        &different,
        r#"{"type":"object","properties":{"changed":{"type":"string"}}}"#,
    )?;
    std::fs::write(
        &proto,
        r#"syntax = "proto3"; message Binary { string value = 1; }"#,
    )?;
    std::fs::write(&bad_proto, "syntax = broken;")?;
    let mut app = FerruleApp::default();
    app.begin_extra_target();
    app.stage_extra_target_schema(config.clone());
    let schema = target_flextext(&app).schema()?;
    let options = target_flextext(&app).options()?;
    assert_eq!(crate::new_mapping::import_schema(&matching)?, schema);
    let staged = format!("{:?}", app.extra_target_draft);
    app.stage_extra_target_schema(different.clone());
    assert_eq!(app.status, "failed to load target schema");
    assert_eq!(format!("{:?}", app.extra_target_draft), staged);
    app.stage_extra_target_schema(matching);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert!(draft.flextext_draft.is_none());
    assert_eq!(draft.schema.as_ref(), Some(&schema));
    assert_eq!(draft.options, options);
    app.finish_extra_target();
    assert_eq!(app.project.extra_targets[0].options, options);
    assert!(app.project.extra_targets[0].path.is_none());
    assert!(!directory.0.join("must-not-be-opened.txt").exists());
    app.edit_extra_target(0);
    let saved = mapping::project_file::encode_pretty(&app.project)?;
    let staged = format!("{:?}", app.extra_target_draft);
    app.stage_extra_target_schema(different.clone());
    assert_eq!(format!("{:?}", app.extra_target_draft), staged);
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, saved);
    app.stage_extra_target_schema(proto);
    assert!(
        app.extra_target_draft
            .as_ref()
            .unwrap()
            .flextext_draft
            .is_none()
    );
    app.extra_target_draft
        .as_mut()
        .unwrap()
        .protobuf_draft
        .as_mut()
        .unwrap()
        .set_root_message("Binary".into());
    app.stage_extra_target_schema(config);
    assert!(
        app.extra_target_draft
            .as_ref()
            .unwrap()
            .protobuf_draft
            .is_none()
    );
    let staged = format!("{:?}", app.extra_target_draft);
    app.stage_extra_target_schema(bad_proto);
    assert_eq!(format!("{:?}", app.extra_target_draft), staged);
    app.extra_target_draft.as_mut().unwrap().use_path_format();
    app.stage_extra_target_schema(different);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert!(draft.flextext_draft.is_none());
    assert!(draft.options.flextext.is_none());
    assert!(draft.schema.as_ref().unwrap().child("changed").is_some());
    app.finish_extra_target();
    assert!(app.project.extra_targets[0].options.flextext.is_none());
    Ok(())
}

#[test]
fn invalid_pending_flextext_cannot_fall_back_to_previous_named_boundary() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let config = directory.layout()?;
    let mut app = FerruleApp::default();
    app.begin_extra_target();
    app.extra_target_draft.as_mut().unwrap().schema =
        Some(SchemaNode::scalar("old", ScalarType::String));
    app.stage_extra_target_schema(config);
    app.extra_target_draft
        .as_mut()
        .unwrap()
        .flextext_draft
        .as_mut()
        .unwrap()
        .instance_path = "bad\0path".into();
    assert!(!app.extra_target_draft.as_ref().unwrap().schema_is_ready());
    assert!(matches!(
        app.extra_target_draft.as_ref().unwrap().clone().build(&[]),
        Err(crate::extra_targets::ExtraTargetDraftError::InvalidFlexText(_))
    ));
    app.finish_extra_target();
    assert!(app.project.extra_targets.is_empty());
    assert!(app.extra_target_draft.is_some());
    assert_eq!(app.status, "target is incomplete");
    Ok(())
}

#[test]
fn named_flextext_and_fixed_width_pending_formats_are_transactional_and_exclusive()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let config = directory.layout()?;
    let bad = directory.0.join("bad.mft");
    let matching = directory.0.join("matching.xsd");
    std::fs::write(&bad, "<FlexText>")?;
    std::fs::write(
        &matching,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="Result"><xs:complexType><xs:sequence><xs:element name="text" type="xs:string"/>
</xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let mut app = FerruleApp::default();
    app.begin_extra_target();
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.name = "fixed".into();
    draft.schema = Some(SchemaNode::group(
        "old",
        vec![
            SchemaNode::scalar("first", ScalarType::String),
            SchemaNode::scalar("second", ScalarType::Int),
        ],
    ));
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    draft.options = draft.clone().build(&[])?.1.options;
    let old_options = draft.options.clone();
    let staged = format!("{:?}", app.extra_target_draft);
    app.stage_extra_target_schema(bad);
    assert_eq!(format!("{:?}", app.extra_target_draft), staged);
    app.stage_extra_target_schema(config);
    let draft = app.extra_target_draft.as_mut().unwrap();
    assert!(draft.fixed_width_draft.is_none());
    assert!(draft.flextext_draft.is_some());
    assert_eq!(draft.options, old_options);
    assert!(draft.begin_fixed_width().is_err());
    assert!(draft.fixed_width_draft.is_none());
    let selected = draft.flextext_draft.as_ref().unwrap().options()?;
    // Matching replacement uses the pending FlexText codec, not saved two-field widths.
    app.stage_extra_target_schema(matching);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert!(draft.flextext_draft.is_none());
    assert_eq!(draft.options, selected);
    app.finish_extra_target();
    assert_eq!(app.project.extra_targets[0].options, selected);
    app.edit_extra_target(0);
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    assert!(draft.flextext_draft.is_none());
    assert!(draft.fixed_width_draft.is_some());
    draft.abandon_fixed_width();
    assert_eq!(draft.options, selected);
    app.finish_extra_target();
    assert_eq!(app.project.extra_targets[0].options, selected);
    Ok(())
}
