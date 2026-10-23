//! Auto-connect, named source and target, and primary source option edits
//! recorded as typed commands, checked against the snapshot history they
//! replace.

use editor_ui::{GraphAction, GraphDocument, ScopeLocation, TargetId};
use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, NamedTarget};

use super::typed_edit_oracle::{
    assert_stale_undo_refused, assert_typed_edit_matches_snapshots, newest_command,
};
use super::*;
use crate::test_ui::ContextExt as _;

/// `Code`, and `Name` inside a `Header` group.
fn fields(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::group(
                "Header",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
        ],
    )
}

/// Matching source and target fields, with a named copy of the target when
/// `document` is a named target, which is then active.
fn connect_app(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = fields("Input");
    app.project.target = fields("Output");
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    if let MappingDocument::Target(_) = document {
        app.project.extra_targets.push(NamedTarget {
            name: "Copy".into(),
            path: Some("copy.json".into()),
            schema: fields("Copy"),
            options: app.project.target_options.clone(),
            root: Scope::default(),
        });
    }
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.mark_clean();
    app.rebase_history();
    app
}

#[test]
fn auto_connect_is_one_typed_command_matching_snapshots() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let app = assert_typed_edit_matches_snapshots(
            || connect_app(document),
            |app| {
                app.begin_auto_connect();
                app.apply_pending_auto_connect();
            },
            "Auto-connect fields",
        );
        let command = newest_command(&app);
        let expected = match document {
            MappingDocument::Target(_) => GraphDocument::Target(TargetId::Named("Copy".into())),
            _ => GraphDocument::Target(TargetId::Primary),
        };
        assert_eq!(command.document(), &expected);
        assert_eq!(
            command.action(),
            &GraphAction::AutoConnect {
                scope: ScopeLocation::root(),
                connected: 2,
            }
        );
        assert_eq!(command.patch().added_nodes().count(), 2);
        assert_eq!(
            command.patch().children().count(),
            1,
            "the Header scope is created for its binding"
        );
    }
}

fn source_draft(name: &str) -> crate::extra_sources::ExtraSourceDraft {
    let mut draft = crate::extra_sources::ExtraSourceDraft::default();
    draft.name = name.into();
    draft.instance_path = format!("{name}.json");
    draft.schema = Some(fields(name));
    draft.options.json_document = true;
    draft
}

#[test]
fn named_source_additions_and_removals_are_typed_commands_matching_snapshots() {
    let app = assert_typed_edit_matches_snapshots(
        || connect_app(MappingDocument::Main),
        |app| {
            app.extra_source_draft = Some(source_draft("lookup"));
            app.finish_extra_source();
        },
        "Add named source",
    );
    assert_eq!(newest_command(&app).document(), &GraphDocument::Project);
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::AddNamedSource {
            name: "lookup".into()
        }
    );

    let with_source = || {
        let mut app = connect_app(MappingDocument::Main);
        app.extra_source_draft = Some(source_draft("lookup"));
        app.finish_extra_source();
        app.mark_clean();
        app.rebase_history();
        app
    };
    let mut app = assert_typed_edit_matches_snapshots(
        with_source,
        |app| app.remove_project_extra_source(0),
        "Remove named source",
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::RemoveNamedSource {
            name: "lookup".into()
        }
    );
    // Named sources changed behind the history refuse the step.
    assert_stale_undo_refused(&mut app, |project| {
        let other = source_draft("other")
            .build(&project.extra_sources)
            .expect("a complete source draft");
        project.extra_sources.push(other);
    });
}

fn target_draft(name: &str) -> crate::extra_targets::ExtraTargetDraft {
    crate::extra_targets::ExtraTargetDraft {
        name: name.into(),
        output_path: format!("{name}.json"),
        schema: Some(fields(name)),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        ..Default::default()
    }
}

#[test]
fn named_target_additions_edits_and_removals_are_typed_commands_matching_snapshots() {
    let app = assert_typed_edit_matches_snapshots(
        || connect_app(MappingDocument::Main),
        |app| {
            app.extra_target_draft = Some(target_draft("Audit"));
            app.finish_extra_target();
        },
        "Add named target",
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::AddNamedTarget {
            name: "Audit".into()
        }
    );

    let app = assert_typed_edit_matches_snapshots(
        || connect_app(MappingDocument::Target(0)),
        |app| {
            app.edit_extra_target(0);
            if let Some(draft) = app.extra_target_draft.as_mut() {
                draft.name = "Ledger".into();
            }
            app.finish_extra_target();
        },
        "Edit named target",
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::EditNamedTarget {
            name: "Copy".into(),
            renamed: "Ledger".into(),
        }
    );

    let mut app = assert_typed_edit_matches_snapshots(
        || connect_app(MappingDocument::Target(0)),
        |app| app.remove_extra_target_now(0),
        "Remove named target",
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::RemoveNamedTarget {
            name: "Copy".into()
        }
    );
    assert_stale_undo_refused(&mut app, |project| {
        project.extra_targets.push(NamedTarget {
            name: "Other".into(),
            path: None,
            schema: fields("Other"),
            options: FormatOptions::default(),
            root: Scope::default(),
        });
    });
}

fn xml_root_app() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = super::primary_xml_input::tests::schema();
    app.project.target = fields("Output");
    app.project.source_path = Some("input.xml".into());
    app.project.target_path = Some("output.json".into());
    app.project.target_options.json_document = true;
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    app
}

fn source_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_source_explorer(ui, true),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn text_center(shape: &egui::epaint::Shape, wanted: &str) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == wanted => {
            Some(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            shapes.iter().find_map(|shape| text_center(shape, wanted))
        }
        _ => None,
    }
}

fn click_source_control(app: &mut FerruleApp, context: &egui::Context, label: &str) {
    let mut output = source_frame(app, context, Vec::new());
    for _ in 0..3 {
        output = source_frame(app, context, Vec::new());
    }
    let position = output
        .shapes
        .iter()
        .find_map(|shape| text_center(&shape.shape, label))
        .unwrap_or_else(|| panic!("rendered source control {label:?}"));
    for pressed in [true, false] {
        source_frame(
            app,
            context,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn primary_xml_root_options_are_typed_commands_matching_snapshots() {
    let app = assert_typed_edit_matches_snapshots(
        xml_root_app,
        |app| {
            let context = egui::Context::default();
            crate::icons::install(&context);
            click_source_control(app, &context, "Primary XML root input");
            click_source_control(
                app,
                &context,
                "Read declared fields outside the selected type",
            );
        },
        "Change source format options",
    );
    assert!(
        app.project
            .source_options
            .xml_allow_inactive_root_type_members
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::EditSourceOptions
    );
    assert_eq!(newest_command(&app).patch().nodes().count(), 0);
}
