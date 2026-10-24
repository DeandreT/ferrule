use crate::{EditError, EditOperation, Index, plan};
use editor_ui::{
    ComponentChange, DocumentOrigin, EditHistory, GraphCommand, HistoryRecord, RevisionChange,
    RevisionTracker,
};
use mapping::Project;

/// One operation is one undo step, even when it plans several graph commands.
/// No project snapshot is retained per step.
pub struct EditSession {
    project: Project,
    history: EditHistory<std::convert::Infallible, Vec<GraphCommand>>,
    revisions: RevisionTracker,
    applied: Vec<EditOperation>,
    undone: Vec<EditOperation>,
}

impl EditSession {
    pub fn new(project: Project) -> Self {
        let history = match editor_ui::HistoryLimit::new(2_000) {
            Some(limit) => EditHistory::with_history_limit(limit),
            None => EditHistory::default(),
        };
        Self {
            project,
            history,
            revisions: RevisionTracker::new(DocumentOrigin::Saved),
            applied: Vec::new(),
            undone: Vec::new(),
        }
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn revisions(&self) -> &RevisionTracker {
        &self.revisions
    }
    pub fn operations(&self) -> &[EditOperation] {
        &self.applied
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }
    pub fn apply(&mut self, operation: EditOperation) -> Result<bool, EditError> {
        let commands = plan(&self.project, &operation)?;
        if commands.is_empty() {
            return Ok(false);
        }
        // Preflight the entire transaction before changing the live model.
        apply_commands(&mut self.project, &commands, false)?;
        self.history
            .record(HistoryRecord::Command(commands), operation.label());
        self.applied.push(operation);
        self.undone.clear();
        self.changed();
        Ok(true)
    }
    pub fn undo(&mut self) -> Result<bool, EditError> {
        let changed = self
            .history
            .undo(|record| match record {
                HistoryRecord::Command(commands) => {
                    apply_commands(&mut self.project, commands, true)
                }
                HistoryRecord::Snapshot(never) => match *never {},
            })?
            .is_some();
        if changed {
            if let Some(operation) = self.applied.pop() {
                self.undone.push(operation);
            }
            self.changed();
        }
        Ok(changed)
    }
    pub fn redo(&mut self) -> Result<bool, EditError> {
        let changed = self
            .history
            .redo(|record| match record {
                HistoryRecord::Command(commands) => {
                    apply_commands(&mut self.project, commands, false)
                }
                HistoryRecord::Snapshot(never) => match *never {},
            })?
            .is_some();
        if changed {
            if let Some(operation) = self.undone.pop() {
                self.applied.push(operation);
            }
            self.changed();
        }
        Ok(changed)
    }
    pub fn replay(project: Project, operations: &[EditOperation]) -> Result<Self, EditError> {
        let mut session = Self::new(project);
        for (index, operation) in operations.iter().enumerate() {
            session
                .apply(operation.clone())
                .map_err(|source| EditError::Replay {
                    index: Index(index as u32),
                    source: Box::new(source),
                })?;
        }
        Ok(session)
    }
    fn changed(&mut self) {
        self.revisions.record(RevisionChange {
            semantic: if self.applied.is_empty() {
                ComponentChange::MatchesSaved
            } else {
                ComponentChange::Changed
            },
            layout: ComponentChange::Unchanged,
        });
    }
}

fn apply_commands(
    project: &mut Project,
    commands: &[GraphCommand],
    reverse: bool,
) -> Result<(), EditError> {
    let mut candidate = project.clone();
    if reverse {
        for command in commands.iter().rev() {
            command.revert_on_matching(&mut candidate, crate::planner::same_node)?;
        }
    } else {
        for command in commands {
            command.apply_to_matching(&mut candidate, crate::planner::same_node)?;
        }
    }
    *project = candidate;
    Ok(())
}
