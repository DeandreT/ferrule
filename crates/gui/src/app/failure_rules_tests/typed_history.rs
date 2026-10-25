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

#[test]
fn edit_ops_parity_for_source_failure_rule_actions() {
    use edit_ops::{
        EditOperation, Expression, FailureIteration, FailureRule, FailureSelection, Index, NodeId,
        SourceCollection,
    };
    let compare = |before: mapping::Project, app: &FerruleApp, operation| {
        let mut session = edit_ops::EditSession::new(before.clone());
        session.apply(operation).unwrap();
        assert_eq!(
            crate::project_state::model_key(session.project()),
            crate::project_state::model_key(&app.project)
        );
        session.undo().unwrap();
        assert_eq!(
            crate::project_state::model_key(session.project()),
            crate::project_state::model_key(&before)
        );
    };
    let mut app = fixture();
    let before = app.project.clone();
    click(&mut app, &context(), "Add source rule");
    compare(
        before,
        &app,
        EditOperation::AddFailureRule {
            rule: FailureRule {
                iteration: FailureIteration::Source {
                    collection: SourceCollection::Legacy { path: vec![] },
                },
                selection: FailureSelection::All {},
                message: None,
            },
        },
    );
    let mut app = two_rules();
    let before = app.project.clone();
    let ctx = context();
    click(&mut app, &ctx, "Expression is true");
    click(&mut app, &ctx, "Expression is false");
    compare(
        before,
        &app,
        EditOperation::EditFailureRule {
            index: Index(1),
            rule: FailureRule {
                iteration: FailureIteration::Source {
                    collection: SourceCollection::Legacy { path: vec![] },
                },
                selection: FailureSelection::WhenFalse {
                    expression: Expression::Existing { node: NodeId(0) },
                },
                message: None,
            },
        },
    );
    let mut app = two_rules();
    let before = app.project.clone();
    click(&mut app, &context(), "Move up");
    compare(
        before,
        &app,
        EditOperation::MoveFailureRule {
            from: Index(1),
            to: Index(0),
        },
    );
    let mut app = two_rules();
    let before = app.project.clone();
    click(&mut app, &context(), "Remove rule");
    compare(
        before,
        &app,
        EditOperation::RemoveFailureRule { index: Index(1) },
    );
}

#[test]
fn edit_ops_parity_for_generated_failure_rule_creation() {
    use edit_ops::{
        EditOperation, Expression, FailureIteration, FailureRule, FailureSelection,
        GeneratedSequence, NodeId, Scalar,
    };
    for label in [
        "Add integer range rule",
        "Add split text rule",
        "Add fixed-length text rule",
        "Add regex text rule",
    ] {
        let mut app = fixture();
        let before = app.project.clone();
        click(&mut app, &context(), label);
        let mapping::FailureIteration::Sequence { sequence } =
            &app.project.failure_rules[0].iteration
        else {
            panic!()
        };
        let constant = |id| {
            let mapping::Node::Const { value } = &app.project.graph.nodes[&id] else {
                panic!()
            };
            let value = match value {
                ir::Value::Int(value) => Scalar::Int {
                    text: value.to_string(),
                },
                ir::Value::String(value) => Scalar::String {
                    text: value.clone(),
                },
                _ => panic!("unexpected generated-rule default"),
            };
            Expression::Constant {
                id: NodeId(id),
                value,
            }
        };
        let sequence = match sequence {
            mapping::SequenceExpr::Generate { from, to, item } => GeneratedSequence::Generate {
                from: from.map(constant),
                to: constant(*to),
                item: NodeId(*item),
            },
            mapping::SequenceExpr::Tokenize {
                input,
                delimiter,
                item,
            } => GeneratedSequence::Tokenize {
                input: constant(*input),
                delimiter: constant(*delimiter),
                item: NodeId(*item),
            },
            mapping::SequenceExpr::TokenizeByLength {
                input,
                length,
                item,
            } => GeneratedSequence::TokenizeByLength {
                input: constant(*input),
                length: constant(*length),
                item: NodeId(*item),
            },
            mapping::SequenceExpr::TokenizeRegex {
                input,
                pattern,
                flags,
                item,
            } => GeneratedSequence::TokenizeRegex {
                input: constant(*input),
                pattern: constant(*pattern),
                flags: flags.map(constant),
                item: NodeId(*item),
            },
            _ => panic!("unexpected generated-rule kind"),
        };
        let mut session = edit_ops::EditSession::new(before.clone());
        session
            .apply(EditOperation::AddFailureRule {
                rule: FailureRule {
                    iteration: FailureIteration::Generated { sequence },
                    selection: FailureSelection::All {},
                    message: None,
                },
            })
            .unwrap();
        assert_eq!(
            crate::project_state::model_key(session.project()),
            crate::project_state::model_key(&app.project)
        );
        session.undo().unwrap();
        assert_eq!(
            crate::project_state::model_key(session.project()),
            crate::project_state::model_key(&before)
        );
    }
}
