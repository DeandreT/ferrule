use std::collections::VecDeque;
use std::sync::Arc;

use mapping::Project;

use crate::{EditorCommand, EditorTransaction, WorkspaceSelection};

/// Default number of project states retained in each history direction.
const DEFAULT_HISTORY_LIMIT: usize = 100;

/// A validated, non-zero bound for undo and redo history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryLimit(usize);

impl HistoryLimit {
    pub const fn new(value: usize) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

impl Default for HistoryLimit {
    fn default() -> Self {
        Self(DEFAULT_HISTORY_LIMIT)
    }
}

/// Whether a newly opened editor document has a persisted save point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentOrigin {
    Saved,
    Unsaved,
}

/// Result metadata for a successfully applied command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    project_changed: bool,
    selection_changed: bool,
}

impl CommandOutcome {
    pub fn project_changed(self) -> bool {
        self.project_changed
    }

    pub fn selection_changed(self) -> bool {
        self.selection_changed
    }
}

/// Result metadata for undo and redo requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryOutcome {
    label: String,
}

impl HistoryOutcome {
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Clone)]
struct HistoryEntry {
    project: Project,
    revision: Revision,
    label: String,
}

#[derive(Clone)]
struct Revision(Arc<()>);

impl Revision {
    fn new() -> Self {
        Self(Arc::new(()))
    }
}

impl PartialEq for Revision {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// A mapping project plus host-independent editor state.
///
/// Hosts can read the project directly, but all mutations pass through
/// [`EditorDocument::execute`] so failures are atomic and history remains
/// coherent. Workspace selection is transient and never affects dirty state.
pub struct EditorDocument {
    project: Project,
    selection: WorkspaceSelection,
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    history_limit: HistoryLimit,
    revision: Revision,
    save_point: Option<Revision>,
}

impl EditorDocument {
    pub fn new(project: Project, origin: DocumentOrigin) -> Self {
        Self::with_history_limit(project, origin, HistoryLimit::default())
    }

    pub fn with_history_limit(
        project: Project,
        origin: DocumentOrigin,
        history_limit: HistoryLimit,
    ) -> Self {
        let revision = Revision::new();
        Self {
            project,
            selection: WorkspaceSelection::None,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            history_limit,
            save_point: (origin == DocumentOrigin::Saved).then(|| revision.clone()),
            revision,
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn selection(&self) -> &WorkspaceSelection {
        &self.selection
    }

    pub fn set_selection(&mut self, selection: WorkspaceSelection) {
        self.selection = selection;
    }

    pub fn clear_selection(&mut self) {
        self.selection = WorkspaceSelection::None;
    }

    pub fn is_dirty(&self) -> bool {
        self.save_point.as_ref() != Some(&self.revision)
    }

    /// Records that the host successfully persisted the current project.
    pub fn mark_saved(&mut self) {
        self.save_point = Some(self.revision.clone());
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|entry| entry.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.back().map(|entry| entry.label.as_str())
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Applies a command to an isolated draft and commits it only on success.
    pub fn execute<C>(&mut self, command: C) -> Result<CommandOutcome, C::Error>
    where
        C: EditorCommand,
    {
        let label = command.label().to_owned();
        let mut selection = self.selection.clone();
        let mut transaction = EditorTransaction::new(&self.project, &mut selection);
        command.apply(&mut transaction)?;
        let changed_project = transaction.into_changed_project();
        let project_changed = changed_project.is_some();
        let selection_changed = selection != self.selection;

        if let Some(project) = changed_project {
            let previous = HistoryEntry {
                project: std::mem::replace(&mut self.project, project),
                revision: self.revision.clone(),
                label,
            };
            self.push_undo(previous);
            self.redo.clear();
            self.revision = Revision::new();
        }
        self.selection = selection;

        Ok(CommandOutcome {
            project_changed,
            selection_changed,
        })
    }

    pub fn undo(&mut self) -> Option<HistoryOutcome> {
        let previous = self.undo.pop_back()?;
        let label = previous.label.clone();
        let current = HistoryEntry {
            project: std::mem::replace(&mut self.project, previous.project),
            revision: self.revision.clone(),
            label: previous.label,
        };
        self.push_redo(current);
        self.revision = previous.revision;
        self.clear_selection();
        Some(HistoryOutcome { label })
    }

    pub fn redo(&mut self) -> Option<HistoryOutcome> {
        let next = self.redo.pop_back()?;
        let label = next.label.clone();
        let current = HistoryEntry {
            project: std::mem::replace(&mut self.project, next.project),
            revision: self.revision.clone(),
            label: next.label,
        };
        self.push_undo(current);
        self.revision = next.revision;
        self.clear_selection();
        Some(HistoryOutcome { label })
    }

    fn push_undo(&mut self, entry: HistoryEntry) {
        push_bounded(&mut self.undo, entry, self.history_limit);
    }

    fn push_redo(&mut self, entry: HistoryEntry) {
        push_bounded(&mut self.redo, entry, self.history_limit);
    }
}

fn push_bounded(entries: &mut VecDeque<HistoryEntry>, entry: HistoryEntry, limit: HistoryLimit) {
    if entries.len() == limit.get() {
        entries.pop_front();
    }
    entries.push_back(entry);
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use ir::{ScalarType, SchemaNode};
    use mapping::{Graph, Scope};

    use super::*;
    use crate::GraphNodeSelection;

    fn project() -> Project {
        Project {
            source: SchemaNode::group("source", vec![]),
            target: SchemaNode::group("target", vec![]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph::default(),
            root: Scope::default(),
        }
    }

    struct RenameTarget<'a>(&'a str);

    impl EditorCommand for RenameTarget<'_> {
        type Error = Infallible;

        fn label(&self) -> &str {
            "Rename target"
        }

        fn apply(self, transaction: &mut EditorTransaction<'_>) -> Result<(), Self::Error> {
            transaction.project_mut().target.name = self.0.to_owned();
            Ok(())
        }
    }

    struct SelectNode(mapping::NodeId);

    impl EditorCommand for SelectNode {
        type Error = Infallible;

        fn label(&self) -> &str {
            "Select node"
        }

        fn apply(self, transaction: &mut EditorTransaction<'_>) -> Result<(), Self::Error> {
            transaction.set_selection(WorkspaceSelection::GraphNodes(GraphNodeSelection::new(
                self.0,
            )));
            Ok(())
        }
    }

    struct FailingEdit;

    impl EditorCommand for FailingEdit {
        type Error = &'static str;

        fn label(&self) -> &str {
            "Fail"
        }

        fn apply(self, transaction: &mut EditorTransaction<'_>) -> Result<(), Self::Error> {
            transaction.project_mut().target.name = "not committed".to_owned();
            transaction.set_selection(WorkspaceSelection::Project);
            Err("rejected")
        }
    }

    #[test]
    fn saved_document_tracks_save_point_across_undo_and_redo() {
        let mut document = EditorDocument::new(project(), DocumentOrigin::Saved);
        assert!(!document.is_dirty());

        let outcome = document.execute(RenameTarget("first"));
        assert!(outcome.is_ok());
        assert!(document.is_dirty());
        document.mark_saved();
        assert!(!document.is_dirty());

        let outcome = document.execute(RenameTarget("second"));
        assert!(outcome.is_ok());
        assert!(document.is_dirty());
        assert_eq!(
            document.undo().map(|item| item.label),
            Some("Rename target".into())
        );
        assert!(!document.is_dirty());
        assert_eq!(
            document.redo().map(|item| item.label),
            Some("Rename target".into())
        );
        assert!(document.is_dirty());
    }

    #[test]
    fn unsaved_document_remains_dirty_until_first_successful_save() {
        let mut document = EditorDocument::new(project(), DocumentOrigin::Unsaved);
        assert!(document.is_dirty());

        document.mark_saved();
        assert!(!document.is_dirty());
    }

    #[test]
    fn selection_only_commands_do_not_dirty_or_enter_history() {
        let mut document = EditorDocument::new(project(), DocumentOrigin::Saved);

        let outcome = document.execute(SelectNode(7));
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => match error {},
        };

        assert!(!outcome.project_changed());
        assert!(outcome.selection_changed());
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert!(matches!(
            document.selection(),
            WorkspaceSelection::GraphNodes(nodes) if nodes.primary() == 7
        ));
    }

    #[test]
    fn failed_commands_roll_back_project_selection_and_history() {
        let mut document = EditorDocument::new(project(), DocumentOrigin::Saved);
        document.set_selection(WorkspaceSelection::FailureRule { index: 2 });

        assert_eq!(document.execute(FailingEdit), Err("rejected"));
        assert_eq!(document.project().target.name, "target");
        assert_eq!(
            document.selection(),
            &WorkspaceSelection::FailureRule { index: 2 }
        );
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
    }

    #[test]
    fn history_is_bounded_and_new_edits_clear_redo() {
        let limit = match HistoryLimit::new(2) {
            Some(limit) => limit,
            None => panic!("two is a non-zero history limit"),
        };
        let mut document =
            EditorDocument::with_history_limit(project(), DocumentOrigin::Saved, limit);
        for name in ["one", "two", "three"] {
            let result = document.execute(RenameTarget(name));
            assert!(result.is_ok());
        }
        assert_eq!(document.undo_len(), 2);
        assert_eq!(document.undo_label(), Some("Rename target"));

        assert!(document.undo().is_some());
        assert!(document.undo().is_some());
        assert!(document.undo().is_none());
        assert_eq!(document.project().target.name, "one");
        assert_eq!(document.redo_len(), 2);

        let result = document.execute(RenameTarget("branch"));
        assert!(result.is_ok());
        assert!(!document.can_redo());
        assert_eq!(document.project().target.name, "branch");
    }

    #[test]
    fn undo_and_redo_clear_transient_selection() {
        let mut document = EditorDocument::new(project(), DocumentOrigin::Saved);
        let result = document.execute(RenameTarget("changed"));
        assert!(result.is_ok());
        document.set_selection(WorkspaceSelection::GraphNodes(GraphNodeSelection::new(4)));

        assert!(document.undo().is_some());
        assert_eq!(document.selection(), &WorkspaceSelection::None);
        document.set_selection(WorkspaceSelection::SchemaNode {
            boundary: crate::BoundaryId::Target(crate::TargetId::Primary),
            path: vec!["field".to_owned()],
        });

        assert!(document.redo().is_some());
        assert_eq!(document.selection(), &WorkspaceSelection::None);
    }

    #[test]
    fn transaction_can_add_schema_content_atomically() {
        struct AddField;

        impl EditorCommand for AddField {
            type Error = Infallible;

            fn label(&self) -> &str {
                "Add field"
            }

            fn apply(self, transaction: &mut EditorTransaction<'_>) -> Result<(), Self::Error> {
                if let ir::SchemaKind::Group { children, .. } =
                    &mut transaction.project_mut().target.kind
                {
                    children.push(SchemaNode::scalar("value", ScalarType::String));
                }
                Ok(())
            }
        }

        let mut document = EditorDocument::new(project(), DocumentOrigin::Saved);
        let result = document.execute(AddField);
        assert!(result.is_ok());
        assert!(document.project().target.child("value").is_some());
    }
}
