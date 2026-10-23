//! Sequence consumer duplication recorded as a typed command, checked
//! against the snapshot history it replaces.

use editor_ui::GraphAction;

use super::super::typed_edit_oracle::{assert_typed_edit_matches_snapshots, newest_command};
use super::*;

#[test]
fn every_admitted_duplication_is_a_typed_command_matching_snapshots() {
    let originals = Originals::new();
    let corpus = literals(&originals);
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "admitted")
    {
        let consumer = id(&case["selected_consumer"]);
        let context = document(&case["active_context"]);
        let app = assert_typed_edit_matches_snapshots(
            || prepared(&case["snapshots"]["before"], false),
            |app| app.apply_sequence_duplication_request(context, consumer),
            "Duplicate sequence consumer",
        );
        let command = newest_command(&app);
        let GraphAction::DuplicateSequenceConsumer {
            original,
            duplicate,
        } = *command.action()
        else {
            panic!("a duplication action, got {:?}", command.action());
        };
        assert_eq!(original, consumer);
        assert!(app.project.graph.nodes.contains_key(&duplicate));
        assert!(
            command
                .patch()
                .nodes()
                .all(|(_, change)| change.before().is_none())
        );
    }
}
