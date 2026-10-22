//! Join authoring recorded as typed commands, checked against the snapshot
//! history it replaces.

use editor_ui::{GraphAction, ScopeLocation};

use super::super::typed_edit_oracle::{
    assert_stale_undo_refused, assert_typed_edit_matches_snapshots, newest_command,
};
use super::*;

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

/// The fixture after creating a join with the real controls, as a clean
/// baseline.
fn joined(document: MappingDocument) -> FerruleApp {
    let mut app = fixture(document, false);
    let context = context();
    click(&mut app, &context, true, "Join two collections", false);
    click(&mut app, &context, true, "Create inner join", false);
    assert!(active_scope(&app).join().is_some());
    app.mark_clean();
    app.rebase_history();
    app
}

fn rows() -> ScopeLocation {
    ScopeLocation::of_children(&[0])
}

#[test]
fn join_creation_key_edits_and_outputs_are_typed_commands_matching_snapshots() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut created = assert_typed_edit_matches_snapshots(
            || fixture(document, false),
            |app| {
                let context = context();
                click(app, &context, true, "Join two collections", false);
                click(app, &context, true, "Create inner join", false);
            },
            "Create join",
        );
        let (join, _) = active_scope(&created).join().expect("a join");
        assert_eq!(
            newest_command(&created).action(),
            &GraphAction::CreateJoin {
                scope: rows(),
                join,
            }
        );
        assert_stale_undo_refused(&mut created, |project| {
            let root = match document {
                MappingDocument::Target(index) => &mut project.extra_targets[index].root,
                _ => &mut project.root,
            };
            root.children[0].iteration = ScopeIteration::None;
        });

        let rekeyed = assert_typed_edit_matches_snapshots(
            || joined(document),
            |app| {
                let context = context();
                click(app, &context, true, "Edit equality keys", false);
                choose(app, &context, "Key", "Code");
                choose(app, &context, "Key", "Name");
                click(app, &context, true, "Apply equality keys", false);
            },
            "Edit join keys",
        );
        assert_eq!(
            newest_command(&rekeyed).action(),
            &GraphAction::EditJoinKeys {
                scope: rows(),
                join,
            }
        );

        let projected = assert_typed_edit_matches_snapshots(
            || joined(document),
            |app| {
                let context = context();
                click(app, &context, true, "Add joined output", false);
                click(app, &context, true, "Bind joined output", false);
            },
            "Add joined output",
        );
        let binding = active_scope(&projected)
            .bindings
            .last()
            .map(editor_ui::TargetBinding::of)
            .expect("a joined output binding");
        assert_eq!(
            newest_command(&projected).action(),
            &GraphAction::AddJoinedOutput {
                scope: rows(),
                join,
                binding: binding.clone(),
            }
        );
        assert_eq!(
            newest_command(&projected)
                .patch()
                .added_nodes()
                .collect::<Vec<_>>(),
            [binding.node]
        );
    }
}
