use mapping::Project;

use crate::WorkspaceSelection;

/// One atomic editor operation.
///
/// Commands operate on an isolated draft. Returning an error discards every
/// project and selection change. Calling [`EditorTransaction::project_mut`]
/// declares a semantic project edit and therefore creates an undo entry when
/// the command succeeds.
pub trait EditorCommand {
    type Error;

    fn label(&self) -> &str;

    fn apply(self, transaction: &mut EditorTransaction<'_>) -> Result<(), Self::Error>;
}

/// Mutable draft exposed while an [`EditorCommand`] is being applied.
pub struct EditorTransaction<'a> {
    original_project: &'a Project,
    changed_project: Option<Project>,
    selection: &'a mut WorkspaceSelection,
}

impl<'a> EditorTransaction<'a> {
    pub(crate) fn new(project: &'a Project, selection: &'a mut WorkspaceSelection) -> Self {
        Self {
            original_project: project,
            changed_project: None,
            selection,
        }
    }

    pub fn project(&self) -> &Project {
        self.changed_project
            .as_ref()
            .unwrap_or(self.original_project)
    }

    /// Returns the project draft and marks this command as history-worthy.
    pub fn project_mut(&mut self) -> &mut Project {
        self.changed_project
            .get_or_insert_with(|| self.original_project.clone())
    }

    pub fn selection(&self) -> &WorkspaceSelection {
        self.selection
    }

    pub fn set_selection(&mut self, selection: WorkspaceSelection) {
        *self.selection = selection;
    }

    pub fn clear_selection(&mut self) {
        *self.selection = WorkspaceSelection::None;
    }

    pub(crate) fn into_changed_project(self) -> Option<Project> {
        self.changed_project
    }
}
