//! Failure rule edits recorded as typed commands, checked against the
//! snapshot history they replace.

use editor_ui::{GraphAction, GraphDocument};

use super::super::super::typed_edit_oracle::{
    assert_stale_undo_refused, assert_typed_edit_matches_snapshots, newest_command,
};
use super::*;

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

/// The fixture with two source rules as a clean baseline, the second one
/// selected.
fn two_rules() -> FerruleApp {
    let mut app = fixture();
    let context = context();
    click(&mut app, &context, "Add source rule");
    click(&mut app, &context, "Add source rule");
    app.project.failure_rules[1].selection = mapping::FailureSelection::WhenTrue { predicate: 0 };
    app.mark_clean();
    app.rebase_history();
    // Rebasing clears diagnostic navigation, which owns the selection.
    app.selected_failure_rule = Some(1);
    app
}

#[test]
fn failure_rule_additions_are_typed_commands_matching_snapshots() {
    let app = assert_typed_edit_matches_snapshots(
        fixture,
        |app| click(app, &context(), "Add source rule"),
        "Add failure rule",
    );
    let command = newest_command(&app);
    assert_eq!(command.document(), &GraphDocument::Project);
    assert_eq!(command.action(), &GraphAction::AddFailureRule { index: 0 });
    assert_eq!(command.patch().nodes().count(), 0);

    // A generated rule adds its bounds and the item it owns.
    let app = assert_typed_edit_matches_snapshots(
        fixture,
        |app| click(app, &context(), "Add integer range rule"),
        "Add failure rule",
    );
    assert_eq!(newest_command(&app).patch().added_nodes().count(), 3);
}

#[test]
fn failure_rule_edits_moves_and_removals_are_typed_commands_matching_snapshots() {
    let app = assert_typed_edit_matches_snapshots(
        two_rules,
        |app| {
            let context = context();
            click(app, &context, "Expression is true");
            click(app, &context, "Expression is false");
        },
        "Edit failure rule",
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::EditFailureRule { index: 1 }
    );

    let app = assert_typed_edit_matches_snapshots(
        two_rules,
        |app| click(app, &context(), "Move up"),
        "Move failure rule",
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::MoveFailureRule { from: 1, to: 0 }
    );

    let mut app = assert_typed_edit_matches_snapshots(
        two_rules,
        |app| click(app, &context(), "Remove rule"),
        "Remove failure rule",
    );
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::RemoveFailureRule { index: 1 }
    );
    // Rules changed behind the history refuse the step.
    assert_stale_undo_refused(&mut app, |project| {
        project.failure_rules.clear();
    });
}
