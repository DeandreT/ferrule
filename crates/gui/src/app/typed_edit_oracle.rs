//! Compares typed edit history with the snapshot history it replaces.

use editor_ui::{GraphCommand, HistoryRecord};

use super::*;
use crate::project_state::project_snapshot_key;

pub(super) fn key(app: &FerruleApp) -> String {
    project_snapshot_key(&app.project)
}

/// The single command of the newest undo step.
pub(super) fn newest_command(app: &FerruleApp) -> &GraphCommand {
    match app.history.next_undo() {
        Some(HistoryRecord::Command(record)) => match record.commands() {
            [command] => command,
            commands => panic!("one command in the newest step, got {}", commands.len()),
        },
        _ => panic!("the newest step is a command"),
    }
}

/// Builds two apps with `build` and runs `edit` on both. One records typed
/// commands; the oracle records snapshots as before the migration. The
/// newest step must be `label` in the typed app and a snapshot in the
/// oracle, and both must hold the same project after the edit, after undo,
/// and after redo. Returns the typed app after redo.
pub(super) fn assert_typed_edit_matches_snapshots(
    build: impl Fn() -> FerruleApp,
    edit: impl Fn(&mut FerruleApp),
    label: &str,
) -> FerruleApp {
    let mut typed = build();
    let mut oracle = build();
    oracle.snapshot_oracle = true;
    let before = key(&typed);
    assert_eq!(key(&oracle), before);
    let steps = typed.history.undo_len();
    for app in [&mut typed, &mut oracle] {
        edit(app);
        app.observe_editor_history(std::time::Instant::now(), false);
    }
    assert_eq!(typed.history.undo_label(), Some(label));
    assert!(matches!(
        typed.history.next_undo(),
        Some(HistoryRecord::Command(_))
    ));
    assert_eq!(oracle.history.undo_label(), Some("Edit mapping"));
    assert_eq!(typed.history.undo_len(), steps + 1, "the edit is one step");
    let edited = key(&typed);
    assert_ne!(edited, before, "the edit changes the project");
    assert_eq!(key(&oracle), edited);
    for app in [&mut typed, &mut oracle] {
        app.undo_project();
        assert_eq!(key(app), before);
    }
    assert_eq!(typed.status, format!("undid {label}"));
    for app in [&mut typed, &mut oracle] {
        app.redo_project();
    }
    assert_eq!(typed.status, format!("redid {label}"));
    assert_eq!(key(&typed), edited);
    assert_eq!(key(&oracle), edited);
    typed
}

/// Undoing `app`'s newest step after its observed project was changed
/// behind the history refuses without changing the project or history.
pub(super) fn assert_stale_undo_refused(app: &mut FerruleApp, stale: impl FnOnce(&mut Project)) {
    stale(&mut app.observed_editor.project);
    let project = key(app);
    let steps = (app.history.undo_len(), app.history.redo_len());
    app.undo_project();
    assert_eq!(app.status, "Undo failed");
    assert_eq!(key(app), project);
    assert_eq!((app.history.undo_len(), app.history.redo_len()), steps);
}
