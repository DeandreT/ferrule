use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Node, Scope, ScopeIteration, TabularBoundaryKind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-fixed-target-{}-{}",
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

fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    )
}

fn mapped_rows() -> Scope {
    Scope {
        iteration: ScopeIteration::Source(Vec::new()),
        bindings: vec![
            Binding {
                target_field: "Name".into(),
                node: 0,
            },
            Binding {
                target_field: "Count".into(),
                node: 1,
            },
        ],
        ..Scope::default()
    }
}

fn csv_options() -> FormatOptions {
    FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        has_header_row: Some(true),
        ..FormatOptions::default()
    }
}

#[test]
fn named_fixed_width_target_saves_reopens_and_emits_exact_unicode_bytes_from_both_hosts()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source_path = directory.0.join("input.csv");
    let primary_path = directory.0.join("main.csv");
    let fixed_path = directory.0.join("output.xlsx");
    let renamed_path = directory.0.join("renamed.db");
    let project_path = directory.0.join("mapping.json");
    let input = "Name,Count\nZoë,7\nMia,9\n";
    let expected = "Zoë___7__\nMia___9__\n".as_bytes();
    std::fs::write(&source_path, input)?;

    let mut app = FerruleApp::default();
    app.project.source = row_schema();
    app.project.target = row_schema();
    app.project.source_path = Some(source_path.to_str().unwrap().to_owned());
    app.project.target_path = Some(primary_path.to_str().unwrap().to_owned());
    app.project.source_options = csv_options();
    app.project.target_options = csv_options();
    app.project.root = mapped_rows();
    for (node, name) in [(0, "Name"), (1, "Count")] {
        app.project.graph.nodes.insert(
            node,
            Node::SourceField {
                path: vec![name.into()],
                frame: None,
            },
        );
    }
    app.begin_extra_target();
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.name = "fixed".into();
    draft.output_path = fixed_path.to_str().unwrap().to_owned();
    draft.schema = Some(row_schema());
    draft.root = Some(mapped_rows());
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    let pending = draft.fixed_width_draft.as_mut().unwrap();
    pending.widths = vec!["6".into(), "3".into()];
    pending.fill = "_".into();
    pending.record_delimiters = true;
    pending.treat_empty_as_absent = false;
    assert!(draft.schema_is_ready());
    let untouched_options = draft.options.clone();
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_target_setup(ui.ctx())
    });
    assert_eq!(
        app.extra_target_draft.as_ref().unwrap().options,
        untouched_options
    );
    app.finish_extra_target();
    assert_eq!(app.project.extra_targets.len(), 1);
    let layout = app.project.extra_targets[0]
        .options
        .fixed_width
        .clone()
        .unwrap();
    assert_eq!(layout.field_widths()[0].get(), 6);
    assert_eq!(layout.field_widths()[1].get(), 3);
    assert!(!layout.treat_empty_as_absent());
    assert!(cli::validate(&app.project).is_empty());

    // Editing metadata and a misleading filename must preserve the embedded codec.
    app.edit_extra_target(0);
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.name = "fixed-renamed".into();
    draft.output_path = renamed_path.to_str().unwrap().to_owned();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_target_setup(ui.ctx())
    });
    assert_eq!(
        app.extra_target_draft.as_ref().unwrap().options.fixed_width,
        Some(layout.clone())
    );
    app.finish_extra_target();
    app.save_document_to(&project_path)?;
    app.load_project_from(&project_path);
    assert_eq!(app.project.extra_targets[0].name, "fixed-renamed");
    assert_eq!(
        app.project.extra_targets[0].options.fixed_width,
        Some(layout.clone())
    );
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&renamed_path)?, expected);
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, input.as_bytes())?),
    )?;
    let named = payload
        .artifacts
        .iter()
        .find(|artifact| artifact.target == "fixed-renamed")
        .ok_or_else(|| anyhow::anyhow!("missing named payload artifact"))?;
    assert_eq!(named.path, renamed_path);
    assert_eq!(named.bytes, expected);

    // This closed shape has a native-shaped exporter. Local reimport is a
    // structural/runtime check, not reference-application acceptance.
    let design = directory.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&app.project, &design, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible());
    let reimported = mfd::import_with_profile(
        &design,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?;
    assert!(reimported.imported.warnings.is_empty());
    assert_eq!(
        reimported.imported.project.extra_targets[0]
            .options
            .fixed_width,
        Some(layout.clone())
    );
    let rerun = cli::run_project_value_payloads(
        &reimported.imported.project,
        &design,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, input.as_bytes())?),
    )?;
    let restored_named = rerun
        .artifacts
        .iter()
        .find(|artifact| artifact.target == "fixed-renamed")
        .ok_or_else(|| anyhow::anyhow!("missing restored fixed-width artifact"))?;
    assert_eq!(restored_named.bytes, expected);

    app.edit_extra_target(0);
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    draft.fixed_width_draft.as_mut().unwrap().record_delimiters = false;
    app.finish_extra_target();
    let contiguous = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, input.as_bytes())?),
    )?;
    let contiguous_named = contiguous
        .artifacts
        .iter()
        .find(|artifact| artifact.target == "fixed-renamed")
        .ok_or_else(|| anyhow::anyhow!("missing contiguous fixed-width artifact"))?;
    assert_eq!(contiguous_named.bytes, "Zoë___7__Mia___9__".as_bytes());

    // The text codec retains XML-only field metadata while editing an existing layout.
    let ir::SchemaKind::Group { children, .. } = &mut app.project.extra_targets[0].schema.kind
    else {
        panic!("expected flat target schema");
    };
    children[0].attribute = true;
    app.edit_extra_target(0);
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.name = "fixed-with-field-metadata".into();
    let metadata_path = directory.0.join("metadata.xlsx");
    draft.output_path = metadata_path.to_str().unwrap().to_owned();
    assert!(draft.schema_is_ready());
    app.finish_extra_target();
    assert!(app.extra_target_draft.is_none());
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    app.load_project_from(&project_path);
    let ir::SchemaKind::Group { children, .. } = &app.project.extra_targets[0].schema.kind else {
        panic!("expected saved flat target schema");
    };
    assert!(children[0].attribute);
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&metadata_path)?, contiguous_named.bytes);
    let metadata_output = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, input.as_bytes())?),
    )?;
    let metadata_named = metadata_output
        .artifacts
        .iter()
        .find(|artifact| artifact.target == "fixed-with-field-metadata")
        .ok_or_else(|| anyhow::anyhow!("missing metadata-preserving target"))?;
    assert_eq!(metadata_named.bytes, contiguous_named.bytes);
    Ok(())
}

#[test]
fn fixed_width_setup_is_transactional_and_refuses_invalid_or_incompatible_drafts()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    app.begin_extra_target();
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.name = "fixed".into();
    draft.schema = Some(SchemaNode::group(
        "Nested",
        vec![SchemaNode::group(
            "Child",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )],
    ));
    assert!(draft.begin_fixed_width().is_err());
    assert!(draft.fixed_width_draft.is_none());
    draft.schema = Some(row_schema());
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    let saved_options = draft.options.clone();
    draft.fixed_width_draft.as_mut().unwrap().widths[0] = "0".into();
    assert!(!draft.schema_is_ready());
    app.finish_extra_target();
    assert!(app.project.extra_targets.is_empty());
    assert!(app.extra_target_draft.is_some());
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.fixed_width_draft.as_mut().unwrap().widths[0] = "6".into();
    draft.fixed_width_draft.as_mut().unwrap().fill = "\n".into();
    assert!(!draft.schema_is_ready());
    assert!(draft.clone().build(&[]).is_err());
    draft.fixed_width_draft.as_mut().unwrap().fill = "_".into();
    let schema_before = draft.schema.clone();
    let replacement = directory.0.join("different.json");
    std::fs::write(
        &replacement,
        r#"{"type":"object","properties":{"Other":{"type":"string"}}}"#,
    )?;
    app.stage_extra_target_schema(replacement);
    assert_eq!(
        app.extra_target_draft.as_ref().unwrap().schema,
        schema_before
    );
    assert!(
        app.extra_target_draft
            .as_ref()
            .unwrap()
            .fixed_width_draft
            .is_some()
    );
    assert!(app.project.extra_targets.is_empty());
    let draft = app.extra_target_draft.as_mut().unwrap();
    draft.abandon_fixed_width();
    assert_eq!(draft.options, saved_options);
    assert!(draft.fixed_width_draft.is_none());
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    draft.schema = Some(SchemaNode::group(
        "Other",
        vec![SchemaNode::scalar("Only", ScalarType::String)],
    ));
    assert!(!draft.schema_is_ready());
    assert!(draft.clone().build(&[]).is_err());
    draft.use_path_format();
    assert!(draft.fixed_width_draft.is_none());
    assert_eq!(draft.options, FormatOptions::default());

    let layout = mapping::FixedWidthLayout::new(
        vec![
            mapping::FixedFieldWidth::new(6).unwrap(),
            mapping::FixedFieldWidth::new(3).unwrap(),
        ],
        '_',
        false,
        false,
    )?;
    let mut edit = ExtraTargetDraft {
        name: "existing".into(),
        schema: Some(row_schema()),
        options: FormatOptions {
            fixed_width: Some(layout.clone()),
            ..FormatOptions::default()
        },
        ..ExtraTargetDraft::default()
    };
    edit.begin_fixed_width().map_err(anyhow::Error::msg)?;
    edit.fixed_width_draft.as_mut().unwrap().widths[0] = "999".into();
    edit.output_path = "renamed.xlsx".into();
    edit.abandon_fixed_width();
    assert_eq!(edit.options.fixed_width, Some(layout.clone()));
    let reordered = directory.0.join("reordered.json");
    std::fs::write(
        &reordered,
        r#"{"title":"Row","type":"object","properties":{"Count":{"type":"integer"},"Name":{"type":"string"}}}"#,
    )?;
    let replacement = crate::new_mapping::import_schema(&reordered)?;
    assert_eq!(
        crate::extra_targets::flat_scalar_fields(&replacement)
            .map_err(anyhow::Error::msg)?
            .len(),
        2
    );
    assert_eq!(
        crate::extra_targets::flat_scalar_fields(&replacement)
            .map_err(anyhow::Error::msg)?
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["Count", "Name"]
    );
    assert_ne!(replacement, row_schema());
    app.extra_target_draft = Some(edit.clone());
    app.stage_extra_target_schema(reordered);
    let retained = app.extra_target_draft.as_ref().unwrap();
    assert_eq!(retained.schema.as_ref(), Some(&row_schema()));
    assert_eq!(retained.options.fixed_width, Some(layout.clone()));
    assert!(app.status.contains("failed"));
    assert_eq!(edit.build(&[])?.1.options.fixed_width, Some(layout));
    Ok(())
}
