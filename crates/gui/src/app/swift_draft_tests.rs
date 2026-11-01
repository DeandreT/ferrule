//! Real Inspector interactions using self-authored headers and one app/context.

use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use ir::{ScalarType, SchemaNode};
use mapping::{
    FormatOptions, NamedTarget, SwiftCharset, SwiftFieldLayout, SwiftMessageLayout,
    SwiftMtDirection, SwiftMtLayout, SwiftValueExpr,
};

const BASIC: &str = "F01BANKBEBBAXXX0000000001";
const SAVED: &str = "F01BANKBEBBAXXX0000000001 BANKDEFFXXXXN";
const NEW: &str = "F01BANKBEBBAXXX0000000002 BANKDEFFXXXXN";

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-swift-drafts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Project {
    let options = FormatOptions {
        swift_mt: Some(
            SwiftMtLayout::new(vec![SwiftMessageLayout::new(
                "MT950",
                vec![SwiftFieldLayout::new(
                    "20",
                    vec!["MT950".into(), "20".into()],
                    false,
                    SwiftValueExpr::Capture {
                        path: Vec::new(),
                        min: 1,
                        max: 16,
                        charset: SwiftCharset::Text,
                    },
                )],
            )])
            .unwrap(),
        ),
        swift_mt_output: swift_output::parse_headers(SwiftMtDirection::Input, SAVED).unwrap(),
        ..Default::default()
    };
    let mut project = blank_project();
    project.target = SchemaNode::group(
        "SWIFT",
        vec![
            SchemaNode::group(
                "Message",
                vec![
                    SchemaNode::group(
                        "Application Header",
                        vec![
                            SchemaNode::group("Input", Vec::new()),
                            SchemaNode::group("Output", Vec::new()),
                        ],
                    ),
                    SchemaNode::group("MT950", vec![SchemaNode::scalar("20", ScalarType::String)]),
                ],
            )
            .repeating(),
        ],
    );
    project.target_path = Some("output.edi".into());
    project.target_options = options;
    for name in ["audit", "receipt"] {
        project.extra_targets.push(NamedTarget {
            name: name.into(),
            path: Some(format!("{name}.edi")),
            schema: project.target.clone(),
            options: project.target_options.clone(),
            root: project.root.clone(),
        });
    }
    assert!(cli::validate(&project).is_empty());
    project
}

struct Editor {
    app: FerruleApp,
    open: bool,
    editing: bool,
    stage: bool,
    pipeline: bool,
}

impl Editor {
    fn current(&mut self) -> &mut FerruleApp {
        if self.stage {
            &mut self.app.pipeline_stage_canvas.as_mut().unwrap().editor
        } else {
            &mut self.app
        }
    }
}

fn inspector(app: FerruleApp) -> Harness<'static, Editor> {
    let mut initialized = false;
    let mut harness = Harness::builder()
        .with_size((800.0, 900.0))
        .with_os(egui::os::OperatingSystem::Nix)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, editor: &mut Editor| {
                if !initialized {
                    crate::icons::install(ui.ctx());
                    ui.ctx().all_styles_mut(|style| {
                        style.visuals.text_cursor.blink = false;
                        style.animation_time = 0.0;
                    });
                    initialized = true;
                }
                if editor.pipeline {
                    editor.app.show_pipeline_editor(ui.ctx());
                    return;
                }
                let open = editor.open;
                let editing = editor.editing;
                let app = editor.current();
                if open {
                    app.show_inspector(ui, editing);
                }
                app.observe_editor_history(std::time::Instant::now(), false);
            },
            Editor {
                app,
                open: true,
                editing: true,
                stage: false,
                pipeline: false,
            },
        );
    click(&mut harness, "Schema");
    harness
}

fn click(harness: &mut Harness<'_, Editor>, label: &str) {
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, label)
        .click();
    harness.run();
}

fn replace(harness: &mut Harness<'_, Editor>, text: &str) {
    harness
        .get_by_role(egui::accesskit::Role::MultilineTextInput)
        .click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    // Replace in one input event: an intermediate blank would clear saved headers.
    harness.event(egui::Event::Text(text.into()));
    harness.run();
}

fn text(harness: &Harness<'_, Editor>) -> String {
    harness
        .get_by_role(egui::accesskit::Role::MultilineTextInput)
        .accesskit_node()
        .value()
        .unwrap()
}

fn text_id(harness: &Harness<'_, Editor>) -> u128 {
    harness
        .get_by_role(egui::accesskit::Role::MultilineTextInput)
        .accesskit_node()
        .id()
        .into()
}

fn assert_direction(harness: &Harness<'_, Editor>, label: &str) {
    assert_eq!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, label)
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::True),
    );
}

fn activate(harness: &mut Harness<'_, Editor>, document: MappingDocument) {
    let app = harness.state_mut().current();
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
    }
    app.mapping_workspace.active = document;
    harness.run();
}

fn reload_case(document: MappingDocument, same_file: bool) {
    let directory = Directory::new();
    let a = directory.0.join("a.json");
    let b = directory.0.join("b.json");
    let bytes = mapping::project_file::encode_pretty(&fixture())
        .unwrap()
        .into_bytes();
    std::fs::write(&a, &bytes).unwrap();
    std::fs::write(&b, &bytes).unwrap();
    let mut app = FerruleApp::default();
    app.load_project_from(&a);
    let mut harness = inspector(app);
    let unrelated = egui::Id::unique("unrelated-swift-reload-state");
    harness
        .ctx
        .data_mut(|data| data.insert_temp(unrelated, "retain me".to_owned()));

    for partial in [BASIC, "unfinished header with extra parts"] {
        harness.state_mut().app.load_project_from(&a);
        activate(&mut harness, document);
        let original = typed_edit_oracle::key(&harness.state().app);
        replace(&mut harness, partial);
        click(&mut harness, "Output");
        assert_eq!(text(&harness), partial);
        assert_direction(&harness, "Output");
        harness.get_by_label("Line 1: expected a basic header and an application header");
        assert_eq!(typed_edit_oracle::key(&harness.state().app), original);
        harness
            .get_by_role(egui::accesskit::Role::MultilineTextInput)
            .click();
        harness.run();
        let previous_widget = text_id(&harness);
        let previous_owner = harness.state().app.document_ui_id;

        let destination = if same_file { &a } else { &b };
        harness.state_mut().app.load_project_from(destination);
        activate(&mut harness, document);
        assert_ne!(harness.state().app.document_ui_id, previous_owner);
        assert_eq!(text(&harness), SAVED, "stale {document:?} draft");
        assert!(
            harness
                .query_by_label("Line 1: expected a basic header and an application header")
                .is_none(),
        );
        assert_ne!(text_id(&harness), previous_widget, "new text-input state");
        assert_direction(&harness, "Input");
        assert_eq!(typed_edit_oracle::key(&harness.state().app), original);
        assert!(!harness.state().app.is_dirty());
        assert!(!harness.state().app.can_undo());

        // Neither typing delivered to old focus nor the old widget's undo may
        // reintroduce a draft into the new document.
        harness.event(egui::Event::Text("stale focus".into()));
        harness.run();
        assert_eq!(text(&harness), SAVED);
        harness
            .get_by_role(egui::accesskit::Role::MultilineTextInput)
            .click();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(text(&harness), SAVED);
        assert_eq!(typed_edit_oracle::key(&harness.state().app), original);

        replace(&mut harness, NEW);
        let mut expected = fixture();
        let options = match document {
            MappingDocument::Main => &mut expected.target_options,
            MappingDocument::Target(index) => &mut expected.extra_targets[index].options,
            MappingDocument::Function(_) => unreachable!(),
        };
        options.swift_mt_output =
            swift_output::parse_headers(SwiftMtDirection::Input, NEW).unwrap();
        assert_eq!(
            typed_edit_oracle::key(&harness.state().app),
            crate::project_state::project_snapshot_key(&expected),
            "ordinary editing changes only the new selected target",
        );
        assert_eq!(std::fs::read(&a).unwrap(), bytes);
        assert_eq!(std::fs::read(&b).unwrap(), bytes);
        assert_eq!(
            harness.ctx.data(|data| data.get_temp::<String>(unrelated)),
            Some("retain me".into()),
        );
    }
}

#[test]
fn primary_drafts_are_isolated_on_same_file_reload() {
    reload_case(MappingDocument::Main, true);
}

#[test]
fn primary_drafts_are_isolated_on_different_file_reload() {
    reload_case(MappingDocument::Main, false);
}

#[test]
fn named_drafts_are_isolated_on_same_file_reload() {
    reload_case(MappingDocument::Target(0), true);
}

#[test]
fn named_drafts_are_isolated_on_different_file_reload() {
    reload_case(MappingDocument::Target(0), false);
}

#[test]
fn drafts_and_text_state_follow_target_names_across_reordering_and_reopening() {
    let mut app = FerruleApp {
        project: fixture(),
        ..FerruleApp::default()
    };
    app.mark_clean();
    app.rebase_history();
    let mut harness = inspector(app);
    let original = typed_edit_oracle::key(&harness.state().app);
    replace(&mut harness, "primary-partial");
    let primary_id = text_id(&harness);
    activate(&mut harness, MappingDocument::Target(0));
    assert_eq!(text(&harness), SAVED);
    replace(&mut harness, "audit-partial");
    click(&mut harness, "Output");
    let audit_id = text_id(&harness);
    assert_ne!(primary_id, audit_id);

    activate(&mut harness, MappingDocument::Target(1));
    assert_eq!(text(&harness), SAVED);
    replace(&mut harness, "receipt-partial");
    let receipt_id = text_id(&harness);
    assert_ne!(receipt_id, audit_id);
    assert_eq!(typed_edit_oracle::key(&harness.state().app), original);
    assert!(!harness.state().app.can_undo());
    harness.state_mut().app.project.extra_targets.swap(0, 1);
    activate(&mut harness, MappingDocument::Target(1));
    assert_eq!(text(&harness), "audit-partial");
    assert_eq!(text_id(&harness), audit_id);
    assert_direction(&harness, "Output");

    harness.state_mut().open = false;
    harness.run();
    harness.state_mut().open = true;
    harness.run();
    assert_eq!(text(&harness), "audit-partial");
    assert_eq!(text_id(&harness), audit_id);
    activate(&mut harness, MappingDocument::Target(0));
    assert_eq!(text(&harness), "receipt-partial");
    assert_eq!(text_id(&harness), receipt_id);
    activate(&mut harness, MappingDocument::Main);
    assert_eq!(text(&harness), "primary-partial");
    assert_eq!(text_id(&harness), primary_id);
    harness.state_mut().open = false;
    harness.run();
    harness.state_mut().open = true;
    harness.run();
    assert_eq!(text(&harness), "primary-partial");
    assert_eq!(text_id(&harness), primary_id);
    harness.state_mut().app.project.extra_targets.swap(0, 1);
    assert_eq!(typed_edit_oracle::key(&harness.state().app), original);
}

#[test]
fn locked_primary_and_named_headers_preserve_saved_options_and_drafts() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = FerruleApp {
            project: fixture(),
            ..FerruleApp::default()
        };
        app.mark_clean();
        app.rebase_history();
        let mut harness = inspector(app);
        activate(&mut harness, document);
        let original = typed_edit_oracle::key(&harness.state().app);
        replace(&mut harness, BASIC);
        click(&mut harness, "Output");
        harness.state_mut().editing = false;
        harness.run();
        click(&mut harness, "Input");
        replace(&mut harness, NEW);
        assert_eq!(text(&harness), BASIC);
        assert_direction(&harness, "Output");
        assert_eq!(typed_edit_oracle::key(&harness.state().app), original);
        assert!(!harness.state().app.is_dirty());
        assert!(!harness.state().app.can_undo());
        harness.state_mut().editing = true;
        harness.run();
        assert_eq!(text(&harness), BASIC);
        assert_direction(&harness, "Output");
    }
}

fn assert_partial(harness: &Harness<'_, Editor>, id: u128) {
    assert_eq!(text(harness), BASIC);
    assert_eq!(text_id(harness), id, "same document keeps TextEdit state");
    assert_direction(harness, "Output");
    let expected = swift_output::parse_headers(SwiftMtDirection::Input, SAVED).unwrap();
    let app = &harness.state().app;
    assert_eq!(app.project.target_options.swift_mt_output, expected);
    for target in &app.project.extra_targets {
        assert_eq!(target.options.swift_mt_output, expected);
    }
}

#[test]
fn source_add_remove_and_undo_preserve_primary_and_named_header_drafts() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = FerruleApp {
            project: fixture(),
            ..Default::default()
        };
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.mark_clean();
        app.rebase_history();
        let mut harness = inspector(app);
        activate(&mut harness, document);
        replace(&mut harness, BASIC);
        click(&mut harness, "Output");
        let id = text_id(&harness);
        let owner = harness.state().app.document_ui_id;
        let old_canvas = harness.state().app.main_canvas.tools_id;
        let mut source = ExtraSourceDraft::default();
        source.name = "unrelated".into();
        source.instance_path = "unrelated.json".into();
        source.schema = Some(SchemaNode::group("Lookup", Vec::new()));
        source.options.json_document = true;
        harness.state_mut().app.extra_source_draft = Some(source);
        harness.state_mut().app.finish_extra_source();
        harness.run();
        assert_eq!(harness.state().app.project.extra_sources.len(), 1);
        assert_ne!(harness.state().app.main_canvas.tools_id, old_canvas);
        assert_partial(&harness, id);

        harness.state_mut().app.remove_project_extra_source(0);
        harness.run();
        assert!(harness.state().app.project.extra_sources.is_empty());
        assert_partial(&harness, id);

        let old_canvas = harness.state().app.main_canvas.tools_id;
        harness.state_mut().app.undo_project();
        activate(&mut harness, document);
        assert_eq!(harness.state().app.project.extra_sources.len(), 1);
        assert_ne!(harness.state().app.main_canvas.tools_id, old_canvas);
        assert_partial(&harness, id);

        harness.state_mut().app.undo_project();
        activate(&mut harness, document);
        assert!(harness.state().app.project.extra_sources.is_empty());
        assert_partial(&harness, id);
        harness.state_mut().app.redo_project();
        activate(&mut harness, document);
        assert_eq!(harness.state().app.project.extra_sources.len(), 1);
        assert_partial(&harness, id);
        assert_eq!(harness.state().app.document_ui_id, owner);
    }
}

#[test]
fn save_as_and_failed_load_keep_the_current_header_draft() {
    let directory = Directory::new();
    let original = directory.0.join("original.json");
    std::fs::write(
        &original,
        mapping::project_file::encode_pretty(&fixture()).unwrap(),
    )
    .unwrap();
    let mut app = FerruleApp::default();
    app.load_project_from(&original);
    let mut harness = inspector(app);
    replace(&mut harness, BASIC);
    click(&mut harness, "Output");
    let id = text_id(&harness);
    let owner = harness.state().app.document_ui_id;
    let destination = directory.0.join("renamed.json");
    harness
        .state_mut()
        .app
        .save_document_to(&destination)
        .unwrap();
    harness.run();
    assert_partial(&harness, id);
    assert_eq!(
        harness.state().app.document.saved_path(),
        Some(destination.as_path())
    );
    let saved = mapping::project_file::decode_bytes(&std::fs::read(&destination).unwrap()).unwrap();
    assert_eq!(saved.target_options, fixture().target_options);
    harness
        .state_mut()
        .app
        .load_project_from(&directory.0.join("missing.json"));
    harness.run();
    assert_partial(&harness, id);
    assert_eq!(harness.state().app.document_ui_id, owner);
}

#[test]
fn new_mapping_replacements_have_fresh_owners_but_failed_creation_does_not() {
    use crate::new_mapping::{ImportedSchema, MappingBoundary};

    let mut app = FerruleApp::default();
    assert_ne!(app.document_ui_id, FerruleApp::default().document_ui_id);
    let original = app.document_ui_id;
    app.perform_destructive_action(DestructiveAction::NewProject, &egui::Context::default());
    let blank = app.document_ui_id;
    assert_ne!(blank, original);
    app.finish_new_mapping();
    assert_eq!(
        app.document_ui_id, blank,
        "incomplete wizard preserves its owner"
    );
    let boundary = |name| {
        MappingBoundary::Schema(Box::new(ImportedSchema {
            path: format!("{name}.xsd").into(),
            schema: SchemaNode::group(name, Vec::new()),
        }))
    };
    app.new_mapping_setup = Some(NewMappingSetup {
        source: Some(boundary("Input")),
        target: Some(boundary("Output")),
    });
    app.finish_new_mapping();
    assert_ne!(app.document_ui_id, blank);
    let created = app.document_ui_id;
    app.mark_clean();
    app.rebase_history();
    assert_eq!(app.document_ui_id, created);
}

#[test]
fn embedded_stage_header_drafts_never_cross_parent_stage_or_reopened_session() {
    let directory = Directory::new();
    let project = fixture();
    let mut pipeline =
        crate::pipeline_edit::PipelineEditorDocument::create(&directory.0.join("pipeline.json"))
            .unwrap();
    pipeline.pipeline.stages = ["first", "second"]
        .into_iter()
        .map(|name| mapping::PipelineStage {
            id: name.into(),
            mapping_path: None,
            project: project.clone(),
            source: mapping::PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: Vec::new(),
        })
        .collect();
    let mut app = FerruleApp {
        project,
        ..Default::default()
    };
    app.mark_clean();
    app.rebase_history();
    let mut harness = inspector(app);
    replace(&mut harness, BASIC);
    click(&mut harness, "Output");
    let parent_id = text_id(&harness);
    harness.state_mut().app.pipeline_editor =
        Some(pipeline_editor_ui::PipelineEditorUi::new(pipeline));
    let mut previous_stage_id = None;
    for stage in [0, 1, 0] {
        harness.state_mut().pipeline = true;
        harness.run();
        let label = if stage == 0 { "1. first" } else { "2. second" };
        click(&mut harness, label);
        harness.state_mut().pipeline = false;
        harness.run();
        let editor = harness.state_mut();
        assert_eq!(
            editor.app.pipeline_editor.as_ref().unwrap().selected_stage,
            Some(stage)
        );
        editor.app.begin_pipeline_stage_canvas(stage);
        assert!(editor.app.pipeline_stage_canvas.is_some());
        editor.stage = true;
        harness.run();
        assert_eq!(text(&harness), SAVED);
        assert_direction(&harness, "Input");
        let id = text_id(&harness);
        assert_ne!(id, parent_id);
        assert_ne!(Some(id), previous_stage_id);
        previous_stage_id = Some(id);
        replace(&mut harness, BASIC);
        click(&mut harness, "Output");
        let context = harness.ctx.clone();
        assert!(
            !harness
                .state_mut()
                .app
                .guard_pipeline_stage_canvas_close(&context)
        );
        harness.state_mut().stage = false;
        harness.run();
        assert_partial(&harness, parent_id);
    }
}
